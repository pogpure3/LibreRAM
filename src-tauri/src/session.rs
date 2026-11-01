// SPDX-License-Identifier: GPL-2.0-only
//! Per-user logon configuration for a new, empty session disk.
//! Keeps the session.json format of the Python version.
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const TASK_NAME: &str = "LibreRAM-Session";

#[derive(Serialize, Deserialize, Clone)]
pub struct SessionConfig {
    pub letter: String,
    pub size_mib: u64,
    pub filesystem: String,
    pub allocation_size: u32,
    pub folders: Vec<String>,
    #[serde(default)]
    pub label: String,
}

fn program_files() -> PathBuf {
    std::env::var("ProgramFiles").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(r"C:\Program Files"))
}

pub fn installed() -> bool {
    std::env::current_exe()
        .map(|exe| exe.starts_with(program_files().join("LibreRAM")))
        .unwrap_or(false)
}

pub fn data_dir() -> PathBuf {
    if installed() {
        std::env::current_exe().unwrap().parent().unwrap().join("data")
    } else {
        std::env::var("LOCALAPPDATA").map(PathBuf::from).unwrap().join("LibreRAM")
    }
}

fn config_path() -> PathBuf {
    data_dir().join("session.json")
}

pub fn log_event(message: &str) {
    let dir = data_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("startup.log"))
        .and_then(|mut f| std::io::Write::write_all(&mut f, format!("{stamp} {message}\n").as_bytes()));
}

pub fn load() -> Result<SessionConfig, String> {
    let path = config_path();
    if !path.is_file() {
        return Err("нет сохранённых параметров".into());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("Неверный формат параметров автозапуска. {e}"))
}

pub fn enable(config: &SessionConfig) -> Result<(), String> {
    let value = serde_json::to_value(config).map_err(|e| e.to_string())?;
    crate::imdisk::validate_session_config(&value)?;
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string(config).map_err(|e| e.to_string())?;
    let temporary = dir.join("session.json.tmp");
    std::fs::write(&temporary, json.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, config_path()).map_err(|e| e.to_string())?;
    scheduled_task("enable")
}

pub fn disable() -> Result<(), String> {
    scheduled_task("disable")?;
    Ok(())
}

/// Register or unregister the per-user logon task that starts this exe elevated.
fn scheduled_task(operation: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let workdir = exe.parent().unwrap().to_string_lossy().into_owned();
    let payload = serde_json::json!({
        "name": TASK_NAME,
        "exe": exe.to_string_lossy(),
        "args": "--session-startup",
        "operation": operation,
        "workdir": workdir,
    });
    let encoded_payload = base64::engine::general_purpose::STANDARD.encode(payload.to_string());
    let script = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
trap { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }
$p = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('__PAYLOAD__')) | ConvertFrom-Json
$task = Get-ScheduledTask -TaskPath '\' -TaskName $p.name -ErrorAction SilentlyContinue
if ($task) {
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $principalSid = ([Security.Principal.NTAccount]::new($task.Principal.UserId)).Translate([Security.Principal.SecurityIdentifier]).Value
    $triggerSid = if (@($task.Triggers).Count -eq 1 -and $task.Triggers[0].UserId) { ([Security.Principal.NTAccount]::new($task.Triggers[0].UserId)).Translate([Security.Principal.SecurityIdentifier]).Value } else { '' }
    if (@($task.Actions).Count -ne 1 -or $task.Actions[0].Execute -ine $p.exe -or $task.Principal.RunLevel -ne 'Highest' -or $task.Principal.LogonType -ne 'Interactive' -or $principalSid -ne $sid -or $triggerSid -ne $sid) {
        throw 'A different scheduled task uses this name.'
    }
}
if ($p.operation -eq 'enable' -and -not $task) {
    $user = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    $a = New-ScheduledTaskAction -Execute $p.exe -Argument $p.args -WorkingDirectory $p.workdir
    $t = New-ScheduledTaskTrigger -AtLogOn -User $user
    $principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest
    Register-ScheduledTask -TaskName $p.name -TaskPath '\' -Action $a -Trigger $t -Principal $principal | Out-Null
}
if ($p.operation -eq 'disable' -and $task) {
    Unregister-ScheduledTask -TaskName $p.name -TaskPath '\' -Confirm:$false
}
exit 0
"#.replace("__PAYLOAD__", &encoded_payload);
    let encoded_script = base64::engine::general_purpose::STANDARD
        .encode(script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect::<Vec<u8>>());
    run_powershell(&encoded_script)
}

fn run_powershell(encoded: &str) -> Result<(), String> {
    let powershell = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let powershell = PathBuf::from(powershell)
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let output = std::process::Command::new(powershell)
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded])
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf16_lossy(
        &output.stderr.as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes([p[0], p[1]])).collect::<Vec<_>>(),
    );
    Err(format!("Не удалось изменить задачу автозапуска Windows: {}", stderr.trim()))
}

/// Create the disk described by the saved config at logon.
pub fn restore() -> Result<(), String> {
    let config = load().map_err(|e| {
        log_event(&format!("Ошибка чтения настроек: {e}"));
        e
    })?;
    if std::path::Path::new(&format!("{}\\", config.letter)).exists() {
        let message = format!("Буква {} занята. Существующий диск не изменён.", config.letter);
        log_event(&message);
        return Err(message);
    }
    let request = crate::imdisk::CreateRequest {
        letter: config.letter.clone(),
        size_mib: config.size_mib,
        filesystem: config.filesystem.clone(),
        image: None,
        persistent: false,
        allocation_size: config.allocation_size,
    };
    crate::imdisk::create(&request)?;
    if !config.label.is_empty() {
        if let Err(e) = crate::volume::set_label(&config.letter, &config.label) {
            log_event(&format!("Ошибка метки тома: {e}"));
        }
    }
    if let Err(e) = crate::volume::create_folders(&config.letter, &config.folders) {
        log_event(&format!("Ошибка создания папок: {e}"));
    }
    log_event("Диск создан");
    Ok(())
}
