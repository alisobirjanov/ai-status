//! What a provider reported, in the shape the panel draws.
//!
//! Port of `UsageWindow` / `ProviderUsage` from the macOS app
//! (`Sources/Pulse/Usage/ProviderUsage.swift`). Every figure here is one the
//! provider stated: Pulse never derives a percentage of its own.

use serde::{Deserialize, Serialize};

/// The cap on how old a reading may be and still be shown, dated.
pub const MAXIMUM_AGE_MS: i64 = 24 * 3600 * 1000;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    ClaudeCode,
    Codex,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::ClaudeCode, Provider::Codex];

    pub fn id(self) -> &'static str {
        match self {
            Provider::ClaudeCode => "claudeCode",
            Provider::Codex => "codex",
        }
    }

    /// A product name, so never translated.
    pub fn display_name(self) -> &'static str {
        match self {
            Provider::ClaudeCode => "Claude Code",
            Provider::Codex => "Codex",
        }
    }
}

/// What kind of window this is, kept as meaning rather than text so the
/// name is built in whichever language is current when it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowKind {
    FiveHour,
    Weekly,
    Spend,
    /// A length nobody has a name for; `window_seconds` carries it.
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    /// Stable across refreshes.
    pub id: String,
    pub kind: WindowKind,
    /// The model this limit is scoped to. A product name, never translated.
    pub scope: Option<String>,
    /// 0...1 normally; a provider may report past 100% once a limit is exceeded.
    pub used_fraction: f64,
    pub window_seconds: i64,
    /// Unix milliseconds.
    pub resets_at: Option<i64>,
    /// The provider's own word that this limit is spent — never `>= 100%`.
    #[serde(default)]
    pub is_exhausted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Live,
    /// Real figures with a date on them: a cached reading, shown with "as of".
    Stale,
    Unavailable,
}

/// Why there is nothing to show. Shared cases name no provider; the ones
/// that do are where the remedy names a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// Seeded before the first answer. Never cached.
    NotChecked,
    NoLimitsReported,
    ClaudeSignInRequired,
    ClaudeLoginExpired,
    CodexSignInRequired,
    CodexServerFailed,
    Unreachable,
    UnreadableReply,
    RateLimited,
    ServerError,
}

impl Reason {
    /// A missing credential is not a stumble to be papered over with
    /// yesterday's figures: it is the one thing the reader needs told.
    pub fn is_missing_credential(self) -> bool {
        matches!(self, Reason::ClaudeSignInRequired)
    }
}

/// Which route produced the figures. A stable token, like `--json`'s `source`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Route {
    Endpoint,
    AppServer,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsage {
    pub provider: Provider,
    pub windows: Vec<UsageWindow>,
    /// When the provider's figures were taken, not when Pulse asked. Unix ms.
    pub observed_at: Option<i64>,
    pub state: State,
    pub reason: Option<Reason>,
    pub plan: Option<String>,
    pub credit_balance: Option<String>,
    pub origin: Option<Route>,
    /// Set only by cache restoration.
    #[serde(default)]
    pub is_cached: bool,
}

impl ProviderUsage {
    pub fn live(provider: Provider, windows: Vec<UsageWindow>, plan: Option<String>, credit_balance: Option<String>) -> Self {
        let empty = windows.is_empty() && credit_balance.is_none();
        ProviderUsage {
            provider,
            windows,
            observed_at: Some(now_ms()),
            state: if empty { State::Unavailable } else { State::Live },
            reason: if empty { Some(Reason::NoLimitsReported) } else { None },
            plan,
            credit_balance,
            origin: None,
            is_cached: false,
        }
    }

    pub fn unavailable(provider: Provider, reason: Reason) -> Self {
        ProviderUsage {
            provider,
            windows: Vec::new(),
            observed_at: None,
            state: State::Unavailable,
            reason: Some(reason),
            plan: None,
            credit_balance: None,
            origin: None,
            is_cached: false,
        }
    }

    pub fn recording(mut self, route: Route) -> Self {
        self.origin = Some(route);
        self
    }

    pub fn reports_something(&self) -> bool {
        !self.windows.is_empty() || self.credit_balance.is_some()
    }

    /// The reading as it stands at `now`: windows whose reset has passed are
    /// gone, and a reading older than the cache keeps is gone entirely. `None`
    /// means nothing current survives.
    pub fn current(&self, now: i64) -> Option<ProviderUsage> {
        if let Some(observed) = self.observed_at {
            if now - observed > MAXIMUM_AGE_MS {
                return None;
            }
        }
        let kept: Vec<UsageWindow> = self
            .windows
            .iter()
            .filter(|w| w.resets_at.map_or(true, |at| at > now))
            .cloned()
            .collect();
        if kept.is_empty() && self.credit_balance.is_none() {
            return None;
        }
        let mut copy = self.clone();
        copy.windows = kept;
        Some(copy)
    }

    /// The window the ring shows: the one closest to its limit.
    pub fn headline(&self) -> Option<&UsageWindow> {
        self.windows
            .iter()
            .max_by(|a, b| a.used_fraction.total_cmp(&b.used_fraction))
    }
}

/// A fraction as a whole percentage that never rounds away the fact that
/// there is *some*, or that there is *not all*: anything used reads at least
/// 1%, anything short of full at most 99%. The panel draws the same rule.
pub fn percent_value(fraction: f64) -> i64 {
    if !fraction.is_finite() {
        return 0;
    }
    let percent = fraction.clamp(0.0, 1.0) * 100.0;
    if percent <= 0.0 {
        return 0;
    }
    if percent >= 100.0 {
        return 100;
    }
    (percent.round() as i64).clamp(1, 99)
}

/// Read a JSON number whether it arrived as an integer or a float.
pub fn number(value: Option<&serde_json::Value>) -> Option<f64> {
    value.and_then(|v| v.as_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: &str, fraction: f64, resets_at: Option<i64>) -> UsageWindow {
        UsageWindow {
            id: id.into(),
            kind: WindowKind::FiveHour,
            scope: None,
            used_fraction: fraction,
            window_seconds: 18_000,
            resets_at,
            is_exhausted: false,
        }
    }

    #[test]
    fn percent_never_rounds_away_the_ends() {
        assert_eq!(percent_value(0.0), 0);
        assert_eq!(percent_value(0.001), 1);
        assert_eq!(percent_value(0.996), 99);
        assert_eq!(percent_value(1.0), 100);
        assert_eq!(percent_value(1.4), 100);
        assert_eq!(percent_value(f64::NAN), 0);
    }

    #[test]
    fn a_window_past_its_reset_is_dropped_not_aged() {
        let now = 1_000_000;
        let mut usage = ProviderUsage::live(
            Provider::ClaudeCode,
            vec![window("gone", 0.9, Some(now - 1)), window("kept", 0.2, Some(now + 1))],
            None,
            None,
        );
        usage.observed_at = Some(now);
        let current = usage.current(now).unwrap();
        assert_eq!(current.windows.len(), 1);
        assert_eq!(current.windows[0].id, "kept");
    }

    #[test]
    fn a_reading_past_the_age_cap_is_gone() {
        let now = 10 * MAXIMUM_AGE_MS;
        let mut usage = ProviderUsage::live(Provider::Codex, vec![window("w", 0.5, None)], None, None);
        usage.observed_at = Some(now - MAXIMUM_AGE_MS - 1);
        assert!(usage.current(now).is_none());
    }
}
