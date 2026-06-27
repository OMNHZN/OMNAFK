use std::time::{Duration, Instant};

/// Returns an instant `ttl` ago, or `Instant::now()` when the monotonic clock
/// has not advanced far enough yet (common during the first seconds after boot).
pub fn instant_ttl_ago(ttl: Duration) -> Instant {
    Instant::now().checked_sub(ttl).unwrap_or_else(Instant::now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instant_ttl_ago_never_panics() {
        let _ = instant_ttl_ago(Duration::from_secs(3600));
    }
}
