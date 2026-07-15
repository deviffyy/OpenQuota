use std::thread;

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::{NotificationExt, PermissionState};

use crate::{
    pacing::{NotificationEvaluator, PaceAlert},
    popup::PopupDismissGuard,
    service::UsageViewState,
    settings::SettingsService,
    tray_presentation,
    window::{show_popup, MAIN_WINDOW},
};

pub fn permission(app: &AppHandle) -> &'static str {
    match app.notification().permission_state() {
        Ok(PermissionState::Granted) => "granted",
        Ok(PermissionState::Denied) => "denied",
        Ok(PermissionState::Prompt | PermissionState::PromptWithRationale) => "prompt",
        Err(_) => "unavailable",
    }
}

pub fn finish_refresh(
    app: &AppHandle,
    state: &UsageViewState,
    settings: &SettingsService,
    notifications: &NotificationEvaluator,
) {
    let preferences = settings.get();
    tray_presentation::update(app, state, &preferences, settings.registry());
    notifications.prune(&preferences);
    for provider_state in state.providers.values() {
        if provider_state.error.is_none() {
            if let Some(snapshot) = provider_state.snapshot.as_ref() {
                let alerts = notifications.evaluate(
                    snapshot,
                    &preferences,
                    settings.registry(),
                    chrono::Utc::now(),
                );
                let failed = deliver(app, &alerts);
                if !failed.is_empty() {
                    notifications.rollback(&failed);
                }
            }
        }
    }
}

fn deliver(app: &AppHandle, alerts: &[PaceAlert]) -> Vec<PaceAlert> {
    if permission(app) != "granted" {
        return alerts.to_vec();
    }
    alerts
        .iter()
        .filter_map(|alert| {
            show(
                app,
                alert.milestone.title(),
                &format!(
                    "{} · {}\n{}",
                    alert.provider,
                    alert.metric,
                    alert.milestone.body()
                ),
            )
            .is_err()
            .then_some(alert.clone())
        })
        .collect()
}

fn show(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    let mut notification = notify_rust::Notification::new();
    notification.summary(title).body(body).appname("OpenQuota");
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    notification.action("default", "Open OpenQuota");
    #[cfg(target_os = "windows")]
    notification.app_id(&app.config().identifier);
    #[cfg(target_os = "macos")]
    let _ = notify_rust::set_application(if tauri::is_dev() {
        "com.apple.Terminal"
    } else {
        &app.config().identifier
    });

    let handle = notification
        .show()
        .map_err(|_| "The notification could not be delivered.".to_owned())?;
    let app = app.clone();
    thread::spawn(move || {
        let _ = handle.wait_for_response(move |response: &notify_rust::NotificationResponse| {
            if !response_opens_window(response) {
                return;
            }
            let app_for_window = app.clone();
            let _ = app.run_on_main_thread(move || {
                app_for_window.state::<PopupDismissGuard>().cancel_pending();
                if let Some(window) = app_for_window.get_webview_window(MAIN_WINDOW) {
                    show_popup(&window);
                }
            });
        });
    });
    Ok(())
}

fn response_opens_window(response: &notify_rust::NotificationResponse) -> bool {
    matches!(
        response,
        notify_rust::NotificationResponse::Default | notify_rust::NotificationResponse::Action(_)
    )
}

#[cfg(test)]
mod tests {
    use notify_rust::{CloseReason, NotificationResponse};

    use super::response_opens_window;

    #[test]
    fn notification_clicks_open_the_window_but_dismissals_do_not() {
        assert!(response_opens_window(&NotificationResponse::Default));
        assert!(response_opens_window(&NotificationResponse::Action(
            "open".into()
        )));
        assert!(!response_opens_window(&NotificationResponse::Closed(
            CloseReason::Dismissed
        )));
    }
}
