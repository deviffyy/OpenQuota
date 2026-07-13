use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::{
    commands::settings::settings_view_state,
    models::SettingsViewState,
    service::{ProviderService, UsageViewState},
    settings::SettingsService,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapState {
    pub usage: UsageViewState,
    pub settings: SettingsViewState,
}

#[tauri::command]
pub fn get_bootstrap_state(
    app: AppHandle,
    service: State<'_, Arc<ProviderService>>,
    settings: State<'_, Arc<SettingsService>>,
) -> BootstrapState {
    BootstrapState {
        usage: service.state(),
        settings: settings_view_state(&app, &settings),
    }
}
