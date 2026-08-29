use std::sync::{atomic::AtomicU64, Arc};

use tauri::{AppHandle, Emitter, State};

use crate::{
    commands::settings::emit_settings_if_account_changed,
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
        .any(|id| crate::providers::provider_family(id) == "codex")
    {
        return Err("Codex is not enabled.".to_owned());
    }
    let claims = claims.inner().clone();
    let outcome =
        tauri::async_runtime::spawn_blocking(move || claims.claim(expires_at, &redeem_request_id))
            .await
            .map_err(|_| "The reset claim could not be completed.".to_owned())?;

    if outcome != ResetClaimOutcome::Failed {
        let observed_account_revision = AtomicU64::new(settings.account_revision());
        let codex_ids: Vec<_> = settings
            .enabled_provider_ids()
            .into_iter()
            .filter(|id| crate::providers::provider_family(id) == "codex")
            .collect();
        for codex_id in &codex_ids {
            service.refresh(codex_id, true).await;
        }
        let state = service.state();
        emit_settings_if_account_changed(&app, &settings, &observed_account_revision);
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
    let progress_app = app.clone();
    let progress_settings = settings.inner().clone();
    let observed_account_revision = Arc::new(AtomicU64::new(settings.account_revision()));
    let progress_account_revision = observed_account_revision.clone();
    let state = service
        .refresh_all_with_progress(&settings.enabled_provider_ids(), true, move |state| {
            emit_settings_if_account_changed(
                &progress_app,
                &progress_settings,
                &progress_account_revision,
            );
            let _ = progress_app.emit("usage-state", state);
        })
        .await;
    emit_settings_if_account_changed(&app, &settings, &observed_account_revision);
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

    let observed_account_revision = AtomicU64::new(settings.account_revision());
    service.refresh(&provider_id, true).await;
    let state = service.state();
    emit_settings_if_account_changed(&app, &settings, &observed_account_revision);
    let _ = app.emit("usage-state", &state);
    finish_refresh(&app, &state, &settings, &notifications);
    Ok(state)
}
