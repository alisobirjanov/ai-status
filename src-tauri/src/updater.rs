//! New versions of Pulse itself.
//!
//! Pulse asks its feed (`latest.json` on the repository's latest release,
//! which the release workflow publishes) every few hours. A new version is
//! announced once, with a notification, and stays on offer in the tray menu
//! and in Settings.
//! **It is installed only when somebody asks**: the installer replaces the
//! running app, and nobody should find Pulse gone from under them.
//!
//! The installer is trusted because it is signed by the private half of the
//! key whose public half is in `tauri.conf.json`, not because of where it was
//! downloaded from. A copy built to try changes (Pulse Dev) never updates: the
//! feed's installer is the real Pulse.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::i18n::text;
use crate::model::now_ms;
use crate::store::AppState;
use crate::IS_DEV_COPY;

/// Not at launch: a login item starts before the network is up, and the
/// first minute belongs to the readings.
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(60);
/// How often the automatic check looks at the clock. The wall clock, not a
/// timer, decides whether a check is due, so a PC that slept through the
/// interval checks when it wakes.
const TICK: Duration = Duration::from_secs(30 * 60);
const CHECK_EVERY_MS: i64 = 6 * 3600 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Installing,
    Failed,
    /// Pulse Dev: there is nothing to update it to.
    Unsupported,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub status: Status,
    pub current: &'static str,
    /// The version on offer.
    pub version: Option<String>,
    pub notes: Option<String>,
    /// 0...1 while downloading, when the size is known.
    pub progress: Option<f64>,
    pub checked_at: Option<i64>,
    pub error: Option<String>,
}

pub struct UpdateState {
    info: Mutex<UpdateInfo>,
    pending: Mutex<Option<Update>>,
}

impl UpdateState {
    pub fn new() -> UpdateState {
        UpdateState {
            info: Mutex::new(UpdateInfo {
                status: if IS_DEV_COPY { Status::Unsupported } else { Status::Idle },
                current: crate::VERSION,
                version: None,
                notes: None,
                progress: None,
                checked_at: None,
                error: None,
            }),
            pending: Mutex::new(None),
        }
    }
}

pub fn info(app: &AppHandle) -> UpdateInfo {
    app.state::<UpdateState>().info.lock().unwrap().clone()
}

fn change(app: &AppHandle, edit: impl FnOnce(&mut UpdateInfo)) {
    let snapshot = {
        let state = app.state::<UpdateState>();
        let mut info = state.info.lock().unwrap();
        edit(&mut info);
        info.clone()
    };
    let _ = app.emit("update", snapshot);
}

/// Ask the feed. `manual` is somebody pressing Check Now: only then is a
/// failure worth showing — before the first release there is no feed at all,
/// and every automatic check would otherwise read as broken.
pub async fn check(app: &AppHandle, manual: bool) {
    let busy = matches!(
        info(app).status,
        Status::Unsupported | Status::Checking | Status::Downloading | Status::Installing
    );
    if busy {
        return;
    }
    let before = info(app);
    change(app, |info| {
        info.status = Status::Checking;
        info.error = None;
    });

    let handle = app.clone();
    let result = match app
        .updater_builder()
        // The installer ends this process with `exit(0)`. Taken down first,
        // the tray icon does not linger in the notification area until the
        // pointer happens to pass over it.
        .on_before_exit(move || {
            let _ = handle.remove_tray_by_id(crate::tray::TRAY_ID);
        })
        .build()
    {
        Ok(updater) => updater.check().await,
        Err(error) => Err(error),
    };

    match result {
        Ok(Some(update)) => {
            let version = update.version.clone();
            let notes = update.body.clone().filter(|n| !n.trim().is_empty());
            *app.state::<UpdateState>().pending.lock().unwrap() = Some(update);
            change(app, |info| {
                info.status = Status::Available;
                info.version = Some(version.clone());
                info.notes = notes;
                info.progress = None;
                info.checked_at = Some(now_ms());
            });
            crate::tray::show_update(app, &version);
            announce_once(app, &version);
        }
        Ok(None) => {
            *app.state::<UpdateState>().pending.lock().unwrap() = None;
            change(app, |info| {
                info.status = Status::UpToDate;
                info.version = None;
                info.notes = None;
                info.checked_at = Some(now_ms());
            });
            crate::tray::hide_update(app);
        }
        Err(error) => {
            let message = error.to_string();
            change(app, |info| {
                if manual {
                    info.status = Status::Failed;
                    info.error = Some(message);
                } else {
                    // Whatever was known before still stands: an update
                    // already found is still there to install.
                    info.status = before.status;
                    info.error = before.error;
                }
            });
        }
    }
}

/// One notification per version, however many times it is found again —
/// across launches too.
fn announce_once(app: &AppHandle, version: &str) {
    let state = app.state::<AppState>();
    {
        let mut settings = state.settings.lock().unwrap();
        if settings.update_announced.as_deref() == Some(version) {
            return;
        }
        settings.update_announced = Some(version.to_string());
        settings.save();
    }
    let _ = app
        .notification()
        .builder()
        .title(text("updateTitle").replace("{0}", version))
        .body(text("updateBody"))
        .show();
}

/// Download the version on offer and hand over to its installer, which
/// replaces Pulse and starts it again. On success this never returns: the
/// updater ends the process so the installer can overwrite it.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let Some(update) = app.state::<UpdateState>().pending.lock().unwrap().clone() else {
        return Err("No update to install.".into());
    };
    if matches!(info(app).status, Status::Downloading | Status::Installing) {
        return Ok(());
    }
    change(app, |info| {
        info.status = Status::Downloading;
        info.progress = Some(0.0);
        info.error = None;
    });

    let progress_app = app.clone();
    let installing_app = app.clone();
    let mut downloaded: u64 = 0;
    let mut last_percent: i64 = -1;
    let result = update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                if let Some(total) = total.filter(|t| *t > 0) {
                    let fraction = (downloaded as f64 / total as f64).min(1.0);
                    // A page redraw per whole percent, not per network chunk.
                    let percent = (fraction * 100.0) as i64;
                    if percent != last_percent {
                        last_percent = percent;
                        change(&progress_app, |info| info.progress = Some(fraction));
                    }
                }
            },
            move || {
                change(&installing_app, |info| {
                    info.status = Status::Installing;
                    info.progress = Some(1.0);
                });
            },
        )
        .await;

    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            let message = error.to_string();
            change(app, |info| {
                info.status = Status::Failed;
                info.progress = None;
                info.error = Some(message.clone());
            });
            Err(message)
        }
    }
}

/// The automatic check. Pulse Dev has none.
pub fn start(app: AppHandle) {
    if IS_DEV_COPY {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_AFTER).await;
        // Attempts count, not only answers: a feed that is not there yet
        // must not be asked every half hour.
        let mut attempted_at: Option<i64> = None;
        loop {
            let wanted = app.state::<AppState>().settings.lock().unwrap().checks_for_updates;
            let last = attempted_at.max(info(&app).checked_at);
            let due = last.map_or(true, |at| now_ms() - at >= CHECK_EVERY_MS);
            if wanted && due {
                attempted_at = Some(now_ms());
                check(&app, false).await;
            }
            tokio::time::sleep(TICK).await;
        }
    });
}
