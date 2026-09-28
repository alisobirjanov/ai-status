//! What the reader chose, persisted as `%APPDATA%\Pulse\settings.json`.

use serde::{Deserialize, Deserializer, Serialize};

use crate::model::Provider;
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
            panel_visible: true,
            rail_position: None,
            login_item_decided: false,
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
}
