//! What the reader chose, persisted as `%APPDATA%\Pulse\settings.json`.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

use crate::model::{Provider, ProviderUsage, UsageWindow, WindowKind};
use crate::paths;
use crate::providers::codex;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Switched-on providers, in rail order. First launch enables nothing.
    #[serde(deserialize_with = "known_providers")]
    pub enabled: Vec<Provider>,
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
    /// Launch at login is on by default and decided **once**: a reader who
    /// turned it off is never turned back on by a later launch.
    pub login_item_decided: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: Vec::new(),
            has_chosen: false,
            codex_source: codex::Source::Automatic,
            refresh_minutes: None,
            shows_remaining: false,
            warning_at: 75,
            ring_shows: RingShows::default(),
            limit_letters: true,
            panel_visible: true,
            rail_position: None,
            login_item_decided: false,
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

/// The choices Settings offers for where red begins. A short list, not a
/// slider: this is the one step in the colour language that means "pay
/// attention", and every option sits above the yellow step at 50%.
pub const WARNING_CHOICES: [u32; 6] = [60, 70, 75, 80, 85, 90];

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
        self.enabled.retain(|p| seen.insert(*p));
        if !WARNING_CHOICES.contains(&self.warning_at) {
            self.warning_at = 75;
        }
        if let Some(minutes) = self.refresh_minutes {
            self.refresh_minutes = Some(minutes.clamp(1, 60));
        }
    }

    pub fn is_enabled(&self, provider: Provider) -> bool {
        self.enabled.contains(&provider)
    }
}

/// Unknown names are dropped rather than failing the whole file: a setting
/// written by a newer version must not reset everything else.
fn known_providers<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Provider>, D::Error> {
    let raw: Vec<serde_json::Value> = Vec::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
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

    #[test]
    fn unknown_providers_are_dropped_not_fatal() {
        let settings: Settings =
            serde_json::from_str(r#"{ "enabled": ["codex", "somethingNew", "codex"], "hasChosen": true, "warningAt": 73 }"#)
                .unwrap();
        let mut settings = settings;
        settings.normalize();
        assert_eq!(settings.enabled, vec![Provider::Codex]);
        assert!(settings.has_chosen);
        assert_eq!(settings.warning_at, 75);
        assert!(settings.panel_visible);
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
