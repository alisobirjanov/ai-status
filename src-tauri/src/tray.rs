//! The notification-area icon: the always-there way into Pulse, whether or
//! not the panel is showing.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::i18n::text;
use crate::model::{percent_value, State};
use crate::store::Snapshot;

const TRAY_ID: &str = "main";

pub struct TrayMenu {
    panel_item: CheckMenuItem<Wry>,
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
    app.manage(TrayMenu { panel_item });

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

/// "Claude Code 6% · Codex 89%", so a glance at the tray answers the
/// question even with the panel hidden.
pub fn update_tooltip(app: &AppHandle, snapshot: &Snapshot) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let figures: Vec<String> = snapshot
        .accounts
        .iter()
        .filter(|a| a.enabled)
        .map(|account| {
            let usage = &account.usage;
            let figure = match (usage.headline(), usage.state) {
                (Some(window), State::Live | State::Stale) => {
                    let fraction = if snapshot.settings.shows_remaining { 1.0 - window.used_fraction } else { window.used_fraction };
                    format!("{}%", percent_value(fraction))
                }
                _ => "–".to_string(),
            };
            format!("{} {}", account.name, figure)
        })
        .collect();
    let name = crate::APP_NAME;
    let tooltip = if figures.is_empty() { name.to_string() } else { format!("{name}\n{}", figures.join(" · ")) };
    // The notification area cuts a tooltip at 127 characters.
    let _ = tray.set_tooltip(Some(tooltip.chars().take(127).collect::<String>()));
}
