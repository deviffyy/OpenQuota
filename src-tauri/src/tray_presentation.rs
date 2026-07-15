use tauri::AppHandle;

#[cfg(target_os = "macos")]
use tauri::image::Image;

use crate::{
    models::{
        AppSettings, MetricValue, MetricValueKind, ProviderSnapshot, UsageDisplay, UsagePeriod,
    },
    service::UsageViewState,
};

const TRAY_ID: &str = "openquota-tray";
#[cfg(target_os = "macos")]
const BAR_ICON_SIZE: u32 = 18;
#[cfg(target_os = "macos")]
const MAX_BARS: usize = 4;

#[derive(Debug, Clone, PartialEq)]
struct TrayMetric {
    compact: String,
    detail: String,
    fraction: Option<f64>,
}

pub fn update(app: &AppHandle, state: &UsageViewState, settings: &AppSettings) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let metrics = resolved_metrics(state, settings);
    let tooltip = if metrics.is_empty() {
        "OpenQuota".to_owned()
    } else {
        format!(
            "OpenQuota\n{}",
            metrics
                .iter()
                .map(|metric| metric.detail.as_str())
                .collect::<Vec<_>>()
                .join(" · ")
        )
    };
    let _ = tray.set_tooltip(Some(tooltip));

    #[cfg(target_os = "macos")]
    {
        match settings.menu_bar_style {
            crate::models::MenuBarStyle::Text => {
                let _ = tray.set_icon_with_as_template(Some(mark_icon()), true);
                let title = (!metrics.is_empty()).then(|| {
                    format!(
                        " {}",
                        metrics
                            .iter()
                            .map(|metric| metric.compact.as_str())
                            .collect::<Vec<_>>()
                            .join(" · ")
                    )
                });
                let _ = tray.set_title(title);
            }
            crate::models::MenuBarStyle::Bars => {
                let fractions = metrics
                    .iter()
                    .filter_map(|metric| metric.fraction)
                    .take(MAX_BARS)
                    .collect::<Vec<_>>();
                let _ = tray.set_title(None::<&str>);
                if fractions.is_empty() {
                    let _ = tray.set_icon_with_as_template(Some(mark_icon()), true);
                } else {
                    let _ = tray.set_icon_with_as_template(Some(bar_icon(&fractions)), true);
                }
            }
        }
    }
}

fn resolved_metrics(state: &UsageViewState, settings: &AppSettings) -> Vec<TrayMetric> {
    settings
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .flat_map(|provider| {
            let snapshot = state
                .providers
                .get(&provider.id)
                .and_then(|state| state.snapshot.as_ref());
            provider
                .metrics
                .iter()
                .filter(|metric| metric.enabled && metric.pinned)
                .filter_map(move |metric| {
                    let snapshot = snapshot?;
                    let mut resolved = tray_metric(&metric.id, snapshot, settings.usage_display)?;
                    let display_name = provider_display_name(&provider.id);
                    resolved.compact =
                        format!("{} {}", provider_short_name(&provider.id), resolved.compact);
                    resolved.detail = format!("{display_name} {}", resolved.detail);
                    Some(resolved)
                })
        })
        .collect()
}

fn tray_metric(
    metric_id: &str,
    snapshot: &ProviderSnapshot,
    display: UsageDisplay,
) -> Option<TrayMetric> {
    let quota = |id: &str, short: &str| {
        snapshot
            .quotas
            .iter()
            .find(|quota| quota.id == id)
            .map(|quota| {
                let percent = match display {
                    UsageDisplay::Used => quota.used_percent,
                    UsageDisplay::Left => 100.0 - quota.used_percent,
                }
                .clamp(0.0, 100.0);
                let word = match display {
                    UsageDisplay::Used => "used",
                    UsageDisplay::Left => "left",
                };
                TrayMetric {
                    compact: format!("{short} {percent:.0}%"),
                    detail: format!("{} {percent:.0}% {word}", quota.label),
                    fraction: Some(percent / 100.0),
                }
            })
    };
    match metric_id {
        id if id.ends_with(".session") => quota("session", "S"),
        id if id.ends_with(".weekly") => quota("weekly", "W"),
        id if id.ends_with(".sonnet") => quota("sonnet", "Sn"),
        id if id.ends_with(".fable") => quota("fable", "F"),
        id if id.ends_with(".extra") => {
            quota("extra", "E").or_else(|| value_metric("E", snapshot, "extra", None))
        }
        id if id.ends_with(".credits") => value_metric("E", snapshot, "credits", None),
        id if id.ends_with(".rateLimitResets") => {
            value_metric("R", snapshot, "rateLimitResets", Some("resets"))
        }
        id if id.ends_with(".geminiPro") => quota("geminiPro", "S"),
        id if id.ends_with(".geminiWeekly") => quota("geminiWeekly", "W"),
        id if id.ends_with(".claude") => quota("claude", "C"),
        id if id.ends_with(".claudeWeekly") => quota("claudeWeekly", "CW"),
        id if id.ends_with(".today") => usage_metric("T", "Today", snapshot.usage.today.as_ref()),
        id if id.ends_with(".yesterday") => {
            usage_metric("Y", "Yesterday", snapshot.usage.yesterday.as_ref())
        }
        id if id.ends_with(".last30") => {
            usage_metric("M", "30 Days", snapshot.usage.last_30_days.as_ref())
        }
        _ => None,
    }
}

fn value_metric(
    short: &str,
    snapshot: &ProviderSnapshot,
    source_id: &str,
    tray_suffix: Option<&str>,
) -> Option<TrayMetric> {
    let metric = snapshot
        .value_metrics
        .iter()
        .find(|metric| metric.id == source_id)?;
    let compact = metric
        .values
        .iter()
        .map(format_tray_value)
        .collect::<Vec<_>>()
        .join(" · ");
    let detail = metric
        .values
        .iter()
        .map(format_detail_value)
        .collect::<Vec<_>>()
        .join(" · ");
    let compact = tray_suffix
        .map(|suffix| format!("{compact} {suffix}"))
        .unwrap_or(compact);
    Some(TrayMetric {
        compact: format!("{short} {compact}"),
        detail: format!("{} {detail}", metric.label),
        fraction: None,
    })
}

fn format_tray_value(value: &MetricValue) -> String {
    let number = match value.kind {
        MetricValueKind::Dollars => format!("${:.0}", value.number),
        MetricValueKind::Count => format_tokens(value.number.max(0.0) as u64),
    };
    value
        .label
        .as_deref()
        .map(|label| format!("{number} {label}"))
        .unwrap_or(number)
}

fn format_detail_value(value: &MetricValue) -> String {
    let number = match value.kind {
        MetricValueKind::Dollars => format!("${:.2}", value.number),
        MetricValueKind::Count => format!("{:.0}", value.number),
    };
    value
        .label
        .as_deref()
        .map(|label| format!("{number} {label}"))
        .unwrap_or(number)
}

fn provider_display_name(id: &str) -> &'static str {
    match id {
        "claude" => "Claude",
        "antigravity" => "Antigravity",
        _ => "Codex",
    }
}

fn provider_short_name(id: &str) -> &'static str {
    match id {
        "claude" => "Cl",
        "antigravity" => "A",
        _ => "Cx",
    }
}

fn usage_metric(short: &str, label: &str, period: Option<&UsagePeriod>) -> Option<TrayMetric> {
    let period = period?;
    let value = period
        .estimated_cost_usd
        .map(|value| format!("${value:.2}"))
        .unwrap_or_else(|| format_tokens(period.tokens));
    Some(TrayMetric {
        compact: format!("{short} {value}"),
        detail: format!("{label} {value}"),
        fraction: None,
    })
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

#[cfg(target_os = "macos")]
fn bar_icon(fractions: &[f64]) -> Image<'static> {
    let size = BAR_ICON_SIZE as usize;
    let mut rgba = vec![0_u8; size * size * 4];
    let count = fractions.len().min(MAX_BARS);
    let bar_width = if count <= 2 { 4 } else { 3 };
    let gap = 1;
    let total_width = count * bar_width + count.saturating_sub(1) * gap;
    let start_x = (size.saturating_sub(total_width)) / 2;
    let baseline = size - 3;
    let max_height = size - 6;

    for (index, fraction) in fractions.iter().take(MAX_BARS).enumerate() {
        let height = ((*fraction).clamp(0.0, 1.0) * max_height as f64)
            .round()
            .max(1.0) as usize;
        let x0 = start_x + index * (bar_width + gap);
        for y in baseline - height..baseline {
            for x in x0..x0 + bar_width {
                let pixel = (y * size + x) * 4;
                rgba[pixel] = 0;
                rgba[pixel + 1] = 0;
                rgba[pixel + 2] = 0;
                rgba[pixel + 3] = 255;
            }
        }
    }
    Image::new_owned(rgba, BAR_ICON_SIZE, BAR_ICON_SIZE)
}

#[cfg(target_os = "macos")]
fn mark_icon() -> Image<'static> {
    let size = BAR_ICON_SIZE as usize;
    let mut rgba = vec![0_u8; size * size * 4];
    let center = 8.0;
    for y in 2..16 {
        for x in 2..16 {
            let distance = ((x as f64 - center).powi(2) + (y as f64 - center).powi(2)).sqrt();
            let ring = (4.5..=6.5).contains(&distance);
            let tail = (10..=15).contains(&x)
                && (10..=15).contains(&y)
                && (x as isize - y as isize).abs() <= 1;
            if ring || tail {
                let pixel = (y * size + x) * 4;
                rgba[pixel + 3] = 255;
            }
        }
    }
    Image::new_owned(rgba, BAR_ICON_SIZE, BAR_ICON_SIZE)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::Utc;

    use crate::{
        models::{
            MetricValue, MetricValueKind, ProviderSnapshot, ProviderViewState, QuotaWindow,
            SnapshotSource, UsageHistory, ValueMetric,
        },
        settings::default_settings,
    };

    use super::{format_tokens, resolved_metrics};
    use crate::service::UsageViewState;

    #[test]
    fn pinned_quota_metrics_resolve_in_layout_order() {
        let snapshot = ProviderSnapshot {
            provider_id: "codex".into(),
            plan: None,
            quotas: vec![
                QuotaWindow {
                    id: "session".into(),
                    label: "Session".into(),
                    used_percent: 25.0,
                    resets_at: None,
                    period_seconds: 18_000,
                    format: crate::models::QuotaFormat::Percent,
                    used_value: None,
                    limit_value: None,
                },
                QuotaWindow {
                    id: "weekly".into(),
                    label: "Weekly".into(),
                    used_percent: 60.0,
                    resets_at: None,
                    period_seconds: 604_800,
                    format: crate::models::QuotaFormat::Percent,
                    used_value: None,
                    limit_value: None,
                },
            ],
            value_metrics: Vec::new(),
            usage: UsageHistory::default(),
            warnings: Vec::new(),
            refreshed_at: Utc::now(),
        };
        let provider_state = ProviderViewState {
            snapshot: Some(snapshot),
            source: SnapshotSource::Live,
            ..ProviderViewState::default()
        };
        let state = UsageViewState {
            providers: [("codex".into(), provider_state)].into_iter().collect(),
            last_full_refresh_at: None,
        };
        let metrics = resolved_metrics(
            &state,
            &default_settings(&HashSet::from(["codex".to_owned()])),
        );
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].compact, "Cx S 75%");
        assert_eq!(metrics[1].compact, "Cx W 40%");
        assert_eq!(metrics[0].fraction, Some(0.75));
    }

    #[test]
    fn token_fallback_stays_compact() {
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(12_340), "12.3K");
        assert_eq!(format_tokens(2_500_000), "2.5M");
    }

    #[test]
    fn pinned_value_metrics_keep_numeric_values_outside_quota_bars() {
        let snapshot = ProviderSnapshot {
            provider_id: "codex".into(),
            plan: None,
            quotas: Vec::new(),
            value_metrics: vec![ValueMetric {
                id: "credits".into(),
                label: "Extra Usage".into(),
                values: vec![
                    MetricValue {
                        number: 32.84,
                        kind: MetricValueKind::Dollars,
                        label: None,
                    },
                    MetricValue {
                        number: 821.0,
                        kind: MetricValueKind::Count,
                        label: Some("credits".into()),
                    },
                ],
                expiries_at: Vec::new(),
            }],
            usage: UsageHistory::default(),
            warnings: Vec::new(),
            refreshed_at: Utc::now(),
        };
        let metric = super::tray_metric(
            "codex.credits",
            &snapshot,
            crate::models::UsageDisplay::Left,
        )
        .unwrap();
        assert_eq!(metric.compact, "E $33 · 821 credits");
        assert_eq!(metric.detail, "Extra Usage $32.84 · 821 credits");
        assert_eq!(metric.fraction, None);
    }
}
