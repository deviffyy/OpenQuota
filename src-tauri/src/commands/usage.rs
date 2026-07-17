use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::{
    notifications::finish_refresh,
    pacing::NotificationEvaluator,
    providers::codex::reset_claim::{CodexResetClaimService, ResetClaimOutcome},
    service::{ProviderService, UsageViewState},
    settings::SettingsService,
};

#[tauri::command]
pub async fn claim_codex_reset_credit(
    app: AppHandle,
    claims: State<'_, Arc<CodexResetClaimService>>,
    service: State<'_, Arc<ProviderService>>,
    settings: State<'_, Arc<SettingsService>>,
    notifications: State<'_, Arc<NotificationEvaluator>>,
    expires_at: chrono::DateTime<chrono::Utc>,
    redeem_request_id: String,
) -> Result<ResetClaimOutcome, String> {
    if !settings
        .enabled_provider_ids()
        .iter()
        .any(|id| id == "codex")
    {
        return Err("Codex is not enabled.".to_owned());
    }
    let claims = claims.inner().clone();
    let outcome =
        tauri::async_runtime::spawn_blocking(move || claims.claim(expires_at, &redeem_request_id))
            .await
            .map_err(|_| "The reset claim could not be completed.".to_owned())?;

    if outcome != ResetClaimOutcome::Failed {
        service.refresh("codex", true).await;
        let state = service.state();
        let _ = app.emit("usage-state", &state);
        finish_refresh(&app, &state, &settings, &notifications);
    }
    Ok(outcome)
}

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
