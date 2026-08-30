use std::{
    collections::BTreeMap,
    env,
    fmt,
    path::PathBuf,
    sync::Arc,
};

use chrono::{DateTime, Local, Utc};
use serde::Serialize;

use crate::{
    logging,
    models::{
        MetricValue, MetricValueKind, QuotaFormat, QuotaWindow, SnapshotSource, StatusMetric,
        UsagePeriod, ValueMetric,
    },
    pricing::PricingStore,
    provider_environment,
    providers::ProviderRegistry,
    service::{ProviderService, UsageViewState},
    settings::SettingsService,
    storage::Storage,
};

const APP_IDENTIFIER: &str = "io.github.deviffyy.openquota";

#[derive(Debug)]
pub(crate) struct RuntimeError(String);

impl RuntimeError {
    fn from_display(error: impl fmt::Display) -> Self {
        Self(error.to_string())
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RuntimeError {}

pub(crate) struct RuntimeServices {
    pub(crate) storage: Arc<Storage>,
    pub(crate) registry: Arc<ProviderRegistry>,
    pub(crate) service: Arc<ProviderService>,
    pub(crate) settings: Arc<SettingsService>,
    pub(crate) credential_detection_plan: crate::settings::CredentialDetectionPlan,
}

pub(crate) fn initialize_services(
    data_directory: PathBuf,
) -> Result<RuntimeServices, RuntimeError> {
    let database_path = data_directory.join("openquota.db");
    let storage = Arc::new(Storage::open(&database_path).map_err(RuntimeError::from_display)?);
    provider_environment::initialize(
        storage
            .load_provider_environment()
            .map_err(RuntimeError::from_display)?,
    );

    let pricing = Arc::new(
        PricingStore::new(data_directory.join("pricing")).map_err(RuntimeError::from_display)?,
    );
    let registry = crate::build_provider_registry(&data_directory, storage.clone(), pricing)
        .map_err(RuntimeError::from_display)?;
    let (settings_service, credential_detection_plan) =
        SettingsService::new_deferred(storage.clone(), registry.clone())
            .map_err(RuntimeError::from_display)?;
    let settings = Arc::new(settings_service);
    let service = Arc::new(ProviderService::new_with_settings(
        registry.clone(),
        storage.clone(),
        settings.clone(),
    ));

    Ok(RuntimeServices {
        storage,
        registry,
        service,
        settings,
        credential_detection_plan,
    })
}

pub(crate) fn data_directory() -> Result<PathBuf, RuntimeError> {
    if let Some(path) = env::var_os("OPENQUOTA_DATA_DIR").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    let home = || {
        env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .map(PathBuf::from)
    };

    #[cfg(target_os = "windows")]
    {
        return env::var_os("APPDATA")
            .filter(|path| !path.is_empty())
            .or_else(|| env::var_os("LOCALAPPDATA").filter(|path| !path.is_empty()))
            .map(PathBuf::from)
            .map(|path| path.join(APP_IDENTIFIER))
            .or_else(|| {
                home().map(|path| path.join("AppData").join("Roaming").join(APP_IDENTIFIER))
            })
            .ok_or_else(missing_data_directory);
    }

    #[cfg(target_os = "macos")]
    {
        return home()
            .map(|path| {
                path.join("Library")
                    .join("Application Support")
                    .join(APP_IDENTIFIER)
            })
            .ok_or_else(missing_data_directory);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return env::var_os("XDG_DATA_HOME")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .or_else(|| home().map(|path| path.join(".local").join("share")))
            .map(|path| path.join(APP_IDENTIFIER))
            .ok_or_else(missing_data_directory);
    }

    #[allow(unreachable_code)]
    Err(RuntimeError("Could not determine the OpenQuota data directory.".into()))
}

fn missing_data_directory() -> RuntimeError {
    RuntimeError("Could not determine the OpenQuota data directory.".into())
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    Version,
    Providers { json: bool },
    Status(StatusOptions),
}

#[derive(Debug, PartialEq, Eq)]
struct StatusOptions {
    json: bool,
    cached: bool,
    provider: Option<String>,
}

impl Default for StatusOptions {
    fn default() -> Self {
        Self {
            json: false,
            cached: false,
            provider: None,
        }
    }
}

const HELP: &str = "OpenQuota command line interface

Usage:
  openquota status [options]       Show provider usage (refreshes by default)
  openquota providers [--json]     List available providers
  openquota --help                 Show this help
  openquota --version              Show the version

Status options:
  --provider <id>                  Limit the output to one provider
  --cached                         Use the last cached values without refreshing
  --json                            Print machine-readable JSON

Environment:
  OPENQUOTA_DATA_DIR                Override the application data directory
";

fn parse_args(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("No command was provided.".into());
    }
    if matches!(args[0].as_str(), "-h" | "--help") {
        return Ok(Command::Help);
    }
    if matches!(args[0].as_str(), "-V" | "--version") {
        return Ok(Command::Version);
    }

    let (command, options) = if args[0] == "status" {
        ("status", &args[1..])
    } else if args[0] == "providers" {
        ("providers", &args[1..])
    } else if args[0].starts_with('-') {
        ("status", args)
    } else {
        return Err(format!("Unknown command '{}'.", args[0]));
    };

    if command == "providers" {
        let mut json = false;
        for option in options {
            match option.as_str() {
                "--json" => json = true,
                "-h" | "--help" => return Ok(Command::Help),
                other => return Err(format!("Unknown providers option '{other}'.")),
            }
        }
        return Ok(Command::Providers { json });
    }

    let mut parsed = StatusOptions::default();
    let mut index = 0;
    while index < options.len() {
        match options[index].as_str() {
            "--json" => parsed.json = true,
            "--cached" => parsed.cached = true,
            "-h" | "--help" => return Ok(Command::Help),
            "--provider" => {
                index += 1;
                let Some(provider) = options.get(index) else {
                    return Err("--provider requires a provider id.".into());
                };
                if provider.starts_with('-') || provider.is_empty() {
                    return Err("--provider requires a provider id.".into());
                }
                parsed.provider = Some(provider.clone());
            }
            option if option.starts_with("--provider=") => {
                let provider = option.trim_start_matches("--provider=");
                if provider.is_empty() {
                    return Err("--provider requires a provider id.".into());
                }
                parsed.provider = Some(provider.to_owned());
            }
            other => return Err(format!("Unknown status option '{other}'.")),
        }
        index += 1;
    }
    Ok(Command::Status(parsed))
}

pub fn run_if_requested() -> Option<i32> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return None;
    }

    match parse_args(&args) {
        Ok(Command::Help) => {
            print!("{HELP}");
            Some(0)
        }
        Ok(Command::Version) => {
            println!("OpenQuota {}", env!("CARGO_PKG_VERSION"));
            Some(0)
        }
        Ok(Command::Providers { json }) => match run_providers(json) {
            Ok(()) => Some(0),
            Err(error) => report_error(error, 1),
        },
        Ok(Command::Status(options)) => match run_status(options) {
            Ok(code) => Some(code),
            Err(error) => report_error(error, 1),
        },
        Err(error) => report_error(format!("{error}\n\n{HELP}"), 2),
    }
}

fn report_error(error: impl fmt::Display, code: i32) -> Option<i32> {
    eprintln!("OpenQuota: {error}");
    Some(code)
}

fn run_providers(json: bool) -> Result<(), RuntimeError> {
    let runtime = initialize_for_cli()?;
    let settings = runtime.settings.get();
    let providers = runtime
        .registry
        .catalog()
        .providers
        .iter()
        .map(|definition| CliProvider {
            id: definition.id.clone(),
            name: settings.provider_display_name(definition).to_owned(),
            enabled: settings
                .providers
                .iter()
                .find(|provider| provider.id == definition.id)
                .is_some_and(|provider| provider.enabled),
        })
        .collect::<Vec<_>>();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&providers).map_err(RuntimeError::from_display)?
        );
    } else {
        println!("Available providers:");
        for provider in providers {
            println!(
                "  {:<14} {}{}",
                provider.id,
                provider.name,
                if provider.enabled { " (enabled)" } else { "" }
            );
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliProvider {
    id: String,
    name: String,
    enabled: bool,
}

fn run_status(options: StatusOptions) -> Result<i32, RuntimeError> {
    let runtime = initialize_for_cli()?;
    let configured_ids = runtime.settings.enabled_provider_ids();
    let provider_ids = if let Some(provider) = options.provider.as_deref() {
        if runtime.registry.definition(provider).is_none() {
            return Err(RuntimeError(format!("Unknown provider '{provider}'.")));
        }
        vec![provider.to_owned()]
    } else if configured_ids.is_empty() {
        return Err(RuntimeError("No providers are enabled.".into()));
    } else {
        configured_ids
    };

    let state = if options.cached {
        runtime.service.state()
    } else if options.provider.is_some() {
        tauri::async_runtime::block_on(runtime.service.refresh(
            provider_ids.first().expect("provider id was selected"),
            true,
        ));
        runtime.service.state()
    } else {
        tauri::async_runtime::block_on(runtime.service.refresh_enabled_with_progress(
            &provider_ids,
            true,
            |_| {},
        ))
    };
    let state = select_providers(state, &provider_ids);

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&state).map_err(RuntimeError::from_display)?
        );
    } else {
        print_human_status(&state, &runtime, options.cached);
    }

    if state
        .providers
        .values()
        .any(|provider| provider.error.is_some())
    {
        Ok(1)
    } else {
        Ok(0)
    }
}

fn initialize_for_cli() -> Result<RuntimeServices, RuntimeError> {
    logging::set_level(crate::models::LogLevel::Error);
    initialize_services(data_directory()?)
}

fn select_providers(state: UsageViewState, provider_ids: &[String]) -> UsageViewState {
    let providers = provider_ids
        .iter()
        .filter_map(|id| state.providers.get(id).cloned().map(|state| (id.clone(), state)))
        .collect::<BTreeMap<_, _>>();
    UsageViewState {
        providers,
        last_full_refresh_at: state.last_full_refresh_at,
    }
}

fn print_human_status(state: &UsageViewState, runtime: &RuntimeServices, cached: bool) {
    println!(
        "OpenQuota status{}",
        if cached { " (cached)" } else { "" }
    );
    for (id, provider) in &state.providers {
        let Some(definition) = runtime.registry.definition(id) else {
            continue;
        };
        let name = runtime.settings.provider_display_name(definition);
        println!("\n{name} ({id}) [{}]", source_label(provider.source));
        if let Some(error) = &provider.error {
            println!("  Error: {error}");
        }
        if provider.stale {
            println!("  Warning: cached data is stale");
        }
        let Some(snapshot) = &provider.snapshot else {
            if provider.error.is_none() {
                println!("  No usage data available.");
            }
            continue;
        };
        if let Some(plan) = &snapshot.plan {
            println!("  Plan: {plan}");
        }
        for quota in &snapshot.quotas {
            println!("  {}: {}", quota.label, format_quota(quota));
        }
        for metric in &snapshot.value_metrics {
            println!("  {}: {}", metric.label, format_value_metric(metric));
        }
        for metric in &snapshot.status_metrics {
            println!("  {}: {}", metric.label, format_status_metric(metric));
        }
        if let Some(today) = &snapshot.usage.today {
            println!("  Today: {}", format_usage(today));
        }
        for warning in &snapshot.warnings {
            println!("  Warning: {warning}");
        }
    }
}

fn source_label(source: SnapshotSource) -> &'static str {
    match source {
        SnapshotSource::None => "no data",
        SnapshotSource::Cache => "cache",
        SnapshotSource::Live => "live",
    }
}

fn format_quota(quota: &QuotaWindow) -> String {
    let value = match quota.format {
        QuotaFormat::Dollars => match (quota.used_value, quota.limit_value) {
            (Some(used), Some(limit)) => format!(
                "${used:.2} / ${limit:.2} ({:.0}% used)",
                quota.used_percent
            ),
            _ => format!("{:.0}% used", quota.used_percent),
        },
        QuotaFormat::Count => match (quota.used_value, quota.limit_value) {
            (Some(used), Some(limit)) => format!(
                "{} / {} {} ({:.0}% used)",
                format_number(used),
                format_number(limit),
                quota.unit.as_deref().unwrap_or("requests"),
                quota.used_percent
            ),
            _ => format!("{:.0}% used", quota.used_percent),
        },
        QuotaFormat::Percent => format!("{:.0}% used", quota.used_percent),
    };
    match quota.resets_at {
        Some(resets_at) => format!("{value}; resets {}", format_datetime(resets_at)),
        None => value,
    }
}

fn format_value_metric(metric: &ValueMetric) -> String {
    metric
        .values
        .iter()
        .map(format_metric_value)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_metric_value(value: &MetricValue) -> String {
    let formatted = match value.kind {
        MetricValueKind::Dollars => format!("${:.2}", value.number),
        MetricValueKind::Count => format_number(value.number),
    };
    match &value.label {
        Some(label) => format!("{label}: {formatted}"),
        None => formatted,
    }
}

fn format_status_metric(metric: &StatusMetric) -> String {
    match &metric.subtitle {
        Some(subtitle) => format!("{} ({subtitle})", metric.text),
        None => metric.text.clone(),
    }
}

fn format_usage(usage: &UsagePeriod) -> String {
    let cost = usage
        .estimated_cost_usd
        .map(|cost| format!("; ${cost:.2} estimated"))
        .unwrap_or_default();
    format!("{} tokens{cost}", format_number(usage.tokens as f64))
}

fn format_number(number: f64) -> String {
    if number.fract() == 0.0 {
        format!("{number:.0}")
    } else {
        format!("{number:.2}")
    }
}

fn format_datetime(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{parse_args, Command, StatusOptions};

    #[test]
    fn parses_status_options() {
        let args = [
            "status".into(),
            "--provider".into(),
            "codex".into(),
            "--cached".into(),
            "--json".into(),
        ];
        assert_eq!(
            parse_args(&args),
            Ok(Command::Status(StatusOptions {
                json: true,
                cached: true,
                provider: Some("codex".into()),
            }))
        );
    }

    #[test]
    fn status_options_can_be_used_without_a_subcommand() {
        let args = ["--json".into()];
        assert_eq!(
            parse_args(&args),
            Ok(Command::Status(StatusOptions {
                json: true,
                ..StatusOptions::default()
            }))
        );
    }

    #[test]
    fn rejects_unknown_options_and_missing_provider_ids() {
        assert!(parse_args(&["status".into(), "--wat".into()]).is_err());
        assert!(parse_args(&["status".into(), "--provider".into()]).is_err());
    }
}
