use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::{
    notifications::finish_refresh, pacing::NotificationEvaluator, policy::REFRESH_INTERVAL,
    service::ProviderService, settings::SettingsService,
};

pub fn spawn(
    app: AppHandle,
    service: Arc<ProviderService>,
    settings: Arc<SettingsService>,
    notifications: Arc<NotificationEvaluator>,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            let provider_ids = settings.enabled_provider_ids();
            if !provider_ids.is_empty() {
                let state = service.refresh_all(&provider_ids, false).await;
                let _ = app.emit("usage-state", &state);
                finish_refresh(&app, &state, &settings, &notifications);
            }
            tokio::time::sleep(REFRESH_INTERVAL).await;
        }
    });
}
