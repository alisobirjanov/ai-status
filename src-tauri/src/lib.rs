//! Dipstick for Windows: Claude Code and Codex usage in a floating rail.
//!
//! Rust owns everything that is not drawing — reading credentials, asking
//! the endpoints, the cache, the refresh loop, the window and where the
//! pointer is. The pages only draw what they are sent and say what was
//! clicked.

mod accounts;
mod adaptive;
mod cache;
mod i18n;
mod model;
mod panel;
mod paths;
mod providers;
mod renamed;
mod report;
mod settings;
mod store;
mod switch;
mod tray;
mod updater;

use std::sync::Mutex;

use serde::Deserialize;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use model::{AccountId, Provider};
use panel::{Layout, PanelState, Rect};
use settings::{RailFollows, RingShows, Settings, Theme};
use store::{AppState, Snapshot, Store};

const SETTINGS_LABEL: &str = "settings";

/// A copy built to try changes beside the installed Dipstick: any debug build,
/// or a release built with `tauri.dev.conf.json`. It keeps its own settings
/// and readings and is never made a login item, so running it cannot move,
/// reconfigure or replace the Dipstick somebody actually uses. The overlay also
/// gives it its own identifier and product name, which is what keeps the
/// single-instance lock, the WebView profile and the login entry apart.
pub(crate) const IS_DEV_COPY: bool = cfg!(any(debug_assertions, feature = "dev-copy"));

/// On the tray and the data folder, so the two copies can be told apart.
pub(crate) const APP_NAME: &str = if IS_DEV_COPY { "Dipstick Dev" } else { "Dipstick" };

/// The name before 0.1.10, when Dipstick was Pulse. What Pulse left under it
/// is taken over once: see `paths::data_dir` and `renamed`.
pub(crate) const OLD_APP_NAME: &str = if IS_DEV_COPY { "Pulse Dev" } else { "Pulse" };

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
    enabled: Option<Vec<AccountId>>,
    rail_follows: Option<RailFollows>,
    says_who_is_free: Option<bool>,
    codex_source: Option<providers::codex::Source>,
    /// 0 is adaptive.
    refresh_minutes: Option<u32>,
    shows_remaining: Option<bool>,
    warning_at: Option<u32>,
    ring_shows: Option<RingShows>,
    limit_letters: Option<bool>,
    panel_visible: Option<bool>,
    tucks_away: Option<bool>,
    shows_card: Option<bool>,
    theme: Option<Theme>,
    rail_stays_dark: Option<bool>,
    glass: Option<bool>,
    glass_transparency: Option<u32>,
    checks_for_updates: Option<bool>,
}

impl SettingsPatch {
    /// Everything Settings offers, as on a first launch — except which
    /// services are on, since turning them off would empty the rail.
    fn defaults() -> SettingsPatch {
        let defaults = Settings::default();
        SettingsPatch {
            enabled: None,
            rail_follows: Some(defaults.rail_follows),
            says_who_is_free: Some(defaults.says_who_is_free),
            codex_source: Some(defaults.codex_source),
            refresh_minutes: Some(defaults.refresh_minutes.unwrap_or(0)),
            shows_remaining: Some(defaults.shows_remaining),
            warning_at: Some(defaults.warning_at),
            ring_shows: Some(defaults.ring_shows),
            limit_letters: Some(defaults.limit_letters),
            panel_visible: Some(defaults.panel_visible),
            tucks_away: Some(defaults.tucks_away),
            shows_card: Some(defaults.shows_card),
            theme: Some(defaults.theme),
            rail_stays_dark: Some(defaults.rail_stays_dark),
            glass: Some(defaults.glass),
            glass_transparency: Some(defaults.glass_transparency),
            checks_for_updates: Some(defaults.checks_for_updates),
        }
    }
}

fn apply(settings: &mut Settings, patch: SettingsPatch) {
    if let Some(enabled) = patch.enabled {
        // Once monitoring has started the rail is never empty: nothing
        // to hover, nothing to grab. Hiding the panel is the way out.
        if !enabled.is_empty() || !settings.has_chosen {
            settings.enabled = enabled;
        }
    }
    if let Some(follows) = patch.rail_follows {
        settings.rail_follows = follows;
    }
    if let Some(says) = patch.says_who_is_free {
        settings.says_who_is_free = says;
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
    if let Some(tucks) = patch.tucks_away {
        settings.tucks_away = tucks;
    }
    if let Some(card) = patch.shows_card {
        settings.shows_card = card;
    }
    if let Some(theme) = patch.theme {
        settings.theme = theme;
    }
    if let Some(dark) = patch.rail_stays_dark {
        settings.rail_stays_dark = dark;
    }
    if let Some(glass) = patch.glass {
        settings.glass = glass;
    }
    if let Some(transparency) = patch.glass_transparency {
        settings.glass_transparency = transparency;
    }
    if let Some(checks) = patch.checks_for_updates {
        settings.checks_for_updates = checks;
    }
    // Switching a provider on is the initial choice.
    if !settings.enabled.is_empty() {
        settings.has_chosen = true;
    }
    settings.normalize();
}

/// Run off the main thread, as is every command that can make a window: a
/// plain command runs inside WebView2's callback for the page's call, and a
/// webview made there never finishes (tauri's "Known issues"). The first
/// choice makes the rail that way, and it hung a first launch: a blank
/// rail, and a Settings window that wouldn't close.
#[tauri::command(async)]
fn update_settings(app: AppHandle, patch: SettingsPatch) {
    change_settings(&app, |settings| apply(settings, patch));
}

/// Change the settings, keep them, and have everything follow: the panel,
/// the pages, and a reading for whatever has just been switched on.
pub(crate) fn change_settings(app: &AppHandle, change: impl FnOnce(&mut Settings)) {
    let state = app.state::<AppState>();
    let (old, new) = {
        let mut settings = state.settings.lock().unwrap();
        let old = settings.clone();
        change(&mut settings);
        settings.normalize();
        settings.save();
        (old, settings.clone())
    };

    state.store.lock().unwrap().seed(&new);

    let codex = Provider::Codex.id().to_string();
    let mut ask: Vec<AccountId> = new.monitored().into_iter().filter(|id| !old.is_enabled(id)).collect();
    if new.codex_source != old.codex_source && new.is_enabled(&codex) && !ask.contains(&codex) {
        ask.push(codex);
    }
    // What the rings show sets how long the rail is — and across the top of
    // the screen, so do the letters beside a pair of figures.
    if new.enabled != old.enabled
        || new.panel_visible != old.panel_visible
        || new.has_chosen != old.has_chosen
        || new.ring_shows != old.ring_shows
        || new.limit_letters != old.limit_letters
    {
        panel::sync(app);
    }
    if new.panel_visible && !old.panel_visible {
        // The panel coming back is a reason to look now.
        ask = new.monitored();
    }
    if new.theme != old.theme {
        if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
            let _ = window.set_theme(new.theme.for_window());
            let _ = window.set_background_color(Some(settings_background(new.theme)));
        }
    }
    store::emit_snapshot(app);
    store::refresh(app, &ask);
    state.wake.notify_one();

    // Switched back on: look now rather than in six hours.
    if new.checks_for_updates && !old.checks_for_updates {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { updater::check(&app, false).await });
    }
}

/// Settings' "Reset all settings". Where the rail sits is not a setting.
/// Off the main thread, as `update_settings` is: it can bring the rail back.
#[tauri::command(async)]
fn reset_settings(app: AppHandle) {
    update_settings(app, SettingsPatch::defaults());
}

#[tauri::command]
fn refresh(app: AppHandle, account: Option<AccountId>) {
    match account {
        Some(account) => store::refresh(&app, &[account]),
        None => store::refresh_all(&app),
    }
}

/// Settings' "Add Claude account", or "Sign in again" on one: Claude Code
/// opens the browser, and the account is there once it has been signed in to.
#[tauri::command]
fn sign_in_claude(app: AppHandle, account: Option<AccountId>) {
    accounts::sign_in(&app, account);
}

#[tauri::command]
fn cancel_sign_in(app: AppHandle) {
    accounts::cancel_sign_in(&app);
}

#[tauri::command]
fn remove_account(app: AppHandle, account: AccountId) {
    accounts::remove(&app, account);
}

#[tauri::command]
fn rename_account(app: AppHandle, account: AccountId, label: String) {
    accounts::rename(&app, account, label);
}

#[tauri::command]
fn use_account(app: AppHandle, account: AccountId) {
    accounts::use_in_claude_code(&app, account);
}

#[tauri::command]
fn dismiss_switch(app: AppHandle) {
    accounts::dismiss_switch(&app);
}

#[tauri::command]
fn set_hit_rects(app: AppHandle, rects: Vec<Rect>) {
    panel::set_hit_rects(&app, rects);
}

/// The panel has drawn a layout and it is on screen (`panel::drawn`).
#[tauri::command]
fn layout_drawn(app: AppHandle, generation: u64) {
    panel::drawn(&app, generation);
}

#[tauri::command]
fn note_looked(app: AppHandle) {
    store::note_looked(&app);
}

/// The rail was pulled past a click: it follows the pointer from here
/// (`panel::carry`).
#[tauri::command]
fn panel_drag(app: AppHandle) {
    panel::carry(&app);
}

/// The rail's own menu, on a right click — a way into Settings that does not
/// depend on finding the tray icon.
///
/// Async, so the menu goes up from the event loop, as Tauri's own menus for
/// a page do. A plain command runs inside WebView2's callback for the page's
/// call, and the menu holds the main thread until it goes: WebView2 doesn't
/// support a wait like that inside its callbacks.
#[tauri::command]
async fn panel_menu(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
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
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = panel::pop_up_menu(&handle, &window, &menu);
    })
    .map_err(|e| e.to_string())
}

/// Off the main thread, as `update_settings` is: it can make the window.
#[tauri::command(async)]
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

/// Where Settings links to. Named rather than given as a URL, so the page
/// cannot have Dipstick open anything else.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Link {
    Issues,
    Source,
    Changelog,
    DataFolder,
}

const REPOSITORY: &str = "https://github.com/alisobirjanov/ai-status";

#[tauri::command]
fn open_link(link: Link) {
    let target = match link {
        Link::Issues => format!("{REPOSITORY}/issues/new"),
        Link::Source => REPOSITORY.to_string(),
        Link::Changelog => format!("{REPOSITORY}/blob/main/CHANGELOG.md"),
        Link::DataFolder => {
            let folder = paths::data_dir();
            let _ = std::fs::create_dir_all(&folder);
            folder.display().to_string()
        }
    };
    shell_open(&target);
}

#[tauri::command]
fn get_update(app: AppHandle) -> updater::UpdateInfo {
    updater::info(&app)
}

#[tauri::command]
async fn check_for_update(app: AppHandle) {
    updater::check(&app, true).await;
}

#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    updater::install(&app).await
}

// MARK: - Shell

pub(crate) fn show_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let theme = app.state::<AppState>().settings.lock().unwrap().theme;
    let built = WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("settings.html".into()))
        .title(i18n::text("settingsTitle"))
        // The title bar in the page's theme, and the window too before the
        // page has painted.
        .theme(theme.for_window())
        .background_color(settings_background(theme))
        .inner_size(1120.0, 880.0)
        .min_inner_size(760.0, 560.0)
        // Smaller on a small screen rather than past its edges.
        .prevent_overflow_with_margin(tauri::LogicalSize::new(32.0, 32.0))
        .center()
        .build();
    // Brought to the front like one already open. Shown new, it is only
    // asked for: Windows can leave it behind the active window, when that
    // belongs to another app.
    if let Ok(window) = built {
        let _ = window.set_focus();
    }
}

/// The page's own background, for the moment before it paints and for the
/// edge a quick resize uncovers.
fn settings_background(theme: Theme) -> tauri::window::Color {
    if theme.resolved() == tauri::Theme::Dark {
        tauri::window::Color(0x12, 0x11, 0x10, 0xff)
    } else {
        tauri::window::Color(0xf7, 0xf5, 0xf2, 0xff)
    }
}

/// A web page in the browser, or a folder in Explorer.
#[cfg(windows)]
fn shell_open(target: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |text: &str| text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (verb, file) = (wide("open"), wide(target));
    // SAFETY: both strings are NUL-terminated and outlive the call; the
    // other pointers may be null.
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

#[cfg(not(windows))]
fn shell_open(_target: &str) {}

/// "Show panel" in the tray's menu. With nothing switched on there is no panel to
/// show, so this is the way to the chooser instead.
fn toggle_panel(app: &AppHandle) {
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState {
            settings: Mutex::new(Settings::load()),
            store: Mutex::new(Store::new()),
            wake: tokio::sync::Notify::new(),
        })
        .manage(PanelState::new())
        .manage(updater::UpdateState::new())
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_layout,
            update_settings,
            refresh,
            sign_in_claude,
            cancel_sign_in,
            remove_account,
            rename_account,
            use_account,
            dismiss_switch,
            set_hit_rects,
            layout_drawn,
            note_looked,
            panel_drag,
            panel_menu,
            open_settings,
            reset_settings,
            open_link,
            get_autostart,
            set_autostart,
            get_update,
            check_for_update,
            install_update,
        ])
        // Windows switched between light and dark. Only a window left to
        // follow it hears of it — the panel always is — and the pages are told.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::ThemeChanged(_) = event {
                store::emit_snapshot(window.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle-panel" => toggle_panel(app),
            "hide-panel" => update_settings(app.clone(), SettingsPatch { panel_visible: Some(false), ..Default::default() }),
            "refresh" => store::refresh_all(app),
            "settings" => show_settings(app),
            "install-update" => {
                // Settings shows the download; the installer takes over after.
                show_settings(app);
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = updater::install(&app).await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .setup(|app| {
            let handle = app.handle().clone();
            tray::create(&handle)?;
            renamed::carry_login_item(&handle);
            decide_login_item(&handle);

            let settings = handle.state::<AppState>().settings.lock().unwrap().clone();
            handle.state::<AppState>().store.lock().unwrap().seed(&settings);

            panel::sync(&handle);
            panel::start_pointer_watch(handle.clone());
            store::start_loop(handle.clone());
            updater::start(handle.clone());

            // First launch enables nothing and reads nothing: it asks.
            if !settings.has_chosen {
                show_settings(&handle);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Dipstick failed to start");

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reset_keeps_the_services_and_where_the_rail_is() {
        let mut settings = Settings {
            enabled: vec!["codex".into(), "claudeCode".into()],
            claude_accounts: vec!["a1".into()],
            account_labels: [("claudeCode#a1".to_string(), "Work".to_string())].into(),
            rail_follows: RailFollows::EachInTurn,
            says_who_is_free: false,
            has_chosen: true,
            codex_source: providers::codex::Source::Tooling,
            refresh_minutes: Some(5),
            shows_remaining: true,
            warning_at: 90,
            ring_shows: RingShows::BothNested,
            limit_letters: false,
            panel_visible: false,
            rail_position: Some((10, 20)),
            rail_dock: Some(panel::Side::Left),
            tucks_away: true,
            shows_card: false,
            theme: Theme::Light,
            rail_stays_dark: true,
            glass: true,
            glass_transparency: 75,
            login_item_decided: true,
            checks_for_updates: false,
            update_announced: Some("0.2.0".into()),
        };
        apply(&mut settings, SettingsPatch::defaults());

        assert_eq!(settings.enabled, vec!["codex", "claudeCode"]);
        assert_eq!(settings.rail_follows, RailFollows::InUse);
        assert!(settings.says_who_is_free);
        // Accounts are not settings: a reset keeps them, and what they are called.
        assert_eq!(settings.claude_accounts, vec!["a1"]);
        assert_eq!(settings.account_labels["claudeCode#a1"], "Work");
        assert!(settings.has_chosen);
        assert_eq!(settings.rail_position, Some((10, 20)));
        assert_eq!(settings.rail_dock, Some(panel::Side::Left));
        assert!(settings.login_item_decided);
        assert_eq!(settings.update_announced.as_deref(), Some("0.2.0"));
        // Everything else is as a first launch has it.
        let rest = Settings {
            enabled: Vec::new(),
            claude_accounts: Vec::new(),
            account_labels: Default::default(),
            has_chosen: false,
            rail_position: None,
            rail_dock: None,
            login_item_decided: false,
            update_announced: None,
            ..settings
        };
        assert_eq!(rest, Settings::default());
    }

    #[test]
    fn the_last_service_stays_on() {
        let mut settings = Settings { enabled: vec!["claudeCode".into()], has_chosen: true, ..Settings::default() };
        apply(&mut settings, SettingsPatch { enabled: Some(Vec::new()), ..Default::default() });
        assert_eq!(settings.enabled, vec!["claudeCode"]);
    }
}
