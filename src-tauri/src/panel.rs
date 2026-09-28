//! The floating panel: a transparent, always-on-top, non-activating window
//! holding the rail and, beside it, room for the card.
//!
//! **The window does not resize while a card opens** (macOS
//! `Docs/ui/panel-geometry.md`): it is sized once for the rail plus the
//! largest card, and everything outside what is actually drawn lets clicks
//! through. Which parts are drawn, the page reports (`set_hit_rects`); where
//! the pointer is, a thread samples — entering and leaving are both decided
//! here, because a window that ignores the cursor hears no leave event.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use crate::store::AppState;

pub const LABEL: &str = "panel";

// Budgets in logical pixels. The page lays the rail out from these (they
// travel in `Layout`), so the window and what is drawn in it cannot drift.
const MARGIN: f64 = 12.0;
const RAIL_WIDTH: f64 = 64.0;
const RAIL_PAD_TOP: f64 = 16.0;
const RAIL_PAD_BOTTOM: f64 = 14.0;
/// Ring (36 + a 4px stroke), a 6px gap, a 16px label.
const ITEM_HEIGHT: f64 = 62.0;
const ITEM_SPACING: f64 = 22.0;
const GAP: f64 = 8.0;
const CARD_WIDTH: f64 = 250.0;
const POINTER_WIDTH: f64 = 20.0;
/// Room for the tallest card: a header and six limits with a footnote.
const CARD_MAX_HEIGHT: f64 = 460.0;
/// From the screen edge, where a first launch puts the rail.
const EDGE_INSET: f64 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    /// The rail on the window's left; the card opens to its right.
    Left,
    /// The rail on the window's right; the card opens to its left.
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// Everything the page needs to place the rail and the card.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub side: Side,
    pub rail: Rect,
    pub pad_top: f64,
    pub item_height: f64,
    pub item_spacing: f64,
    pub gap: f64,
    pub card_width: f64,
    pub pointer_width: f64,
    pub margin: f64,
    /// The part of the window that is on screen, so a card is never placed
    /// where nobody can see it. Window coordinates.
    pub visible: Rect,
}

pub struct PanelState {
    side: Mutex<Side>,
    hit_rects: Mutex<Vec<Rect>>,
    /// Window origin (physical) and scale, cached for the pointer thread so
    /// it does not ask the event loop thirty times a second.
    origin: Mutex<(i32, i32, f64)>,
    shown: AtomicBool,
    move_generation: AtomicU64,
    layout: Mutex<Option<Layout>>,
    /// How many rings the window was last sized for.
    sized_for: Mutex<Option<usize>>,
}

impl PanelState {
    pub fn new() -> PanelState {
        PanelState {
            side: Mutex::new(Side::Right),
            hit_rects: Mutex::new(Vec::new()),
            origin: Mutex::new((0, 0, 1.0)),
            shown: AtomicBool::new(false),
            move_generation: AtomicU64::new(0),
            layout: Mutex::new(None),
            sized_for: Mutex::new(None),
        }
    }
}

fn rail_height(count: usize) -> f64 {
    let n = count.max(1) as f64;
    RAIL_PAD_TOP + RAIL_PAD_BOTTOM + n * ITEM_HEIGHT + (n - 1.0) * ITEM_SPACING
}

fn window_size(count: usize) -> (f64, f64) {
    let width = MARGIN * 2.0 + RAIL_WIDTH + GAP + POINTER_WIDTH + CARD_WIDTH;
    let height = rail_height(count).max(CARD_MAX_HEIGHT) + MARGIN * 2.0;
    (width, height)
}

/// Where the rail sits inside the window, logical pixels.
fn rail_rect(count: usize, side: Side) -> Rect {
    let (width, height) = window_size(count);
    let rail_h = rail_height(count);
    Rect {
        x: match side {
            Side::Left => MARGIN,
            Side::Right => width - MARGIN - RAIL_WIDTH,
        },
        y: ((height - rail_h) / 2.0).round(),
        width: RAIL_WIDTH,
        height: rail_h,
    }
}

/// Show, hide, create or re-lay the panel to match the settings.
pub fn sync(app: &AppHandle) {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let wanted = settings.has_chosen && !settings.enabled.is_empty() && settings.panel_visible;
    let panel = app.state::<PanelState>();

    if !wanted {
        if let Some(window) = app.get_webview_window(LABEL) {
            let _ = window.hide();
        }
        panel.shown.store(false, Ordering::SeqCst);
        crate::tray::set_panel_checked(app, false);
        return;
    }

    let window = match app.get_webview_window(LABEL) {
        Some(window) => window,
        None => match create(app, settings.enabled.len()) {
            Some(window) => window,
            None => return,
        },
    };

    place(app, &window, settings.enabled.len(), settings.rail_position);
    let _ = window.show();
    panel.shown.store(true, Ordering::SeqCst);
    crate::tray::set_panel_checked(app, true);
}

fn create(app: &AppHandle, count: usize) -> Option<WebviewWindow> {
    let (width, height) = window_size(count);
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Pulse")
        .inner_size(width, height)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        // WS_EX_NOACTIVATE: clicking a ring must not take focus from
        // whatever the reader is typing into.
        .focusable(false)
        .focused(false)
        .visible(false)
        .build()
        .ok()?;
    // Click-through until the pointer is over something drawn.
    let _ = window.set_ignore_cursor_events(true);

    let handle = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Moved(position) => {
            let panel = handle.state::<PanelState>();
            {
                let mut origin = panel.origin.lock().unwrap();
                origin.0 = position.x;
                origin.1 = position.y;
            }
            // Settle once the drag stops, not on every step of it.
            let generation = panel.move_generation.fetch_add(1, Ordering::SeqCst) + 1;
            let app = handle.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                if app.state::<PanelState>().move_generation.load(Ordering::SeqCst) == generation {
                    settle(&app);
                }
            });
        }
        WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
            handle.state::<PanelState>().origin.lock().unwrap().2 = *scale_factor;
        }
        _ => {}
    });
    Some(window)
}

/// Size the window for `count` rings and put the rail where it was left, or
/// against the right edge of the main screen the first time.
fn place(app: &AppHandle, window: &WebviewWindow, count: usize, rail_position: Option<(i32, i32)>) {
    let rail_h = rail_height(count);
    let (rail_x, rail_y, scale) = match rail_position.and_then(|(x, y)| {
        let monitor = window.monitor_from_point(x as f64, y as f64).ok().flatten()?;
        Some((x, y, monitor.scale_factor()))
    }) {
        Some(found) => found,
        None => {
            let Some(monitor) = window.primary_monitor().ok().flatten() else { return };
            let scale = monitor.scale_factor();
            let area = monitor.work_area();
            let x = area.position.x + area.size.width as i32 - ((RAIL_WIDTH + EDGE_INSET) * scale) as i32;
            let y = area.position.y + ((area.size.height as f64 - rail_h * scale) / 2.0) as i32;
            (x, y, scale)
        }
    };

    let side = side_for(window, rail_x, rail_y, count, scale);
    position_for_rail(app, window, count, side, (rail_x, rail_y), scale);
}

/// The card opens towards the middle of the screen the rail is on.
fn side_for(window: &WebviewWindow, rail_x: i32, rail_y: i32, count: usize, scale: f64) -> Side {
    let center_x = rail_x as f64 + RAIL_WIDTH * scale / 2.0;
    let center_y = rail_y as f64 + rail_height(count) * scale / 2.0;
    match window.monitor_from_point(center_x, center_y).ok().flatten() {
        Some(monitor) => {
            let middle = monitor.position().x as f64 + monitor.size().width as f64 / 2.0;
            if center_x > middle { Side::Right } else { Side::Left }
        }
        None => Side::Right,
    }
}

/// Move and size the window so the rail lands at `rail` (physical), clamped
/// into the work area of the screen it is on, then tell the page.
fn position_for_rail(app: &AppHandle, window: &WebviewWindow, count: usize, side: Side, rail: (i32, i32), scale: f64) {
    let (width, height) = window_size(count);
    let inside = rail_rect(count, side);

    let (mut rail_x, mut rail_y) = rail;
    let rail_w = (RAIL_WIDTH * scale) as i32;
    let rail_h = (inside.height * scale) as i32;
    if let Some(monitor) = window
        .monitor_from_point(rail_x as f64 + rail_w as f64 / 2.0, rail_y as f64 + rail_h as f64 / 2.0)
        .ok()
        .flatten()
    {
        let area = monitor.work_area();
        let (left, top) = (area.position.x, area.position.y);
        let (right, bottom) = (left + area.size.width as i32, top + area.size.height as i32);
        rail_x = rail_x.clamp(left, (right - rail_w).max(left));
        rail_y = rail_y.clamp(top, (bottom - rail_h).max(top));
    }

    let origin_x = rail_x - (inside.x * scale).round() as i32;
    let origin_y = rail_y - (inside.y * scale).round() as i32;

    let panel = app.state::<PanelState>();
    *panel.side.lock().unwrap() = side;
    // Moved only when it has to: our own move settles like a drag would, and
    // one that changed nothing must not start another round.
    let moved = {
        let mut origin = panel.origin.lock().unwrap();
        let moved = (origin.0, origin.1) != (origin_x, origin_y);
        *origin = (origin_x, origin_y, scale);
        moved
    };
    if moved || !panel.shown.load(Ordering::SeqCst) {
        let _ = window.set_position(PhysicalPosition::new(origin_x, origin_y));
    }
    let resized = {
        let mut sized = panel.sized_for.lock().unwrap();
        let resized = *sized != Some(count);
        *sized = Some(count);
        resized
    };
    if resized {
        let _ = window.set_size(LogicalSize::new(width, height));
    }

    {
        let state = app.state::<AppState>();
        let mut settings = state.settings.lock().unwrap();
        if settings.rail_position != Some((rail_x, rail_y)) {
            settings.rail_position = Some((rail_x, rail_y));
            settings.save();
        }
    }
    publish_layout(app, window, count, side, (origin_x, origin_y), scale);
}

fn publish_layout(app: &AppHandle, window: &WebviewWindow, count: usize, side: Side, origin: (i32, i32), scale: f64) {
    let (width, height) = window_size(count);
    let rail = rail_rect(count, side);

    // The window's on-screen part, in its own logical coordinates.
    let mut visible = Rect { x: 0.0, y: 0.0, width, height };
    let center = (origin.0 as f64 + rail.x * scale, origin.1 as f64 + (rail.y + rail.height / 2.0) * scale);
    if let Some(monitor) = window.monitor_from_point(center.0, center.1).ok().flatten() {
        let area = monitor.work_area();
        let top = ((area.position.y - origin.1) as f64 / scale).max(0.0);
        let bottom = (((area.position.y + area.size.height as i32) - origin.1) as f64 / scale).min(height);
        let left = ((area.position.x - origin.0) as f64 / scale).max(0.0);
        let right = (((area.position.x + area.size.width as i32) - origin.0) as f64 / scale).min(width);
        visible = Rect { x: left, y: top, width: (right - left).max(0.0), height: (bottom - top).max(0.0) };
    }

    let layout = Layout {
        side,
        rail,
        pad_top: RAIL_PAD_TOP,
        item_height: ITEM_HEIGHT,
        item_spacing: ITEM_SPACING,
        gap: GAP,
        card_width: CARD_WIDTH,
        pointer_width: POINTER_WIDTH,
        margin: MARGIN,
        visible,
    };
    let panel = app.state::<PanelState>();
    *panel.layout.lock().unwrap() = Some(layout.clone());
    // Until the page reports what it drew, the rail is what is there.
    *panel.hit_rects.lock().unwrap() = vec![rail];
    let _ = app.emit_to(LABEL, "layout", layout);
}

pub fn current_layout(app: &AppHandle) -> Option<Layout> {
    app.state::<PanelState>().layout.lock().unwrap().clone()
}

/// A drag ended: keep the rail on screen, open the card towards the middle
/// of whichever screen it is now on, and remember where it was left.
fn settle(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else { return };
    let count = app.state::<AppState>().settings.lock().unwrap().enabled.len();
    let Ok(origin) = window.outer_position() else { return };
    let scale = window.scale_factor().unwrap_or(1.0);
    let side = *app.state::<PanelState>().side.lock().unwrap();
    let inside = rail_rect(count, side);

    let rail = (
        origin.x + (inside.x * scale).round() as i32,
        origin.y + (inside.y * scale).round() as i32,
    );
    let new_side = side_for(&window, rail.0, rail.1, count, scale);
    position_for_rail(app, &window, count, new_side, rail, scale);
}

pub fn set_hit_rects(app: &AppHandle, rects: Vec<Rect>) {
    *app.state::<PanelState>().hit_rects.lock().unwrap() = rects;
}

/// Samples the pointer and makes the window take clicks exactly where
/// something is drawn. Also the only source of "the pointer left".
pub fn start_pointer_watch(app: AppHandle) {
    std::thread::spawn(move || {
        let mut inside = false;
        loop {
            std::thread::sleep(Duration::from_millis(33));
            let panel = app.state::<PanelState>();
            if !panel.shown.load(Ordering::SeqCst) {
                if inside {
                    inside = false;
                    let _ = app.emit_to(LABEL, "pointer", false);
                }
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            let Some((cursor_x, cursor_y)) = cursor() else { continue };
            let (origin_x, origin_y, scale) = *panel.origin.lock().unwrap();
            let x = (cursor_x - origin_x) as f64 / scale;
            let y = (cursor_y - origin_y) as f64 / scale;

            let over = panel.hit_rects.lock().unwrap().iter().any(|r| r.contains(x, y));
            // A drag in progress keeps the window: the pointer can outrun it.
            let now_inside = over || (inside && primary_button_down());
            if now_inside != inside {
                inside = now_inside;
                if let Some(window) = app.get_webview_window(LABEL) {
                    let _ = window.set_ignore_cursor_events(!inside);
                }
                let _ = app.emit_to(LABEL, "pointer", inside);
            }
        }
    });
}

#[cfg(windows)]
fn cursor() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut point = POINT { x: 0, y: 0 };
    // SAFETY: GetCursorPos writes one POINT into memory we own.
    (unsafe { GetCursorPos(&mut point) } != 0).then_some((point.x, point.y))
}

#[cfg(windows)]
fn primary_button_down() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    // SAFETY: a plain query with no pointers.
    (unsafe { GetAsyncKeyState(VK_LBUTTON as i32) } as u16 & 0x8000) != 0
}

#[cfg(not(windows))]
fn cursor() -> Option<(i32, i32)> {
    None
}

#[cfg(not(windows))]
fn primary_button_down() -> bool {
    false
}
