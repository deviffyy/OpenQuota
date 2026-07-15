use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};
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
pub struct UpdateFailure {
    code: &'static str,
    message: String,
    action: &'static str,
    retryable: bool,
}

impl UpdateFailure {
    fn new(
        code: &'static str,
        message: impl Into<String>,
        action: &'static str,
        retryable: bool,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            action,
            retryable,
        }
    }
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

fn classify_updater_error(error: &UpdaterError, operation: &'static str) -> UpdateFailure {
    let detail = error.to_string();
    let normalized = detail.to_ascii_lowercase();
    if normalized.contains("403") || normalized.contains("forbidden") {
        return UpdateFailure::new(
            "download_forbidden",
            "GitHub refused the update download.",
            "Try again. If it still fails, download the verified installer from the release page.",
            true,
        );
    }
    if normalized.contains("429") || normalized.contains("rate limit") {
        return UpdateFailure::new(
            "rate_limited",
            "GitHub temporarily limited update requests.",
            "Wait a few minutes, then try again.",
            true,
        );
    }
    if matches!(
        error,
        UpdaterError::Minisign(_) | UpdaterError::SignatureUtf8(_)
    ) {
        return UpdateFailure::new(
            "signature_invalid",
            "The downloaded update failed its security check.",
            "Do not install this download. Open the release page or try again later.",
            false,
        );
    }
    if matches!(error, UpdaterError::Reqwest(_) | UpdaterError::Network(_)) {
        return UpdateFailure::new(
            "network",
            format!("OpenQuota could not {operation} because the network request failed."),
            "Check your connection or proxy, then try again.",
            true,
        );
    }
    UpdateFailure::new(
        "update_failed",
        format!("OpenQuota could not {operation}."),
        "Try again or use the release page to download the installer manually.",
        true,
    )
}

fn retryable_download_error(error: &UpdaterError) -> bool {
    let detail = error.to_string().to_ascii_lowercase();
    matches!(error, UpdaterError::Reqwest(_) | UpdaterError::Network(_))
        || detail.contains("403")
        || detail.contains("429")
        || detail.contains("502")
        || detail.contains("503")
        || detail.contains("504")
}

async fn download_and_install_once(app: &AppHandle, update: &Update) -> Result<(), UpdaterError> {
    let downloaded = Arc::new(AtomicU64::new(0));
    let callback_downloaded = downloaded.clone();
    let progress_app = app.clone();
    let finish_app = app.clone();
    update
        .download_and_install(
            move |chunk_length, total| {
                let current = callback_downloaded.fetch_add(chunk_length as u64, Ordering::Relaxed)
                    + chunk_length as u64;
                let _ =
                    progress_app.emit("update-progress", progress(current, total, "downloading"));
            },
            move || {
                let _ = finish_app.emit("update-progress", progress(0, None, "installing"));
            },
        )
        .await
}

#[tauri::command]
pub async fn check_for_updates(
    app: AppHandle,
    coordinator: State<'_, UpdateCoordinator>,
) -> Result<UpdateStatus, UpdateFailure> {
    crate::app_info!("updates", "update check started");
    let _operation = coordinator.operation.try_lock().map_err(|_| {
        UpdateFailure::new(
            "busy",
            "Another update operation is already running.",
            "Wait for it to finish, then try again.",
            true,
        )
    })?;
    let current_version = app.package_info().version.to_string();
    let update = app
        .updater()
        .map_err(|_| {
            UpdateFailure::new(
                "not_configured",
                "Automatic updates are not configured in this build.",
                "Download the latest version from the release page.",
                false,
            )
        })?
        .check()
        .await
        .map_err(|error| {
            crate::app_warn!("updates", "update check failed: {error}");
            classify_updater_error(&error, "check for updates")
        })?;
    crate::app_info!(
        "updates",
        "update check finished ({})",
        if update.is_some() {
            "update available"
        } else {
            "up to date"
        }
    );
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
) -> Result<(), UpdateFailure> {
    crate::app_info!("updates", "signed update installation started");
    if !supports_in_app_install() {
        return Err(UpdateFailure::new(
            "manual_install_required",
            "This Linux package cannot update itself.",
            "Download the new package from the release page and install it normally.",
            false,
        ));
    }
    let _operation = coordinator.operation.try_lock().map_err(|_| {
        UpdateFailure::new(
            "busy",
            "Another update operation is already running.",
            "Wait for it to finish, then try again.",
            true,
        )
    })?;
    let update = app
        .updater()
        .map_err(|_| {
            UpdateFailure::new(
                "not_configured",
                "Automatic updates are not configured in this build.",
                "Download the latest version from the release page.",
                false,
            )
        })?
        .check()
        .await
        .map_err(|error| classify_updater_error(&error, "check for updates"))?
        .ok_or_else(|| {
            UpdateFailure::new(
                "up_to_date",
                "OpenQuota is already up to date.",
                "No action is needed.",
                false,
            )
        })?;

    let _ = app.emit("update-progress", progress(0, None, "downloading"));
    if let Err(first_error) = download_and_install_once(&app, &update).await {
        if !retryable_download_error(&first_error) {
            crate::app_warn!("updates", "update installation failed: {first_error}");
            return Err(classify_updater_error(
                &first_error,
                "install the signed update",
            ));
        }
        crate::app_warn!(
            "updates",
            "update download failed; retrying once: {first_error}"
        );
        let _ = app.emit("update-progress", progress(0, None, "retrying"));
        tokio::time::sleep(Duration::from_millis(1200)).await;
        download_and_install_once(&app, &update)
            .await
            .map_err(|error| {
                crate::app_warn!("updates", "update retry failed: {error}");
                classify_updater_error(&error, "install the signed update")
            })?;
    }
    crate::app_info!("updates", "signed update installed; restarting");
    app.restart();
}

#[tauri::command]
pub fn open_update_page() -> Result<(), String> {
    crate::app_info!("updates", "opening release download page");
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
    use super::{
        classify_updater_error, progress, retryable_download_error, supports_in_app_install_for,
    };
    use tauri_plugin_updater::Error as UpdaterError;

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

    #[test]
    fn forbidden_downloads_have_a_safe_retry_and_manual_fallback() {
        let error =
            UpdaterError::Network("Download request failed with status: 403 Forbidden".into());
        let failure = classify_updater_error(&error, "install the signed update");
        assert_eq!(failure.code, "download_forbidden");
        assert!(failure.retryable);
        assert!(failure.action.contains("release page"));
        assert!(retryable_download_error(&error));
    }

    #[test]
    fn signature_failures_are_never_retried() {
        let error = UpdaterError::SignatureUtf8("bad signature".into());
        let failure = classify_updater_error(&error, "install the signed update");
        assert_eq!(failure.code, "signature_invalid");
        assert!(!failure.retryable);
        assert!(!retryable_download_error(&error));
    }
}
