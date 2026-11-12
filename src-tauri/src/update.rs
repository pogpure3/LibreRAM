// SPDX-License-Identifier: GPL-2.0-only
//! Check GitHub release metadata for a newer Windows x64 release.
use semver::Version;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::time::Duration;

const RELEASES_API: &str = "https://api.github.com/repos/pogpure3/LibreRAM/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/pogpure3/LibreRAM/releases";

#[derive(Debug, Serialize)]
pub struct UpdateInfo {
    pub version: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    state: String,
}

pub fn check() -> Result<Option<UpdateInfo>, String> {
    let response = ureq::AgentBuilder::new()
        .redirects(0)
        .build()
        .get(RELEASES_API)
        .set("User-Agent", "LibreRAM-updater")
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(15))
        .call();
    check_response(response, env!("CARGO_PKG_VERSION"))
}

fn check_response(
    response: Result<ureq::Response, ureq::Error>,
    current: &str,
) -> Result<Option<UpdateInfo>, String> {
    let response = match response {
        Ok(response) if response.status() == 200 => response,
        Ok(response) => {
            return Err(format!("Неожиданный ответ GitHub: HTTP {}.", response.status()));
        }
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(error) => return Err(format!("Не удалось связаться с GitHub: {error}")),
    };
    let release: Release = response
        .into_json()
        .map_err(|error| format!("Не удалось прочитать ответ GitHub: {error}"))?;
    if release.draft || release.prerelease {
        return Ok(None);
    }

    let tag = release.tag_name.strip_prefix('v').unwrap_or(&release.tag_name);
    let candidate = Version::parse(tag)
        .map_err(|_| "GitHub вернул недопустимый номер версии.".to_string())?;
    let current = Version::parse(current)
        .map_err(|_| "Не удалось определить текущую версию LibreRAM.".to_string())?;
    if !candidate.pre.is_empty() || candidate.cmp_precedence(&current) != Ordering::Greater {
        return Ok(None);
    }

    let name = format!("LibreRAM-{candidate}-preview-x64-tauri.msi");
    let available = release
        .assets
        .iter()
        .any(|asset| asset.name == name && asset.state == "uploaded");
    Ok(available.then(|| UpdateInfo {
        version: candidate.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const CURRENT: &str = "0.3.18";

    fn release(tag: &str) -> Value {
        let version = tag.trim_start_matches('v');
        json!({
            "tag_name": tag,
            "draft": false,
            "prerelease": false,
            "assets": [{
                "name": format!("LibreRAM-{version}-preview-x64-tauri.msi"),
                "state": "uploaded",
                "browser_download_url": "https://example.invalid/installer.msi"
            }]
        })
    }

    fn check_release(release: Value) -> Result<Option<UpdateInfo>, String> {
        check_response(ureq::Response::new(200, "OK", &release.to_string()), CURRENT)
    }

    #[test]
    fn finds_newer_release_and_returns_only_its_version() {
        let update = check_release(release("v0.3.19")).unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(update).unwrap(),
            json!({ "version": "0.3.19" })
        );
    }

    #[test]
    fn ignores_equal_older_and_prerelease_versions() {
        for tag in ["v0.3.18", "v0.3.9", "v0.3.19-rc.1", "v0.3.18+rebuild"] {
            assert!(check_release(release(tag)).unwrap().is_none(), "{tag}");
        }
        assert_eq!(
            check_release(release("0.3.100")).unwrap().unwrap().version,
            "0.3.100"
        );
    }

    #[test]
    fn ignores_drafts_and_releases_marked_as_prerelease() {
        for field in ["draft", "prerelease"] {
            let mut unpublished = release("v0.3.19");
            unpublished[field] = json!(true);
            assert!(check_release(unpublished).unwrap().is_none(), "{field}");
        }
    }

    #[test]
    fn requires_uploaded_tauri_x64_msi_for_the_release_version() {
        let mut candidate = release("v0.3.19");
        candidate["assets"] = json!([
            { "name": "LibreRAM-0.3.19-preview-x64-tauri.msi", "state": "starter" },
            { "name": "LibreRAM-0.3.19-preview-arm64-tauri.msi", "state": "uploaded" },
            { "name": "LibreRAM-0.3.18-preview-x64-tauri.msi", "state": "uploaded" }
        ]);
        assert!(check_release(candidate).unwrap().is_none());
    }

    #[test]
    fn rejects_malformed_tags_and_metadata() {
        assert!(check_release(release("v0.4.invalid")).is_err());
        assert!(check_release(json!({ "tag_name": "v0.3.19" })).is_err());
        assert!(check_response(ureq::Response::new(200, "OK", "invalid JSON"), CURRENT).is_err());
    }

    #[test]
    fn distinguishes_no_release_from_github_errors() {
        let response = ureq::Response::new(404, "Not Found", "{}").unwrap();
        assert!(check_response(Err(ureq::Error::Status(404, response)), CURRENT)
            .unwrap()
            .is_none());
        let response = ureq::Response::new(503, "Service Unavailable", "{}").unwrap();
        assert!(check_response(Err(ureq::Error::Status(503, response)), CURRENT).is_err());
    }
}
