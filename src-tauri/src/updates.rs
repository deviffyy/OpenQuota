use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    available: bool,
    current_version: String,
    version: Option<String>,
    body: Option<String>,
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<UpdateStatus, String> {
    let current_version = app.package_info().version.to_string();
    let update = app
        .updater()
        .map_err(|_| "The updater is not configured.".to_owned())?
        .check()
        .await
        .map_err(|_| "OpenQuota could not reach the update service.".to_owned())?;
    Ok(match update {
        Some(update) => UpdateStatus {
            available: true,
            current_version,
            version: Some(update.version),
            body: update.body,
        },
        None => UpdateStatus {
            available: false,
            current_version,
            version: None,
            body: None,
        },
    })
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let update = app
        .updater()
        .map_err(|_| "The updater is not configured.".to_owned())?
        .check()
        .await
        .map_err(|_| "OpenQuota could not reach the update service.".to_owned())?
        .ok_or_else(|| "OpenQuota is already up to date.".to_owned())?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|_| "The signed update could not be installed.".to_owned())?;
    app.restart();
}
