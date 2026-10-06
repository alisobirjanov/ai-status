//! What the reader chose, persisted as `%APPDATA%\Dipstick\settings.json`.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

use crate::model::{provider_of, slot_of, AccountId, ProviderUsage, UsageWindow, WindowKind};
use crate::panel::Side;
use crate::paths;
use crate::providers::codex;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Switched-on services, in rail order. `claudeCode` stands for every
    /// Claude account: each is read, and one at a time is on the rail.
    /// First launch enables nothing.
    #[serde(deserialize_with = "known_accounts")]
    pub enabled: Vec<AccountId>,
    /// Claude accounts added beyond the one Claude Code itself is signed in
    /// to, by slot: each is a folder of its own that Claude Code keeps the
    /// login in (`paths::claude_account_dir`).
    pub claude_accounts: Vec<String>,
    /// What the reader calls an account, by id. Without one, a Claude
    /// account goes by the name on its email.
    pub account_labels: BTreeMap<AccountId, String>,
    /// With several Claude accounts, whose rings sit on the rail.
    #[serde(deserialize_with = "or_default")]
    pub rail_follows: RailFollows,
    /// The Claude account in use ran out: say which other one has room.
    pub says_who_is_free: bool,
    /// The initial choice has been made. Until it has, nothing is read or
    /// fetched and there is no panel — only the chooser.
    pub has_chosen: bool,
    pub codex_source: codex::Source,
    /// `None` is adaptive (2–30 minutes); a number is that many minutes for
    /// everything, because somebody who picked five meant five.
    pub refresh_minutes: Option<u32>,
    /// Count what is left instead of what is gone. Colour still means
    /// closeness to the limit.
    pub shows_remaining: bool,
    /// Where the ring turns red, in percent.
    pub warning_at: u32,
    #[serde(deserialize_with = "or_default")]
    pub ring_shows: RingShows,
    /// With both limits on a ring, a letter beside each figure says which is
    /// which.
    pub limit_letters: bool,
    pub panel_visible: bool,
    /// The rail's top-left corner on screen, in physical pixels.
    pub rail_position: Option<(i32, i32)>,
    /// The edge of the screen the rail is fused to, if it was left against
    /// one. Kept by Rust with `rail_position`, never by a page.
    #[serde(deserialize_with = "or_default")]
    pub rail_dock: Option<Side>,
    /// Docked, the rail winds down to a sliver against the edge while the
    /// pointer is elsewhere. Off — the default — a docked rail stays open,
    /// and off the edge it always does. 0.1.2 called this `autoCollapse`
    /// and had it on; that key is left unread, so the default reaches
    /// everybody once.
    pub tucks_away: bool,
    /// Hovering a ring opens its card. Off, the rail is only the rings.
    pub shows_card: bool,
    /// Light or dark, for Settings and the rail.
    #[serde(deserialize_with = "or_default")]
    pub theme: Theme,
    /// Light, the rail can stay dark anyway: dark reads over any wallpaper.
    pub rail_stays_dark: bool,
    /// The rail lets the desktop show through it. Its card stays solid: it is
    /// there to be read.
    pub glass: bool,
    /// How much shows through, in percent: one of `GLASS_CHOICES`.
    pub glass_transparency: u32,
    /// Launch at login is on by default and decided **once**: a reader who
    /// turned it off is never turned back on by a later launch.
    pub login_item_decided: bool,
    /// Ask the feed for a new Dipstick every few hours. Installing is always
    /// the reader's call; this is only whether to look.
    pub checks_for_updates: bool,
    /// The version whose arrival has been announced, so a new version makes
    /// one notification rather than one per check or per launch.
    pub update_announced: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: Vec::new(),
            claude_accounts: Vec::new(),
            account_labels: BTreeMap::new(),
            rail_follows: RailFollows::default(),
            says_who_is_free: true,
            has_chosen: false,
            codex_source: codex::Source::Automatic,
            refresh_minutes: None,
            shows_remaining: false,
            warning_at: 75,
            ring_shows: RingShows::default(),
            limit_letters: true,
            panel_visible: true,
            rail_position: None,
            rail_dock: None,
            tucks_away: false,
            shows_card: true,
            theme: Theme::default(),
            rail_stays_dark: false,
            glass: false,
            glass_transparency: 50,
            login_item_decided: false,
            checks_for_updates: true,
            update_announced: None,
        }
    }
}

/// Which limit a ring stands for. One choice for every service.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RingShows {
    /// Whichever limit is closest to running out.
    #[default]
    Fullest,
    FiveHour,
    Weekly,
    /// Both in one ring: the 5-hour limit on the top half, the weekly one on
    /// the bottom half.
    BothSplit,
    /// Both, one ring above the other.
    BothStacked,
    /// Both, the 5-hour limit round the outside and the weekly one inside it.
    BothNested,
}

impl RingShows {
    /// What a ring draws, top or outer first. One limit for a single choice,
    /// falling back to the fullest when the service doesn't report the one
    /// chosen. For both, the 5-hour and the weekly limit, either of which may
    /// be missing — unless the service reports neither but something else,
    /// which is then shown on its own rather than hidden.
    pub fn windows(self, usage: &ProviderUsage) -> Vec<Option<&UsageWindow>> {
        let chosen = |kind| usage.fullest_of(kind).or_else(|| usage.headline());
        match self {
            RingShows::Fullest => vec![usage.headline()],
            RingShows::FiveHour => vec![chosen(WindowKind::FiveHour)],
            RingShows::Weekly => vec![chosen(WindowKind::Weekly)],
            RingShows::BothSplit | RingShows::BothStacked | RingShows::BothNested => {
                match (usage.fullest_of(WindowKind::FiveHour), usage.fullest_of(WindowKind::Weekly)) {
                    (None, None) if usage.headline().is_some() => vec![usage.headline()],
                    (five_hour, weekly) => vec![five_hour, weekly],
                }
            }
        }
    }
}

/// Which of several Claude accounts the rail shows. The card on hover lists
/// every one whichever it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RailFollows {
    /// The one Claude Code is signed in to.
    #[default]
    InUse,
    /// Whichever is furthest from a limit.
    MostRoom,
    /// Each, one after another.
    EachInTurn,
}

/// Light or dark. One choice for Settings and the rail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    /// Whichever Windows is set to for apps, following it when it changes.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    /// What a window is told: nothing for System, so that it follows Windows.
    pub fn for_window(self) -> Option<tauri::Theme> {
        match self {
            Theme::System => None,
            Theme::Light => Some(tauri::Theme::Light),
            Theme::Dark => Some(tauri::Theme::Dark),
        }
    }

    /// Light or dark, System settled by asking Windows.
    pub fn resolved(self) -> tauri::Theme {
        self.for_window().unwrap_or_else(system_theme)
    }
}

/// The light-or-dark choice Windows has for apps. Unset is light, as on a
/// fresh install. A page can't ask its WebView instead: the WebView's
/// `prefers-color-scheme` is shared by every window, and follows whichever
/// window last had its theme set.
#[cfg(windows)]
pub fn system_theme() -> tauri::Theme {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let wide = |text: &str| text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (key, value) = (wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"), wide("AppsUseLightTheme"));
    let mut light: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: both names are NUL-terminated and outlive the call, and `size`
    // is the size of the buffer `light` gives it.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut light as *mut u32).cast(),
            &mut size,
        )
    };
    if status == 0 && light == 0 {
        tauri::Theme::Dark
    } else {
        tauri::Theme::Light
    }
}

#[cfg(not(windows))]
pub fn system_theme() -> tauri::Theme {
    tauri::Theme::Light
}

/// The choices Settings offers for where red begins. A short list, not a
/// slider: this is the one step in the colour language that means "pay
/// attention", and every option sits above the yellow step at 50%.
pub const WARNING_CHOICES: [u32; 6] = [60, 70, 75, 80, 85, 90];
pub const GLASS_CHOICES: [u32; 3] = [25, 50, 75];
/// An account's name is a word or two, not a paragraph.
pub const LABEL_MAX_CHARS: usize = 32;

impl Settings {
    fn file() -> std::path::PathBuf {
        paths::data_dir().join("settings.json")
    }

    pub fn load() -> Settings {
        let mut settings: Settings = std::fs::read_to_string(Self::file())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        settings.normalize();
        settings
    }

    pub fn save(&self) {
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let _ = paths::write_atomically(&Self::file(), &bytes);
        }
    }

    /// Never trust the stored list as written.
    pub fn normalize(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.claude_accounts.retain(|slot| crate::model::is_slot(slot) && seen.insert(slot.clone()));
        // A service, not an account: an account switched on by itself, as
        // Pulse Dev builds had it before 0.1.7, switches its service on.
        for id in &mut self.enabled {
            if let Some(provider) = provider_of(id) {
                *id = provider.id().to_string();
            }
        }
        let mut seen = std::collections::HashSet::new();
        self.enabled.retain(|id| provider_of(id).is_some() && seen.insert(id.clone()));
        let known = self.all_accounts();
        self.account_labels.retain(|id, _| known.contains(id));
        for label in self.account_labels.values_mut() {
            *label = tidy_label(label);
        }
        self.account_labels.retain(|_, label| !label.is_empty());
        if !WARNING_CHOICES.contains(&self.warning_at) {
            self.warning_at = 75;
        }
        if !GLASS_CHOICES.contains(&self.glass_transparency) {
            self.glass_transparency = 50;
        }
        if let Some(minutes) = self.refresh_minutes {
            self.refresh_minutes = Some(minutes.clamp(1, 60));
        }
    }

    /// Whether an account is read: its service is on, and it is still there.
    pub fn is_enabled(&self, account: &str) -> bool {
        let Some(provider) = provider_of(account) else { return false };
        self.enabled.iter().any(|id| id == provider.id()) && slot_of(account).map_or(true, |slot| self.claude_accounts.iter().any(|s| s == slot))
    }

    /// Every account read, in rail order: each service switched on, Claude
    /// with all of its accounts.
    pub fn monitored(&self) -> Vec<AccountId> {
        let all = self.all_accounts();
        self.enabled
            .iter()
            .flat_map(|service| all.iter().filter(move |id| provider_of(id).map(|p| p.id()) == Some(service.as_str())).cloned())
            .collect()
    }

    /// Every account there is, on the rail or not: Claude Code's own login,
    /// the Claude accounts added after it, then Codex.
    pub fn all_accounts(&self) -> Vec<AccountId> {
        std::iter::once("claudeCode".to_string())
            .chain(self.claude_accounts.iter().map(|slot| format!("claudeCode#{slot}")))
            .chain(std::iter::once("codex".to_string()))
            .collect()
    }

    /// How many Claude accounts there are, on the rail or not.
    pub fn claude_account_count(&self) -> usize {
        1 + self.claude_accounts.len()
    }
}

/// One line, no longer than a name needs to be.
pub fn tidy_label(label: &str) -> String {
    let words: Vec<&str> = label.split_whitespace().collect();
    words.join(" ").chars().take(LABEL_MAX_CHARS).collect::<String>().trim_end().to_string()
}

/// Unknown ids are dropped rather than failing the whole file: a setting
/// written by a newer version must not reset everything else.
fn known_accounts<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<AccountId>, D::Error> {
    let raw: Vec<serde_json::Value> = Vec::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .filter_map(|value| value.as_str().filter(|id| provider_of(id).is_some()).map(str::to_string))
        .collect())
}

/// A value this version doesn't know falls back to the default, for the same
/// reason.
fn or_default<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(deserializer: D) -> Result<T, D::Error> {
    let raw = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(raw).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Provider;

    #[test]
    fn unknown_providers_are_dropped_not_fatal() {
        let settings: Settings = serde_json::from_str(
            r#"{ "enabled": ["codex", "somethingNew", "codex", 7, "codex#a1"], "hasChosen": true, "warningAt": 73 }"#,
        )
        .unwrap();
        let mut settings = settings;
        settings.normalize();
        assert_eq!(settings.enabled, vec!["codex"]);
        // A file from before there could be more than one Claude account has only the one.
        assert!(settings.claude_accounts.is_empty());
        assert_eq!(settings.all_accounts(), vec!["claudeCode", "codex"]);
        assert!(settings.has_chosen);
        assert_eq!(settings.warning_at, 75);
        assert!(settings.panel_visible);
        // A file from before updates were checked has the check on.
        assert!(settings.checks_for_updates);
        // One from before several accounts has the rail follow the one in use, and say who is free.
        assert_eq!(settings.rail_follows, RailFollows::InUse);
        assert!(settings.says_who_is_free);
        // One from before docking has a floating rail that stays open once docked.
        assert_eq!(settings.rail_dock, None);
        assert!(!settings.tucks_away);
        // And one from before the card could be switched off shows it.
        assert!(settings.shows_card);
        // One from before light mode follows Windows, the rail included.
        assert_eq!(settings.theme, Theme::System);
        assert!(!settings.rail_stays_dark);
        // And one from before glass has a solid rail.
        assert!(!settings.glass);
        assert_eq!(settings.glass_transparency, 50);
        assert_eq!(settings.update_announced, None);
    }

    #[test]
    fn added_claude_accounts_come_after_the_first() {
        let mut settings: Settings = serde_json::from_str(
            r#"{
                "enabled": ["claudeCode#b2", "claudeCode", "claudeCode#gone", "claudeCode#a1", "claudeCode#b2"],
                "claudeAccounts": ["a1", "b2", "a1", "../x"],
                "accountLabels": { "claudeCode#a1": "  Work\n  laptop ", "claudeCode#gone": "Old", "codex": "   " }
            }"#,
        )
        .unwrap();
        settings.normalize();
        assert_eq!(settings.claude_accounts, vec!["a1", "b2"]);
        // Rail order is kept; one that was removed is gone from it.
        // Switched on by account, as before there was one Claude ring: the service is.
        assert_eq!(settings.enabled, vec!["claudeCode"]);
        assert_eq!(settings.all_accounts(), vec!["claudeCode", "claudeCode#a1", "claudeCode#b2", "codex"]);
        assert_eq!(settings.monitored(), vec!["claudeCode", "claudeCode#a1", "claudeCode#b2"]);
        assert!(!settings.is_enabled("claudeCode#gone"));
        assert_eq!(settings.claude_account_count(), 3);
        assert_eq!(settings.account_labels.len(), 1);
        assert_eq!(settings.account_labels["claudeCode#a1"], "Work laptop");
        assert!(settings.is_enabled("claudeCode#a1"));
        assert!(!settings.is_enabled("codex"));
    }

    #[test]
    fn a_label_is_one_short_line() {
        assert_eq!(tidy_label("  a \t b  "), "a b");
        assert_eq!(tidy_label(&"x".repeat(50)).chars().count(), LABEL_MAX_CHARS);
        assert_eq!(tidy_label(&format!("{} y", "x".repeat(LABEL_MAX_CHARS - 1))), "x".repeat(LABEL_MAX_CHARS - 1));
    }

    #[test]
    fn glass_is_one_of_the_choices() {
        let mut settings: Settings = serde_json::from_str(r#"{ "glass": true, "glassTransparency": 60 }"#).unwrap();
        settings.normalize();
        assert!(settings.glass);
        assert_eq!(settings.glass_transparency, 50);

        let mut settings: Settings = serde_json::from_str(r#"{ "glass": true, "glassTransparency": 75 }"#).unwrap();
        settings.normalize();
        assert_eq!(settings.glass_transparency, 75);
    }

    #[test]
    fn an_unknown_theme_is_the_default_not_fatal() {
        let settings: Settings = serde_json::from_str(r#"{ "hasChosen": true, "theme": "sepia" }"#).unwrap();
        assert!(settings.has_chosen);
        assert_eq!(settings.theme, Theme::System);

        let settings: Settings = serde_json::from_str(r#"{ "theme": "light", "railStaysDark": true }"#).unwrap();
        assert_eq!(settings.theme, Theme::Light);
        assert!(settings.rail_stays_dark);
    }

    #[test]
    fn a_chosen_theme_is_what_the_window_is_told() {
        assert_eq!(Theme::System.for_window(), None);
        assert_eq!(Theme::Light.for_window(), Some(tauri::Theme::Light));
        assert_eq!(Theme::Dark.resolved(), tauri::Theme::Dark);
    }

    #[test]
    fn a_rail_left_across_the_top_is_put_back_there() {
        let settings: Settings = serde_json::from_str(r#"{ "railPosition": [500, 0], "railDock": "top" }"#).unwrap();
        assert_eq!(settings.rail_position, Some((500, 0)));
        assert_eq!(settings.rail_dock, Some(Side::Top));
        // An edge this version doesn't know floats it where it was.
        let settings: Settings = serde_json::from_str(r#"{ "railPosition": [500, 0], "railDock": "bottom" }"#).unwrap();
        assert_eq!(settings.rail_position, Some((500, 0)));
        assert_eq!(settings.rail_dock, None);
    }

    #[test]
    fn tucking_away_from_0_1_2_is_not_carried_over() {
        let settings: Settings = serde_json::from_str(r#"{ "hasChosen": true, "autoCollapse": true }"#).unwrap();
        assert!(!settings.tucks_away);
        let settings: Settings = serde_json::from_str(r#"{ "tucksAway": true }"#).unwrap();
        assert!(settings.tucks_away);
    }

    #[test]
    fn an_unknown_ring_choice_is_the_default_not_fatal() {
        let settings: Settings = serde_json::from_str(r#"{ "hasChosen": true, "ringShows": "somethingNew" }"#).unwrap();
        assert!(settings.has_chosen);
        assert_eq!(settings.ring_shows, RingShows::Fullest);
        // A file from before the letters has them on.
        assert!(settings.limit_letters);

        let settings: Settings = serde_json::from_str(r#"{ "ringShows": "bothNested" }"#).unwrap();
        assert_eq!(settings.ring_shows, RingShows::BothNested);
    }

    fn window(id: &str, kind: WindowKind, fraction: f64) -> UsageWindow {
        UsageWindow {
            id: id.into(),
            kind,
            scope: None,
            used_fraction: fraction,
            window_seconds: 0,
            resets_at: None,
            is_exhausted: false,
        }
    }

    fn ids(shows: RingShows, usage: &ProviderUsage) -> Vec<Option<&str>> {
        shows.windows(usage).into_iter().map(|w| w.map(|w| w.id.as_str())).collect()
    }

    #[test]
    fn a_ring_shows_the_limit_chosen() {
        let usage = ProviderUsage::live(
            Provider::ClaudeCode,
            vec![
                window("session", WindowKind::FiveHour, 0.1),
                window("weekly", WindowKind::Weekly, 0.27),
                window("opus", WindowKind::Weekly, 0.4),
            ],
            None,
            None,
        );
        assert_eq!(ids(RingShows::Fullest, &usage), [Some("opus")]);
        assert_eq!(ids(RingShows::FiveHour, &usage), [Some("session")]);
        // The fullest weekly limit, a per-model one included.
        assert_eq!(ids(RingShows::Weekly, &usage), [Some("opus")]);
        assert_eq!(ids(RingShows::BothSplit, &usage), [Some("session"), Some("opus")]);
    }

    #[test]
    fn a_limit_not_reported_falls_back_or_stays_empty() {
        let weekly_only =
            ProviderUsage::live(Provider::Codex, vec![window("weekly", WindowKind::Weekly, 0.3)], None, None);
        assert_eq!(ids(RingShows::FiveHour, &weekly_only), [Some("weekly")]);
        assert_eq!(ids(RingShows::BothStacked, &weekly_only), [None, Some("weekly")]);

        let other_only = ProviderUsage::live(Provider::Codex, vec![window("day", WindowKind::Other, 0.5)], None, None);
        assert_eq!(ids(RingShows::BothNested, &other_only), [Some("day")]);

        let nothing = ProviderUsage::unavailable(Provider::Codex, crate::model::Reason::NotChecked);
        assert_eq!(ids(RingShows::Weekly, &nothing), [None]);
        assert_eq!(ids(RingShows::BothSplit, &nothing), [None, None]);
    }
}
