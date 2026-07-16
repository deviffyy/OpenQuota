use tauri::{image::Image, AppHandle};

#[cfg(not(target_os = "macos"))]
use crate::tray_icon;
use crate::{
    models::{
        AppSettings, MetricDefinition, MetricSource, MetricValue, MetricValueKind,
        ProviderSnapshot, QuotaFormat, UsageDisplay, UsagePeriod, UsagePeriodSelection,
    },
    providers::ProviderRegistry,
    service::UsageViewState,
};

const TRAY_ID: &str = "openquota-tray";

#[derive(Debug, Clone, PartialEq)]
struct TrayMetric {
    value: String,
    detail: String,
    gauge: Option<TrayGauge>,
}

#[derive(Debug, Clone, PartialEq)]
struct TrayGroup {
    #[cfg(any(target_os = "macos", test))]
    provider_id: String,
    metrics: Vec<TrayMetric>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TrayGauge {
    display_fraction: f64,
    #[cfg(any(not(target_os = "macos"), test))]
    remaining_fraction: f64,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, PartialEq)]
enum MacMenuBarIcon {
    Mark,
    Text(Vec<crate::menu_bar::TextGroup>),
    Bars(Vec<f64>),
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, PartialEq)]
struct MacMenuBarPresentation {
    icon: MacMenuBarIcon,
}

pub fn update(
    app: &AppHandle,
    state: &UsageViewState,
    settings: &AppSettings,
    registry: &ProviderRegistry,
) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let groups = resolved_groups(state, settings, registry);
    let tooltip = if groups.is_empty() {
        "OpenQuota".to_owned()
    } else {
        format!(
            "OpenQuota\n{}",
            groups
                .iter()
                .flat_map(|group| group.metrics.iter())
                .map(|metric| metric.detail.as_str())
                .collect::<Vec<_>>()
                .join(" · ")
        )
    };
    #[cfg(not(target_os = "linux"))]
    if tray.set_tooltip(Some(tooltip)).is_err() {
        crate::app_warn!("tray", "tray tooltip update failed");
    }
    #[cfg(target_os = "linux")]
    let _ = tooltip;

    #[cfg(not(target_os = "macos"))]
    {
        let icon = primary_gauge(&groups)
            .map(|gauge| tray_icon::render_gauge(gauge.display_fraction, gauge.remaining_fraction))
            .unwrap_or_else(mark_icon);
        if tray.set_icon(Some(icon)).is_err() {
            crate::app_warn!("tray", "tray icon update failed");
        }
    }

    #[cfg(target_os = "macos")]
    apply_mac_menu_bar_presentation(
        &tray,
        mac_menu_bar_presentation(&groups, settings.menu_bar_style),
    );
}

#[cfg(any(target_os = "macos", test))]
fn mac_menu_bar_presentation(
    groups: &[TrayGroup],
    style: crate::models::MenuBarStyle,
) -> MacMenuBarPresentation {
    match style {
        crate::models::MenuBarStyle::Text => {
            let text_groups = text_groups(groups);
            MacMenuBarPresentation {
                icon: if text_groups.is_empty() {
                    MacMenuBarIcon::Mark
                } else {
                    MacMenuBarIcon::Text(text_groups)
                },
            }
        }
        crate::models::MenuBarStyle::Bars => {
            let fractions = bar_fractions(groups);
            MacMenuBarPresentation {
                icon: if fractions.is_empty() {
                    MacMenuBarIcon::Mark
                } else {
                    MacMenuBarIcon::Bars(fractions)
                },
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn apply_mac_menu_bar_presentation(
    tray: &tauri::tray::TrayIcon,
    presentation: MacMenuBarPresentation,
) {
    match presentation.icon {
        MacMenuBarIcon::Mark => {
            // An empty value explicitly clears stale native text before the fallback mark is shown.
            if tray.set_title(Some("")).is_err() {
                crate::app_warn!("tray", "macOS menu bar title clear failed");
            }
            if tray
                .set_icon_with_as_template(Some(mark_icon()), true)
                .is_err()
            {
                crate::app_warn!("tray", "macOS menu bar icon update failed");
            }
        }
        MacMenuBarIcon::Text(groups) => {
            // Text is one template strip image (provider marks + values), matching the single native
            // status-item ownership model while allowing each provider to keep its visual identity.
            if tray.set_title(Some("")).is_err() {
                crate::app_warn!("tray", "macOS menu bar title clear failed");
            }
            let icon = crate::menu_bar::text_icon(&groups).unwrap_or_else(mark_icon);
            if tray.set_icon_with_as_template(Some(icon), true).is_err() {
                crate::app_warn!("tray", "macOS menu bar icon update failed");
            }
        }
        MacMenuBarIcon::Bars(fractions) => {
            // Clear Text before installing Bars so no stale value can remain beside the compact glyph.
            if tray.set_title(Some("")).is_err() {
                crate::app_warn!("tray", "macOS menu bar title clear failed");
            }
            if tray
                .set_icon_with_as_template(Some(crate::menu_bar::bar_icon(&fractions)), true)
                .is_err()
            {
                crate::app_warn!("tray", "macOS menu bar icon update failed");
            }
        }
    }
}

#[cfg(any(target_os = "macos", test))]
fn bar_fractions(groups: &[TrayGroup]) -> Vec<f64> {
    groups
        .iter()
        .flat_map(|group| group.metrics.iter())
        .filter_map(|metric| metric.gauge.map(|gauge| gauge.display_fraction))
        .take(crate::menu_bar::MAX_BARS)
        .collect()
}

#[cfg(any(not(target_os = "macos"), test))]
fn primary_gauge(groups: &[TrayGroup]) -> Option<TrayGauge> {
    groups
        .iter()
        .flat_map(|group| group.metrics.iter())
        .find_map(|metric| metric.gauge)
}

#[cfg(any(target_os = "macos", test))]
fn text_groups(groups: &[TrayGroup]) -> Vec<crate::menu_bar::TextGroup> {
    groups
        .iter()
        .map(|group| crate::menu_bar::TextGroup {
            provider_id: group.provider_id.clone(),
            values: group
                .metrics
                .iter()
                .take(crate::settings::MAX_PINS_PER_PROVIDER)
                .map(|metric| metric.value.clone())
                .collect(),
        })
        .filter(|group| !group.values.is_empty())
        .collect()
}

fn resolved_groups(
    state: &UsageViewState,
    settings: &AppSettings,
    registry: &ProviderRegistry,
) -> Vec<TrayGroup> {
    settings
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .filter_map(|provider| {
            let definition = registry.definition(&provider.id)?;
            let snapshot = state
                .providers
                .get(&provider.id)
                .and_then(|state| state.snapshot.as_ref())?;
            let metrics = provider
                .metrics
                .iter()
                .filter(|metric| metric.pinned)
                .filter_map(|metric| {
                    let metric_definition = registry.metric(&metric.id)?;
                    let mut resolved =
                        tray_metric(metric_definition, snapshot, settings.usage_display)?;
                    resolved.detail = format!("{} {}", definition.display_name, resolved.detail);
                    Some(resolved)
                })
                .collect::<Vec<_>>();
            (!metrics.is_empty()).then_some(TrayGroup {
                #[cfg(any(target_os = "macos", test))]
                provider_id: definition.id.clone(),
                metrics,
            })
        })
        .collect()
}

fn tray_metric(
    definition: &MetricDefinition,
    snapshot: &ProviderSnapshot,
    display: UsageDisplay,
) -> Option<TrayMetric> {
    let tray = definition.tray.as_ref()?;
    let quota = |id: &str| {
        snapshot
            .quotas
            .iter()
            .find(|quota| quota.id == id)
            .map(|quota| {
                if quota.format == QuotaFormat::Count {
                    if let (Some(used), Some(limit)) = (quota.used_value, quota.limit_value) {
                        let used_fraction = (limit > 0.0).then(|| (used / limit).clamp(0.0, 1.0));
                        let value = match display {
                            UsageDisplay::Used => used,
                            UsageDisplay::Left => (limit - used).max(0.0),
                        };
                        let word = match display {
                            UsageDisplay::Used => "used",
                            UsageDisplay::Left => "left",
                        };
                        return TrayMetric {
                            value: format!("{value:.0}"),
                            detail: format!("{} {value:.0} requests {word}", quota.label),
                            gauge: used_fraction.map(|used_fraction| TrayGauge {
                                display_fraction: match display {
                                    UsageDisplay::Used => used_fraction,
                                    UsageDisplay::Left => 1.0 - used_fraction,
                                },
                                #[cfg(any(not(target_os = "macos"), test))]
                                remaining_fraction: 1.0 - used_fraction,
                            }),
                        };
                    }
                }
                let used_fraction = (quota.used_percent / 100.0).clamp(0.0, 1.0);
                let display_fraction = match display {
                    UsageDisplay::Used => used_fraction,
                    UsageDisplay::Left => 1.0 - used_fraction,
                };
                let percent = display_fraction * 100.0;
                let word = match display {
                    UsageDisplay::Used => "used",
                    UsageDisplay::Left => "left",
                };
                TrayMetric {
                    value: format!("{percent:.0}%"),
                    detail: format!("{} {percent:.0}% {word}", quota.label),
                    gauge: Some(TrayGauge {
                        display_fraction,
                        #[cfg(any(not(target_os = "macos"), test))]
                        remaining_fraction: 1.0 - used_fraction,
                    }),
                }
            })
    };
    match &definition.source {
        MetricSource::Quota { source_id, .. } => quota(source_id),
        MetricSource::QuotaOrValue { source_id, .. } => {
            quota(source_id).or_else(|| value_metric(snapshot, source_id, tray.suffix.as_deref()))
        }
        MetricSource::Value { source_id } => {
            value_metric(snapshot, source_id, tray.suffix.as_deref())
        }
        MetricSource::Usage { period } => {
            usage_metric(&definition.label, usage_period(snapshot, *period))
        }
        MetricSource::Trend => None,
    }
}

fn usage_period(snapshot: &ProviderSnapshot, period: UsagePeriodSelection) -> Option<&UsagePeriod> {
    match period {
        UsagePeriodSelection::Today => snapshot.usage.today.as_ref(),
        UsagePeriodSelection::Yesterday => snapshot.usage.yesterday.as_ref(),
        UsagePeriodSelection::Last30Days => snapshot.usage.last_30_days.as_ref(),
    }
}

fn value_metric(
    snapshot: &ProviderSnapshot,
    source_id: &str,
    tray_suffix: Option<&str>,
) -> Option<TrayMetric> {
    let metric = snapshot
        .value_metrics
        .iter()
        .find(|metric| metric.id == source_id)?;
    let value = metric
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
    let value = tray_suffix
        .map(|suffix| format!("{value} {suffix}"))
        .unwrap_or(value);
    Some(TrayMetric {
        value,
        detail: format!("{} {detail}", metric.label),
        gauge: None,
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

fn usage_metric(label: &str, period: Option<&UsagePeriod>) -> Option<TrayMetric> {
    let period = period?;
    let value = period
        .estimated_cost_usd
        .map(|value| format!("${value:.2}"))
        .unwrap_or_else(|| format_tokens(period.tokens));
    let detail = format!("{label} {value}");
    Some(TrayMetric {
        value,
        detail,
        gauge: None,
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

fn mark_icon() -> Image<'static> {
    Image::from_bytes(include_bytes!("../icons/32x32.png"))
        .expect("bundled OpenQuota tray mark must be a valid PNG")
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
        providers::{codex, ProviderRegistry},
        settings::default_settings,
    };

    use super::{
        bar_fractions, format_tokens, mac_menu_bar_presentation, primary_gauge, resolved_groups,
        text_groups, MacMenuBarIcon, MacMenuBarPresentation, TrayGauge, TrayGroup, TrayMetric,
    };
    use crate::service::UsageViewState;

    #[test]
    fn bundled_tray_mark_decodes_at_the_expected_size() {
        let image = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))
            .expect("bundled tray mark should decode");
        assert_eq!((image.width(), image.height()), (32, 32));
    }

    #[test]
    fn text_groups_keep_provider_identity_and_values_without_choosing_a_primary_metric() {
        let metric = |value: &str| TrayMetric {
            value: value.into(),
            detail: value.into(),
            gauge: None,
        };
        let groups = vec![
            TrayGroup {
                provider_id: "claude".into(),
                metrics: vec![metric("75%"), metric("40%")],
            },
            TrayGroup {
                provider_id: "codex".into(),
                metrics: vec![metric("90%")],
            },
        ];

        assert_eq!(
            text_groups(&groups),
            vec![
                crate::menu_bar::TextGroup {
                    provider_id: "claude".into(),
                    values: vec!["75%".into(), "40%".into()],
                },
                crate::menu_bar::TextGroup {
                    provider_id: "codex".into(),
                    values: vec!["90%".into()],
                },
            ]
        );
        assert!(text_groups(&[]).is_empty());
    }

    #[test]
    fn mac_text_to_bars_transition_explicitly_clears_the_native_title() {
        let groups = vec![TrayGroup {
            provider_id: "codex".into(),
            metrics: vec![TrayMetric {
                value: "75%".into(),
                detail: String::new(),
                gauge: Some(TrayGauge {
                    display_fraction: 0.75,
                    remaining_fraction: 0.75,
                }),
            }],
        }];

        assert_eq!(
            mac_menu_bar_presentation(&groups, crate::models::MenuBarStyle::Text),
            MacMenuBarPresentation {
                icon: MacMenuBarIcon::Text(vec![crate::menu_bar::TextGroup {
                    provider_id: "codex".into(),
                    values: vec!["75%".into()],
                }]),
            }
        );
        assert_eq!(
            mac_menu_bar_presentation(&groups, crate::models::MenuBarStyle::Bars),
            MacMenuBarPresentation {
                icon: MacMenuBarIcon::Bars(vec![0.75]),
            }
        );
    }

    #[test]
    fn mac_empty_and_unbounded_bar_states_fall_back_without_stale_text() {
        let unbounded = vec![TrayGroup {
            provider_id: "codex".into(),
            metrics: vec![TrayMetric {
                value: "$4".into(),
                detail: String::new(),
                gauge: None,
            }],
        }];
        let fallback = MacMenuBarPresentation {
            icon: MacMenuBarIcon::Mark,
        };

        assert_eq!(
            mac_menu_bar_presentation(&[], crate::models::MenuBarStyle::Text),
            fallback
        );
        assert_eq!(
            mac_menu_bar_presentation(&[], crate::models::MenuBarStyle::Bars),
            fallback
        );
        assert_eq!(
            mac_menu_bar_presentation(&unbounded, crate::models::MenuBarStyle::Bars),
            fallback
        );
    }

    #[test]
    fn bars_use_the_first_four_bounded_metrics_in_layout_order() {
        let metric = |value: f64| TrayMetric {
            value: format!("{value:.0}%"),
            detail: String::new(),
            gauge: Some(TrayGauge {
                display_fraction: value / 100.0,
                remaining_fraction: value / 100.0,
            }),
        };
        let groups = vec![
            TrayGroup {
                provider_id: "claude".into(),
                metrics: vec![
                    metric(10.0),
                    TrayMetric {
                        value: "$4".into(),
                        detail: String::new(),
                        gauge: None,
                    },
                    metric(20.0),
                ],
            },
            TrayGroup {
                provider_id: "codex".into(),
                metrics: vec![metric(30.0), metric(40.0), metric(50.0)],
            },
        ];

        assert_eq!(bar_fractions(&groups), vec![0.1, 0.2, 0.3, 0.4]);
    }

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
            notices: Vec::new(),
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
        let catalog = ProviderRegistry::from_definitions(vec![codex::definition()]).unwrap();
        let groups = resolved_groups(
            &state,
            &default_settings(&catalog, &HashSet::from(["codex".to_owned()])),
            &catalog,
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].provider_id, "codex");
        assert_eq!(groups[0].metrics.len(), 2);
        assert_eq!(groups[0].metrics[0].value, "75%");
        assert_eq!(groups[0].metrics[1].value, "40%");
        assert_eq!(
            text_groups(&groups),
            vec![crate::menu_bar::TextGroup {
                provider_id: "codex".into(),
                values: vec!["75%".into(), "40%".into()],
            }]
        );
        assert_eq!(
            groups[0].metrics[0].gauge,
            Some(TrayGauge {
                display_fraction: 0.75,
                remaining_fraction: 0.75,
            })
        );

        let mut dashboard_hidden = default_settings(&catalog, &HashSet::from(["codex".to_owned()]));
        dashboard_hidden.providers[0].metrics[0].enabled = false;
        let hidden_groups = resolved_groups(&state, &dashboard_hidden, &catalog);
        assert_eq!(hidden_groups[0].metrics[0].value, "75%");

        let mut used_settings = default_settings(&catalog, &HashSet::from(["codex".to_owned()]));
        used_settings.usage_display = crate::models::UsageDisplay::Used;
        let used_groups = resolved_groups(&state, &used_settings, &catalog);
        assert_eq!(
            used_groups[0].metrics[0].gauge,
            Some(TrayGauge {
                display_fraction: 0.25,
                remaining_fraction: 0.75,
            })
        );
    }

    #[test]
    fn pinned_metrics_without_a_snapshot_do_not_leave_placeholder_content() {
        let catalog = ProviderRegistry::from_definitions(vec![codex::definition()]).unwrap();
        let settings = default_settings(&catalog, &HashSet::from(["codex".to_owned()]));
        assert!(resolved_groups(&UsageViewState::default(), &settings, &catalog).is_empty());
    }

    #[test]
    fn token_fallback_stays_compact() {
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(12_340), "12.3K");
        assert_eq!(format_tokens(2_500_000), "2.5M");
    }

    #[test]
    fn gauge_uses_the_first_pinned_quota_metric() {
        let groups = vec![TrayGroup {
            provider_id: "codex".into(),
            metrics: vec![
                TrayMetric {
                    value: "10".into(),
                    detail: "Credits 10".into(),
                    gauge: None,
                },
                TrayMetric {
                    value: "40%".into(),
                    detail: "Session 40% left".into(),
                    gauge: Some(TrayGauge {
                        display_fraction: 0.4,
                        remaining_fraction: 0.4,
                    }),
                },
                TrayMetric {
                    value: "80%".into(),
                    detail: "Weekly 80% left".into(),
                    gauge: Some(TrayGauge {
                        display_fraction: 0.8,
                        remaining_fraction: 0.8,
                    }),
                },
            ],
        }];
        assert_eq!(
            primary_gauge(&groups),
            Some(TrayGauge {
                display_fraction: 0.4,
                remaining_fraction: 0.4,
            })
        );
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
            notices: Vec::new(),
            usage: UsageHistory::default(),
            warnings: Vec::new(),
            refreshed_at: Utc::now(),
        };
        let catalog = ProviderRegistry::from_definitions(vec![codex::definition()]).unwrap();
        let metric = super::tray_metric(
            catalog.metric("codex.credits").unwrap(),
            &snapshot,
            crate::models::UsageDisplay::Left,
        )
        .unwrap();
        assert_eq!(metric.value, "$33 · 821 credits");
        assert_eq!(metric.detail, "Extra Usage $32.84 · 821 credits");
        assert_eq!(metric.gauge, None);
    }
}
