// SPDX-License-Identifier: GPL-2.0-only
//! LibreRAM — RAM disk manager over the ImDisk CLI (Tauri frontend).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod imdisk;
mod save_image;
mod session;
mod volume;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
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

#[tauri::command]
fn list_disks() -> Result<Vec<DiskRow>, String> {
    let rows = imdisk::disks()?
        .into_iter()
        .map(|disk| {
            let info = volume::volume_info(&disk.letter);
            let kind = disk.description.lines().last().unwrap_or("").to_string();
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
fn create_disk(request: imdisk::CreateRequest, folders: String, label: String, session_auto: bool) -> Result<(), String> {
    let folders = volume::parse_folders(&folders)?;
    let filesystem = request.filesystem.clone();
    imdisk::create(&request)?;
    if !label.is_empty() {
        volume::set_label(&request.letter, &label)?;
    }
    volume::create_folders(&request.letter, &folders)?;
    if session_auto {
        if request.image.as_deref().map(str::trim).filter(|s| !s.is_empty()).is_some() || request.persistent {
            return Err("Для чистого диска при входе уберите образ и восстановление из образа.".into());
        }
        session::enable(&session::SessionConfig {
            letter: request.letter,
            size_mib: request.size_mib,
            filesystem,
            allocation_size: request.allocation_size,
            folders,
            label,
        })?;
    }
    Ok(())
}

#[tauri::command]
fn remove_disk(unit: u32, remove_persistent: bool) -> Result<(), String> {
    imdisk::remove(unit, remove_persistent)
}

#[tauri::command]
fn disk_properties(unit: u32) -> Result<serde_json::Value, String> {
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
fn save_image(letter: String, size: u64, destination: String) -> Result<(), String> {
    save_image::save_volume(&letter, size, &destination)
}

#[tauri::command]
fn session_load() -> Result<serde_json::Value, String> {
    match session::load() {
        Ok(config) => serde_json::to_value(config).map_err(|e| e.to_string()),
        Err(e) if e == "нет сохранённых параметров" => Ok(serde_json::json!(null)),
        Err(e) => Err(e),
    }
}

#[tauri::command]
fn session_enable(config: session::SessionConfig) -> Result<(), String> {
    session::enable(&config)
}

#[tauri::command]
fn session_disable() -> Result<(), String> {
    session::disable()
}

#[tauri::command]
fn imdisk_present() -> bool {
    imdisk::imdisk_path().is_ok()
}

#[tauri::command]
fn install_imdisk(app: AppHandle) -> Result<(), String> {
    let resources = app.path().resource_dir().map_err(|e| e.to_string())?;
    let setup = resources.join("resources/imdisk-setup");
    if !setup.join("install.cmd").is_file() {
        return Err("Установочные файлы ImDisk не найдены в пакете.".into());
    }
    let output = std::process::Command::new("cmd")
        .args(["/c", "set IMDISK_SILENT_SETUP=1&& call install.cmd"])
        .current_dir(&setup)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() && imdisk::imdisk_path().is_ok() {
        return Ok(());
    }
    Err("Установка ImDisk не завершилась. Запустите install.cmd из каталога imdisk-setup вручную.".into())
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
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            list_disks, memory_info, free_letters, create_disk, remove_disk, disk_properties,
            change_label, save_image, session_load, session_enable, session_disable,
            imdisk_present, install_imdisk,
        ])
        .setup(move |app| {
            let window = app.get_webview_window("main").unwrap();
            let open_item = MenuItem::with_id(app, "open", "Открыть LibreRAM", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&open_item, &quit_item])?;
            let tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&tray_menu)
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
                    if matches!(event, TrayIconEvent::Click { .. }) {
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
