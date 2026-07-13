use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_notification::NotificationExt;

use crate::{
    apply_shortcut_change, autostart_is_enabled, child_process,
    desktop_integration::DesktopIntegration,
    models::{AppSettings, SettingsViewState},
    notifications::{finish_refresh, permission as notification_permission},
    pacing::NotificationEvaluator,
    service::ProviderService,
    set_autostart,
    settings::{default_settings, SettingsService},
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
    tray_presentation::update(&app, &service.state(), &updated);
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
        service.refresh_enabled(&newly_enabled, true).await;
        let state = service.state();
        let _ = app.emit("usage-state", &state);
        finish_refresh(&app, &state, &settings_service, &notifications);
    }
    Ok(settings_view_state(&app, &settings_service))
}

#[tauri::command]
pub async fn reset_customization(
    app: AppHandle,
    service: State<'_, Arc<ProviderService>>,
    settings: State<'_, Arc<SettingsService>>,
    notifications: State<'_, Arc<NotificationEvaluator>>,
) -> Result<SettingsViewState, String> {
    let mut next = settings.get();
    next.providers = default_settings(&settings.detected_provider_ids()).providers;
    next.detection_notice_dismissed = false;
    let next = settings.update(next)?;
    tray_presentation::update(&app, &service.state(), &next);
    let state = settings_view_state(&app, &settings);
    let _ = app.emit("settings-state", &state);
    let usage_state = service
        .refresh_all(&settings.enabled_provider_ids(), true)
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
    let updated = settings.reset_provider(&provider_id)?;
    tray_presentation::update(&app, &service.state(), &updated);
    let state = settings_view_state(&app, &settings);
    let _ = app.emit("settings-state", &state);
    Ok(state)
}

#[tauri::command]
pub fn request_notification_permission(
    app: AppHandle,
    settings: State<'_, Arc<SettingsService>>,
) -> SettingsViewState {
    let error = app
        .notification()
        .request_permission()
        .err()
        .map(|_| "Notification permission could not be requested.".to_owned());
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
