mod child_process;
mod commands;
mod desktop_integration;
mod logging;
#[cfg(any(target_os = "macos", test))]
mod menu_bar;
mod models;
mod notifications;
mod pacing;
mod policy;
mod popup;
mod pricing;
mod providers;
mod refresh_loop;
mod service;
mod settings;
mod storage;
#[cfg(any(not(target_os = "macos"), test))]
mod tray_icon;
mod tray_presentation;
mod updates;
mod window;
#[cfg(any(target_os = "linux", test))]
mod xdg_autostart;

use std::sync::Arc;

use popup::PopupDismissGuard;
use service::ProviderService;
use settings::{CredentialDetectionPlan, SettingsService};
#[cfg(not(target_os = "linux"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager,
};
#[cfg(not(target_os = "linux"))]
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::{
    desktop_integration::DesktopIntegration,
    pacing::NotificationEvaluator,
    pricing::PricingStore,
    providers::{
        antigravity::AntigravityProvider, claude::ClaudeProvider, codex::CodexProvider,
        cursor::CursorProvider, detect_local_credentials, ProviderRegistry, UsageProvider,
    },
    storage::Storage,
    window::{handle_window_event, open_screen, show_popup, toggle_popup, MAIN_WINDOW},
};

fn spawn_startup_credential_detection(
    app: AppHandle,
    registry: Arc<ProviderRegistry>,
    service: Arc<ProviderService>,
    settings: Arc<SettingsService>,
    notifications: Arc<NotificationEvaluator>,
    plan: CredentialDetectionPlan,
) {
    tauri::async_runtime::spawn(async move {
        app_info!("config", "startup credential detection began");
        let detected = detect_local_credentials(registry, plan.provider_ids()).await;
        let Ok(outcome) = settings.apply_credential_detection(&plan, &detected) else {
            app_error!(
                "config",
                "startup credential detection could not be applied"
            );
            return;
        };
        app_info!(
            "config",
            "startup credential detection completed ({} detected, {} newly enabled)",
            detected.len(),
            outcome.newly_enabled_provider_ids.len()
        );

        tray_presentation::update(
            &app,
            &service.state(),
            &outcome.settings,
            settings.registry(),
        );
        let _ = app.emit(
            "settings-state",
            commands::settings::settings_view_state(&app, &settings),
        );
        if outcome.newly_enabled_provider_ids.is_empty() {
            return;
        }
        service
            .refresh_enabled(&outcome.newly_enabled_provider_ids, true)
            .await;
        let state = service.state();
        let _ = app.emit("usage-state", &state);
        notifications::finish_refresh(&app, &state, &settings, &notifications);
    });
}

fn register_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _, event| {
            if event.state == ShortcutState::Released {
                toggle_popup(app);
            }
        })
        .map_err(|_| {
            crate::app_warn!("config", "global shortcut registration failed");
            "That global shortcut is invalid or already in use.".to_owned()
        })
}

pub(crate) fn apply_shortcut_change(
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
    crate::app_debug!("config", "global shortcut configuration updated");
    Ok(())
}

pub(crate) fn set_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        xdg_autostart::set_enabled(enabled)
            .map_err(|_| "Launch at login could not be updated.".to_owned())
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

pub(crate) fn autostart_is_enabled(app: &AppHandle) -> Result<bool, ()> {
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        window::activate_existing_instance(app);
    }));

    builder
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(PopupDismissGuard::default())
        .manage(updates::UpdateCoordinator::default())
        .setup(|app| {
            logging::init(logging::default_log_path(), models::LogLevel::Info);

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            app.handle().plugin(tauri_plugin_positioner::init())?;
            let desktop_integration = DesktopIntegration::detect();
            app_info!(
                "lifecycle",
                "desktop integration detected (standalone={})",
                desktop_integration.standalone_window
            );
            app.manage(desktop_integration.clone());

            let database_path = app.path().app_data_dir()?.join("openquota.db");
            let storage = Arc::new(Storage::open(&database_path)?);
            app_debug!("cache", "application database opened");
            let pricing = Arc::new(PricingStore::new(
                app.path().app_data_dir()?.join("pricing"),
            )?);
            let providers: Vec<Arc<dyn UsageProvider>> = vec![
                Arc::new(ClaudeProvider::new(storage.clone(), pricing.clone())?),
                Arc::new(CodexProvider::new(storage.clone(), pricing.clone())?),
                Arc::new(CursorProvider::new(pricing.clone())?),
                Arc::new(AntigravityProvider::new()?),
            ];
            let registry = Arc::new(ProviderRegistry::new(providers)?);
            let service = Arc::new(ProviderService::new(registry.clone(), storage.clone()));
            let (settings_service, credential_detection_plan) =
                SettingsService::new_deferred(storage, registry.clone())?;
            let settings = Arc::new(settings_service);
            logging::set_level(settings.get().log_level);
            app_info!(
                "config",
                "OpenQuota v{} starting (level={}, log=OpenQuota.log)",
                app.package_info().version,
                logging::current_level().log_label()
            );
            let notifications = Arc::new(NotificationEvaluator::default());
            app.manage(registry.clone());
            app.manage(service.clone());
            app.manage(settings.clone());
            app.manage(notifications.clone());

            if let Some(shortcut) = settings.get().global_shortcut {
                let _ = register_shortcut(app.handle(), &shortcut);
            }

            if !desktop_integration.standalone_window {
                #[cfg(target_os = "macos")]
                let menu = {
                    let settings_item =
                        MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
                    let separator = PredefinedMenuItem::separator(app)?;
                    let quit =
                        MenuItem::with_id(app, "quit", "Quit OpenQuota", true, None::<&str>)?;
                    Menu::with_items(app, &[&settings_item, &separator, &quit])?
                };
                #[cfg(not(target_os = "macos"))]
                let menu = {
                    let open =
                        MenuItem::with_id(app, "open", "Open OpenQuota", true, None::<&str>)?;
                    let customize =
                        MenuItem::with_id(app, "customize", "Customize…", true, None::<&str>)?;
                    let settings_item =
                        MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
                    let separator = PredefinedMenuItem::separator(app)?;
                    let quit =
                        MenuItem::with_id(app, "quit", "Quit OpenQuota", true, None::<&str>)?;
                    Menu::with_items(app, &[&open, &customize, &settings_item, &separator, &quit])?
                };

                let tray = TrayIconBuilder::with_id("openquota-tray")
                    .icon(
                        app.default_window_icon()
                            .expect("OpenQuota requires a bundled application icon")
                            .clone(),
                    )
                    .menu(&menu);
                #[cfg(not(target_os = "linux"))]
                let tray = tray.tooltip("OpenQuota").show_menu_on_left_click(false);
                let tray = tray.on_menu_event(|app, event| match event.id.as_ref() {
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
                });
                #[cfg(not(target_os = "linux"))]
                let tray = tray.on_tray_icon_event(|tray, event| {
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
                });
                tray.build(app)?;
                app_info!("lifecycle", "system tray integration ready");
            }

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

            tray_presentation::update(
                app.handle(),
                &service.state(),
                &settings.get(),
                settings.registry(),
            );
            spawn_startup_credential_detection(
                app.handle().clone(),
                registry,
                service.clone(),
                settings.clone(),
                notifications.clone(),
                credential_detection_plan,
            );
            refresh_loop::spawn(app.handle().clone(), service, settings, notifications);
            app_info!("lifecycle", "OpenQuota startup completed");

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap::get_bootstrap_state,
            commands::provider::open_provider_link,
            commands::usage::refresh_usage,
            commands::usage::refresh_provider_usage,
            commands::settings::get_app_settings,
            commands::settings::save_app_settings,
            commands::settings::reset_customization,
            commands::settings::reset_provider_customization,
            commands::settings::request_notification_permission,
            commands::settings::open_notification_settings,
            commands::settings::get_log_path,
            commands::settings::open_log_folder,
            commands::window::dismiss_main_window,
            commands::window::resize_main_window,
            commands::window::quit_app,
            updates::check_for_updates,
            updates::install_update,
            updates::open_update_page
        ])
        .on_window_event(handle_window_event)
        .run(tauri::generate_context!())
        .expect("error while running OpenQuota");
}
