// SPDX-License-Identifier: GPL-2.0-only
//! ImDisk CLI wrapper: list, create, remove, persistence checks.
use serde::Serialize;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use windows::core::{s, w, BOOL};
use windows::Win32::Foundation::{FreeLibrary, ERROR_SUCCESS, HMODULE};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
use windows::Win32::System::Registry::{RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_VALUE_TYPE};
use crate::volume::{pcw, string_from, validate_label, validate_letter, wide};

const CREATE_NO_WINDOW: u32 = 0x08000000;
const IMDISK_DEVICE_INACCESSIBLE: i32 = 2;

#[derive(Serialize, Clone)]
pub struct Disk {
    pub unit: u32,
    pub letter: String,
    pub size: u64,
    pub description: String,
}

pub fn imdisk_path() -> Result<PathBuf, String> {
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let path = PathBuf::from(system).join(r"System32\imdisk.exe");
    if path.is_file() {
        Ok(path)
    } else {
        Err("Не найден imdisk.exe в системном каталоге Windows. Установите утилиты ImDisk.".into())
    }
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Decode console output: CP866 Cyrillic on a ru-RU system, ASCII otherwise.
pub(crate) fn decode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| match b {
        0x80..=0xAF => char::from_u32(0x0410 + (b - 0x80) as u32).unwrap_or('?'),
        0xE0..=0xEF => char::from_u32(0x0440 + (b - 0xE0) as u32).unwrap_or('?'),
        0xF0 => 'Ё',
        0xF1 => 'ё',
        _ => *b as char,
    }).collect()
}

fn run(args: &[&str]) -> Result<Output, String> {
    let exe = imdisk_path()?;
    let output = std::process::Command::new(&exe)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    Ok(Output {
        code: output.status.code().unwrap_or(-1),
        stdout: decode(&output.stdout),
        stderr: decode(&output.stderr),
    })
}

fn fail(out: &Output, list_query: bool) -> Result<String, String> {
    let text = format!("{}\n{}", out.stdout.trim(), out.stderr.trim());
    let list_success = list_query && out.code == 1 && out.stderr.trim().is_empty();
    if out.stderr.to_lowercase().contains("registry edit failed") {
        return Err(format!("Диск мог быть создан, но настройки автозапуска не сохранены. Обновите список.\n{text}"));
    }
    if (out.code != 0 && !list_success) || text.to_lowercase().contains("driver is not loaded") {
        return Err(if text.trim().is_empty() { format!("ImDisk завершился с кодом {}.", out.code) } else { text });
    }
    Ok(out.stdout.clone())
}

fn disk_units() -> Result<Vec<u32>, String> {
    let out = run(&["-l", "-n"])?;
    let stdout = fail(&out, true)?;
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| l.parse::<u32>().map_err(|_| format!("Неожиданный ответ ImDisk: {stdout}")))
        .collect()
}

pub fn disks() -> Result<Vec<Disk>, String> {
    let disks: Vec<Disk> = disk_units()?
        .iter()
        .filter_map(|u| match parse_disk(*u) {
            Ok(disk) => Some(disk),
            Err(error) => {
                session_log(&format!("Устройство {u} пропущено: {error}"));
                None
            }
        })
        .collect();
    Ok(disks)
}

// Left a per-unit failure drop the whole list once; one vanished unit must
// not blank the UI, so misses degrade into a log line instead.
fn session_log(message: &str) {
    crate::session::log_event(message)
}

fn parse_disk(unit: u32) -> Result<Disk, String> {
    let out = run(&["-l", "-u", &unit.to_string()])?;
    let text = fail(&out, false)?;
    parse_device(unit, &text)
}

fn parse_device(unit: u32, text: &str) -> Result<Disk, String> {
    let mut letter = "—".to_string();
    let mut size = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Drive letter:").or(trimmed.strip_prefix("Mount point:")) {
            if let Some(l) = rest.trim().chars().next() {
                if l.is_ascii_uppercase() {
                    letter = format!("{l}:");
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("Size:") {
            if let Some(bytes) = rest.split_whitespace().next().and_then(|v| v.parse::<u64>().ok()) {
                size = Some(bytes);
            }
        }
    }
    match size {
        Some(size) => Ok(Disk { unit, letter, size, description: text.trim().to_string() }),
        None => Err(format!("Не удалось прочитать параметры устройства {unit}.")),
    }
}

/// Registry FileNameN value for a persistent image, if configured.
pub fn persistent_image(unit: u32) -> Option<String> {
    let key_name = wide(r"SYSTEM\CurrentControlSet\Services\ImDisk\Parameters");
    let value_name = wide(&format!("FileName{unit}"));
    let mut key = HKEY::default();
    // SAFETY: standard registry read of an HKLM value.
    unsafe {
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, pcw(&key_name), None, KEY_READ, &mut key) != ERROR_SUCCESS {
            return None;
        }
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0u32;
        let status = RegQueryValueExW(key, pcw(&value_name), None, Some(&mut kind), None, Some(&mut size));
        if status != ERROR_SUCCESS || size == 0 {
            let _ = windows::Win32::System::Registry::RegCloseKey(key);
            return None;
        }
        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        let status = RegQueryValueExW(
            key, pcw(&value_name), None, None,
            Some(buffer.as_mut_ptr().cast()), Some(&mut size),
        );
        let _ = windows::Win32::System::Registry::RegCloseKey(key);
        if status != ERROR_SUCCESS {
            return None;
        }
        let value = string_from(&buffer);
        Some(value.trim_start_matches("\\??\\").to_string())
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRequest {
    pub letter: String,
    pub size_mib: u64,
    pub filesystem: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub persistent: bool,
    #[serde(default)]
    pub allocation_size: u32,
}

pub fn create(request: &CreateRequest) -> Result<(), String> {
    validate_letter(&request.letter)?;
    if !(1..=1048576).contains(&request.size_mib) {
        return Err("Размер диска должен составлять от 1 до 1 048 576 МиБ.".into());
    }
    if !matches!(request.filesystem.as_str(), "NTFS" | "FAT32" | "exFAT") {
        return Err("Неизвестная файловая система.".into());
    }
    if ![0, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536].contains(&request.allocation_size) {
        return Err("Недопустимый размер единицы распределения.".into());
    }
    let image = request.image.as_deref().filter(|s| !s.trim().is_empty());
    if image.is_some() && request.allocation_size != 0 {
        return Err("Размер единицы распределения нельзя задать для существующего образа.".into());
    }
    if request.persistent && image.is_none() {
        return Err("Для автозапуска нужен существующий образ.".into());
    }
    if std::path::Path::new(&format!("{}\\", request.letter)).exists() {
        return Err("Буква диска уже занята.".into());
    }
    let size = format!("{}M", request.size_mib);
    let mut args: Vec<String> = vec![
        "-a".into(), "-t".into(), "vm".into(), "-m".into(), request.letter.clone(), "-s".into(), size,
    ];
    match image {
        Some(path) => {
            let path = PathBuf::from(path);
            if !path.is_file() {
                return Err("Файл образа не найден.".into());
            }
            let disk_bytes = request
                .size_mib
                .checked_mul(1024 * 1024)
                .ok_or("Слишком большой размер диска.".to_string())?;
            if path.metadata().map_err(|e| e.to_string())?.len() > disk_bytes {
                return Err("Размер образа превышает размер диска.".into());
            }
            args.push("-f".into());
            args.push(std::path::absolute(&path).map_err(|e| e.to_string())?.to_string_lossy().into_owned());
        }
        None => {
            let allocation = if request.allocation_size == 0 {
                String::new()
            } else {
                format!(" /a:{}", request.allocation_size)
            };
            args.push("-p".into());
            args.push(format!("/fs:{} /q /y{}", request.filesystem, allocation));
        }
    }
    if request.persistent {
        args.push("-P".into());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run(&refs)?;
    fail(&out, false)?;
    if request.persistent {
        let expected = image
            .map(|p| std::path::absolute(p).map_err(|e| e.to_string()))
            .transpose()?
            .map(|p| p.to_string_lossy().to_lowercase());
        let registered = disks()?
            .iter()
            .find(|d| d.letter == request.letter)
            .and_then(|d| persistent_image(d.unit))
            .map(|p| p.to_lowercase());
        match (expected, registered) {
            (Some(expected), Some(registered)) if expected == registered => {}
            _ => return Err("Диск мог быть создан, но автозапуск не подтверждён. Обновите список.".into()),
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct DetachError {
    message: String,
    can_force: bool,
}

impl From<String> for DetachError {
    fn from(message: String) -> Self {
        Self {
            message,
            can_force: false,
        }
    }
}

fn detach_status(out: &Output, force: bool) -> Result<(), DetachError> {
    fail(out, false)
        .map(|_| ())
        .map_err(|message| DetachError {
            message,
            can_force: !force
                && out.code == IMDISK_DEVICE_INACCESSIBLE
                && out.stdout.lines().last() == Some("Locking volume..."),
        })
}

type RemoveRegistrySettings = unsafe extern "system" fn(u32) -> BOOL;

struct Persistence {
    module: HMODULE,
    remove_settings: RemoveRegistrySettings,
}

impl Persistence {
    fn load() -> Result<Self, String> {
        // SAFETY: Load the ImDisk API only from the Windows system directory.
        let module = unsafe { LoadLibraryExW(w!("imdisk.cpl"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
            .map_err(|e| format!("Не удалось открыть параметры автозапуска ImDisk: {e}"))?;
        let remove_settings = unsafe { GetProcAddress(module, s!("ImDiskRemoveRegistrySettings")) };
        let Some(remove_settings) = remove_settings else {
            unsafe {
                let _ = FreeLibrary(module);
            }
            return Err(
                "В установленной версии ImDisk недоступно удаление параметров автозапуска.".into(),
            );
        };
        // SAFETY: The named ImDisk export is BOOL WINAPI ImDiskRemoveRegistrySettings(DWORD).
        let remove_settings = unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, RemoveRegistrySettings>(
                remove_settings,
            )
        };
        Ok(Self {
            module,
            remove_settings,
        })
    }

    fn remove(&self, unit: u32) -> Result<(), String> {
        // SAFETY: The module and its export remain loaded until self is dropped.
        if !unsafe { (self.remove_settings)(unit) }.as_bool() || persistent_image(unit).is_some() {
            return Err("Диск отключён, но настройка автозапуска ImDisk не удалена.".into());
        }
        Ok(())
    }
}

impl Drop for Persistence {
    fn drop(&mut self) {
        // SAFETY: The module is owned by this instance and freed once.
        unsafe {
            let _ = FreeLibrary(self.module);
        }
    }
}

pub fn remove(unit: u32, remove_persistent: bool, force: bool) -> Result<(), DetachError> {
    let persistence = remove_persistent.then(Persistence::load).transpose()?;
    let mode = if force { "-D" } else { "-d" };
    let out = run(&[mode, "-u", &unit.to_string()])?;
    detach_status(&out, force)?;
    if let Some(persistence) = persistence {
        // ImDisk's -P removes settings before it tries to lock the volume.
        let units =
            disk_units().map_err(|e| format!("Не удалось подтвердить отключение диска: {e}"))?;
        if units.contains(&unit) {
            return Err(
                "Устройство остаётся в списке ImDisk. Обновите список дисков."
                    .to_string()
                    .into(),
            );
        }
        persistence.remove(unit)?;
    }
    Ok(())
}

pub fn validate_session_config(config: &serde_json::Value) -> Result<crate::session::SessionConfig, String> {
    let config: crate::session::SessionConfig =
        serde_json::from_value(config.clone()).map_err(|e| format!("Неверный формат параметров автозапуска. {e}"))?;
    validate_letter(&config.letter)?;
    if !(1..=1048576).contains(&config.size_mib) {
        return Err("Размер диска должен составлять от 1 до 1 048 576 МиБ.".into());
    }
    if !matches!(config.filesystem.as_str(), "NTFS" | "FAT32" | "exFAT")
        || ![0, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536].contains(&config.allocation_size)
    {
        return Err("Недопустимые параметры файловой системы.".into());
    }
    if config.folders.len() > 100
        || config.folders.iter().any(|name| crate::volume::parse_folders(name).as_deref() != Ok(std::slice::from_ref(name)))
    {
        return Err("Недопустимый список папок.".into());
    }
    validate_label(&config.label, &config.filesystem)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_letter_size_and_description() {
        let text = "Parameter name: Type\r\nDrive letter: R\r\nSize: 1073741824 bytes (1 GB), Virtual Memory, HDD.\r\n\r\nSize: 1073741824 bytes (1 GB), Virtual Memory, HDD.";
        let disk = parse_device(3, text).unwrap();
        assert_eq!(disk.unit, 3);
        assert_eq!(disk.letter, "R:");
        assert_eq!(disk.size, 1073741824);
        assert!(disk.description.contains("Virtual Memory"));
    }

    #[test]
    fn keeps_dash_when_no_letter() {
        let text = "Size: 67108864 bytes (64 MB), Virtual Memory, HDD.";
        assert_eq!(parse_device(1, text).unwrap().letter, "—");
    }

    #[test]
    fn missing_size_fails_closed() {
        assert!(parse_device(1, "Drive letter: R").is_err());
    }

    #[test]
    fn force_is_offered_only_after_a_normal_lock_failure() {
        let mut output = Output {
            code: 2,
            stdout: "Flushing file buffers...\r\nLocking volume...\r\n".into(),
            stderr: "Volume is busy".into(),
        };
        assert!(detach_status(&output, false).unwrap_err().can_force);
        assert!(!detach_status(&output, true).unwrap_err().can_force);
        output.stdout.clear();
        assert!(!detach_status(&output, false).unwrap_err().can_force);
    }

    #[test]
    fn session_rejects_invalid_settings() {
        let mut value = serde_json::json!({
            "letter": "O:", "size_mib": 1024, "filesystem": "NTFS",
            "allocation_size": 0, "folders": ["Temp"], "label": ""
        });
        assert!(validate_session_config(&value).is_ok());
        value["letter"] = serde_json::json!("oo");
        assert!(validate_session_config(&value).is_err());
        value["letter"] = serde_json::json!("O:");
        value["size_mib"] = serde_json::json!(0);
        assert!(validate_session_config(&value).is_err());
        value["size_mib"] = serde_json::json!(1024);
        value["filesystem"] = serde_json::json!("ext4");
        assert!(validate_session_config(&value).is_err());
        value["filesystem"] = serde_json::json!("NTFS");
        value["folders"] = serde_json::json!(["a?b"]);
        assert!(validate_session_config(&value).is_err());
    }

}
