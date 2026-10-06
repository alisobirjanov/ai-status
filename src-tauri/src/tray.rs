//! The notification-area icon: the always-there way into Dipstick, whether or
//! not the panel is showing.

use std::sync::Mutex;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::i18n::text;
use crate::model::{percent_value, Provider, State};
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
    /// What the icon was last drawn with.
    drawn: Mutex<Option<Look>>,
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
    // Empty until there are figures to fill it with.
    let look = Look { claude: None, codex: None, ..Look::here() };
    app.manage(TrayMenu {
        menu: menu.clone(),
        panel_item,
        update_item,
        update_separator,
        update_shown: Mutex::new(false),
        drawn: Mutex::new(Some(look)),
    });

    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(crate::APP_NAME)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                crate::show_settings(tray.app_handle());
            }
        });
    let rgba = draw(look);
    builder.icon(Image::new(&rgba, look.size, look.size)).build(app)?;
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
        .filter(|a| a.enabled && a.same_as.is_none())
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
            format!("{} {}", account.title, figure.join(" / "))
        })
        .collect();
    let name = crate::APP_NAME;
    let tooltip = if figures.is_empty() { name.to_string() } else { format!("{name}\n{}", figures.join(" · ")) };
    // The notification area cuts a tooltip at 127 characters.
    let _ = tray.set_tooltip(Some(tooltip.chars().take(127).collect::<String>()));
}

// MARK: - The icon

/// What the icon is drawn from: how full each stick is, in percent, and
/// what it is drawn for. Drawn again only when one of these changes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    claude: Option<i64>,
    codex: Option<i64>,
    /// A light taskbar, rather than a dark one.
    light: bool,
    /// Pixels across, as the notification area shows an icon.
    size: u32,
}

impl Look {
    /// The taskbar's light or dark, and its icon size, as they are now.
    fn here() -> Look {
        Look { claude: None, codex: None, light: crate::settings::taskbar_is_light(), size: icon_size() }
    }

    fn of(snapshot: &Snapshot) -> Look {
        Look { claude: level(snapshot, Provider::ClaudeCode), codex: level(snapshot, Provider::Codex), ..Look::here() }
    }
}

/// The figure the sticks fill to: the limit closest to running out, as the
/// rail counts it (used, or what is left). Claude's is the account Claude
/// Code is signed in to, or else the first switched on. A service switched
/// off, or with no figures, has none, and its stick is empty.
fn level(snapshot: &Snapshot, provider: Provider) -> Option<i64> {
    let mut shown = snapshot.accounts.iter().filter(|a| a.provider == provider && a.enabled && a.same_as.is_none());
    let first = shown.clone().next()?;
    let account = shown.find(|a| a.in_claude_code).unwrap_or(first);
    if !matches!(account.usage.state, State::Live | State::Stale) {
        return None;
    }
    let used = account.usage.headline()?.used_fraction;
    Some(percent_value(if snapshot.settings.shows_remaining { 1.0 - used } else { used }))
}

/// The icon after a change in the figures, or in the taskbar's light or dark.
pub fn update_icon(app: &AppHandle, snapshot: &Snapshot) {
    let Some(tray_menu) = app.try_state::<TrayMenu>() else { return };
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let look = Look::of(snapshot);
    let mut drawn = tray_menu.drawn.lock().unwrap();
    if *drawn == Some(look) {
        return;
    }
    let rgba = draw(look);
    if tray.set_icon(Some(Image::new(&rgba, look.size, look.size))).is_ok() {
        *drawn = Some(look);
    }
}

/// The notification area's icon size: 16 at 100% scaling, 32 at 200%. Drawn
/// at that size rather than scaled down to it, so the sticks stay sharp.
#[cfg(windows)]
fn icon_size() -> u32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
    // SAFETY: a plain Win32 call with no pointers.
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
    if size > 0 {
        (size as u32).clamp(16, 64)
    } else {
        32
    }
}

#[cfg(not(windows))]
fn icon_size() -> u32 {
    32
}

type Rgb = [u8; 3];

/// Where the two sticks are, in pixels: the logo's proportions (each stick
/// 0.3 of the icon wide and 0.9 tall, 0.12 apart), rounded to whole pixels
/// and centred, so their sides stay sharp at 16 pixels.
struct Sticks {
    left: [f64; 2],
    width: f64,
    top: f64,
    height: f64,
}

impl Sticks {
    fn for_size(size: u32) -> Sticks {
        let size = f64::from(size);
        let width = (size * 0.3).round();
        let mut gap = (size * 0.12).round();
        // The same margin either side, and above and below.
        if (size - 2.0 * width - gap) % 2.0 != 0.0 {
            gap += 1.0;
        }
        let mut height = (size * 0.9).round();
        if (size - height) % 2.0 != 0.0 {
            height += 1.0;
        }
        let left = (size - 2.0 * width - gap) / 2.0;
        Sticks { left: [left, left + width + gap], width, top: (size - height) / 2.0, height }
    }

    /// Inside the stick whose left side is at `left`: a rectangle with round
    /// ends.
    fn contain(&self, x: f64, y: f64, left: f64) -> bool {
        let radius = self.width / 2.0;
        if x < left || x > left + self.width || y < self.top || y > self.top + self.height {
            return false;
        }
        let centre_y = y.clamp(self.top + radius, self.top + self.height - radius);
        let (dx, dy) = (x - (left + radius), y - centre_y);
        dx * dx + dy * dy <= radius * radius
    }
}

/// The track the colour fills, then Claude's colour and Codex's: the rail's
/// 5-hour orange and weekly lilac. A dev copy turns them, as its app icon
/// does, so the two copies' icons can be told apart.
fn colours(light: bool) -> (Rgb, Rgb, Rgb) {
    let track = if light { [0xda, 0xd5, 0xce] } else { [0x34, 0x31, 0x2d] };
    if crate::IS_DEV_COPY {
        (track, [0x1a, 0xb1, 0xcc], [0xb6, 0xb7, 0x62])
    } else {
        (track, [0xff, 0x7a, 0x45], [0xb9, 0xa6, 0xff])
    }
}

/// RGBA, row by row from the top. Each pixel is the average of a 4 × 4 grid
/// of points in it, so the round ends are smooth.
fn draw(look: Look) -> Vec<u8> {
    const GRID: u32 = 4;
    let (track, claude, codex) = colours(look.light);
    let sticks = Sticks::for_size(look.size);
    let each = [(sticks.left[0], claude, look.claude), (sticks.left[1], codex, look.codex)];
    let mut rgba = Vec::with_capacity((look.size * look.size * 4) as usize);
    for row in 0..look.size {
        for column in 0..look.size {
            // Each colour weighted by how much of the pixel it covers.
            let (mut sum, mut covered) = ([0.0; 3], 0.0);
            for step_y in 0..GRID {
                for step_x in 0..GRID {
                    let x = f64::from(column) + (f64::from(step_x) + 0.5) / f64::from(GRID);
                    let y = f64::from(row) + (f64::from(step_y) + 0.5) / f64::from(GRID);
                    for &(left, colour, level) in &each {
                        if !sticks.contain(x, y, left) {
                            continue;
                        }
                        let filled = level.unwrap_or(0).clamp(0, 100) as f64 / 100.0;
                        let surface = sticks.top + sticks.height * (1.0 - filled);
                        let colour = if filled > 0.0 && y >= surface { colour } else { track };
                        for (total, channel) in sum.iter_mut().zip(colour) {
                            *total += f64::from(channel);
                        }
                        covered += 1.0;
                    }
                }
            }
            if covered == 0.0 {
                rgba.extend([0, 0, 0, 0]);
            } else {
                rgba.extend(sum.map(|total| (total / covered).round() as u8));
                rgba.push((covered / f64::from(GRID * GRID) * 255.0).round() as u8);
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * size + x) * 4) as usize;
        [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
    }

    #[test]
    fn the_sticks_fill_to_the_figures() {
        let size = 32;
        let look = Look { claude: Some(50), codex: Some(100), light: false, size };
        let rgba = draw(look);
        let (track, claude, codex) = colours(false);
        let opaque = |rgb: Rgb| [rgb[0], rgb[1], rgb[2], 255];
        // Claude's stick, centred at 0.29 across: colour below halfway, track above.
        assert_eq!(pixel(&rgba, size, 9, 26), opaque(claude));
        assert_eq!(pixel(&rgba, size, 9, 6), opaque(track));
        // Codex's, full, centred at 0.71 across.
        assert_eq!(pixel(&rgba, size, 22, 6), opaque(codex));
        assert_eq!(pixel(&rgba, size, 22, 26), opaque(codex));
        // Between the sticks, and in a corner, nothing.
        assert_eq!(pixel(&rgba, size, 15, 16)[3], 0);
        assert_eq!(pixel(&rgba, size, 0, 0)[3], 0);
        // The round ends are smoothed: a pixel there is partly covered.
        let edge = (0..size).map(|y| pixel(&rgba, size, 5, y)[3]).find(|alpha| *alpha > 0 && *alpha < 255);
        assert!(edge.is_some());
    }

    #[test]
    fn the_sticks_sit_on_whole_pixels() {
        // left sides, width, top, height
        for (size, left, width, top, height) in [(16, [2.0, 9.0], 5.0, 1.0, 14.0), (20, [3.0, 11.0], 6.0, 1.0, 18.0), (24, [3.0, 14.0], 7.0, 1.0, 22.0), (32, [4.0, 18.0], 10.0, 1.0, 30.0)] {
            let sticks = Sticks::for_size(size);
            assert_eq!((sticks.left, sticks.width, sticks.top, sticks.height), (left, width, top, height), "at {size}");
            // As far from the right as the left.
            assert_eq!(f64::from(size) - sticks.left[1] - sticks.width, sticks.left[0], "at {size}");
        }
    }

    #[test]
    fn no_figures_is_an_empty_track() {
        let size = 16;
        let rgba = draw(Look { claude: None, codex: Some(0), light: true, size });
        let (track, _, _) = colours(true);
        for (x, y) in [(4, 14), (11, 14), (4, 2), (11, 2)] {
            assert_eq!(&pixel(&rgba, size, x, y)[..3], &track[..], "at {x}, {y}");
        }
    }
}
