use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};

use chrono::{DateTime, Duration, Utc};

use crate::models::{AppSettings, NotificationPreferences, ProviderSnapshot, QuotaWindow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PaceSeverity {
    Untracked,
    Healthy,
    Close,
    RunningOut,
    Spent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaceProjection {
    pub severity: PaceSeverity,
    pub projected_used_percent: Option<f64>,
    pub even_pace_percent: Option<f64>,
    pub run_out_at: Option<DateTime<Utc>>,
}

pub fn project(window: &QuotaWindow, now: DateTime<Utc>) -> PaceProjection {
    let used = window.used_percent.clamp(0.0, 100.0);
    if used >= 99.5 {
        return PaceProjection {
            severity: PaceSeverity::Spent,
            projected_used_percent: Some(100.0),
            even_pace_percent: None,
            run_out_at: Some(now),
        };
    }
    let Some(resets_at) = window.resets_at else {
        return level_projection(used);
    };
    if window.period_seconds == 0 || resets_at <= now {
        return level_projection(used);
    }
    let period = Duration::seconds(window.period_seconds as i64);
    let starts_at = resets_at - period;
    let elapsed_seconds = now.signed_duration_since(starts_at).num_seconds().max(0) as f64;
    let progress = (elapsed_seconds / window.period_seconds as f64).clamp(0.0, 1.0);
    // Very young windows carry too little signal for a useful burn-rate projection.
    if elapsed_seconds < 60.0 || progress < 0.02 {
        return level_projection(used);
    }
    let projected = used / progress;
    let severity = if projected >= 100.0 {
        PaceSeverity::RunningOut
    } else if 100.0 - projected < 10.0 {
        PaceSeverity::Close
    } else {
        PaceSeverity::Healthy
    };
    let run_out_at = (projected >= 100.0 && used > 0.0).then(|| {
        starts_at + Duration::milliseconds((elapsed_seconds * 1000.0 * 100.0 / used) as i64)
    });
    PaceProjection {
        severity,
        projected_used_percent: Some(projected),
        even_pace_percent: Some(progress * 100.0),
        run_out_at,
    }
}

fn level_projection(_used: f64) -> PaceProjection {
    PaceProjection {
        severity: PaceSeverity::Untracked,
        projected_used_percent: None,
        even_pace_percent: None,
        run_out_at: None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Milestone {
    AlmostOut,
    CuttingItClose,
    WillRunOut,
}

impl Milestone {
    pub fn title(self) -> &'static str {
        match self {
            Self::AlmostOut => "Almost Out",
            Self::CuttingItClose => "Cutting It Close",
            Self::WillRunOut => "Will Run Out",
        }
    }

    pub fn body(self) -> &'static str {
        match self {
            Self::AlmostOut => "Under 10% usage remaining for this window.",
            Self::CuttingItClose => "Projected to finish close to your limit.",
            Self::WillRunOut => "Projected to run out before the limit resets.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaceAlert {
    pub milestone: Milestone,
    pub provider: String,
    pub metric: String,
}

#[derive(Debug, Clone, Default)]
struct NotificationState {
    resets_at: Option<DateTime<Utc>>,
    fired: HashSet<Milestone>,
    previous: Option<PaceSeverity>,
    was_under_ten: bool,
    primed: bool,
}

#[derive(Default)]
pub struct NotificationEvaluator {
    states: Mutex<HashMap<String, NotificationState>>,
}

impl NotificationEvaluator {
    pub fn evaluate(
        &self,
        snapshot: &ProviderSnapshot,
        settings: &AppSettings,
        now: DateTime<Utc>,
    ) -> Vec<PaceAlert> {
        let Some(provider) = settings
            .providers
            .iter()
            .find(|provider| provider.id == snapshot.provider_id && provider.enabled)
        else {
            return Vec::new();
        };
        let enabled = provider
            .metrics
            .iter()
            .filter(|metric| metric.enabled)
            .map(|metric| metric.id.as_str())
            .collect::<HashSet<_>>();
        let Ok(mut states) = self.states.lock() else {
            return Vec::new();
        };
        let mut alerts = Vec::new();
        for window in &snapshot.quotas {
            let metric_id = format!("{}.{}", snapshot.provider_id, window.id);
            if !enabled.contains(metric_id.as_str()) {
                continue;
            }
            let projection = project(window, now);
            let state = states.entry(metric_id).or_default();
            let mut new_alerts = transition(
                state,
                projection.severity,
                100.0 - window.used_percent.clamp(0.0, 100.0),
                window.resets_at,
                &settings.notifications,
                &window.label,
            );
            for alert in &mut new_alerts {
                alert.provider = provider_display_name(&snapshot.provider_id).into();
            }
            alerts.extend(new_alerts);
        }
        alerts
    }
}

fn transition(
    state: &mut NotificationState,
    severity: PaceSeverity,
    remaining_percent: f64,
    resets_at: Option<DateTime<Utc>>,
    toggles: &NotificationPreferences,
    metric: &str,
) -> Vec<PaceAlert> {
    if reset_advanced(resets_at, state.resets_at) {
        state.fired.clear();
        state.previous = None;
        state.was_under_ten = false;
    }
    state.resets_at = resets_at.or(state.resets_at);
    if !state.primed {
        state.primed = true;
        state.previous = (severity != PaceSeverity::Untracked).then_some(severity);
        state.was_under_ten = remaining_percent < 10.0;
        return Vec::new();
    }

    let mut milestones = Vec::new();
    let previous = state.previous;
    if severity != PaceSeverity::Untracked {
        if severity == PaceSeverity::Close
            && previous.is_none_or(|value| value < PaceSeverity::Close)
            && toggles.cutting_it_close
            && !state.fired.contains(&Milestone::CuttingItClose)
        {
            milestones.push(Milestone::CuttingItClose);
        }
        if severity >= PaceSeverity::RunningOut
            && previous.is_none_or(|value| value < PaceSeverity::RunningOut)
            && toggles.will_run_out
            && !state.fired.contains(&Milestone::WillRunOut)
        {
            milestones.push(Milestone::WillRunOut);
        }
        if previous.is_some_and(|value| severity < value) {
            if severity <= PaceSeverity::Healthy {
                state.fired.remove(&Milestone::CuttingItClose);
            }
            if severity <= PaceSeverity::Close {
                state.fired.remove(&Milestone::WillRunOut);
            }
        }
        if previous.is_none_or(|value| severity <= value) || !milestones.is_empty() {
            state.previous = Some(severity);
        }
    }

    let under_ten = remaining_percent < 10.0;
    if under_ten
        && !state.was_under_ten
        && toggles.almost_out
        && !state.fired.contains(&Milestone::AlmostOut)
    {
        milestones.push(Milestone::AlmostOut);
    }
    if !under_ten {
        state.fired.remove(&Milestone::AlmostOut);
    }
    if !under_ten || milestones.contains(&Milestone::AlmostOut) {
        state.was_under_ten = under_ten;
    }
    for milestone in &milestones {
        state.fired.insert(*milestone);
    }
    milestones
        .into_iter()
        .map(|milestone| PaceAlert {
            milestone,
            provider: String::new(),
            metric: metric.into(),
        })
        .collect()
}

fn provider_display_name(id: &str) -> &'static str {
    match id {
        "claude" => "Claude",
        "antigravity" => "Antigravity",
        _ => "Codex",
    }
}

fn reset_advanced(current: Option<DateTime<Utc>>, previous: Option<DateTime<Utc>>) -> bool {
    match (current, previous) {
        (Some(_), None) => true,
        (Some(current), Some(previous)) => {
            current.signed_duration_since(previous).num_milliseconds() > 1_000
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};

    use super::{project, transition, NotificationState, PaceSeverity};
    use crate::models::{NotificationPreferences, QuotaWindow};

    fn window(used: f64, elapsed_fraction: f64) -> QuotaWindow {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let period = 10_000_u64;
        QuotaWindow {
            id: "session".into(),
            label: "Session".into(),
            used_percent: used,
            resets_at: Some(
                now + Duration::seconds(((1.0 - elapsed_fraction) * period as f64) as i64),
            ),
            period_seconds: period,
            format: crate::models::QuotaFormat::Percent,
            used_value: None,
            limit_value: None,
        }
    }

    #[test]
    fn projection_colors_by_expected_usage_at_reset() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        assert_eq!(
            project(&window(30.0, 0.5), now).severity,
            PaceSeverity::Healthy
        );
        assert_eq!(
            project(&window(46.0, 0.5), now).severity,
            PaceSeverity::Close
        );
        assert_eq!(
            project(&window(60.0, 0.5), now).severity,
            PaceSeverity::RunningOut
        );
    }

    #[test]
    fn notifications_prime_then_fire_once_on_worsening() {
        let toggles = NotificationPreferences {
            cutting_it_close: true,
            will_run_out: true,
            almost_out: true,
        };
        let reset = Some(Utc.timestamp_opt(1_800_010_000, 0).unwrap());
        let mut state = NotificationState::default();
        assert!(transition(
            &mut state,
            PaceSeverity::Healthy,
            50.0,
            reset,
            &toggles,
            "Weekly"
        )
        .is_empty());
        let first = transition(
            &mut state,
            PaceSeverity::Close,
            8.0,
            reset,
            &toggles,
            "Weekly",
        );
        assert_eq!(first.len(), 2);
        assert!(transition(
            &mut state,
            PaceSeverity::Close,
            8.0,
            reset,
            &toggles,
            "Weekly"
        )
        .is_empty());
    }

    #[test]
    fn a_later_reset_window_rearms_notifications() {
        let toggles = NotificationPreferences {
            cutting_it_close: true,
            will_run_out: true,
            almost_out: false,
        };
        let reset = Utc.timestamp_opt(1_800_010_000, 0).unwrap();
        let mut state = NotificationState::default();
        transition(
            &mut state,
            PaceSeverity::Healthy,
            50.0,
            Some(reset),
            &toggles,
            "Weekly",
        );
        assert_eq!(
            transition(
                &mut state,
                PaceSeverity::Close,
                8.0,
                Some(reset),
                &toggles,
                "Weekly"
            )
            .len(),
            1
        );
        assert!(transition(
            &mut state,
            PaceSeverity::Close,
            8.0,
            Some(reset),
            &toggles,
            "Weekly"
        )
        .is_empty());
        let next_reset = reset + Duration::hours(1);
        assert_eq!(
            transition(
                &mut state,
                PaceSeverity::Close,
                8.0,
                Some(next_reset),
                &toggles,
                "Weekly"
            )
            .len(),
            1
        );
    }
}
