use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::UpdaterExt;
use tokio::sync::Mutex;

use crate::child_process::background_command;

const RELEASE_URL: &str = "https://github.com/deviffyy/OpenQuota/releases/latest";

#[derive(Default)]
pub struct UpdateCoordinator {
    operation: Mutex<()>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    available: bool,
    current_version: String,
    version: Option<String>,
    body: Option<String>,
    installable: bool,
    release_url: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    phase: &'static str,
    downloaded: u64,
    total: Option<u64>,
    percent: Option<u8>,
}

fn progress(downloaded: u64, total: Option<u64>, phase: &'static str) -> UpdateProgress {
    let percent = total
        .filter(|total| *total > 0)
        .map(|total| ((downloaded.saturating_mul(100) / total).min(100)) as u8);
    UpdateProgress {
        phase,
        downloaded,
        total,
        percent,
    }
}

fn supports_in_app_install() -> bool {
    supports_in_app_install_for(
        cfg!(target_os = "linux"),
        std::env::var_os("APPIMAGE").is_some(),
    )
}

fn supports_in_app_install_for(is_linux: bool, appimage_present: bool) -> bool {
    !is_linux || appimage_present
}

#[tauri::command]
pub async fn check_for_updates(
    app: AppHandle,
    coordinator: State<'_, UpdateCoordinator>,
) -> Result<UpdateStatus, String> {
    let _operation = coordinator
        .operation
        .try_lock()
        .map_err(|_| "Another update operation is already running.".to_owned())?;
    let current_version = app.package_info().version.to_string();
    let update = app
        .updater()
        .map_err(|error| format!("The updater is not configured: {error}"))?
        .check()
        .await
        .map_err(|error| format!("OpenQuota could not reach the update service: {error}"))?;
    Ok(match update {
        Some(update) => UpdateStatus {
            available: true,
            current_version,
            version: Some(update.version),
            body: update.body,
            installable: supports_in_app_install(),
            release_url: RELEASE_URL,
        },
        None => UpdateStatus {
            available: false,
            current_version,
            version: None,
            body: None,
            installable: supports_in_app_install(),
            release_url: RELEASE_URL,
        },
    })
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    coordinator: State<'_, UpdateCoordinator>,
) -> Result<(), String> {
    if !supports_in_app_install() {
        return Err(
            "Debian packages must be updated through the package installer on GitHub.".to_owned(),
        );
    }
    let _operation = coordinator
        .operation
        .try_lock()
        .map_err(|_| "Another update operation is already running.".to_owned())?;
    let update = app
        .updater()
        .map_err(|error| format!("The updater is not configured: {error}"))?
        .check()
        .await
        .map_err(|error| format!("OpenQuota could not reach the update service: {error}"))?
        .ok_or_else(|| "OpenQuota is already up to date.".to_owned())?;

    let downloaded = AtomicU64::new(0);
    let progress_app = app.clone();
    let finish_app = app.clone();
    let _ = app.emit("update-progress", progress(0, None, "downloading"));
    update
        .download_and_install(
            move |chunk_length, total| {
                let current = downloaded.fetch_add(chunk_length as u64, Ordering::Relaxed)
                    + chunk_length as u64;
                let _ =
                    progress_app.emit("update-progress", progress(current, total, "downloading"));
            },
            move || {
                let _ = finish_app.emit("update-progress", progress(0, None, "installing"));
            },
        )
        .await
        .map_err(|error| format!("The signed update could not be installed: {error}"))?;
    app.restart();
}

#[tauri::command]
pub fn open_update_page() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let (program, arguments) = ("explorer.exe", vec![RELEASE_URL]);
    #[cfg(target_os = "macos")]
    let (program, arguments) = ("open", vec![RELEASE_URL]);
    #[cfg(target_os = "linux")]
    let (program, arguments) = ("xdg-open", vec![RELEASE_URL]);

    background_command(program)
        .args(arguments)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("The OpenQuota download page could not be opened: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{progress, supports_in_app_install_for};

    #[test]
    fn download_progress_is_bounded_and_handles_unknown_totals() {
        assert_eq!(progress(25, Some(100), "downloading").percent, Some(25));
        assert_eq!(progress(125, Some(100), "downloading").percent, Some(100));
        assert_eq!(progress(25, None, "downloading").percent, None);
        assert_eq!(progress(25, Some(0), "downloading").percent, None);
    }

    #[test]
    fn only_appimage_uses_in_app_installation_on_linux() {
        assert!(supports_in_app_install_for(false, false));
        assert!(supports_in_app_install_for(true, true));
        assert!(!supports_in_app_install_for(true, false));
    }
}
