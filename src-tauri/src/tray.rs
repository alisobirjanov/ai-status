//! The notification-area icon: the always-there way into Pulse, whether or
//! not the panel is showing.

use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::i18n::text;
use crate::model::{percent_value, State};
use crate::store::Snapshot;

pub const TRAY_ID: &str = "main";

pub struct TrayMenu {
    menu: Menu<Wry>,
    panel_item: CheckMenuItem<Wry>,
    /// At the top of the menu while a new version is on offer, and not there
    /// otherwise: a disabled "no update" line would be a menu item that does
    /// nothing.
    update_item: MenuItem<Wry>,
    update_separator: PredefinedMenuItem<Wry>,
    update_shown: Mutex<bool>,
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let panel_item = CheckMenuItem::with_id(app, "toggle-panel", text("showPanel"), true, false, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &panel_item,
            &MenuItem::with_id(app, "refresh", text("refresh"), true, None::<&str>)?,
            &MenuItem::with_id(app, "settings", text("settings"), true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", text("quit"), true, None::<&str>)?,
        ],
    )?;
    let update_item = MenuItem::with_id(app, "install-update", text("installUpdate"), true, None::<&str>)?;
    let update_separator = PredefinedMenuItem::separator(app)?;
    app.manage(TrayMenu {
        menu: menu.clone(),
        panel_item,
        update_item,
        update_separator,
        update_shown: Mutex::new(false),
    });

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(crate::APP_NAME)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                crate::toggle_panel(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn set_panel_checked(app: &AppHandle, checked: bool) {
    if let Some(menu) = app.try_state::<TrayMenu>() {
        let _ = menu.panel_item.set_checked(checked);
    }
}

/// "Install Update 0.2.0…" at the top of the menu.
pub fn show_update(app: &AppHandle, version: &str) {
    let Some(tray) = app.try_state::<TrayMenu>() else { return };
    let _ = tray.update_item.set_text(text("installUpdate").replace("{0}", version));
    let mut shown = tray.update_shown.lock().unwrap();
    if !*shown {
        let _ = tray.menu.insert(&tray.update_separator, 0);
        let _ = tray.menu.insert(&tray.update_item, 0);
        *shown = true;
    }
}

pub fn hide_update(app: &AppHandle) {
    let Some(tray) = app.try_state::<TrayMenu>() else { return };
    let mut shown = tray.update_shown.lock().unwrap();
    if *shown {
        let _ = tray.menu.remove(&tray.update_item);
        let _ = tray.menu.remove(&tray.update_separator);
        *shown = false;
    }
}

/// "Claude Code 6% · Codex 89%", so a glance at the tray answers the
/// question even with the panel hidden. The figures are the rings': with
/// both limits on them, "Claude Code h 6% / w 31%", lettered as the rings are.
pub fn update_tooltip(app: &AppHandle, snapshot: &Snapshot) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let settings = &snapshot.settings;
    let figures: Vec<String> = snapshot
        .accounts
        .iter()
        .filter(|a| a.enabled)
        .map(|account| {
            let usage = &account.usage;
            let windows = settings.ring_shows.windows(usage);
            let lettered = settings.limit_letters && windows.len() == 2;
            let figure: Vec<String> = windows
                .into_iter()
                .zip(["fiveHourLetter", "weeklyLetter"])
                .map(|(window, letter)| {
                    let figure = match (window, usage.state) {
                        (Some(window), State::Live | State::Stale) => {
                            let fraction = if settings.shows_remaining { 1.0 - window.used_fraction } else { window.used_fraction };
                            format!("{}%", percent_value(fraction))
                        }
                        _ => "–".to_string(),
                    };
                    if lettered { format!("{} {figure}", text(letter)) } else { figure }
                })
                .collect();
            format!("{} {}", account.name, figure.join(" / "))
        })
        .collect();
    let name = crate::APP_NAME;
    let tooltip = if figures.is_empty() { name.to_string() } else { format!("{name}\n{}", figures.join(" · ")) };
    // The notification area cuts a tooltip at 127 characters.
    let _ = tray.set_tooltip(Some(tooltip.chars().take(127).collect::<String>()));
}
