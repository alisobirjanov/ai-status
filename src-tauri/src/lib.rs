//! Pulse for Windows: Claude Code and Codex usage in a floating rail.
//!
//! Rust owns everything that is not drawing — reading credentials, asking
//! the endpoints, the cache, the refresh loop, the window and where the
//! pointer is. The pages only draw what they are sent and say what was
//! clicked.

mod adaptive;
mod cache;
mod i18n;
mod model;
mod panel;
mod paths;
mod providers;
mod report;
mod settings;
mod store;
mod tray;

use std::sync::Mutex;

use serde::Deserialize;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use model::Provider;
use panel::{Layout, PanelState, Rect};
use settings::{RingShows, Settings};
use store::{AppState, Snapshot, Store};

const SETTINGS_LABEL: &str = "settings";

/// A copy built to try changes beside the installed Pulse: any debug build,
/// or a release built with `tauri.dev.conf.json`. It keeps its own settings
/// and readings and is never made a login item, so running it cannot move,
/// reconfigure or replace the Pulse somebody actually uses. The overlay also
/// gives it its own identifier and product name, which is what keeps the
/// single-instance lock, the WebView profile and the login entry apart.
pub(crate) const IS_DEV_COPY: bool = cfg!(any(debug_assertions, feature = "dev-copy"));

/// On the tray and the data folder, so the two copies can be told apart.
pub(crate) const APP_NAME: &str = if IS_DEV_COPY { "Pulse Dev" } else { "Pulse" };

pub(crate) const VERSION: &str =
    if IS_DEV_COPY { concat!(env!("CARGO_PKG_VERSION"), "-dev") } else { env!("CARGO_PKG_VERSION") };

// MARK: - Commands

#[tauri::command]
fn get_snapshot(state: tauri::State<'_, AppState>) -> Snapshot {
    let settings = state.settings.lock().unwrap().clone();
    state.store.lock().unwrap().snapshot(&settings)
}

#[tauri::command]
fn get_layout(app: AppHandle) -> Option<Layout> {
    panel::current_layout(&app)
}

/// Only the fields a page means to change. The rail's position is Rust's to
/// keep, so a page holding an older copy of the settings cannot move it back.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsPatch {
    enabled: Option<Vec<Provider>>,
    codex_source: Option<providers::codex::Source>,
    /// 0 is adaptive.
    refresh_minutes: Option<u32>,
    shows_remaining: Option<bool>,
    warning_at: Option<u32>,
    ring_shows: Option<RingShows>,
    limit_letters: Option<bool>,
    panel_visible: Option<bool>,
}

#[tauri::command]
fn update_settings(app: AppHandle, patch: SettingsPatch) {
    let state = app.state::<AppState>();
    let (old, new) = {
        let mut settings = state.settings.lock().unwrap();
        let old = settings.clone();
        if let Some(enabled) = patch.enabled {
            // Once monitoring has started the rail is never empty: nothing
            // to hover, nothing to grab. Hiding the panel is the way out.
            if !enabled.is_empty() || !settings.has_chosen {
                settings.enabled = enabled;
            }
        }
        if let Some(source) = patch.codex_source {
            settings.codex_source = source;
        }
        if let Some(minutes) = patch.refresh_minutes {
            settings.refresh_minutes = (minutes > 0).then_some(minutes);
        }
        if let Some(remaining) = patch.shows_remaining {
            settings.shows_remaining = remaining;
        }
        if let Some(warning) = patch.warning_at {
            settings.warning_at = warning;
        }
        if let Some(shows) = patch.ring_shows {
            settings.ring_shows = shows;
        }
        if let Some(letters) = patch.limit_letters {
            settings.limit_letters = letters;
        }
        if let Some(visible) = patch.panel_visible {
            settings.panel_visible = visible;
        }
        // Switching a provider on is the initial choice.
        if !settings.enabled.is_empty() {
            settings.has_chosen = true;
        }
        settings.normalize();
        settings.save();
        (old, settings.clone())
    };

    state.store.lock().unwrap().seed(&new);

    let mut ask: Vec<Provider> = new.enabled.iter().copied().filter(|p| !old.is_enabled(*p)).collect();
    if new.codex_source != old.codex_source && new.is_enabled(Provider::Codex) && !ask.contains(&Provider::Codex) {
        ask.push(Provider::Codex);
    }
    // What the rings show sets how tall the rail is.
    if new.enabled != old.enabled
        || new.panel_visible != old.panel_visible
        || new.has_chosen != old.has_chosen
        || new.ring_shows != old.ring_shows
    {
        panel::sync(&app);
    }
    if new.panel_visible && !old.panel_visible {
        // The panel coming back is a reason to look now.
        ask = new.enabled.clone();
    }
    store::emit_snapshot(&app);
    store::refresh(&app, &ask);
    state.wake.notify_one();
}

#[tauri::command]
fn refresh(app: AppHandle, provider: Option<Provider>) {
    match provider {
        Some(provider) => store::refresh(&app, &[provider]),
        None => store::refresh_all(&app),
    }
}

#[tauri::command]
fn set_hit_rects(app: AppHandle, rects: Vec<Rect>) {
    panel::set_hit_rects(&app, rects);
}

#[tauri::command]
fn note_looked(app: AppHandle) {
    store::note_looked(&app);
}

/// The rail's own menu, on a right click — a way into Settings that does not
/// depend on finding the tray icon.
#[tauri::command]
fn panel_menu(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    let menu = Menu::with_items(
        &app,
        &[
            &MenuItem::with_id(&app, "refresh", i18n::text("refresh"), true, None::<&str>).map_err(|e| e.to_string())?,
            &MenuItem::with_id(&app, "settings", i18n::text("settings"), true, None::<&str>).map_err(|e| e.to_string())?,
            &MenuItem::with_id(&app, "hide-panel", i18n::text("hidePanel"), true, None::<&str>).map_err(|e| e.to_string())?,
            &PredefinedMenuItem::separator(&app).map_err(|e| e.to_string())?,
            &MenuItem::with_id(&app, "quit", i18n::text("quit"), true, None::<&str>).map_err(|e| e.to_string())?,
        ],
    )
    .map_err(|e| e.to_string())?;
    window.popup_menu(&menu).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    show_settings(&app);
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> bool {
    let manager = app.autolaunch();
    let _ = if enabled { manager.enable() } else { manager.disable() };
    manager.is_enabled().unwrap_or(false)
}

// MARK: - Shell

fn show_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("settings.html".into()))
        .title(i18n::text("settingsTitle"))
        .inner_size(600.0, 720.0)
        .min_inner_size(480.0, 480.0)
        .center()
        .build();
}

/// The tray's left click. With nothing switched on there is no panel to
/// show, so this is the way to the chooser instead.
pub(crate) fn toggle_panel(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (chosen, visible) = {
        let settings = state.settings.lock().unwrap();
        (settings.has_chosen && !settings.enabled.is_empty(), settings.panel_visible)
    };
    if !chosen {
        show_settings(app);
        return;
    }
    update_settings(app.clone(), SettingsPatch { panel_visible: Some(!visible), ..Default::default() });
}

/// On by default, decided **once**: someone who turned it off is never
/// turned back on by a later launch. Only for an installed build — a
/// development binary registered to start at login would outlive the checkout.
fn decide_login_item(app: &AppHandle) {
    if IS_DEV_COPY {
        return;
    }
    let state = app.state::<AppState>();
    let mut settings = state.settings.lock().unwrap();
    if !settings.login_item_decided {
        let _ = app.autolaunch().enable();
        settings.login_item_decided = true;
        settings.save();
    }
}

pub fn run() {
    let app = tauri::Builder::default()
        // A second launch opens Settings in the first rather than starting
        // another rail.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_settings(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .manage(AppState {
            settings: Mutex::new(Settings::load()),
            store: Mutex::new(Store::new()),
            wake: tokio::sync::Notify::new(),
        })
        .manage(PanelState::new())
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_layout,
            update_settings,
            refresh,
            set_hit_rects,
            note_looked,
            panel_menu,
            open_settings,
            get_autostart,
            set_autostart,
        ])
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle-panel" => toggle_panel(app),
            "hide-panel" => update_settings(app.clone(), SettingsPatch { panel_visible: Some(false), ..Default::default() }),
            "refresh" => store::refresh_all(app),
            "settings" => show_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .setup(|app| {
            let handle = app.handle().clone();
            tray::create(&handle)?;
            decide_login_item(&handle);

            let settings = handle.state::<AppState>().settings.lock().unwrap().clone();
            handle.state::<AppState>().store.lock().unwrap().seed(&settings);

            panel::sync(&handle);
            panel::start_pointer_watch(handle.clone());
            store::start_loop(handle.clone());

            // First launch enables nothing and reads nothing: it asks.
            if !settings.has_chosen {
                show_settings(&handle);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Pulse failed to start");

    app.run(|_app, event| {
        // Closing the last window is not quitting: the tray is still there.
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}

/// `--json`, before any window exists.
pub fn print_json() {
    #[cfg(windows)]
    attach_parent_console();
    println!("{}", report::render());
}

/// A release build is a GUI program and has no console of its own. Run from
/// a terminal, it borrows that one; piped into another program, its output
/// handle is already the pipe and is left alone.
#[cfg(windows)]
fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE};
    // SAFETY: plain Win32 calls with no pointers into our memory.
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}
