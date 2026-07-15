use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::{
    notifications::finish_refresh,
    pacing::NotificationEvaluator,
    service::{ProviderService, UsageViewState},
    settings::SettingsService,
};

#[tauri::command]
pub async fn refresh_usage(
    app: AppHandle,
    service: State<'_, Arc<ProviderService>>,
    settings: State<'_, Arc<SettingsService>>,
    notifications: State<'_, Arc<NotificationEvaluator>>,
) -> Result<UsageViewState, ()> {
    let state = service
        .refresh_all(&settings.enabled_provider_ids(), true)
        .await;
    let _ = app.emit("usage-state", &state);
    finish_refresh(&app, &state, &settings, &notifications);
    Ok(state)
}

#[tauri::command]
pub async fn refresh_provider_usage(
    app: AppHandle,
    service: State<'_, Arc<ProviderService>>,
    settings: State<'_, Arc<SettingsService>>,
    notifications: State<'_, Arc<NotificationEvaluator>>,
    provider_id: String,
) -> Result<UsageViewState, String> {
    if !settings.enabled_provider_ids().contains(&provider_id) {
        return Err("Provider is not enabled.".to_owned());
    }

    service.refresh(&provider_id, true).await;
    let state = service.state();
    let _ = app.emit("usage-state", &state);
    finish_refresh(&app, &state, &settings, &notifications);
    Ok(state)
}
