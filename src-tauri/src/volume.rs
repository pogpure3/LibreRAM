// SPDX-License-Identifier: GPL-2.0-only
//! WinAPI helpers: memory status, volumes, drive letters, folders.
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceW, GetDiskFreeSpaceExW, GetLogicalDrives, GetVolumeInformationW,
    SetVolumeLabelW,
};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn pcw(buffer: &[u16]) -> PCWSTR {
    PCWSTR(buffer.as_ptr())
}

#[derive(serde::Serialize)]
pub struct Memory {
    pub avail_bytes: u64,
    pub total_bytes: u64,
}

pub fn memory_info() -> Result<Memory, String> {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: status points at a valid struct of the declared length.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok.is_err() {
        return Err("Не удалось получить сведения об оперативной памяти.".into());
    }
    Ok(Memory { avail_bytes: status.ullAvailPhys, total_bytes: status.ullTotalPhys })
}

/// Drive letters A:..Z: not present in the system drive mask.
pub fn free_letters() -> Vec<String> {
    let mask = unsafe { GetLogicalDrives() };
    (0..26u32)
        .filter(|bit| mask >> bit & 1 == 0)
        .map(|bit| format!("{}:", (b'A' + bit as u8) as char))
        .collect()
}

#[derive(serde::Serialize)]
pub struct VolumeInfo {
    pub filesystem: String,
    pub usage: String,
    pub label: String,
}

pub fn volume_info(letter: &str) -> VolumeInfo {
    if letter == "—" {
        return VolumeInfo { filesystem: "—".into(), usage: "—".into(), label: String::new() };
    }
    let root = wide(&format!("{letter}\\"));
    let mut fs = [0u16; 64];
    let mut label = [0u16; 261];
    let ok = unsafe {
        GetVolumeInformationW(
            pcw(&root),
            Some(&mut label),
            None,
            None,
            None,
            Some(&mut fs),
        )
    };
    let filesystem = if ok.is_ok() { string_from(&fs) } else { "—".to_string() };
    let usage = {
        let mut total: u64 = 0;
        let mut free: u64 = 0;
        let ok = unsafe {
            GetDiskFreeSpaceExW(pcw(&root), None, Some(&mut total), Some(&mut free))
        };
        if ok.is_ok() && total > 0 {
            format!("{}%", (total - free) * 100 / total)
        } else {
            "—".into()
        }
    };
    VolumeInfo { filesystem, usage, label: string_from(&label) }
}

pub fn cluster_size(letter: &str) -> Option<u32> {
    if letter == "—" {
        return None;
    }
    let root = wide(&format!("{letter}\\"));
    let mut sectors = 0u32;
    let mut bytes = 0u32;
    let mut free = 0u32;
    let mut total = 0u32;
    let ok = unsafe {
        GetDiskFreeSpaceW(pcw(&root), Some(&mut sectors), Some(&mut bytes), Some(&mut free), Some(&mut total))
    };
    ok.ok().map(|_| sectors * bytes)
}

pub fn set_label(letter: &str, label: &str) -> Result<(), String> {
    validate_letter(letter)?;
    let root = wide(&format!("{letter}\\"));
    let empty = [0u16];
    let value = if label.is_empty() { PCWSTR(empty.as_ptr()) } else { pcw(&wide(label)) };
    let ok = unsafe { SetVolumeLabelW(pcw(&root), value) };
    ok.map_err(|e| format!("Не удалось установить метку тома: {e}"))
}

pub fn validate_letter(letter: &str) -> Result<(), String> {
    let bytes = letter.as_bytes();
    if bytes.len() == 2 && bytes[0].is_ascii_uppercase() && bytes[1] == b':' {
        Ok(())
    } else {
        Err("Неверная буква диска.".into())
    }
}

pub fn validate_label(label: &str, filesystem: &str) -> Result<(), String> {
    let limit = if filesystem == "FAT32" { 11 } else { 32 };
    if label.len() > limit || label.chars().any(|c| "<>:\"/\\|?*".contains(c) || (c as u32) < 0x20) {
        return Err("Недопустимая метка тома.".into());
    }
    Ok(())
}

/// One safe folder name per item, mirroring backend.parse_folders.
pub fn parse_folders(value: &str) -> Result<Vec<String>, String> {
    let mut folders = Vec::new();
    for name in value.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        let stem = name.split('.').next().unwrap_or("").to_uppercase();
        let digit = stem.as_bytes().last().copied().unwrap_or(0);
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && digit.is_ascii_digit()
                && digit != b'0');
        let bad = name == "." || name == ".."
            || name.ends_with('.') || name.ends_with(' ')
            || name.chars().any(|c| "<>:\"/\\|?*".contains(c) || (c as u32) < 0x20)
            || reserved;
        if bad {
            return Err(format!("Недопустимое имя папки: {name}"));
        }
        folders.push(name.to_string());
    }
    Ok(folders)
}

pub fn create_folders(letter: &str, folders: &[String]) -> Result<(), String> {
    validate_letter(letter)?;
    for name in folders {
        if parse_folders(name).as_deref() != Ok(std::slice::from_ref(name)) {
            return Err("Недопустимое имя папки.".into());
        }
        let path = wide(&format!("{letter}\\{name}"));
        // SAFETY: path is a NUL-terminated UTF-16 buffer.
        let result = unsafe { windows::Win32::Storage::FileSystem::CreateDirectoryW(pcw(&path), None) };
        if result.is_err() {
            let exists = std::path::Path::new(&format!("{letter}\\{name}")).is_dir();
            if !exists {
                return Err(format!("Диск создан, но папка {name} не создана: {}", result.unwrap_err()));
            }
        }
    }
    Ok(())
}

pub(crate) fn string_from(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_single_safe_names() {
        assert_eq!(parse_folders("Temp, Cache").unwrap(), vec!["Temp", "Cache"]);
        for name in ["..", "../outside", r"C:\outside", "NUL", "COM1.txt", "a?b", "x."] {
            assert!(parse_folders(name).is_err(), "{name}");
        }
    }

    #[test]
    fn labels_respect_filesystem_limits() {
        assert!(validate_label("LibreRAM", "NTFS").is_ok());
        assert!(validate_label(&"x".repeat(12), "FAT32").is_err());
        assert!(validate_label(&"x".repeat(32), "NTFS").is_ok());
        assert!(validate_label("bad:name", "NTFS").is_err());
    }

    #[test]
    fn letters_must_be_valid() {
        assert!(validate_letter("R:").is_ok());
        assert!(validate_letter("r:").is_err());
        assert!(validate_letter("R").is_err());
        assert!(validate_letter("RR:").is_err());
    }

    #[test]
    fn free_letters_never_include_taken_ones() {
        let taken = 0b11u32; // A: and B: occupied
        let _ = taken;
        let letters = free_letters();
        assert!(letters.iter().all(|l| l.ends_with(':')));
        assert_eq!(letters.len(), 26.min(letters.len()));
    }
}
