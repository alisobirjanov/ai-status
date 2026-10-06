//! The readings on screen and the loop that keeps them current.
//!
//! One timer, but not one cadence: each account has its own interval and the
//! loop sleeps until whichever is due soonest. Only the timer asks "due only";
//! every other way into a refresh — a setting changed, a click on a ring, the
//! panel coming back — is something happening, and a reason to look now.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

use crate::adaptive::{self, Signals};
use crate::cache::Cache;
use crate::model::{now_ms, provider_of, AccountId, Provider, ProviderUsage, Reason, Route, UsageWindow, WindowKind};
use crate::paths;
use crate::providers::{self, claude::Identity};
use crate::settings::Settings;

/// A pass in flight longer than this is treated as gone: the schedule must
/// not hang on a request that never returns.
const PASS_CEILING_MS: i64 = 180_000;
/// Sweeping the rail must not fire one request per ring.
const LOOK_COOLDOWN_MS: i64 = 60_000;
/// Told to slow down without being told for how long: wait five minutes,
/// as Claude Code does with the same answer.
const QUIET_DEFAULT_MS: i64 = 300_000;
/// However long a server asks for, it is asked again within the hour, as
/// Claude Code caps it.
const QUIET_CEILING_MS: i64 = 3_600_000;

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
    pub id: AccountId,
    pub provider: Provider,
    /// The product's name. Never translated.
    pub name: &'static str,
    /// What the reader named it, if they did.
    pub label: Option<String>,
    /// Who it is signed in as, as Claude Code noted it.
    pub email: Option<String>,
    /// What it is called where there is room for a word: its label; with
    /// more than one Claude account, the name on its email; the product's
    /// name otherwise.
    pub title: String,
    /// One Pulse added, rather than the product's own login.
    pub added: bool,
    /// The account Claude Code itself is signed in to.
    pub in_claude_code: bool,
    /// Claude Code's own login, when it is an account added as well: the
    /// same account, shown once, as that one.
    pub same_as: Option<AccountId>,
    pub enabled: bool,
    pub detected: bool,
    /// The login it is read with, as Settings shows it.
    pub credentials: String,
    pub usage: ProviderUsage,
    pub refreshing: bool,
    pub last_check: Option<Check>,
    /// Refused as too frequent: nothing is asked for it until then. Unix ms.
    pub retry_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Every account, enabled or not, enabled ones first in rail order.
    pub accounts: Vec<AccountView>,
    pub settings: Settings,
    pub version: &'static str,
    /// Light or dark as Windows has it, for a theme left to follow it.
    pub system_theme: tauri::Theme,
    /// Whether there is a Claude Code to sign in to an account with.
    pub claude_code_found: bool,
    pub sign_in: Option<SignIn>,
    pub switch: Option<Switch>,
}

/// A sign-in in the browser under way, or the last one, had it not worked.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignIn {
    /// Signing in again to an account there is; `None` adding one.
    pub account: Option<AccountId>,
    pub waiting: bool,
    pub problem: Option<SignInProblem>,
    /// Claude Code's own words for what went wrong. Never translated.
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SignInProblem {
    NoClaudeCode,
    /// Nobody signed in within the time given.
    TimedOut,
    Failed,
    /// The account signed in to is one Pulse already has.
    AlreadyAdded,
    TooMany,
}

/// Claude Code being handed an account's login, or the last time it was.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Switch {
    pub account: AccountId,
    pub waiting: bool,
    pub problem: Option<SwitchProblem>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SwitchProblem {
    /// The account has no login to hand over.
    NoLogin,
    /// Who Claude Code's own login is can't be told, so it has nowhere safe to go.
    Unknown,
    Failed,
}

pub struct Store {
    usage: HashMap<AccountId, ProviderUsage>,
    last_check: HashMap<AccountId, Check>,
    /// Asked, not answered: an account that refuses every time must not read
    /// as permanently due and spin the loop.
    asked_at: HashMap<AccountId, i64>,
    /// Refused as too frequent: not asked again until then, by the timer or
    /// by a click. Asking anyway only keeps the refusals coming.
    quiet_until: HashMap<AccountId, i64>,
    last_change: HashMap<AccountId, i64>,
    in_flight: HashMap<AccountId, (u64, i64)>,
    /// Who each Claude login is, looked at again after every reading.
    identity: HashMap<AccountId, Identity>,
    /// The limits the Claude account in use has run out of, each said once.
    /// `None` until it is first read: what was spent already at launch was
    /// news before it.
    spent_said: Option<HashSet<String>>,
    pub sign_in: Option<SignIn>,
    /// Says the sign-in under way is not to be waited for any longer.
    pub sign_in_cancel: Option<Arc<Notify>>,
    pub switch: Option<Switch>,
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
            quiet_until: HashMap::new(),
            last_change: HashMap::new(),
            in_flight: HashMap::new(),
            identity: HashMap::new(),
            spent_said: None,
            sign_in: None,
            sign_in_cancel: None,
            switch: None,
            next_pass: 1,
            last_looked: None,
            last_look_refresh: None,
            cache: Cache::new(),
        }
    }

    /// Paint the bank before the first request, so the rail is not blank on
    /// a cold start. Nothing is read for an account that is off — except who
    /// a Claude login is, which Settings shows whether it is on or not.
    pub fn seed(&mut self, settings: &Settings) {
        for account in settings.monitored() {
            if !self.usage.contains_key(&account) {
                let Some(provider) = provider_of(&account) else { continue };
                let seeded = self
                    .cache
                    .reading(&account)
                    .unwrap_or_else(|| ProviderUsage::unavailable(provider, Reason::NotChecked));
                self.usage.insert(account, seeded);
            }
        }
        for account in settings.all_accounts() {
            if provider_of(&account) == Some(Provider::ClaudeCode) && !self.identity.contains_key(&account) {
                let identity = providers::claude::identity(providers::folder_of(&account).as_deref());
                self.identity.insert(account, identity);
            }
        }
    }

    /// Signed in again: a new login is worth asking with at once.
    pub fn note_identity(&mut self, account: &str, identity: Identity) {
        self.identity.insert(account.to_string(), identity);
        self.quiet_until.remove(account);
    }

    /// An account added that Claude Code is signed in to as well. Its own
    /// folder is left alone and it is read through Claude Code: a login
    /// handed over is in both places, and only one of them may be renewed.
    pub fn mirrors(&self, account: &str) -> bool {
        let uuid = |id: &str| self.identity.get(id).and_then(|i| i.uuid.as_ref());
        crate::model::slot_of(account).is_some() && uuid(account).is_some() && uuid(account) == uuid(Provider::ClaudeCode.id())
    }

    /// What is read for an account: Claude Code's own login for one it mirrors.
    pub fn read_as<'a>(&self, account: &'a str) -> &'a str {
        if self.mirrors(account) {
            Provider::ClaudeCode.id()
        } else {
            account
        }
    }

    /// Whether a Claude login is being read, and so perhaps renewed, right now.
    pub fn claude_in_flight(&self, now: i64) -> bool {
        self.in_flight
            .iter()
            .any(|(id, (_, at))| provider_of(id) == Some(Provider::ClaudeCode) && now - at < PASS_CEILING_MS)
    }

    /// Who each Claude login is, looked at again: logins have moved.
    pub fn reread_identities(&mut self, accounts: &[AccountId]) {
        for account in accounts.iter().filter(|id| provider_of(id) == Some(Provider::ClaudeCode)) {
            let identity = providers::claude::identity(providers::folder_of(account).as_deref());
            self.identity.insert(account.clone(), identity);
        }
    }

    /// A login moved from one account to another takes its figures along.
    pub fn copy_readings(&mut self, from: &str, to: &str) {
        if let Some(usage) = self.usage.get(from).cloned() {
            self.usage.insert(to.to_string(), usage);
        }
        if let Some(check) = self.last_check.get(from).cloned() {
            self.last_check.insert(to.to_string(), check);
        }
        // A wait was asked of the login, so it goes where the login goes.
        match self.quiet_until.get(from).copied() {
            Some(until) => self.quiet_until.insert(to.to_string(), until),
            None => self.quiet_until.remove(to),
        };
        self.cache.copy(from, to);
    }

    /// An account removed: nothing of it is kept, on screen or banked.
    pub fn forget(&mut self, account: &str) {
        self.usage.remove(account);
        self.last_check.remove(account);
        self.asked_at.remove(account);
        self.quiet_until.remove(account);
        self.last_change.remove(account);
        self.in_flight.remove(account);
        self.identity.remove(account);
        self.cache.forget(account);
    }

    /// The Claude account in use has just run out of a limit, and another
    /// has room: who, and what to say. Each limit once, until it resets.
    fn who_is_free(&mut self, settings: &Settings) -> Option<(String, String)> {
        if !settings.says_who_is_free || !settings.is_enabled(Provider::ClaudeCode.id()) || settings.claude_accounts.is_empty() {
            return None;
        }
        let snapshot = self.snapshot(settings);
        let accounts: Vec<&AccountView> =
            snapshot.accounts.iter().filter(|a| a.provider == Provider::ClaudeCode && a.same_as.is_none()).collect();
        let in_use = accounts.iter().find(|a| a.in_claude_code)?;
        in_use.usage.observed_at?;
        // Claude Code may change accounts: a limit is that account's.
        let key = |w: &UsageWindow| format!("{}:{}:{}", in_use.email.as_deref().unwrap_or(&in_use.id), w.id, w.resets_at.unwrap_or(0));
        let spent: Vec<&UsageWindow> = in_use.usage.windows.iter().filter(|w| is_spent(w)).collect();
        let fresh = self.spent_said.as_ref().and_then(|said| spent.iter().copied().find(|w| !said.contains(&key(w))));
        self.spent_said = Some(spent.iter().map(|w| key(w)).collect());
        let window = fresh?;
        let free = accounts
            .iter()
            .filter(|a| a.id != in_use.id && a.usage.state != crate::model::State::Unavailable && !a.usage.windows.is_empty())
            .filter(|a| !a.usage.windows.iter().any(is_spent))
            .min_by(|a, b| fullest(a).total_cmp(&fullest(b)))?;
        let limit = crate::i18n::text(if window.kind == WindowKind::Weekly { "weeklyLimit" } else { "fiveHourLimit" });
        let title = crate::i18n::text("freeTitle").replace("{0}", &free.title);
        let figures: Vec<String> = free
            .usage
            .windows
            .iter()
            .filter(|w| matches!(w.kind, WindowKind::FiveHour | WindowKind::Weekly))
            .map(|w| format!("{} {}%", crate::i18n::text(if w.kind == WindowKind::Weekly { "weekly" } else { "fiveHour" }), crate::model::percent_value(w.used_fraction)))
            .collect();
        let body = crate::i18n::text("freeBody").replace("{0}", &in_use.title).replace("{1}", limit).replace("{2}", &figures.join(" · "));
        Some((title, body))
    }

    pub fn snapshot(&self, settings: &Settings) -> Snapshot {
        let now = now_ms();
        let mut order: Vec<AccountId> = settings.monitored();
        order.extend(settings.all_accounts().into_iter().filter(|id| !settings.is_enabled(id)));
        // Who Claude Code is signed in as, and whether that is an account added too.
        let uuid_of = |id: &str| self.identity.get(id).and_then(|i| i.uuid.clone());
        let in_use = uuid_of(Provider::ClaudeCode.id());
        let same_as = in_use.as_ref().and_then(|uuid| {
            settings.all_accounts().into_iter().find(|id| crate::model::slot_of(id).is_some() && uuid_of(id).as_ref() == Some(uuid))
        });

        let accounts = order
            .into_iter()
            .filter_map(|id| {
                let provider = provider_of(&id)?;
                let email = self.identity.get(&id).and_then(|i| i.email.clone());
                let read = self.read_as(&id);
                Some(AccountView {
                    provider,
                    name: provider.display_name(),
                    label: settings.account_labels.get(&id).cloned(),
                    title: title(&id, provider, settings, email.as_deref()),
                    email,
                    added: providers::folder_of(&id).is_some(),
                    in_claude_code: provider == Provider::ClaudeCode && in_use.is_some() && uuid_of(&id) == in_use,
                    same_as: if id == Provider::ClaudeCode.id() { same_as.clone() } else { None },
                    enabled: settings.is_enabled(&id),
                    detected: providers::is_installed(&id),
                    credentials: paths::shown(&providers::credentials_file(&id)),
                    usage: self
                        .usage
                        .get(read)
                        .cloned()
                        .unwrap_or_else(|| ProviderUsage::unavailable(provider, Reason::NotChecked)),
                    refreshing: self.in_flight.get(read).is_some_and(|(_, at)| now - at < PASS_CEILING_MS),
                    last_check: self.last_check.get(read).cloned(),
                    retry_at: self.quiet(read, now),
                    id,
                })
            })
            .collect();

        Snapshot {
            accounts,
            settings: settings.clone(),
            version: crate::VERSION,
            system_theme: crate::settings::system_theme(),
            claude_code_found: providers::claude_code::is_available(),
            sign_in: self.sign_in.clone(),
            switch: self.switch.clone(),
        }
    }

    fn interval_ms(&self, account: &str, settings: &Settings, now: i64) -> i64 {
        if let Some(minutes) = settings.refresh_minutes {
            return minutes as i64 * 60_000;
        }
        let signals = Signals {
            last_agent_activity: adaptive::last_agent_activity(account),
            last_change: self.last_change.get(account).copied(),
            last_looked: self.last_looked,
            is_panel_visible: settings.panel_visible,
        };
        adaptive::interval_secs(&signals, now) * 1000
    }

    /// Until when an account is not to be asked, if it is waiting now.
    fn quiet(&self, account: &str, now: i64) -> Option<i64> {
        self.quiet_until.get(account).copied().filter(|until| *until > now)
    }

    /// Claim an account for a pass. `None` when one is already out for it,
    /// or it was refused as too frequent and the wait isn't over.
    fn begin(&mut self, account: &str, now: i64) -> Option<u64> {
        if self.quiet(account, now).is_some() {
            return None;
        }
        // Claude Code is being handed a login: none is read until it has been.
        if self.switch.as_ref().is_some_and(|s| s.waiting) && provider_of(account) == Some(Provider::ClaudeCode) {
            return None;
        }
        if let Some((_, started)) = self.in_flight.get(account) {
            if now - started < PASS_CEILING_MS {
                return None;
            }
        }
        let pass = self.next_pass;
        self.next_pass += 1;
        self.in_flight.insert(account.to_string(), (pass, now));
        self.asked_at.insert(account.to_string(), now);
        Some(pass)
    }

    /// Write an answer, unless a newer pass has claimed the account since:
    /// abandoned work still answers eventually, and must not overwrite.
    fn commit(&mut self, account: &str, pass: u64, raw: ProviderUsage, identity: Option<Identity>) {
        if self.in_flight.get(account).map(|(p, _)| *p) != Some(pass) {
            return;
        }
        self.in_flight.remove(account);
        let now = now_ms();

        if let Some(identity) = identity {
            self.identity.insert(account.to_string(), identity);
        }
        self.last_check.insert(account.to_string(), Check { reason: raw.reason, origin: raw.origin, checked_at: now });
        if raw.reason == Some(Reason::RateLimited) {
            let wait = raw.retry_after_ms.unwrap_or(QUIET_DEFAULT_MS).min(QUIET_CEILING_MS);
            self.quiet_until.insert(account.to_string(), now + wait);
        } else {
            self.quiet_until.remove(account);
        }

        let shown = self.cache.reconciled(account, raw);
        let moved = self.usage.get(account).map_or(true, |old| !same_figures(&old.windows, &shown.windows));
        if moved && shown.reports_something() {
            self.last_change.insert(account.to_string(), now);
        }
        self.usage.insert(account.to_string(), shown);
    }
}

fn is_spent(window: &UsageWindow) -> bool {
    window.is_exhausted || window.used_fraction >= 1.0
}

/// How close an account is to its nearest limit.
fn fullest(account: &AccountView) -> f64 {
    account.usage.windows.iter().map(|w| w.used_fraction).fold(0.0, f64::max)
}

/// What an account is called where there is room for a word.
fn title(account: &str, provider: Provider, settings: &Settings, email: Option<&str>) -> String {
    if let Some(label) = settings.account_labels.get(account) {
        return label.clone();
    }
    if provider == Provider::ClaudeCode && settings.claude_account_count() > 1 {
        if let Some(name) = email.and_then(|e| e.split('@').next()).filter(|n| !n.is_empty()) {
            return name.to_string();
        }
        // Not signed in yet: which one it is, in the order they were added.
        if let Some(slot) = crate::model::slot_of(account) {
            let place = settings.claude_accounts.iter().position(|s| s == slot).unwrap_or(0);
            return format!("{} {}", provider.display_name(), place + 2);
        }
    }
    provider.display_name().to_string()
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

/// Ask these accounts now. Returns at once; each commits as it answers.
/// Disabled accounts are not fetched, whoever asks.
pub fn refresh(app: &AppHandle, requested: &[AccountId]) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    if !settings.has_chosen {
        return;
    }
    let now = now_ms();

    let claimed: Vec<(AccountId, u64)> = {
        let mut store = state.store.lock().unwrap();
        let mut asked: Vec<AccountId> = Vec::new();
        for id in requested.iter().filter(|id| settings.is_enabled(id)) {
            let read = store.read_as(id).to_string();
            if !asked.contains(&read) {
                asked.push(read);
            }
        }
        asked.into_iter().filter_map(|id| store.begin(&id, now).map(|pass| (id, pass))).collect()
    };
    if claimed.is_empty() {
        return;
    }
    emit_snapshot(app);

    for (account, pass) in claimed {
        let app = app.clone();
        let settings = settings.clone();
        tauri::async_runtime::spawn(async move {
            let raw = providers::fetch(&account, &settings).await;
            // Claude Code may have signed in or out, or renewed, on the way.
            let identity = (raw.provider == Provider::ClaudeCode)
                .then(|| providers::claude::identity(providers::folder_of(&account).as_deref()));
            let free = {
                let state = app.state::<AppState>();
                let settings = state.settings.lock().unwrap().clone();
                let mut store = state.store.lock().unwrap();
                store.commit(&account, pass, raw, identity);
                store.who_is_free(&settings)
            };
            emit_snapshot(&app);
            if let Some((title, body)) = free {
                use tauri_plugin_notification::NotificationExt;
                let _ = app.notification().builder().title(title).body(body).show();
            }
            // The next wait depends on what just came back.
            app.state::<AppState>().wake.notify_one();
        });
    }
}

pub fn refresh_all(app: &AppHandle) {
    let monitored = app.state::<AppState>().settings.lock().unwrap().monitored();
    refresh(app, &monitored);
}

/// The reader hovered the rail. It counts as a signal, and a rail whose
/// figures are older than twice the cadence is refreshed — but never-read is
/// not overdue, and a sweep down the rail asks at most once a minute.
pub fn note_looked(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    let now = now_ms();
    let overdue: Vec<AccountId> = {
        let mut store = state.store.lock().unwrap();
        store.last_looked = Some(now);
        if store.last_look_refresh.is_some_and(|at| now - at < LOOK_COOLDOWN_MS) {
            return;
        }
        let overdue: Vec<AccountId> = settings
            .monitored()
            .into_iter()
            .filter(|id| !store.mirrors(id))
            .filter(|id| {
                let observed = store.usage.get(id).and_then(|u| u.observed_at);
                observed.is_some_and(|at| now - at > 2 * store.interval_ms(id, &settings, now))
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

/// Wait for the Claude logins being read to be done with, a minute at most.
pub async fn settle(app: &AppHandle) {
    for _ in 0..240 {
        if !app.state::<AppState>().store.lock().unwrap().claude_in_flight(now_ms()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// The timer. Sleeps until the soonest account is due, or until woken.
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
                    for account in settings.monitored().into_iter().filter(|id| !store.mirrors(id)) {
                        let interval = store.interval_ms(&account, &settings, now);
                        let next = store.asked_at.get(&account).map_or(now, |at| at + interval);
                        let next = store.quiet(&account, now).map_or(next, |until| next.max(until));
                        if next <= now {
                            due.push(account);
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
            // setting changes. A due account already in flight is re-checked
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

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(five_hour: f64, weekly: f64) -> ProviderUsage {
        let window = |id: &str, kind, used_fraction| UsageWindow {
            id: id.into(),
            kind,
            scope: None,
            used_fraction,
            window_seconds: 0,
            resets_at: Some(1_000),
            is_exhausted: false,
        };
        let windows = vec![window("five_hour", WindowKind::FiveHour, five_hour), window("seven_day", WindowKind::Weekly, weekly)];
        ProviderUsage::live(Provider::ClaudeCode, windows, None, None)
    }

    fn who(uuid: &str, email: &str) -> Identity {
        Identity { uuid: Some(uuid.into()), email: Some(email.into()) }
    }

    fn three_accounts() -> Settings {
        Settings { enabled: vec!["claudeCode".into()], claude_accounts: vec!["a1".into(), "b2".into()], has_chosen: true, ..Settings::default() }
    }

    #[test]
    fn running_out_says_once_which_account_has_room() {
        let settings = three_accounts();
        let mut store = Store::new();
        store.identity.insert("claudeCode".into(), who("u1", "me@example.com"));
        store.identity.insert("claudeCode#a1".into(), who("u2", "work@example.com"));
        store.identity.insert("claudeCode#b2".into(), who("u3", "spare@example.com"));
        store.usage.insert("claudeCode".into(), usage(0.5, 0.2));
        store.usage.insert("claudeCode#a1".into(), usage(0.1, 0.6));
        store.usage.insert("claudeCode#b2".into(), usage(1.0, 0.3));
        // Nothing has run out yet.
        assert_eq!(store.who_is_free(&settings), None);

        // Spare has run out itself, so it is Work that has room.
        store.usage.insert("claudeCode".into(), usage(1.0, 0.2));
        let (title, body) = store.who_is_free(&settings).expect("said");
        assert!(title.contains("work") && !title.contains("spare"), "{title}");
        assert!(body.starts_with("me "), "{body}");
        // Once, not on every reading after.
        assert_eq!(store.who_is_free(&settings), None);

        // Switched off, nothing is said.
        store.usage.insert("claudeCode".into(), usage(1.0, 1.0));
        assert_eq!(store.who_is_free(&Settings { says_who_is_free: false, ..settings.clone() }), None);
    }

    #[test]
    fn spent_already_when_first_read_is_not_news() {
        let settings = three_accounts();
        let mut store = Store::new();
        store.identity.insert("claudeCode".into(), who("u1", "me@example.com"));
        store.identity.insert("claudeCode#a1".into(), who("u2", "work@example.com"));
        store.usage.insert("claudeCode".into(), usage(1.0, 0.2));
        store.usage.insert("claudeCode#a1".into(), usage(0.1, 0.6));
        assert_eq!(store.who_is_free(&settings), None);
    }

    #[test]
    fn claude_codes_own_login_added_too_is_one_account() {
        let settings = three_accounts();
        let mut store = Store::new();
        store.identity.insert("claudeCode".into(), who("u2", "work@example.com"));
        store.identity.insert("claudeCode#a1".into(), who("u2", "work@example.com"));
        store.identity.insert("claudeCode#b2".into(), who("u3", "spare@example.com"));
        let snapshot = store.snapshot(&settings);
        let view = |id: &str| snapshot.accounts.iter().find(|a| a.id == id).unwrap();
        assert_eq!(view("claudeCode").same_as.as_deref(), Some("claudeCode#a1"));
        assert!(view("claudeCode").in_claude_code && view("claudeCode#a1").in_claude_code);
        assert!(view("claudeCode#a1").same_as.is_none() && !view("claudeCode#b2").in_claude_code);
    }

    #[test]
    fn refused_as_too_frequent_it_is_left_alone_for_a_while() {
        let mut store = Store::new();
        let ask = |store: &mut Store| store.begin("claudeCode", now_ms());

        // Not told how long: five minutes, by the timer or a click alike.
        let pass = ask(&mut store).unwrap();
        store.commit("claudeCode", pass, ProviderUsage::rate_limited(Provider::ClaudeCode, None), None);
        let until = store.quiet("claudeCode", now_ms()).expect("waiting");
        assert!((until - now_ms() - QUIET_DEFAULT_MS).abs() < 5_000);
        assert_eq!(ask(&mut store), None);
        assert_eq!(store.snapshot(&three_accounts()).accounts[0].retry_at, Some(until));

        // Told, it waits as long as it was told, but not past the hour.
        store.quiet_until.clear();
        let pass = ask(&mut store).unwrap();
        store.commit("claudeCode", pass, ProviderUsage::rate_limited(Provider::ClaudeCode, Some(30_000)), None);
        assert!(store.quiet("claudeCode", now_ms()).unwrap() - now_ms() <= 30_000);
        store.quiet_until.clear();
        let pass = ask(&mut store).unwrap();
        store.commit("claudeCode", pass, ProviderUsage::rate_limited(Provider::ClaudeCode, Some(86_400_000)), None);
        assert!(store.quiet("claudeCode", now_ms()).unwrap() - now_ms() <= QUIET_CEILING_MS);

        // The wait is the login's, and goes where it goes.
        store.copy_readings("claudeCode", "claudeCode#a1");
        assert!(store.quiet("claudeCode#a1", now_ms()).is_some());

        // Over, and answered, it is asked as usual again.
        store.quiet_until.insert("claudeCode".into(), now_ms() - 1);
        let pass = ask(&mut store).unwrap();
        store.commit("claudeCode", pass, usage(0.2, 0.1), None);
        assert_eq!(store.quiet_until.get("claudeCode"), None);
        assert!(ask(&mut store).is_some());
    }

    #[test]
    fn the_account_in_claude_code_is_read_through_it() {
        let settings = three_accounts();
        let mut store = Store::new();
        store.identity.insert("claudeCode".into(), who("u2", "work@example.com"));
        store.identity.insert("claudeCode#a1".into(), who("u2", "work@example.com"));
        store.identity.insert("claudeCode#b2".into(), who("u3", "spare@example.com"));
        store.usage.insert("claudeCode".into(), usage(0.4, 0.1));
        store.usage.insert("claudeCode#a1".into(), usage(0.9, 0.9));
        assert!(store.mirrors("claudeCode#a1") && !store.mirrors("claudeCode#b2") && !store.mirrors("claudeCode"));
        assert_eq!(store.read_as("claudeCode#a1"), "claudeCode");

        let snapshot = store.snapshot(&settings);
        let work = snapshot.accounts.iter().find(|a| a.id == "claudeCode#a1").unwrap();
        assert_eq!(work.usage.windows[0].used_fraction, 0.4);

        // Signed out of Claude Code, nobody is read through it.
        store.identity.insert("claudeCode".into(), Identity::default());
        assert!(!store.mirrors("claudeCode#a1"));
    }
}
