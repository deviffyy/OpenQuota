use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};

use chrono::{DateTime, Duration, Utc};

use crate::models::{
    AppSettings, NotificationPreferences, ProviderSnapshot, QuotaFormat, QuotaWindow,
};

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

pub fn project(
    window: &QuotaWindow,
    now: DateTime<Utc>,
    is_session_window: bool,
) -> PaceProjection {
    let used = window.used_percent.clamp(0.0, 100.0);
    if is_visibly_spent(window, used) {
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
    if is_session_window && used <= 0.0 {
        return level_projection(used);
    }
    let period = Duration::seconds(window.period_seconds as i64);
    let starts_at = resets_at - period;
    let elapsed_seconds = now
        .signed_duration_since(starts_at)
        .num_milliseconds()
        .max(0) as f64
        / 1000.0;
    let progress = (elapsed_seconds / window.period_seconds as f64).clamp(0.0, 1.0);
    // Very young windows carry too little signal for a useful burn-rate projection.
    if elapsed_seconds < (window.period_seconds as f64 * 0.01).max(60.0) {
        return level_projection(used);
    }
    let projected = used / progress;
    if projected <= 90.0 {
        return PaceProjection {
            severity: PaceSeverity::Healthy,
            projected_used_percent: Some(projected),
            even_pace_percent: Some(progress * 100.0),
            run_out_at: None,
        };
    }
    if used < 5.0 {
        return level_projection(used);
    }
    if projected <= 100.0 {
        return PaceProjection {
            severity: if (100.0 - projected).round() >= 1.0 {
                PaceSeverity::Close
            } else {
                PaceSeverity::RunningOut
            },
            projected_used_percent: Some(projected),
            even_pace_percent: Some(progress * 100.0),
            run_out_at: None,
        };
    }
    let candidate =
        starts_at + Duration::milliseconds((elapsed_seconds * 1000.0 * 100.0 / used) as i64);
    let run_out_at = (candidate > now && candidate < resets_at).then_some(candidate);
    PaceProjection {
        severity: PaceSeverity::RunningOut,
        projected_used_percent: Some(projected),
        even_pace_percent: Some(progress * 100.0),
        run_out_at,
    }
}

fn is_visibly_spent(window: &QuotaWindow, used_percent: f64) -> bool {
    if window.format == QuotaFormat::Dollars {
        if let (Some(used), Some(limit)) = (window.used_value, window.limit_value) {
            if limit > 0.0 {
                return ((limit - used) * 100.0).round() / 100.0 <= 0.0;
            }
        }
    }
    (100.0 - used_percent).round() <= 0.0
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
            let is_session_window = match snapshot.provider_id.as_str() {
                "claude" => window.id == "session",
                "antigravity" => matches!(window.id.as_str(), "geminiPro" | "claude"),
                _ => false,
            };
            let projection = project(window, now, is_session_window);
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
    let severity = match severity {
        PaceSeverity::Spent => PaceSeverity::RunningOut,
        value => value,
    };
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
        if previous.is_some_and(|value| severity <= value) || !milestones.is_empty() {
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
            project(&window(30.0, 0.5), now, false).severity,
            PaceSeverity::Healthy
        );
        assert_eq!(
            project(&window(46.0, 0.5), now, false).severity,
            PaceSeverity::Close
        );
        assert_eq!(
            project(&window(60.0, 0.5), now, false).severity,
            PaceSeverity::RunningOut
        );
    }

    #[test]
    fn projection_uses_reference_signal_and_low_usage_guards() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let ready = project(&window(1.0, 0.015), now, false);
        assert_eq!(ready.severity, PaceSeverity::Healthy);
        assert!((ready.projected_used_percent.unwrap() - 66.666).abs() < 0.01);
        assert_eq!(
            project(&window(1.0, 0.009), now, false).severity,
            PaceSeverity::Untracked
        );
        assert_eq!(
            project(&window(4.0, 0.02), now, false).severity,
            PaceSeverity::Untracked
        );
    }

    #[test]
    fn projection_preserves_subsecond_elapsed_precision() {
        let now = Utc.timestamp_millis_opt(1_800_000_000_500).unwrap();
        let mut value = window(1.0, 0.015);
        value.resets_at = Some(now + Duration::milliseconds(9_849_500));
        let projection = project(&value, now, false);
        assert_eq!(projection.severity, PaceSeverity::Healthy);
        assert!((projection.projected_used_percent.unwrap() - 66.445).abs() < 0.01);
    }

    #[test]
    fn exact_limit_and_zero_spare_have_no_run_out_time() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        for used in [50.0, 49.8] {
            let projection = project(&window(used, 0.5), now, false);
            assert_eq!(projection.severity, PaceSeverity::RunningOut);
            assert_eq!(projection.run_out_at, None);
        }
    }

    #[test]
    fn fresh_session_and_display_precision_match_the_visible_row() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        assert_eq!(
            project(&window(0.0, 0.5), now, true).severity,
            PaceSeverity::Untracked
        );
        assert_eq!(
            project(&window(0.0, 0.5), now, false).severity,
            PaceSeverity::Healthy
        );
        assert_ne!(
            project(&window(99.5, 0.5), now, false).severity,
            PaceSeverity::Spent
        );
        assert_eq!(
            project(&window(99.51, 0.5), now, false).severity,
            PaceSeverity::Spent
        );

        let mut dollars = window(99.0, 0.5);
        dollars.format = crate::models::QuotaFormat::Dollars;
        dollars.used_value = Some(9.996);
        dollars.limit_value = Some(10.0);
        assert_eq!(project(&dollars, now, false).severity, PaceSeverity::Spent);
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

    #[test]
    fn disabled_worsening_is_not_consumed_from_an_untracked_baseline() {
        let reset = Some(Utc.timestamp_opt(1_800_010_000, 0).unwrap());
        let mut state = NotificationState::default();
        let disabled = NotificationPreferences::default();
        transition(
            &mut state,
            PaceSeverity::Untracked,
            50.0,
            reset,
            &disabled,
            "Weekly",
        );
        assert!(transition(
            &mut state,
            PaceSeverity::Close,
            20.0,
            reset,
            &disabled,
            "Weekly",
        )
        .is_empty());
        assert_eq!(state.previous, None);

        let enabled = NotificationPreferences {
            cutting_it_close: true,
            ..NotificationPreferences::default()
        };
        assert_eq!(
            transition(
                &mut state,
                PaceSeverity::Close,
                20.0,
                reset,
                &enabled,
                "Weekly",
            )
            .len(),
            1
        );
    }
}
