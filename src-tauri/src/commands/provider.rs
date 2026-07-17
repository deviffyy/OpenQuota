use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;
use zeroize::Zeroizing;

use crate::{
    commands::settings::settings_view_state,
    models::{ApiKeyStatus, ProviderApiKeyState, ProviderLink},
    notifications::finish_refresh,
    pacing::NotificationEvaluator,
    providers::ProviderRegistry,
    service::ProviderService,
    settings::SettingsService,
    tray_presentation,
};

fn resolve_provider_link<'a>(
    registry: &'a ProviderRegistry,
    provider_id: &str,
    link_index: usize,
) -> Result<&'a ProviderLink, String> {
    registry
        .definition(provider_id)
        .and_then(|provider| provider.links.get(link_index))
        .ok_or_else(|| "That provider link is unavailable.".to_owned())
}

#[tauri::command]
pub fn open_provider_link(
    app: AppHandle,
    registry: State<'_, Arc<ProviderRegistry>>,
    provider_id: String,
    link_index: usize,
) -> Result<(), String> {
    let link = resolve_provider_link(&registry, &provider_id, link_index)?;
    crate::app_debug!(
        "http",
        "opening {provider_id} provider link {}",
        crate::logging::redact_url(&link.url)
    );
    app.opener()
        .open_url(&link.url, None::<&str>)
        .map_err(|_| "That provider link could not be opened.".to_owned())
}

async fn api_key_state(
    registry: Arc<ProviderRegistry>,
    provider_id: String,
) -> Result<Option<ProviderApiKeyState>, String> {
    let runtime = registry
        .runtime(&provider_id)
        .ok_or_else(|| "Unknown provider.".to_owned())?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(status) = runtime.api_key_status() else {
            return Ok(None);
        };
        let status = status.map_err(|error| error.to_string())?;
        Ok(Some(ProviderApiKeyState {
            provider_id,
            status,
        }))
    })
    .await
    .map_err(|_| "The API key status could not be read.".to_owned())?
}

#[tauri::command]
pub async fn get_provider_api_key_state(
    registry: State<'_, Arc<ProviderRegistry>>,
    provider_id: String,
) -> Result<Option<ProviderApiKeyState>, String> {
    api_key_state(registry.inner().clone(), provider_id).await
}

#[tauri::command]
pub async fn save_provider_api_key(
    app: AppHandle,
    registry: State<'_, Arc<ProviderRegistry>>,
    service: State<'_, Arc<ProviderService>>,
    settings: State<'_, Arc<SettingsService>>,
    notifications: State<'_, Arc<NotificationEvaluator>>,
    provider_id: String,
    api_key: String,
) -> Result<ProviderApiKeyState, String> {
    let api_key = Zeroizing::new(api_key);
    let runtime = registry
        .runtime(&provider_id)
        .ok_or_else(|| "Unknown provider.".to_owned())?;
    let provider_for_save = provider_id.clone();
    let state = tauri::async_runtime::spawn_blocking(move || {
        if runtime.api_key_status().is_none() {
            return Err("That provider does not accept an API key.".to_owned());
        }
        runtime
            .save_api_key(api_key.as_str())
            .map_err(|error| error.to_string())?;
        let status = runtime
            .api_key_status()
            .and_then(Result::ok)
            .ok_or_else(|| "The saved API key status could not be read.".to_owned())?;
        Ok(ProviderApiKeyState {
            provider_id: provider_for_save,
            status,
        })
    })
    .await
    .map_err(|_| "The API key could not be saved.".to_owned())??;

    let updated = settings.apply_provider_credential_state(&provider_id, true, true)?;
    tray_presentation::update(&app, &service.state(), &updated, settings.registry());
    let _ = app.emit("settings-state", settings_view_state(&app, &settings));
    service.refresh(&provider_id, true).await;
    let usage = service.state();
    let _ = app.emit("usage-state", &usage);
    finish_refresh(&app, &usage, &settings, &notifications);
    crate::app_info!("auth", "API key saved for {provider_id}");
    Ok(state)
}

#[tauri::command]
pub async fn delete_provider_api_key(
    app: AppHandle,
    registry: State<'_, Arc<ProviderRegistry>>,
    service: State<'_, Arc<ProviderService>>,
    settings: State<'_, Arc<SettingsService>>,
    notifications: State<'_, Arc<NotificationEvaluator>>,
    provider_id: String,
) -> Result<ProviderApiKeyState, String> {
    let runtime = registry
        .runtime(&provider_id)
        .ok_or_else(|| "Unknown provider.".to_owned())?;
    let provider_for_delete = provider_id.clone();
    let state = tauri::async_runtime::spawn_blocking(move || {
        if runtime.api_key_status().is_none() {
            return Err("That provider does not accept an API key.".to_owned());
        }
        runtime
            .delete_api_key()
            .map_err(|error| error.to_string())?;
        let status = runtime
            .api_key_status()
            .and_then(Result::ok)
            .ok_or_else(|| "The API key status could not be read.".to_owned())?;
        Ok(ProviderApiKeyState {
            provider_id: provider_for_delete,
            status,
        })
    })
    .await
    .map_err(|_| "The API key could not be removed.".to_owned())??;

    let detected = state.status != ApiKeyStatus::NotSet;
    let updated = settings.apply_provider_credential_state(&provider_id, detected, false)?;
    tray_presentation::update(&app, &service.state(), &updated, settings.registry());
    let _ = app.emit("settings-state", settings_view_state(&app, &settings));
    if updated
        .providers
        .iter()
        .any(|provider| provider.id == provider_id && provider.enabled)
    {
        service.refresh(&provider_id, true).await;
        let usage = service.state();
        let _ = app.emit("usage-state", &usage);
        finish_refresh(&app, &usage, &settings, &notifications);
    }
    crate::app_info!("auth", "saved API key removed for {provider_id}");
    Ok(state)
}

#[cfg(test)]
mod tests {
    use crate::{
        models::{MetricDefinition, MetricSection, MetricSource, ProviderDefinition, ProviderLink},
        providers::ProviderRegistry,
    };

    use super::resolve_provider_link;

    fn registry() -> ProviderRegistry {
        ProviderRegistry::from_definitions(vec![ProviderDefinition {
            id: "provider".into(),
            display_name: "Provider".into(),
            short_name: "P".into(),
            fallback_enabled: true,
            local_usage_source_note: None,
            links: vec![ProviderLink::new("Status", "https://status.example.com/")],
            metrics: vec![MetricDefinition::new(
                "provider.session",
                "Session",
                MetricSource::Quota {
                    source_id: "session".into(),
                    session_window: true,
                },
                true,
                true,
                MetricSection::AlwaysVisible,
                true,
                Some("S"),
                None,
            )],
        }])
        .unwrap()
    }

    #[test]
    fn resolves_only_links_declared_by_the_provider_registry() {
        let registry = registry();

        assert_eq!(
            resolve_provider_link(&registry, "provider", 0).unwrap().url,
            "https://status.example.com/"
        );
        assert!(resolve_provider_link(&registry, "provider", 1).is_err());
        assert!(resolve_provider_link(&registry, "unknown", 0).is_err());
    }
}
