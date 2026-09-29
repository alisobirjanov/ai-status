//! The readings on screen and the loop that keeps them current.
//!
//! One timer, but not one cadence: each provider has its own interval and the
//! loop sleeps until whichever is due soonest. Only the timer asks "due only";
//! every other way into a refresh — a setting changed, a click on a ring, the
//! panel coming back — is something happening, and a reason to look now.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

use crate::adaptive::{self, Signals};
use crate::cache::Cache;
use crate::model::{now_ms, Provider, ProviderUsage, Reason, Route, UsageWindow};
use crate::paths;
use crate::providers;
use crate::settings::Settings;

/// A pass in flight longer than this is treated as gone: the schedule must
/// not hang on a request that never returns.
const PASS_CEILING_MS: i64 = 180_000;
/// Sweeping the rail must not fire one request per ring.
const LOOK_COOLDOWN_MS: i64 = 60_000;

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub store: Mutex<Store>,
    pub wake: Notify,
}

/// The latest raw result, apart from what is drawn: the panel may be showing
/// banked figures while the last check failed, and Settings says why.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub reason: Option<Reason>,
    pub origin: Option<Route>,
    pub checked_at: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub provider: Provider,
    pub name: &'static str,
    pub enabled: bool,
    pub detected: bool,
    /// The login it is read with, as Settings shows it.
    pub credentials: String,
    pub usage: ProviderUsage,
    pub refreshing: bool,
    pub last_check: Option<Check>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Every provider, enabled or not, enabled ones first in rail order.
    pub accounts: Vec<AccountView>,
    pub settings: Settings,
    pub version: &'static str,
    /// Light or dark as Windows has it, for a theme left to follow it.
    pub system_theme: tauri::Theme,
}

pub struct Store {
    usage: HashMap<Provider, ProviderUsage>,
    last_check: HashMap<Provider, Check>,
    /// Asked, not answered: a provider that refuses every time must not read
    /// as permanently due and spin the loop.
    asked_at: HashMap<Provider, i64>,
    last_change: HashMap<Provider, i64>,
    in_flight: HashMap<Provider, (u64, i64)>,
    next_pass: u64,
    last_looked: Option<i64>,
    last_look_refresh: Option<i64>,
    cache: Cache,
}

impl Store {
    pub fn new() -> Store {
        Store {
            usage: HashMap::new(),
            last_check: HashMap::new(),
            asked_at: HashMap::new(),
            last_change: HashMap::new(),
            in_flight: HashMap::new(),
            next_pass: 1,
            last_looked: None,
            last_look_refresh: None,
            cache: Cache::new(),
        }
    }

    /// Paint the bank before the first request, so the rail is not blank on
    /// a cold start. Nothing is read for a provider that is off.
    pub fn seed(&mut self, settings: &Settings) {
        for &provider in &settings.enabled {
            if !self.usage.contains_key(&provider) {
                let seeded = self
                    .cache
                    .reading(provider)
                    .unwrap_or_else(|| ProviderUsage::unavailable(provider, Reason::NotChecked));
                self.usage.insert(provider, seeded);
            }
        }
    }

    pub fn snapshot(&self, settings: &Settings) -> Snapshot {
        let now = now_ms();
        let mut order: Vec<Provider> = settings.enabled.clone();
        order.extend(Provider::ALL.iter().filter(|p| !settings.is_enabled(**p)));

        let accounts = order
            .into_iter()
            .map(|provider| AccountView {
                provider,
                name: provider.display_name(),
                enabled: settings.is_enabled(provider),
                detected: providers::is_installed(provider),
                credentials: paths::shown(&providers::credentials_file(provider)),
                usage: self
                    .usage
                    .get(&provider)
                    .cloned()
                    .unwrap_or_else(|| ProviderUsage::unavailable(provider, Reason::NotChecked)),
                refreshing: self.in_flight.get(&provider).is_some_and(|(_, at)| now - at < PASS_CEILING_MS),
                last_check: self.last_check.get(&provider).cloned(),
            })
            .collect();

        Snapshot {
            accounts,
            settings: settings.clone(),
            version: crate::VERSION,
            system_theme: crate::settings::system_theme(),
        }
    }

    fn interval_ms(&self, provider: Provider, settings: &Settings, now: i64) -> i64 {
        if let Some(minutes) = settings.refresh_minutes {
            return minutes as i64 * 60_000;
        }
        let signals = Signals {
            last_agent_activity: adaptive::last_agent_activity(provider),
            last_change: self.last_change.get(&provider).copied(),
            last_looked: self.last_looked,
            is_panel_visible: settings.panel_visible,
        };
        adaptive::interval_secs(&signals, now) * 1000
    }

    /// Claim a provider for a pass. `None` when one is already out for it.
    fn begin(&mut self, provider: Provider, now: i64) -> Option<u64> {
        if let Some((_, started)) = self.in_flight.get(&provider) {
            if now - started < PASS_CEILING_MS {
                return None;
            }
        }
        let pass = self.next_pass;
        self.next_pass += 1;
        self.in_flight.insert(provider, (pass, now));
        self.asked_at.insert(provider, now);
        Some(pass)
    }

    /// Write an answer, unless a newer pass has claimed the provider since:
    /// abandoned work still answers eventually, and must not overwrite.
    fn commit(&mut self, provider: Provider, pass: u64, raw: ProviderUsage) {
        if self.in_flight.get(&provider).map(|(p, _)| *p) != Some(pass) {
            return;
        }
        self.in_flight.remove(&provider);
        let now = now_ms();

        self.last_check.insert(provider, Check { reason: raw.reason, origin: raw.origin, checked_at: now });

        let shown = self.cache.reconciled(raw);
        let moved = self.usage.get(&provider).map_or(true, |old| !same_figures(&old.windows, &shown.windows));
        if moved && shown.reports_something() {
            self.last_change.insert(provider, now);
        }
        self.usage.insert(provider, shown);
    }
}

/// Whether the reported windows moved. `observed_at` is left out, or every
/// fetch would look like a change and pin the interval to the floor.
fn same_figures(a: &[UsageWindow], b: &[UsageWindow]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.id == y.id && (x.used_fraction - y.used_fraction).abs() < 1e-9 && x.resets_at == y.resets_at && x.is_exhausted == y.is_exhausted
        })
}

// MARK: - Driving it

pub fn emit_snapshot(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    let snapshot = state.store.lock().unwrap().snapshot(&settings);
    crate::tray::update_tooltip(app, &snapshot);
    let _ = app.emit("snapshot", snapshot);
}

/// Ask these providers now. Returns at once; each commits as it answers.
/// Disabled providers are not fetched, whoever asks.
pub fn refresh(app: &AppHandle, requested: &[Provider]) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    if !settings.has_chosen {
        return;
    }
    let now = now_ms();

    let claimed: Vec<(Provider, u64)> = {
        let mut store = state.store.lock().unwrap();
        requested
            .iter()
            .filter(|p| settings.is_enabled(**p))
            .filter_map(|&p| store.begin(p, now).map(|pass| (p, pass)))
            .collect()
    };
    if claimed.is_empty() {
        return;
    }
    emit_snapshot(app);

    for (provider, pass) in claimed {
        let app = app.clone();
        let settings = settings.clone();
        tauri::async_runtime::spawn(async move {
            let raw = providers::fetch(provider, &settings).await;
            app.state::<AppState>().store.lock().unwrap().commit(provider, pass, raw);
            emit_snapshot(&app);
            // The next wait depends on what just came back.
            app.state::<AppState>().wake.notify_one();
        });
    }
}

pub fn refresh_all(app: &AppHandle) {
    let enabled = app.state::<AppState>().settings.lock().unwrap().enabled.clone();
    refresh(app, &enabled);
}

/// The reader hovered the rail. It counts as a signal, and a rail whose
/// figures are older than twice the cadence is refreshed — but never-read is
/// not overdue, and a sweep down the rail asks at most once a minute.
pub fn note_looked(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    let now = now_ms();
    let overdue: Vec<Provider> = {
        let mut store = state.store.lock().unwrap();
        store.last_looked = Some(now);
        if store.last_look_refresh.is_some_and(|at| now - at < LOOK_COOLDOWN_MS) {
            return;
        }
        let overdue: Vec<Provider> = settings
            .enabled
            .iter()
            .copied()
            .filter(|&p| {
                let observed = store.usage.get(&p).and_then(|u| u.observed_at);
                observed.is_some_and(|at| now - at > 2 * store.interval_ms(p, &settings, now))
            })
            .collect();
        if !overdue.is_empty() {
            store.last_look_refresh = Some(now);
        }
        overdue
    };
    if !overdue.is_empty() {
        refresh(app, &overdue);
    }
}

/// The timer. Sleeps until the soonest provider is due, or until woken.
pub fn start_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let state = app.state::<AppState>();
            let settings = state.settings.lock().unwrap().clone();
            let now = now_ms();

            let (due, wait_ms) = {
                let store = state.store.lock().unwrap();
                let mut due = Vec::new();
                let mut soonest: Option<i64> = None;
                if settings.has_chosen {
                    for &provider in &settings.enabled {
                        let interval = store.interval_ms(provider, &settings, now);
                        let next = store.asked_at.get(&provider).map_or(now, |at| at + interval);
                        if next <= now {
                            due.push(provider);
                        } else {
                            soonest = Some(soonest.map_or(next, |s: i64| s.min(next)));
                        }
                    }
                }
                (due, soonest.map(|s| (s - now).max(1000)))
            };

            if !due.is_empty() {
                refresh(&app, &due);
            }

            // Nothing to wait for means nothing is switched on: sleep until a
            // setting changes. A due provider already in flight is re-checked
            // shortly rather than spun on.
            let wait = match (due.is_empty(), wait_ms) {
                (true, None) => None,
                (false, None) => Some(5_000),
                (_, Some(ms)) => Some(if due.is_empty() { ms } else { ms.min(5_000) }),
            };
            match wait {
                Some(ms) => {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(ms as u64)) => {}
                        _ = state.wake.notified() => {}
                    }
                }
                None => state.wake.notified().await,
            }
        }
    });
}
