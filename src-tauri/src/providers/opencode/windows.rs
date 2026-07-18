use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc};

use crate::models::{QuotaFormat, QuotaWindow};

pub(crate) const SESSION_CAP_USD: f64 = 12.0;
pub(crate) const WEEKLY_CAP_USD: f64 = 30.0;
pub(crate) const MONTHLY_CAP_USD: f64 = 60.0;
pub(crate) const SESSION_SECONDS: u64 = 5 * 60 * 60;
pub(crate) const WEEK_SECONDS: u64 = 7 * 24 * 60 * 60;
pub(crate) const QUOTA_SOURCE_NOTE: &str =
    "Estimated from OpenCode Go activity recorded on this device; activity elsewhere may be missing.";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OpenCodeWindows {
    pub(crate) session_spend: f64,
    pub(crate) session_resets_at: DateTime<Utc>,
    pub(crate) weekly_spend: f64,
    pub(crate) weekly_resets_at: DateTime<Utc>,
    pub(crate) monthly_spend: f64,
    pub(crate) monthly_resets_at: DateTime<Utc>,
    pub(crate) monthly_period_seconds: u64,
}

impl OpenCodeWindows {
    pub(crate) fn compute(
        costs: &[(DateTime<Utc>, f64)],
        anchor: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Self {
        let session_start = now - Duration::seconds(SESSION_SECONDS as i64);
        let session_rows = costs
            .iter()
            .filter(|(timestamp, _)| *timestamp >= session_start && *timestamp < now)
            .collect::<Vec<_>>();
        let session_spend = rounded_cost(session_rows.iter().map(|(_, cost)| *cost).sum());
        let session_resets_at = session_rows
            .iter()
            .map(|(timestamp, _)| *timestamp)
            .min()
            .unwrap_or(now)
            + Duration::seconds(SESSION_SECONDS as i64);

        let week_start_date = now
            .date_naive()
            .checked_sub_days(chrono::Days::new(u64::from(
                now.weekday().num_days_from_monday(),
            )))
            .unwrap_or(now.date_naive());
        let week_start = Utc.from_utc_datetime(
            &week_start_date
                .and_hms_opt(0, 0, 0)
                .expect("midnight is valid"),
        );
        let weekly_resets_at = week_start + Duration::seconds(WEEK_SECONDS as i64);
        let weekly_spend = rounded_cost(sum_range(costs, week_start, weekly_resets_at));

        let (month_start, monthly_resets_at) = month_bounds(now, anchor);
        let monthly_spend = rounded_cost(sum_range(costs, month_start, monthly_resets_at));
        let monthly_period_seconds = monthly_resets_at
            .signed_duration_since(month_start)
            .num_seconds()
            .max(0) as u64;

        Self {
            session_spend,
            session_resets_at,
            weekly_spend,
            weekly_resets_at,
            monthly_spend,
            monthly_resets_at,
            monthly_period_seconds,
        }
    }

    pub(crate) fn quotas(&self) -> Vec<QuotaWindow> {
        vec![
            quota(
                "session",
                "Session",
                self.session_spend,
                SESSION_CAP_USD,
                self.session_resets_at,
                SESSION_SECONDS,
            ),
            quota(
                "weekly",
                "Weekly",
                self.weekly_spend,
                WEEKLY_CAP_USD,
                self.weekly_resets_at,
                WEEK_SECONDS,
            ),
            quota(
                "monthly",
                "Monthly",
                self.monthly_spend,
                MONTHLY_CAP_USD,
                self.monthly_resets_at,
                self.monthly_period_seconds,
            ),
        ]
    }
}

fn quota(
    id: &str,
    label: &str,
    used: f64,
    limit: f64,
    resets_at: DateTime<Utc>,
    period_seconds: u64,
) -> QuotaWindow {
    QuotaWindow {
        id: id.into(),
        label: label.into(),
        used_percent: (used / limit * 100.0).clamp(0.0, 100.0),
        resets_at: Some(resets_at),
        period_seconds,
        format: QuotaFormat::Dollars,
        used_value: Some(used),
        limit_value: Some(limit),
        unit: Some("usd".into()),
        estimated: true,
        source_note: Some(QUOTA_SOURCE_NOTE.into()),
    }
}

fn sum_range(costs: &[(DateTime<Utc>, f64)], start: DateTime<Utc>, end: DateTime<Utc>) -> f64 {
    costs
        .iter()
        .filter(|(timestamp, _)| *timestamp >= start && *timestamp < end)
        .map(|(_, cost)| *cost)
        .sum()
}

fn rounded_cost(cost: f64) -> f64 {
    (cost * 10_000.0).round() / 10_000.0
}

fn month_bounds(
    now: DateTime<Utc>,
    anchor: Option<DateTime<Utc>>,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let Some(anchor) = anchor.filter(|anchor| *anchor <= now) else {
        let start = utc_date(now.year(), now.month(), 1, 0, 0, 0, 0);
        let (next_year, next_month) = shift_month(now.year(), now.month(), 1);
        return (start, utc_date(next_year, next_month, 1, 0, 0, 0, 0));
    };

    let mut year = now.year();
    let mut month = now.month();
    let mut start = anchored_month_start(year, month, anchor);
    if start > now {
        (year, month) = shift_month(year, month, -1);
        start = anchored_month_start(year, month, anchor);
    }
    let (next_year, next_month) = shift_month(year, month, 1);
    (start, anchored_month_start(next_year, next_month, anchor))
}

fn anchored_month_start(year: i32, month: u32, anchor: DateTime<Utc>) -> DateTime<Utc> {
    let day = anchor.day().min(days_in_month(year, month));
    utc_date(
        year,
        month,
        day,
        anchor.hour(),
        anchor.minute(),
        anchor.second(),
        anchor.nanosecond(),
    )
}

fn shift_month(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let index = year * 12 + month as i32 - 1 + delta;
    (index.div_euclid(12), (index.rem_euclid(12) + 1) as u32)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = shift_month(year, month, 1);
    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|next| next.pred_opt())
        .map(|last| last.day())
        .unwrap_or(28)
}

#[allow(clippy::too_many_arguments)]
fn utc_date(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    nanosecond: u32,
) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .expect("validated UTC date")
        .with_nanosecond(nanosecond)
        .expect("source nanoseconds are valid")
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{
        OpenCodeWindows, MONTHLY_CAP_USD, QUOTA_SOURCE_NOTE, SESSION_CAP_USD, SESSION_SECONDS,
        WEEKLY_CAP_USD, WEEK_SECONDS,
    };
    use crate::models::QuotaFormat;

    fn time(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
            .unwrap()
    }

    #[test]
    fn rolling_session_uses_the_oldest_in_window_row_for_reset() {
        let now = time(2026, 7, 12, 12, 0);
        let windows = OpenCodeWindows::compute(
            &[
                (time(2026, 7, 12, 11, 0), 2.0),
                (time(2026, 7, 12, 8, 30), 1.5),
                (time(2026, 7, 12, 6, 0), 9.0),
            ],
            None,
            now,
        );
        assert_eq!(windows.session_spend, 3.5);
        assert_eq!(windows.session_resets_at, time(2026, 7, 12, 13, 30));
    }

    #[test]
    fn week_is_monday_utc_and_boundaries_are_half_open() {
        let now = time(2026, 7, 12, 12, 0);
        let windows = OpenCodeWindows::compute(
            &[
                (time(2026, 7, 6, 0, 0), 4.0),
                (time(2026, 7, 5, 23, 59), 9.0),
                (time(2026, 7, 12, 11, 0), 1.0),
            ],
            None,
            now,
        );
        assert_eq!(windows.weekly_spend, 5.0);
        assert_eq!(windows.weekly_resets_at, time(2026, 7, 13, 0, 0));
    }

    #[test]
    fn anchored_month_clamps_short_months_and_uses_the_live_cycle() {
        let now = time(2026, 6, 15, 12, 0);
        let anchor = time(2026, 1, 31, 9, 30);
        let windows = OpenCodeWindows::compute(&[], Some(anchor), now);
        assert_eq!(windows.monthly_resets_at, time(2026, 6, 30, 9, 30));

        let later = OpenCodeWindows::compute(&[], Some(anchor), time(2026, 7, 12, 12, 0));
        assert_eq!(later.monthly_resets_at, time(2026, 7, 31, 9, 30));
    }

    #[test]
    fn idle_windows_and_calendar_month_fallback_are_stable() {
        let now = time(2026, 7, 12, 12, 0);
        let windows = OpenCodeWindows::compute(&[], None, now);
        assert_eq!(windows.session_spend, 0.0);
        assert_eq!(windows.session_resets_at, time(2026, 7, 12, 17, 0));
        assert_eq!(windows.monthly_resets_at, time(2026, 8, 1, 0, 0));
    }

    #[test]
    fn quota_contract_marks_machine_local_caps_as_estimates() {
        let windows = OpenCodeWindows::compute(&[], None, time(2026, 7, 12, 12, 0));
        let quotas = windows.quotas();
        assert_eq!(
            quotas
                .iter()
                .map(|quota| quota.id.as_str())
                .collect::<Vec<_>>(),
            ["session", "weekly", "monthly"]
        );
        assert_eq!(
            quotas
                .iter()
                .map(|quota| quota.limit_value)
                .collect::<Vec<_>>(),
            [
                Some(SESSION_CAP_USD),
                Some(WEEKLY_CAP_USD),
                Some(MONTHLY_CAP_USD)
            ]
        );
        assert_eq!(quotas[0].period_seconds, SESSION_SECONDS);
        assert_eq!(quotas[1].period_seconds, WEEK_SECONDS);
        assert!(quotas.iter().all(|quota| {
            quota.format == QuotaFormat::Dollars
                && quota.unit.as_deref() == Some("usd")
                && quota.estimated
                && quota.source_note.as_deref() == Some(QUOTA_SOURCE_NOTE)
        }));
    }
}
