// SPDX-License-Identifier: GPL-2.0-only
//! Save a consistent raw volume image without overwriting existing files.
use std::io::Write;

use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use crate::volume::{pcw, wide};

const FSCTL_LOCK_VOLUME: u32 = 0x90018;
const FSCTL_DISMOUNT_VOLUME: u32 = 0x90083;
const BUFFER_SIZE: usize = 1024 * 1024;

pub fn save_volume(letter: &str, size: u64, destination: &str) -> Result<(), String> {
    let bytes = letter.as_bytes();
    if bytes.len() != 2 || !bytes[0].is_ascii_uppercase() || bytes[1] != b':' || size == 0 || !size.is_multiple_of(512) {
        return Err("Неверная буква диска или размер тома.".into());
    }
    let destination = std::path::PathBuf::from(destination);
    let drive_text = destination
        .parent()
        .and_then(|p| std::fs::canonicalize(p).ok())
        .or_else(|| std::fs::canonicalize(&destination).ok())
        .unwrap_or_else(|| destination.clone())
        .to_string_lossy()
        .to_uppercase();
    let same = drive_text.starts_with(&format!(r"\\?\{letter}")) || drive_text.starts_with(letter);
    if same {
        return Err("Нельзя сохранить образ на том же диске.".into());
    }
    if destination.exists() {
        return Err("Файл уже существует. Выберите новое имя.".into());
    }
    let device = wide(&format!(r"\\.\{letter}"));
    // SAFETY: standard raw volume read; handle closed on all paths below.
    let handle = unsafe {
        CreateFileW(
            pcw(&device),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    let result = read_volume(handle, size, &destination);
    unsafe { let _ = CloseHandle(handle); }
    result
}

fn read_volume(handle: HANDLE, size: u64, destination: &std::path::Path) -> Result<(), String> {
    let mut returned = 0u32;
    // SAFETY: valid handle, no overlapped IO.
    let lock = unsafe { DeviceIoControl(handle, FSCTL_LOCK_VOLUME, None, 0, None, 0, Some(&mut returned), None) };
    if lock.is_err() {
        return Err("Не удалось заблокировать том. Закройте файлы и повторите попытку.".into());
    }
    if unsafe { DeviceIoControl(handle, FSCTL_DISMOUNT_VOLUME, None, 0, None, 0, Some(&mut returned), None) }.is_err() {
        return Err("Не удалось отключить том.".into());
    }
    let mut temporary = destination.to_path_buf();
    // CREATE_NEW + random suffix: a planted junction cannot be truncated.
    temporary.set_file_name(format!(".libreram-{}-{}.tmp", std::process::id(), std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true)
        .open(&temporary)
        .map_err(|e| format!("Не удалось создать временный файл: {e}"))?;
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let mut remaining = size;
    while remaining > 0 {
        let chunk = buffer.len().min(remaining as usize);
        let mut read = 0u32;
        // SAFETY: ReadFile writes at most the slice length.
        let ok = unsafe { ReadFile(handle, Some(&mut buffer[..chunk]), Some(&mut read), None) };
        if ok.is_err() {
            let _ = std::fs::remove_file(&temporary);
            return Err("Не удалось прочитать том.".into());
        }
        if read as usize != chunk {
            let _ = std::fs::remove_file(&temporary);
            return Err("Том прочитан не полностью; образ не сохранён.".into());
        }
        file.write_all(&buffer[..read as usize]).map_err(|e| {
            let _ = std::fs::remove_file(&temporary);
            e.to_string()
        })?;
        remaining -= read as u64;
    }
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    std::fs::rename(&temporary, destination).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        e.to_string()
    })
}
