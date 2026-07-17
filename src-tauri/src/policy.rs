use std::time::Duration;

pub const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
pub const FAILURE_RETRY_BACKOFF: Duration = Duration::from_secs(60);
pub const STALE_AFTER: chrono::Duration = chrono::Duration::minutes(10);
