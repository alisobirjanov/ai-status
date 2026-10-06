//! The last good reading per account, so a refusal can show numbers with a
//! date instead of an empty error. Port of `UsageCache.swift`.
//!
//! - A window whose reset has passed is dropped, not aged.
//! - Nothing older than a day is shown at all.
//! - A missing credential is not papered over.
//! - `.live` is not "newest": whichever reading was taken later wins.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{now_ms, provider_of, ProviderUsage, Reason, Route, State, UsageWindow};
use crate::paths;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    windows: Vec<UsageWindow>,
    observed_at: i64,
    plan: Option<String>,
    credit_balance: Option<String>,
    origin: Option<Route>,
}

pub struct Cache {
    file: PathBuf,
    readings: Option<HashMap<String, Stored>>,
}

impl Cache {
    pub fn new() -> Cache {
        Cache { file: paths::data_dir().join("last-readings.json"), readings: None }
    }

    #[cfg(test)]
    fn at(file: PathBuf) -> Cache {
        Cache { file, readings: None }
    }

    /// What to show for a reading just fetched, banking it when it is good.
    pub fn reconciled(&mut self, account: &str, fetched: ProviderUsage) -> ProviderUsage {
        let provider = fetched.provider;
        let now = now_ms();
        let valid = fetched.current(now);

        // Everything it reported has since reset or aged out. Last good
        // figures are still worth more than an error.
        if fetched.reports_something() && valid.is_none() {
            return self
                .reading(account)
                .unwrap_or_else(|| ProviderUsage::unavailable(provider, Reason::NoLimitsReported));
        }
        let current = valid.unwrap_or_else(|| fetched.clone());

        if current.state == State::Live && current.reports_something() {
            // A live reading older than the one banked is not the newer one.
            if let Some(banked) = self.reading(account) {
                if let (Some(banked_at), Some(taken_at)) = (banked.observed_at, current.observed_at) {
                    if taken_at < banked_at {
                        return banked;
                    }
                }
            }
            self.store(account, &current);
            return current;
        }

        if fetched.state == State::Unavailable && fetched.reason.is_some_and(Reason::is_missing_credential) {
            return fetched;
        }

        let Some(cached) = self.reading(account) else { return current };

        // A fetch that came back with something no older than the cache wins;
        // this only fills gaps, it never overrules a real answer.
        if let (Some(fetched_at), Some(cached_at)) = (current.observed_at, cached.observed_at) {
            if fetched_at >= cached_at && current.reports_something() {
                return current;
            }
        }
        cached
    }

    /// The banked reading, marked stale and filtered to what is still current.
    pub fn reading(&mut self, account: &str) -> Option<ProviderUsage> {
        let provider = provider_of(account)?;
        let stored = self.load().get(account)?.clone();
        let restored = ProviderUsage {
            provider,
            windows: stored.windows,
            observed_at: Some(stored.observed_at),
            state: State::Stale,
            reason: None,
            plan: stored.plan,
            credit_balance: stored.credit_balance,
            origin: stored.origin,
            is_cached: true,
        };
        restored.current(now_ms())
    }

    /// A login that moved to another account: its readings are that one's now.
    pub fn copy(&mut self, from: &str, to: &str) {
        if let Some(stored) = self.load().get(from).cloned() {
            self.load().insert(to.to_string(), stored);
            self.save();
        }
    }

    /// An account removed takes its readings with it.
    pub fn forget(&mut self, account: &str) {
        if self.load().remove(account).is_some() {
            self.save();
        }
    }

    fn store(&mut self, account: &str, usage: &ProviderUsage) {
        let stored = Stored {
            windows: usage.windows.clone(),
            observed_at: usage.observed_at.unwrap_or_else(now_ms),
            plan: usage.plan.clone(),
            credit_balance: usage.credit_balance.clone(),
            origin: usage.origin,
        };
        self.load().insert(account.to_string(), stored);
        self.save();
    }

    fn save(&mut self) {
        let file = self.file.clone();
        if let Ok(bytes) = serde_json::to_vec(self.load()) {
            let _ = paths::write_atomically(&file, &bytes);
        }
    }

    fn load(&mut self) -> &mut HashMap<String, Stored> {
        let file = &self.file;
        self.readings.get_or_insert_with(|| {
            std::fs::read_to_string(file)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Provider, WindowKind};

    fn scratch(name: &str) -> Cache {
        let file = std::env::temp_dir().join(format!("pulse-cache-test-{name}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&file);
        Cache::at(file)
    }

    fn reading(fraction: f64, observed_at: i64, resets_at: Option<i64>) -> ProviderUsage {
        let mut usage = ProviderUsage::live(
            Provider::ClaudeCode,
            vec![UsageWindow {
                id: "w".into(),
                kind: WindowKind::FiveHour,
                scope: None,
                used_fraction: fraction,
                window_seconds: 18_000,
                resets_at,
                is_exhausted: false,
            }],
            None,
            None,
        );
        usage.observed_at = Some(observed_at);
        usage
    }

    #[test]
    fn a_failure_is_answered_from_the_bank_marked_stale() {
        let mut cache = scratch("failure");
        let now = now_ms();
        cache.reconciled("claudeCode", reading(0.4, now - 1000, Some(now + 3_600_000)));
        let shown = cache.reconciled("claudeCode", ProviderUsage::unavailable(Provider::ClaudeCode, Reason::Unreachable));
        assert_eq!(shown.state, State::Stale);
        assert!(shown.is_cached);
        assert!((shown.windows[0].used_fraction - 0.4).abs() < 1e-9);
    }

    #[test]
    fn a_missing_login_is_not_papered_over() {
        let mut cache = scratch("missing");
        let now = now_ms();
        cache.reconciled("claudeCode", reading(0.4, now - 1000, None));
        let shown = cache.reconciled("claudeCode", ProviderUsage::unavailable(Provider::ClaudeCode, Reason::ClaudeSignInRequired));
        assert_eq!(shown.state, State::Unavailable);
    }

    #[test]
    fn an_older_live_reading_does_not_overwrite_a_newer_one() {
        let mut cache = scratch("older");
        let now = now_ms();
        cache.reconciled("claudeCode", reading(0.5, now - 1000, None));
        let shown = cache.reconciled("claudeCode", reading(0.2, now - 5000, None));
        assert!((shown.windows[0].used_fraction - 0.5).abs() < 1e-9);
    }

    #[test]
    fn each_account_has_its_own_bank() {
        let mut cache = scratch("accounts");
        let now = now_ms();
        cache.reconciled("claudeCode", reading(0.4, now - 1000, None));
        cache.reconciled("claudeCode#a1", reading(0.7, now - 1000, None));
        assert!((cache.reading("claudeCode").unwrap().windows[0].used_fraction - 0.4).abs() < 1e-9);
        assert!((cache.reading("claudeCode#a1").unwrap().windows[0].used_fraction - 0.7).abs() < 1e-9);
        cache.forget("claudeCode#a1");
        assert!(cache.reading("claudeCode#a1").is_none());
        // Written down, not only remembered.
        let mut again = Cache::at(cache.file.clone());
        assert!(again.reading("claudeCode").is_some());
        assert!(again.reading("claudeCode#a1").is_none());
        assert!(again.reading("somethingNew").is_none());
    }

    #[test]
    fn a_window_that_has_reset_is_not_restored() {
        let mut cache = scratch("reset");
        let now = now_ms();
        cache.reconciled("claudeCode", reading(0.9, now - 1000, Some(now + 50)));
        std::thread::sleep(std::time::Duration::from_millis(80));
        let shown = cache.reconciled("claudeCode", ProviderUsage::unavailable(Provider::ClaudeCode, Reason::Unreachable));
        assert_eq!(shown.state, State::Unavailable);
    }
}
