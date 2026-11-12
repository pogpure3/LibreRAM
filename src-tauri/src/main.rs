// SPDX-License-Identifier: GPL-2.0-only
//! LibreRAM — RAM disk manager over the ImDisk CLI (Tauri frontend).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod imdisk;
mod save_image;
mod session;
mod update;
mod volume;

use serde::Serialize;
use std::os::windows::process::CommandExt;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize)]
struct DiskRow {
    unit: u32,
    letter: String,
    size: u64,
    size_text: String,
    filesystem: String,
    usage: String,
    kind: String,
    persistent: bool,
}

/// ImDisk prints "Size: 1073741824 bytes (1 GB), Virtual Memory, HDD." —
/// keep only the bracketed value, localized ("1 ГБ").
fn imdisk_size_label(description: &str) -> String {
    let line = description.lines().find(|l| l.contains("Size:")).unwrap_or("");
    match (line.find('('), line.find(')')) {
        (Some(start), Some(end)) if end > start => {
            let value = &line[start + 1..end];
            value
                .replace("TB", "ТБ")
                .replace("GB", "ГБ")
                .replace("MB", "МБ")
                .replace("KB", "КБ")
        }
        _ => line.trim().to_string(),
    }
}

#[tauri::command]
async fn list_disks() -> Result<Vec<DiskRow>, String> {
    let rows = imdisk::disks()?
        .into_iter()
        .map(|disk| {
            let info = volume::volume_info(&disk.letter);
            let kind = imdisk_size_label(&disk.description);
            let persistent = imdisk::persistent_image(disk.unit).is_some();
            DiskRow {
                unit: disk.unit,
                letter: disk.letter.clone(),
                size: disk.size,
                size_text: format!("{:.1} МиБ", disk.size as f64 / 1024.0 / 1024.0),
                filesystem: info.filesystem,
                usage: info.usage,
                kind,
                persistent,
            }
        })
        .collect();
    Ok(rows)
}

#[tauri::command]
fn memory_info() -> Result<volume::Memory, String> {
    volume::memory_info()
}

#[tauri::command]
fn free_letters() -> Vec<String> {
    volume::free_letters()
}

#[tauri::command]
fn letter_states() -> Vec<volume::LetterState> {
    volume::letter_states()
}

#[tauri::command]
async fn create_disk(request: imdisk::CreateRequest, folders: String, label: String, session_auto: bool) -> Result<(), String> {
    let folders = volume::parse_folders(&folders)?;
    let filesystem = request.filesystem.clone();
    if session_auto
        && (request.image.as_deref().map(str::trim).filter(|s| !s.is_empty()).is_some() || request.persistent)
    {
        return Err("Для чистого диска при входе уберите образ и восстановление из образа.".into());
    }
    imdisk::create(&request)?;
    let created = format!("Диск {} создан, но настройки не завершены: ", request.letter);
    if !label.is_empty() {
        volume::set_label(&request.letter, &label).map_err(|e| created.clone() + &e)?;
    }
    volume::create_folders(&request.letter, &folders).map_err(|e| created.clone() + &e)?;
    if session_auto {
        // Append to the existing list; a second logon disk must not silently
        // drop the first one.
        let mut disks = session::load().map(|file| file.all().to_vec()).unwrap_or_default();
        disks.retain(|disk| disk.letter != request.letter);
        disks.push(session::SessionConfig {
            letter: request.letter,
            size_mib: request.size_mib,
            filesystem,
            allocation_size: request.allocation_size,
            folders,
            label,
        });
        session::enable(&disks)?;
    }
    Ok(())
}

#[tauri::command]
async fn remove_disk(unit: u32, remove_persistent: bool) -> Result<(), String> {
    imdisk::remove(unit, remove_persistent)
}

#[tauri::command]
async fn disk_properties(unit: u32) -> Result<serde_json::Value, String> {
    let disk = imdisk::disks()?
        .into_iter()
        .find(|d| d.unit == unit)
        .ok_or("Диск больше не подключён.")?;
    let info = volume::volume_info(&disk.letter);
    let cluster = volume::cluster_size(&disk.letter);
    Ok(serde_json::json!({
        "letter": disk.letter,
        "size": disk.size,
        "filesystem": info.filesystem,
        "usage": info.usage,
        "cluster": cluster.map(|c| format!("{c} байт")).unwrap_or_else(|| "—".into()),
        "persistent": imdisk::persistent_image(unit).is_some(),
        "description": disk.description,
    }))
}

#[tauri::command]
fn change_label(letter: String, label: String) -> Result<(), String> {
    volume::set_label(&letter, &label)
}

#[tauri::command]
async fn save_image(letter: String, size: u64, destination: String) -> Result<(), String> {
    save_image::save_volume(&letter, size, &destination)
}

#[tauri::command]
fn session_load() -> Result<serde_json::Value, String> {
    match session::load() {
        Ok(file) => serde_json::to_value(file.all()).map_err(|e| e.to_string()),
        Err(e) if e == "нет сохранённых параметров" => Ok(serde_json::json!(null)),
        Err(e) => Err(e),
    }
}

#[tauri::command]
async fn session_enable(disks: Vec<session::SessionConfig>) -> Result<(), String> {
    session::enable(&disks)
}

#[tauri::command]
async fn session_disable() -> Result<(), String> {
    session::disable()
}

#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    // Elevated process: only drive roots are a legitimate target for this UI.
    let bytes = path.as_bytes();
    let is_drive_root = bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/');
    if !is_drive_root || !std::path::Path::new(&path).is_dir() {
        return Err("Открывать можно только корень подключённого диска.".into());
    }
    let file = crate::volume::wide(&path);
    // SAFETY: ShellExecuteW only reads the NUL-terminated path.
    let result = unsafe {
        ShellExecuteW(None, PCWSTR::null(), PCWSTR(file.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL)
    };
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!("Не удалось открыть {}.", path))
    }
}

#[tauri::command]
async fn check_update() -> Result<Option<update::UpdateInfo>, String> {
    update::check()
}

#[tauri::command]
async fn install_update(url: String, app: tauri::AppHandle) -> Result<(), String> {
    let msi = update::download_and_install(&url, &app)?;
    update::run_installer(&msi)?;
    app_handle_exit();
    Ok(())
}

// The upgrade replaces this binary; exit before msiexec touches it.
fn app_handle_exit() {
    if let Some(app) = TAURI_APP.get() {
        app.exit(0);
    }
}

pub static TAURI_APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

#[tauri::command]
fn imdisk_present() -> bool {
    imdisk::imdisk_path().is_ok()
}

#[tauri::command]
async fn install_imdisk(app: AppHandle) -> Result<(), String> {
    let resources = app.path().resource_dir().map_err(|e| e.to_string())?;
    let setup = resources.join("resources/imdisk-setup");
    let script = setup.join("install-libreram.cmd");
    if !script.is_file() {
        return Err("Установочные файлы ImDisk не найдены в пакете.".into());
    }
    let cmd = std::path::PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
        .join(r"System32\cmd.exe");
    let output = std::process::Command::new(cmd)
        .args(["/c", "call", "install-libreram.cmd"])
        .current_dir(&setup)
        .creation_flags(0x08000000) // CREATE_NO_WINDOW: no console flash
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() && imdisk::imdisk_path().is_ok() {
        return Ok(());
    }
    // Surface the script's own failure line instead of a generic message.
    let text = imdisk::decode(&output.stdout);
    Err(format!(
        "Установка ImDisk не завершилась. {}",
        text.lines().last().unwrap_or("Запустите install-libreram.cmd вручную.")
    ))
}

fn main() {
    let session_start = std::env::args().any(|a| a == "--session-startup");
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
                .invoke_handler(tauri::generate_handler![
            list_disks, memory_info, free_letters, letter_states, create_disk, remove_disk, disk_properties,
            change_label, save_image, session_load, session_enable, session_disable,
            imdisk_present, install_imdisk, open_path, check_update, install_update,
        ])
        .setup(move |app| {
            let _ = TAURI_APP.set(app.handle().clone());
            let window = app.get_webview_window("main").unwrap();
            let open_item = MenuItem::with_id(app, "open", "Открыть LibreRAM", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&open_item, &quit_item])?;
            let tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&tray_menu)
                // Context menu belongs to right click; left click shows the window.
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;
            tray.set_visible(true).map_err(|e| e.to_string())?;

            if session_start {
                // Match the Python version: logon runs hidden and restores the disk.
                window.hide().map_err(|e| e.to_string())?;
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    if let Err(error) = session::restore() {
                        session::log_event(&format!("Ошибка создания диска: {error}"));
                        let _ = handle.get_webview_window("main").map(|w| w.show());
                        let _ = handle.emit("session-error", error);
                    }
                });
            }
            let handle = app.handle().clone();
            window.on_window_event(move |event| {
                // Close hides to tray; Quit lives in the tray menu.
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = handle.get_webview_window("main").map(|w| w.hide());
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running LibreRAM");
}

#[cfg(test)]
mod tests {
    #[test]
    fn size_label_takes_bracketed_value() {
        let description = "Parameter name: Type\r\nDrive letter: R\r\n\r\nSize: 1073741824 bytes (1 GB), Virtual Memory, HDD, Modified.";
        assert_eq!(super::imdisk_size_label(description), "1 ГБ");
        assert_eq!(super::imdisk_size_label("Size: 67108864 bytes (64 MB), Virtual Memory, HDD."), "64 МБ");
        assert_eq!(super::imdisk_size_label("Size: 1152921504606846976 bytes (1 TB), VM."), "1 ТБ");
    }
}
