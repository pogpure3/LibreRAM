// SPDX-License-Identifier: GPL-2.0-only
//! Per-user logon configuration for a new, empty session disk.
//! Keeps the session.json format of the Python version.
use base64::Engine;
use crate::volume::pcw;
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;

const TASK_NAME: &str = "LibreRAM-Session";

#[derive(Serialize, Deserialize, Clone)]
pub struct SessionConfig {
    pub letter: String,
    // session.json keeps Python-compatible snake_case; the UI sends camelCase.
    #[serde(default, alias = "sizeMib")]
    pub size_mib: u64,
    pub filesystem: String,
    #[serde(default, alias = "allocationSize")]
    pub allocation_size: u32,
    pub folders: Vec<String>,
    #[serde(default)]
    pub label: String,
}

/// session.json: {"disks": [...]} plus every logon disk. The old Python
/// format was one flat config object — read it as a single-disk list.
#[derive(Serialize, Deserialize, Clone)]
pub struct SessionFile {
    #[serde(default)]
    pub disks: Vec<SessionConfig>,
    #[serde(default)]
    legacy: Option<SessionConfig>,
}

impl SessionFile {
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: SessionFile = serde_json::from_str(text).map_err(|e| format!("Неверный формат параметров автозапуска. {e}"))?;
        if !file.disks.is_empty() {
            return Ok(file);
        }
        // Either {"disks": []} or the legacy flat object (fields leaked into
        // this struct as nulls). Re-parse as the old shape when a letter exists.
        let value: serde_json::Value = serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
        if value.get("letter").is_some() {
            let legacy: SessionConfig = serde_json::from_value(value)
                .map_err(|e| format!("Неверный формат параметров автозапуска. {e}"))?;
            return Ok(SessionFile { disks: vec![legacy], legacy: None });
        }
        Ok(file)
    }

    pub fn all(&self) -> &[SessionConfig] {
        &self.disks
    }
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
        std::env::var("LOCALAPPDATA")
            .or_else(|_| std::env::var("USERPROFILE").map(|p| format!("{p}\\AppData\\Local")))
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(r"C:\Users\Default\AppData\Local"))
            .join("LibreRAM")
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
        .and_then(|mut f| std::io::Write::write_all(&mut f, format!("{stamp} {}\n", message.replace(['\r', '\n'], " ")).as_bytes()));
}

pub fn load() -> Result<SessionFile, String> {
    let path = config_path();
    if !path.is_file() {
        return Err("нет сохранённых параметров".into());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    SessionFile::parse(&text)
}

pub fn enable(disks: &[SessionConfig]) -> Result<(), String> {
    if disks.is_empty() {
        // Empty save = no autostart at all: unregister, forget the config.
        scheduled_task("disable")?;
        let _ = std::fs::remove_file(config_path());
        return Ok(());
    }
    for config in disks {
        let value = serde_json::to_value(config).map_err(|e| e.to_string())?;
        crate::imdisk::validate_session_config(&value)?;
    }
    let file = SessionFile { disks: disks.to_vec(), legacy: None };
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Task first: a failed registration must not leave a stale session.json.
    scheduled_task("enable")?;
    let json = serde_json::to_string(&file).map_err(|e| e.to_string())?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let temporary = dir.join(format!("session-{nanos}.tmp"));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut f| std::io::Write::write_all(&mut f, json.as_bytes()))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, config_path()).map_err(|e| e.to_string())?;
    Ok(())
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
    $sameExe = @($task.Actions).Count -eq 1 -and $task.Actions[0].Execute -ieq $p.exe
    $libreRamTask = @($task.Actions).Count -ge 1 -and $task.Actions[0].Execute -like ("$env:ProgramFiles\LibreRAM\*")
    if (-not $sameExe -and -not ($libreRamTask -and $principalSid -eq $sid -and $task.Principal.LogonType -eq 'Interactive')) {
        throw 'A different scheduled task uses this name.'
    }
    # Task from another LibreRAM install (e.g. the Python one): replace it.
    if (-not $sameExe) { Unregister-ScheduledTask -TaskName $p.name -TaskPath '\' -Confirm:$false; $task = $null }
    elseif ($task.Principal.RunLevel -ne 'Highest' -or $triggerSid -ne $sid) {
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
        .creation_flags(0x08000000) // CREATE_NO_WINDOW: no console flash
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    // PowerShell console stderr follows the OEM code page, not UTF-16.
    let stderr = crate::imdisk::decode(&output.stderr);
    Err(format!("Не удалось изменить задачу автозапуска Windows: {}", stderr.trim()))
}

/// Create every configured disk at logon; one failure skips that disk only.
pub fn restore() -> Result<(), String> {
    let file = load().map_err(|e| {
        log_event(&format!("Ошибка чтения настроек: {e}"));
        e
    })?;
    let mut errors = Vec::new();
    for config in file.all() {
        if let Err(error) = restore_one(config) {
            log_event(&format!("Диск {}: {error}", config.letter));
            errors.push(format!("{}: {error}", config.letter));
        }
    }
    if errors.is_empty() {
        log_event(&format!("Дисков создано: {}", file.all().len()));
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn restore_one(config: &SessionConfig) -> Result<(), String> {
    if std::path::Path::new(&format!("{}\\", config.letter)).exists() {
        // A live ImDisk disk keeps the letter; a ghost volume gets evicted.
        if backed_by_imdisk(&config.letter) {
            return Err(format!("Буква {} занята. Существующий диск не изменён.", config.letter));
        }
        clear_ghost(&config.letter)?;
    }
    let request = crate::imdisk::CreateRequest {
        letter: config.letter.clone(),
        size_mib: config.size_mib,
        filesystem: config.filesystem.clone(),
        image: None,
        persistent: false,
        allocation_size: config.allocation_size,
    };
    crate::imdisk::validate_session_config(&serde_json::to_value(config).map_err(|e| e.to_string())?)?;
    crate::imdisk::create(&request)?;
    if !config.label.is_empty() {
        if let Err(e) = crate::volume::set_label(&config.letter, &config.label) {
            log_event(&format!("Ошибка метки тома: {e}"));
        }
    }
    if let Err(e) = crate::volume::create_folders(&config.letter, &config.folders) {
        log_event(&format!("Ошибка создания папок: {e}"));
    }
    log_event(&format!("Диск {} создан", config.letter));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_flat_config_as_single_disk() {
        let file = SessionFile::parse(r#"{"letter":"O:","size_mib":1024,"filesystem":"NTFS","allocation_size":0,"folders":["Temp"],"label":"RAM"}"#).unwrap();
        assert_eq!(file.all().len(), 1);
        assert_eq!(file.all()[0].letter, "O:");
    }

    #[test]
    fn parses_disk_list() {
        let file = SessionFile::parse(r#"{"disks":[{"letter":"O:","size_mib":512,"filesystem":"NTFS","allocation_size":0,"folders":[],"label":""},{"letter":"P:","size_mib":256,"filesystem":"FAT32","allocation_size":0,"folders":[],"label":""}]}"#).unwrap();
        assert_eq!(file.all().len(), 2);
        assert_eq!(file.all()[1].letter, "P:");
    }

    #[test]
    fn empty_list_stays_empty() {
        let file = SessionFile::parse(r#"{"disks":[]}"#).unwrap();
        assert!(file.all().is_empty());
    }
}

/// Is the letter held by a live ImDisk device (a real RAM disk of ours)?
fn backed_by_imdisk(letter: &str) -> bool {
    let target = crate::volume::wide(letter);
    let mut buffer = [0u16; 1024];
    // SAFETY: buffer and size match; NUL-separated device names returned.
    let len = unsafe { windows::Win32::Storage::FileSystem::QueryDosDeviceW(pcw(&target), Some(&mut buffer)) };
    if len == 0 {
        return false; // no dos device: letter is free
    }
    let mapping = crate::imdisk::decode(&buffer[..(len as usize).min(buffer.len() - 1)]
        .iter().flat_map(|u| u.to_le_bytes()).collect::<Vec<u8>>());
    mapping.to_uppercase().contains("IMDISK")
}

/// A ghost volume (letter busy, no ImDisk device behind it) blocks logon
/// creation; drop the mount point so the letter frees up.
fn clear_ghost(letter: &str) -> Result<(), String> {
    let cmd = std::path::PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
        .join(r"System32\mountvol.exe");
    let output = std::process::Command::new(cmd)
        .args([letter, "/D"])
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() && !backed_by_imdisk(letter) {
        log_event(&format!("Буква {letter} освобождена от осиротевшего тома."));
        Ok(())
    } else {
        Err(format!("Буква {letter} занята посторонним томом. Диск не создан."))
    }
}
