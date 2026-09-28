//! How long to wait before asking again. Port of `AdaptiveRefresh.swift`.
//!
//! Every signal here is a reason to wait *longer*; the floor is what you get
//! when something is happening. So a new signal can only ever save requests.

use std::path::Path;
use std::time::SystemTime;

use crate::model::Provider;
use crate::paths;

pub const FLOOR_SECS: i64 = 120;
pub const CEILING_SECS: i64 = 1800;

#[derive(Default)]
pub struct Signals {
    /// When the agent last wrote to its session logs. Unix ms.
    pub last_agent_activity: Option<i64>,
    /// When the reported figures last actually moved.
    pub last_change: Option<i64>,
    /// When the reader last hovered the rail.
    pub last_looked: Option<i64>,
    pub is_panel_visible: bool,
}

pub fn interval_secs(signals: &Signals, now: i64) -> i64 {
    // Nothing on screen is showing these numbers. The panel coming back
    // triggers a refresh of its own.
    if !signals.is_panel_visible {
        return CEILING_SECS;
    }

    // The most recent sign that anyone — reader or agent — cares.
    let quiet = [signals.last_agent_activity, signals.last_change, signals.last_looked]
        .into_iter()
        .flatten()
        .map(|at| (now - at) / 1000)
        .filter(|age| *age >= 0)
        .min();

    match quiet {
        None => CEILING_SECS,
        Some(age) if age < 5 * 60 => FLOOR_SECS,
        Some(age) if age < 60 * 60 => 300,
        Some(age) if age < 4 * 60 * 60 => 900,
        Some(_) => CEILING_SECS,
    }
}

/// When the agent last wrote a session log. Metadata only — nothing is read.
///
/// Claude Code keeps one JSONL per session under `projects\<project>\`;
/// Codex files them by day under `sessions\YYYY\MM\DD\`, so only today's and
/// yesterday's folders are looked at.
pub fn last_agent_activity(provider: Provider) -> Option<i64> {
    match provider {
        Provider::ClaudeCode => {
            let projects = paths::claude_dir().join("projects");
            std::fs::read_dir(projects)
                .ok()?
                .flatten()
                .filter_map(|entry| newest_in(&entry.path()))
                .max()
        }
        Provider::Codex => {
            let sessions = paths::codex_dir().join("sessions");
            let today = chrono::Local::now().date_naive();
            [today, today - chrono::Days::new(1)]
                .into_iter()
                .filter_map(|day| newest_in(&sessions.join(day.format("%Y").to_string()).join(day.format("%m").to_string()).join(day.format("%d").to_string())))
                .max()
        }
    }
}

fn newest_in(folder: &Path) -> Option<i64> {
    std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "jsonl"))
        .filter_map(|entry| entry.metadata().ok()?.modified().ok())
        .max()
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ladder() {
        let now = 100_000_000;
        let at = |secs_ago: i64| Some(now - secs_ago * 1000);
        let visible = |activity| Signals { last_agent_activity: activity, is_panel_visible: true, ..Default::default() };

        assert_eq!(interval_secs(&visible(None), now), CEILING_SECS);
        assert_eq!(interval_secs(&visible(at(30)), now), FLOOR_SECS);
        assert_eq!(interval_secs(&visible(at(10 * 60)), now), 300);
        assert_eq!(interval_secs(&visible(at(2 * 3600)), now), 900);
        assert_eq!(interval_secs(&visible(at(5 * 3600)), now), CEILING_SECS);

        let hidden = Signals { last_agent_activity: at(30), is_panel_visible: false, ..Default::default() };
        assert_eq!(interval_secs(&hidden, now), CEILING_SECS);
    }
}
