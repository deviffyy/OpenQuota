use std::{collections::HashSet, sync::Arc};

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use crate::{
    apply_shortcut_change, autostart_is_enabled, child_process,
    desktop_integration::DesktopIntegration,
    models::{AppSettings, SettingsViewState},
    notifications::{finish_refresh, permission as notification_permission},
    pacing::NotificationEvaluator,
    providers::{detect_local_credentials, ProviderRegistry},
    service::ProviderService,
    set_autostart,
    settings::SettingsService,
    tray_presentation,
};

#[tauri::command]
pub fn get_app_settings(
    app: AppHandle,
    settings: State<'_, Arc<SettingsService>>,
) -> SettingsViewState {
    settings_view_state(&app, &settings)
}

#[tauri::command]
pub async fn save_app_settings(
    app: AppHandle,
    service: State<'_, Arc<ProviderService>>,
    settings_service: State<'_, Arc<SettingsService>>,
    notifications: State<'_, Arc<NotificationEvaluator>>,
    settings: AppSettings,
) -> Result<SettingsViewState, String> {
    let previous = settings_service.get();
    let next_shortcut = settings.global_shortcut.clone();
    let autostart_changed = previous.launch_at_login != settings.launch_at_login;
    apply_shortcut_change(
        &app,
        previous.global_shortcut.as_deref(),
        settings.global_shortcut.as_deref(),
    )?;
    if autostart_changed {
        if let Err(error) = set_autostart(&app, settings.launch_at_login) {
            let _ = apply_shortcut_change(
                &app,
                settings.global_shortcut.as_deref(),
                previous.global_shortcut.as_deref(),
            );
            return Err(error);
        }
    }
    let updated = match settings_service.update(settings) {
        Ok(settings) => settings,
        Err(error) => {
            crate::app_error!("config", "settings could not be persisted");
            if autostart_changed {
                let _ = set_autostart(&app, previous.launch_at_login);
            }
            let _ = apply_shortcut_change(
                &app,
                next_shortcut.as_deref(),
                previous.global_shortcut.as_deref(),
            );
            return Err(error);
        }
    };
    if previous.log_level != updated.log_level {
        crate::logging::set_level(updated.log_level);
        crate::app_info!(
            "config",
            "log level changed to {}",
            updated.log_level.log_label()
        );
    }
    crate::app_debug!("config", "application settings persisted");
    tray_presentation::update(
        &app,
        &service.state(),
        &updated,
        settings_service.registry(),
    );
    let _ = app.emit(
        "settings-state",
        settings_view_state(&app, &settings_service),
    );

    let newly_enabled = updated
        .providers
        .iter()
        .filter(|provider| {
            provider.enabled
                && !previous
                    .providers
                    .iter()
                    .any(|old| old.id == provider.id && old.enabled)
        })
        .map(|provider| provider.id.clone())
        .collect::<Vec<_>>();
    if !newly_enabled.is_empty() {
        let progress_app = app.clone();
        service
            .refresh_enabled_with_progress(&newly_enabled, true, move |state| {
                let _ = progress_app.emit("usage-state", state);
            })
            .await;
        let state = service.state();
        let _ = app.emit("usage-state", &state);
        finish_refresh(&app, &state, &settings_service, &notifications);
    }
    Ok(settings_view_state(&app, &settings_service))
}

#[tauri::command]
pub async fn reset_customization(
    app: AppHandle,
    registry: State<'_, Arc<ProviderRegistry>>,
    service: State<'_, Arc<ProviderService>>,
    settings: State<'_, Arc<SettingsService>>,
    notifications: State<'_, Arc<NotificationEvaluator>>,
) -> Result<SettingsViewState, String> {
    crate::app_info!("config", "reset all customization requested");
    let mut next = settings.get();
    let detected_before_reset = next
        .providers
        .iter()
        .filter(|provider| provider.detected)
        .map(|provider| provider.id.clone())
        .collect::<HashSet<_>>();
    next.providers = settings.default_settings(&detected_before_reset).providers;
    next.detection_notice_dismissed = false;
    let next = settings.update(next)?;
    tray_presentation::update(&app, &service.state(), &next, settings.registry());
    let state = settings_view_state(&app, &settings);
    let _ = app.emit("settings-state", &state);
    let plan = settings.reset_detection_plan();
    let detected = detect_local_credentials(registry.inner().clone(), plan.provider_ids()).await;
    let outcome = settings.apply_credential_detection(&plan, &detected)?;
    tray_presentation::update(
        &app,
        &service.state(),
        &outcome.settings,
        settings.registry(),
    );
    let state = settings_view_state(&app, &settings);
    let _ = app.emit("settings-state", &state);
    let progress_app = app.clone();
    let usage_state = service
        .refresh_all_with_progress(&settings.enabled_provider_ids(), true, move |state| {
            let _ = progress_app.emit("usage-state", state);
        })
        .await;
    let _ = app.emit("usage-state", &usage_state);
    finish_refresh(&app, &usage_state, &settings, &notifications);
    Ok(state)
}

#[tauri::command]
pub fn reset_provider_customization(
    app: AppHandle,
    service: State<'_, Arc<ProviderService>>,
    settings: State<'_, Arc<SettingsService>>,
    provider_id: String,
) -> Result<SettingsViewState, String> {
    crate::app_info!("config", "provider customization reset for {provider_id}");
    let updated = settings.reset_provider(&provider_id)?;
    tray_presentation::update(&app, &service.state(), &updated, settings.registry());
    let state = settings_view_state(&app, &settings);
    let _ = app.emit("settings-state", &state);
    Ok(state)
}

#[tauri::command]
pub fn request_notification_permission(
    app: AppHandle,
    settings: State<'_, Arc<SettingsService>>,
) -> SettingsViewState {
    crate::app_info!("notifications", "notification permission requested");
    let error = app
        .notification()
        .request_permission()
        .err()
        .map(|_| "Notification permission could not be requested.".to_owned());
    if error.is_some() {
        crate::app_error!("notifications", "notification permission request failed");
    }
    settings.view_state(
        notification_permission(&app),
        error,
        app.state::<DesktopIntegration>().standalone_window,
        app.state::<DesktopIntegration>().platform_summary(),
    )
}

#[tauri::command]
pub fn open_notification_settings() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let result = child_process::background_command("explorer.exe")
        .arg("ms-settings:notifications")
        .spawn();
    #[cfg(target_os = "macos")]
    let result = child_process::background_command("open")
        .arg("x-apple.systempreferences:com.apple.Notifications-Settings.extension")
        .spawn();
    #[cfg(target_os = "linux")]
    let result = [
        ("gnome-control-center", "notifications"),
        ("systemsettings", "kcm_notifications"),
        ("systemsettings5", "kcm_notifications"),
    ]
    .into_iter()
    .find_map(|(program, argument)| {
        child_process::background_command(program)
            .arg(argument)
            .spawn()
            .ok()
    })
    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "settings unavailable"));

    result
        .map(|_| ())
        .map_err(|_| "Notification settings could not be opened on this system.".to_owned())
}

#[tauri::command]
pub fn get_log_path() -> String {
    crate::logging::log_path().to_string_lossy().into_owned()
}

#[tauri::command]
pub fn open_log_folder(app: AppHandle) -> Result<(), String> {
    let path = crate::logging::log_path();
    let result = if path.is_file() {
        app.opener().reveal_item_in_dir(&path)
    } else if let Some(parent) = path.parent() {
        app.opener()
            .open_path(parent.to_string_lossy(), None::<&str>)
    } else {
        return Err("The OpenQuota log folder is unavailable.".to_owned());
    };
    result
        .inspect(|_| crate::app_debug!("config", "log folder opened"))
        .map_err(|_| {
            crate::app_warn!("config", "log folder could not be opened");
            "The OpenQuota log folder could not be opened.".to_owned()
        })
}

pub(crate) fn settings_view_state(app: &AppHandle, service: &SettingsService) -> SettingsViewState {
    let mut settings = service.get();
    let mut integration_error = match autostart_is_enabled(app) {
        Ok(enabled) => {
            if settings.launch_at_login != enabled {
                settings.launch_at_login = enabled;
                let _ = service.update(settings);
            }
            None
        }
        Err(_) => Some("Launch at login status could not be read.".to_owned()),
    };
    if let Some(shortcut) = service.get().global_shortcut {
        if !app.global_shortcut().is_registered(shortcut.as_str()) {
            integration_error =
                Some("The saved global shortcut is currently unavailable.".to_owned());
        }
    }
    service.view_state(
        notification_permission(app),
        integration_error,
        app.state::<DesktopIntegration>().standalone_window,
        app.state::<DesktopIntegration>().platform_summary(),
    )
}
