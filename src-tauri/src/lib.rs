mod desktop_integration;
mod models;
mod pacing;
mod popup;
mod providers;
mod service;
mod settings;
mod storage;
mod tray_presentation;
mod updates;
#[cfg(any(target_os = "linux", test))]
mod xdg_autostart;

use std::{sync::Arc, thread, time::Duration};

use popup::PopupDismissGuard;
use service::{ProviderService, UsageViewState};
use settings::{default_settings, SettingsService};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WebviewWindow, Window, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::{NotificationExt, PermissionState};
use tauri_plugin_positioner::{Position, WindowExt};

use crate::{
    desktop_integration::DesktopIntegration,
    models::{AppSettings, SettingsViewState},
    pacing::{NotificationEvaluator, PaceAlert},
    providers::{
        antigravity::AntigravityProvider, claude::ClaudeProvider, codex::CodexProvider,
        UsageProvider,
    },
    storage::Storage,
};

const MAIN_WINDOW: &str = "main";
const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[tauri::command]
fn get_app_data_path(app: AppHandle) -> Result<String, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|_| "OpenQuota data directory could not be resolved.".to_owned())
}

#[tauri::command]
async fn get_usage_state(service: State<'_, Arc<ProviderService>>) -> Result<UsageViewState, ()> {
    Ok(service.state())
}

#[tauri::command]
async fn refresh_usage(
    app: AppHandle,
    service: State<'_, Arc<ProviderService>>,
    settings: State<'_, Arc<SettingsService>>,
    notifications: State<'_, Arc<NotificationEvaluator>>,
) -> Result<UsageViewState, ()> {
    let state = service
        .refresh_enabled(&enabled_provider_ids(&settings.get()), true)
        .await;
    let _ = app.emit("usage-state", &state);
    finish_refresh(&app, &state, &settings, &notifications);
    Ok(state)
}

#[tauri::command]
fn get_app_settings(
    app: AppHandle,
    settings: State<'_, Arc<SettingsService>>,
) -> SettingsViewState {
    settings_view_state(&app, &settings)
}

#[tauri::command]
async fn save_app_settings(
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
async fn reset_customization(
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
        .refresh_enabled(&enabled_provider_ids(&settings.get()), true)
        .await;
    let _ = app.emit("usage-state", &usage_state);
    finish_refresh(&app, &usage_state, &settings, &notifications);
    Ok(state)
}

#[tauri::command]
fn request_notification_permission(
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

fn show_popup(window: &WebviewWindow) {
    let standalone = window
        .app_handle()
        .state::<DesktopIntegration>()
        .standalone_window;
    if standalone {
        let _ = window.center();
        let _ = window.unminimize();
    } else {
        let _ = window
            .as_ref()
            .window()
            .move_window_constrained(Position::TrayCenter);
    }
    let _ = window.show();
    let _ = window.set_focus();
}

fn hide_popup(window: &WebviewWindow) {
    let _ = window.hide();
    let _ = window.app_handle().emit("popup-hidden", ());
}

fn toggle_popup(app: &AppHandle) {
    app.state::<PopupDismissGuard>().cancel_pending();

    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };

    if window.is_visible().unwrap_or(false) {
        if app.state::<DesktopIntegration>().standalone_window {
            let _ = window.minimize();
        } else {
            hide_popup(&window);
        }
    } else {
        show_popup(&window);
    }
}

#[tauri::command]
fn dismiss_main_window(app: AppHandle) {
    if app.state::<DesktopIntegration>().standalone_window {
        app.exit(0);
    } else if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        hide_popup(&window);
    }
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

fn open_screen(app: &AppHandle, screen: &str) {
    app.state::<PopupDismissGuard>().cancel_pending();
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        show_popup(&window);
        let _ = app.emit("open-screen", screen);
    }
}

fn register_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _, event| {
            if event.state == ShortcutState::Released {
                toggle_popup(app);
            }
        })
        .map_err(|_| "That global shortcut is invalid or already in use.".to_owned())
}

fn apply_shortcut_change(
    app: &AppHandle,
    previous: Option<&str>,
    next: Option<&str>,
) -> Result<(), String> {
    if previous == next {
        return Ok(());
    }
    if let Some(previous) = previous {
        let _ = app.global_shortcut().unregister(previous);
    }
    if let Some(next) = next.filter(|shortcut| !shortcut.trim().is_empty()) {
        if let Err(error) = register_shortcut(app, next) {
            if let Some(previous) = previous {
                let _ = register_shortcut(app, previous);
            }
            return Err(error);
        }
    }
    Ok(())
}

fn set_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        return xdg_autostart::set_enabled(enabled)
            .map_err(|_| "Launch at login could not be updated.".to_owned());
    }
    #[cfg(not(target_os = "linux"))]
    {
        let manager = app.autolaunch();
        let result = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
        result.map_err(|_| "Launch at login could not be updated.".to_owned())
    }
}

fn autostart_is_enabled(app: &AppHandle) -> Result<bool, ()> {
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        xdg_autostart::is_enabled().map_err(|_| ())
    }
    #[cfg(not(target_os = "linux"))]
    {
        app.autolaunch().is_enabled().map_err(|_| ())
    }
}

fn notification_permission(app: &AppHandle) -> &'static str {
    match app.notification().permission_state() {
        Ok(PermissionState::Granted) => "granted",
        Ok(PermissionState::Denied) => "denied",
        Ok(PermissionState::Prompt | PermissionState::PromptWithRationale) => "prompt",
        Err(_) => "unavailable",
    }
}

fn settings_view_state(app: &AppHandle, service: &SettingsService) -> SettingsViewState {
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

fn finish_refresh(
    app: &AppHandle,
    state: &UsageViewState,
    settings: &SettingsService,
    notifications: &NotificationEvaluator,
) {
    let preferences = settings.get();
    tray_presentation::update(app, state, &preferences);
    for provider_state in state.providers.values() {
        if provider_state.error.is_none() {
            if let Some(snapshot) = provider_state.snapshot.as_ref() {
                let alerts = notifications.evaluate(snapshot, &preferences, chrono::Utc::now());
                deliver_notifications(app, &alerts);
            }
        }
    }
}

fn enabled_provider_ids(settings: &AppSettings) -> Vec<String> {
    settings
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .map(|provider| provider.id.clone())
        .collect()
}

fn deliver_notifications(app: &AppHandle, alerts: &[PaceAlert]) {
    if alerts.is_empty() || notification_permission(app) != "granted" {
        return;
    }
    if alerts.len() == 1 {
        let alert = &alerts[0];
        let _ = app
            .notification()
            .builder()
            .title(alert.milestone.title())
            .body(format!(
                "{} · {}\n{}",
                alert.provider,
                alert.metric,
                alert.milestone.body()
            ))
            .show();
        return;
    }
    let body = alerts
        .iter()
        .map(|alert| {
            format!(
                "{} · {}: {}",
                alert.provider,
                alert.metric,
                alert.milestone.title()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let _ = app
        .notification()
        .builder()
        .title("OpenQuota Quota Alerts")
        .body(body)
        .show();
}

fn schedule_outside_click_dismiss(window: Window) {
    let app = window.app_handle().clone();
    let token = app.state::<PopupDismissGuard>().token();

    thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        let app_for_dismiss = app.clone();
        let _ = app.run_on_main_thread(move || {
            let guard = app_for_dismiss.state::<PopupDismissGuard>();
            let still_unfocused = window.is_focused().is_ok_and(|focused| !focused);

            if guard.is_current(token) && still_unfocused {
                let _ = window.hide();
                let _ = app_for_dismiss.emit("popup-hidden", ());
            }
        });
    });
}

fn handle_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != MAIN_WINDOW {
        return;
    }

    match event {
        WindowEvent::Focused(false)
            if !window
                .app_handle()
                .state::<DesktopIntegration>()
                .standalone_window =>
        {
            schedule_outside_click_dismiss(window.clone())
        }
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            if window
                .app_handle()
                .state::<DesktopIntegration>()
                .standalone_window
            {
                window.app_handle().exit(0);
                return;
            }
            window
                .app_handle()
                .state::<PopupDismissGuard>()
                .cancel_pending();
            let _ = window.hide();
            let _ = window.app_handle().emit("popup-hidden", ());
        }
        _ => {}
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(PopupDismissGuard::default())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            app.handle().plugin(tauri_plugin_positioner::init())?;
            let desktop_integration = DesktopIntegration::detect();
            app.manage(desktop_integration.clone());

            let database_path = app.path().app_data_dir()?.join("openquota.db");
            let storage = Arc::new(Storage::open(&database_path)?);
            let providers: Vec<Arc<dyn UsageProvider>> = vec![
                Arc::new(ClaudeProvider::new(storage.clone())?),
                Arc::new(CodexProvider::new(storage.clone())?),
                Arc::new(AntigravityProvider::new()?),
            ];
            let detected = providers
                .iter()
                .filter(|provider| provider.has_local_credentials())
                .map(|provider| provider.id().to_owned())
                .collect();
            let service = Arc::new(ProviderService::new(providers, storage.clone()));
            let settings = Arc::new(SettingsService::new(storage, &detected));
            let notifications = Arc::new(NotificationEvaluator::default());
            app.manage(service.clone());
            app.manage(settings.clone());
            app.manage(notifications.clone());

            if let Some(shortcut) = settings.get().global_shortcut {
                let _ = register_shortcut(app.handle(), &shortcut);
            }

            let open = MenuItem::with_id(app, "open", "Open OpenQuota", true, None::<&str>)?;
            let customize = MenuItem::with_id(app, "customize", "Customize…", true, None::<&str>)?;
            let settings_item =
                MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit OpenQuota", true, None::<&str>)?;
            let menu =
                Menu::with_items(app, &[&open, &customize, &settings_item, &separator, &quit])?;

            let tray = TrayIconBuilder::with_id("openquota-tray")
                .icon(
                    app.default_window_icon()
                        .expect("OpenQuota requires a bundled application icon")
                        .clone(),
                )
                .tooltip("OpenQuota")
                .menu(&menu);
            #[cfg(target_os = "linux")]
            let tray = tray.show_menu_on_left_click(true);
            #[cfg(not(target_os = "linux"))]
            let tray = tray.show_menu_on_left_click(false);
            tray.on_menu_event(|app, event| match event.id.as_ref() {
                "open" => {
                    app.state::<PopupDismissGuard>().cancel_pending();
                    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                        show_popup(&window);
                    }
                }
                "customize" => open_screen(app, "customize"),
                "settings" => open_screen(app, "settings"),
                "quit" => app.exit(0),
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);

                if matches!(
                    event,
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                ) {
                    toggle_popup(tray.app_handle());
                }
            })
            .build(app)?;

            if desktop_integration.standalone_window {
                if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                    let _ = window.set_skip_taskbar(false);
                    let _ = window.set_always_on_top(false);
                    let _ = window.set_decorations(true);
                    let _ = window.center();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }

            tray_presentation::update(app.handle(), &service.state(), &settings.get());
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let provider_ids = enabled_provider_ids(&settings.get());
                    if !provider_ids.is_empty() {
                        let state = service.refresh_enabled(&provider_ids, false).await;
                        let _ = app_handle.emit("usage-state", &state);
                        finish_refresh(&app_handle, &state, &settings, &notifications);
                    }
                    tokio::time::sleep(REFRESH_INTERVAL).await;
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_usage_state,
            refresh_usage,
            get_app_settings,
            save_app_settings,
            reset_customization,
            request_notification_permission,
            get_app_data_path,
            dismiss_main_window,
            quit_app,
            updates::check_for_updates,
            updates::install_update
        ])
        .on_window_event(handle_window_event)
        .run(tauri::generate_context!())
        .expect("error while running OpenQuota");
}
