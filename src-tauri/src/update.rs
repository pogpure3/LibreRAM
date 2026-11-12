// SPDX-License-Identifier: GPL-2.0-only
//! Update check + install from GitHub releases (latest .msi asset).
use serde::Serialize;
use std::os::windows::process::CommandExt;

const RELEASES_API: &str = "https://api.github.com/repos/pogpure3/LibreRAM/releases/latest";
const USER_AGENT: &str = "LibreRAM-updater";

#[derive(Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub msi_url: String,
}

/// Compare dotted versions: newer(a, b) = a > b.
fn newer(candidate: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v')
            .split('-')
            .next()
            .unwrap_or("")
            .split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(candidate), parse(current));
    for i in 0..a.len().max(b.len()) {
        match (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0)) {
            (x, y) if x != y => return x > y,
            _ => {}
        }
    }
    false
}

/// Latest published release with a newer version and a downloadable .msi.
pub fn check() -> Result<Option<UpdateInfo>, String> {
    let current = env!("CARGO_PKG_VERSION");
    let response = ureq::get(RELEASES_API)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| format!("Не удалось связаться с GitHub: {e}"))?;
    let release: serde_json::Value = response
        .into_json()
        .map_err(|e| format!("Не удалось прочитать ответ GitHub: {e}"))?;
    if release.get("draft").map(|v| v.as_bool().unwrap_or(true)).unwrap_or(true) {
        return Ok(None);
    }
    let tag = release["tag_name"].as_str().unwrap_or("");
    let version = tag.trim_start_matches('v');
    if !newer(version, current) {
        return Ok(None);
    }
    let assets = release["assets"].as_array().cloned().unwrap_or_default();
    let msi = assets
        .iter()
        .filter(|a| a["name"].as_str().map(|n| n.to_lowercase().ends_with(".msi")).unwrap_or(false))
        .min_by_key(|a| a["name"].as_str().unwrap_or("").to_lowercase())
        .and_then(|a| a["browser_download_url"].as_str())
        .map(str::to_string);
    Ok(msi.map(|msi_url| UpdateInfo { version: version.to_string(), msi_url }))
}

/// Download the MSI to a temp file, then hand off to msiexec and exit the app
/// so the upgrade does not fight its own running binary.
pub fn download_and_install(url: &str) -> Result<std::path::PathBuf, String> {
    let response = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .timeout(std::time::Duration::from_secs(600))
        .call()
        .map_err(|e| format!("Не удалось скачать обновление: {e}"))?;
    let file_name = url.rsplit('/').next().unwrap_or("LibreRAM-setup.msi");
    let temp = std::env::temp_dir().join(format!("libreram-update-{file_name}"));
    let mut reader = response.into_reader();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| format!("Не удалось создать файл обновления: {e}"))?;
    std::io::copy(&mut reader, &mut file).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("Загрузка прервалась: {e}")
    })?;
    Ok(temp)
}

/// Launch msiexec detached; after it finishes, the freshly installed exe starts.
/// The app exits right after spawning, so the MajorUpgrade replaces files freely.
pub fn run_installer(msi: &std::path::Path) -> Result<(), String> {
    let cmd = std::path::PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
        .join(r"System32\cmd.exe");
    let target = relaunch_target();
    // Inline `cmd /c start ... && start ...` dies on cmd's outer-quote
    // stripping ("Windows cannot find \"); a temp .cmd sidesteps quoting.
    let script = std::env::temp_dir().join("libreram-update.cmd");
    std::fs::write(
        &script,
        format!(
            "@echo off\r\ndel \"%~f0\" >nul 2>&1\r\nstart \"\" /wait msiexec /i \"{}\" /qb\r\nstart \"\" \"{}\"\r\n",
            msi.to_string_lossy(),
            target.to_string_lossy(),
        ),
    )
    .map_err(|e| format!("Не удалось записать сценарий обновления: {e}"))?;
    std::process::Command::new(cmd)
        .args(["/c", &format!("\"{}\"", script.to_string_lossy()), msi.to_string_lossy().as_ref(), target.to_string_lossy().as_ref()])
        .creation_flags(0x08000008) // CREATE_NO_WINDOW | CREATE_DETACHED
        .spawn()
        .map_err(|e| format!("Не удалось запустить установку: {e}"))?;
    Ok(())
}

/// Where the updated copy will live: same dir for an installed copy,
/// Program Files\LibreRAM for portable ones (MSI's default INSTALLFOLDER).
fn relaunch_target() -> std::path::PathBuf {
    let exe = std::env::current_exe().ok();
    if let Some(dir) = exe.as_ref().and_then(|e| e.parent()) {
        if crate::session::installed() {
            return dir.join("LibreRAM.exe");
        }
    }
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    std::path::PathBuf::from(pf).join(r"LibreRAM\LibreRAM.exe")
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn version_compare() {
        assert!(newer("0.3.2", "0.3.1"));
        assert!(newer("v0.4.0", "0.3.10"));
        assert!(newer("1.0", "0.9.9"));
        assert!(!newer("0.3.1", "0.3.1"));
        assert!(!newer("0.2.9", "0.3.1"));
    }
}
