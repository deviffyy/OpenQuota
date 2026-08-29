mod child_process;
mod commands;
mod desktop_integration;
mod hashing;
mod logging;
#[cfg(any(target_os = "macos", test))]
mod menu_bar;
mod models;
mod notifications;
mod pacing;
mod policy;
mod popup;
mod pricing;
mod provider_environment;
mod providers;
mod refresh_loop;
mod service;
mod settings;
mod storage;
#[cfg(any(not(target_os = "macos"), test))]
mod tray_icon;
mod tray_presentation;
mod updates;
mod webview_memory;
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
    App, AppHandle, Emitter, Manager,
};
#[cfg(not(target_os = "linux"))]
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::{
    desktop_integration::DesktopIntegration,
    pacing::NotificationEvaluator,
    pricing::PricingStore,
    providers::{
        antigravity::AntigravityProvider, claude, codex,
        codex::reset_claim::CodexResetClaimService, copilot::CopilotProvider,
        cursor::CursorProvider, detect_local_credentials, devin::DevinProvider, grok::GrokProvider,
        kimi::KimiProvider, minimax::MiniMaxProvider, opencode::OpenCodeProvider,
        openrouter::OpenRouterProvider, zai::ZaiProvider, ProviderRegistry, UsageProvider,
    },
    storage::Storage,
    window::{
        handle_window_event, open_screen, show_main_window, toggle_main_window, PanelResizeSession,
        MAIN_WINDOW,
    },
};

fn install_tray(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    let menu = {
        let settings_item =
            MenuItem::with_id(app, "settings", "Settings", true, Some("CmdOrCtrl+,"))?;
        let separator = PredefinedMenuItem::separator(app)?;
        let quit = MenuItem::with_id(app, "quit", "Quit OpenQuota", true, Some("CmdOrCtrl+Q"))?;
        Menu::with_items(app, &[&settings_item, &separator, &quit])?
    };
    #[cfg(not(target_os = "macos"))]
    let menu = {
        let summary = MenuItem::with_id(app, "summary", "Average Limit: ...", false, None::<&str>)?;
        let separator_top = PredefinedMenuItem::separator(app)?;
        let open = MenuItem::with_id(app, "open", "Open Dashboard", true, None::<&str>)?;
        let customize = MenuItem::with_id(app, "customize", "Customize…", true, None::<&str>)?;
        let settings_item = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
        let separator = PredefinedMenuItem::separator(app)?;
        let quit = MenuItem::with_id(app, "quit", "Quit OpenQuota", true, None::<&str>)?;
        Menu::with_items(
            app,
            &[
                &summary,
                &separator_top,
                &open,
                &customize,
                &settings_item,
                &separator,
                &quit,
            ],
        )?
    };
    app.manage(menu.clone());

    let icon = app
        .default_window_icon()
        .ok_or_else(|| std::io::Error::other("OpenQuota application icon is unavailable"))?
        .clone();
    let tray = TrayIconBuilder::with_id("openquota-tray")
        .icon(icon)
        .menu(&menu);
    #[cfg(not(target_os = "linux"))]
    let tray = tray.tooltip("OpenQuota").show_menu_on_left_click(false);
    let tray = tray.on_menu_event(|app, event| match event.id.as_ref() {
        "open" => {
            app.state::<PopupDismissGuard>().cancel_pending();
            if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                show_main_window(&window);
            }
        }
        "customize" => open_screen(app, "customize"),
        "settings" => open_screen(app, "settings"),
        "quit" => {
            if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                window::finish_native_panel_resize(&window);
            }
            app.exit(0);
        }
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
            toggle_main_window(tray.app_handle());
        }
    });
    tray.build(app)?;
    Ok(())
}

fn show_standalone_window_fallback(window: &tauri::WebviewWindow) {
    window
        .app_handle()
        .state::<DesktopIntegration>()
        .set_floating(true);
    let _ = window.set_resizable(false);
    let _ = window.set_skip_taskbar(false);
    let _ = window.set_always_on_top(false);
    let _ = window.center();
    show_main_window(window);
}

#[cfg(target_os = "linux")]
fn apply_linux_tray_fallback(app: &AppHandle) {
    let integration = app.state::<DesktopIntegration>();
    if !integration.disable_tray() {
        return;
    }
    app_warn!(
        "lifecycle",
        "system tray became unavailable; using standalone window"
    );
    let _ = app.remove_tray_by_id("openquota-tray");
    app.state::<PopupDismissGuard>().cancel_pending();

    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let mode = app.state::<Arc<SettingsService>>().get().window_mode;
        match window::apply_window_mode(&window, mode, true) {
            Ok(()) => show_main_window(&window),
            Err(error) => {
                app_warn!(
                    "window",
                    "standalone fallback could not apply window mode: {error}"
                );
                show_standalone_window_fallback(&window);
            }
        }
    }

    let settings = app.state::<Arc<SettingsService>>();
    let state = commands::settings::settings_view_state(app, settings.inner().as_ref());
    let _ = app.emit("settings-state", state);
}

#[cfg(target_os = "linux")]
fn spawn_status_notifier_monitor(app: AppHandle) {
    if desktop_integration::status_notifier_monitor_forced_off() {
        return;
    }
    let monitor_app = app.clone();
    if std::thread::Builder::new()
        .name("openquota-tray-monitor".to_owned())
        .spawn(move || {
            if let Err(error) = desktop_integration::wait_for_status_notifier_loss() {
                app_warn!("lifecycle", "system tray monitor stopped: {error}");
            }
            let fallback_app = monitor_app.clone();
            if monitor_app
                .run_on_main_thread(move || apply_linux_tray_fallback(&fallback_app))
                .is_err()
            {
                app_warn!(
                    "lifecycle",
                    "standalone tray fallback could not be scheduled"
                );
            }
        })
        .is_err()
    {
        app_warn!("lifecycle", "system tray monitor could not be started");
        apply_linux_tray_fallback(&app);
    }
}

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
        let command_guard = settings.lock_command_mutation().await;
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
            detected
                .values()
                .filter(|status| { **status == providers::CredentialProbeStatus::Detected })
                .count(),
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
        drop(command_guard);
        if outcome.newly_enabled_provider_ids.is_empty() {
            return;
        }
        let progress_app = app.clone();
        service
            .refresh_enabled_with_progress(
                &outcome.newly_enabled_provider_ids,
                true,
                move |state| {
                    let _ = progress_app.emit("usage-state", state);
                },
            )
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
                toggle_main_window(app);
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
                "desktop integration detected (tray={})",
                desktop_integration.tray_available()
            );
            app.manage(desktop_integration.clone());

            let app_data_dir = app.path().app_data_dir()?;
            let database_path = app_data_dir.join("openquota.db");
            let storage = Arc::new(Storage::open(&database_path)?);
            provider_environment::initialize(storage.load_provider_environment()?);
            provider_environment::refresh_for_next_launch(storage.clone());
            app.manage(Arc::new(PanelResizeSession::new(storage.clone())));
            app_debug!("cache", "application database opened");
            let pricing = Arc::new(PricingStore::new(app_data_dir.join("pricing"))?);
            let mut providers = claude::runtimes(storage.clone(), pricing.clone())?;
            providers.extend(
                codex::runtimes(storage.clone(), pricing.clone()).unwrap_or_else(|error| {
                    crate::app_error!("startup", "failed to launch codex runtimes ({error})");
                    Vec::new()
                }),
            );
            providers.extend(vec![
                Arc::new(CursorProvider::new(pricing.clone())?) as Arc<dyn UsageProvider>,
                Arc::new(AntigravityProvider::new(
                    app_data_dir.join("antigravity").join("auth.json"),
                )?) as Arc<dyn UsageProvider>,
                Arc::new(CopilotProvider::new()?) as Arc<dyn UsageProvider>,
                Arc::new(DevinProvider::new()?) as Arc<dyn UsageProvider>,
                Arc::new(GrokProvider::new(storage.clone(), pricing.clone())?)
                    as Arc<dyn UsageProvider>,
                Arc::new(OpenCodeProvider::new(pricing.clone())) as Arc<dyn UsageProvider>,
                Arc::new(OpenRouterProvider::new()?) as Arc<dyn UsageProvider>,
                Arc::new(ZaiProvider::new()?) as Arc<dyn UsageProvider>,
                Arc::new(KimiProvider::new()?) as Arc<dyn UsageProvider>,
                Arc::new(MiniMaxProvider::new()?) as Arc<dyn UsageProvider>,
            ]);
            let registry = Arc::new(ProviderRegistry::new(providers)?);
            let (settings_service, credential_detection_plan) =
                SettingsService::new_deferred(storage.clone(), registry.clone())?;
            let settings = Arc::new(settings_service);
            let floating_window = desktop_integration.apply_window_mode(settings.get().window_mode);
            let service = Arc::new(ProviderService::new_with_settings(
                registry.clone(),
                storage.clone(),
                settings.clone(),
            ));
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
            app.manage(Arc::new(CodexResetClaimService::new()?));

            if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                if window::apply_panel_surface(&window, settings.get().theme).is_err() {
                    app_warn!("window", "initial panel surface theme could not be applied");
                }
                if !floating_window {
                    webview_memory::set_inactive(&window, true);
                }
            }

            if let Some(shortcut) = settings.get().global_shortcut {
                let _ = register_shortcut(app.handle(), &shortcut);
            }

            let tray_installed = if desktop_integration.tray_available() {
                match install_tray(app) {
                    Ok(()) => {
                        app_info!("lifecycle", "system tray integration ready");
                        true
                    }
                    Err(error) => {
                        app_warn!(
                            "lifecycle",
                            "system tray integration failed; using standalone window: {error}"
                        );
                        desktop_integration.disable_tray();
                        let _ = app.remove_tray_by_id("openquota-tray");
                        false
                    }
                }
            } else {
                false
            };

            if desktop_integration.is_floating() {
                if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                    if let Err(error) =
                        window::apply_window_mode(&window, settings.get().window_mode, true)
                    {
                        app_warn!(
                            "window",
                            "standalone startup mode could not be applied: {error}"
                        );
                        show_standalone_window_fallback(&window);
                    }
                }
            }

            #[cfg(target_os = "linux")]
            if tray_installed {
                spawn_status_notifier_monitor(app.handle().clone());
            }
            #[cfg(not(target_os = "linux"))]
            let _ = tray_installed;

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
            commands::provider::get_provider_api_key_state,
            commands::provider::save_provider_api_key,
            commands::provider::delete_provider_api_key,
            commands::usage::refresh_usage,
            commands::usage::refresh_provider_usage,
            commands::usage::claim_codex_reset_credit,
            commands::settings::get_app_settings,
            commands::settings::save_app_settings,
            commands::settings::reset_customization,
            commands::settings::reset_all_settings,
            commands::settings::reset_provider_customization,
            commands::settings::request_notification_permission,
            commands::settings::open_notification_settings,
            commands::settings::get_log_path,
            commands::settings::open_log_folder,
            commands::window::dismiss_main_window,
            commands::window::get_panel_resize_edge,
            commands::window::get_panel_height_mode,
            commands::window::fit_panel_to_content,
            commands::window::set_panel_height_automatic,
            commands::window::set_panel_height_manual,
            commands::window::begin_panel_resize,
            commands::window::lock_panel_resize_axis,
            commands::window::quit_app,
            updates::check_for_updates,
            updates::install_update,
            updates::open_update_page
        ])
        .on_window_event(handle_window_event)
        .run(tauri::generate_context!())
        .expect("error while running OpenQuota");
}
