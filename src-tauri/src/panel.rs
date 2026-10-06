//! The floating panel: a transparent, always-on-top, non-activating window
//! holding the rail and, beside it, room for the card.
//!
//! **The window does not resize while a card opens** (macOS
//! `Docs/ui/panel-geometry.md`): it is sized once for the rail plus the
//! largest card, and everything outside what is actually drawn lets clicks
//! through. Which parts are drawn, the page reports (`set_hit_rects`); where
//! the pointer is, a thread samples — entering and leaving are both decided
//! here, because a window that ignores the cursor hears no leave event.
//!
//! **Carrying.** Pulled past a click, the rail follows the pointer, moved
//! here rather than by Windows (`carry`), and comes to rest where the
//! pointer lets go of it, kept on its screen.
//!
//! **Docking.** Carried within `DOCK_DISTANCE` of the left or right side of
//! its screen, the rail fuses to it: flush against the edge, its ends
//! sweeping into the edge above and below it (macOS `DockBerthShape`).
//! Carried with the pointer that close to the top of a screen, it turns on
//! its side and fuses there instead, its figures beside its rings and its
//! card opening below it. It fuses on the way, not once it is let go of: it
//! glides onto the edge, slides along it with the pointer, and holds on to
//! it until pulled `UNDOCK_DISTANCE` off — a magnet, so the edge need not be
//! hit exactly, and a rail parked near one on purpose stays put. With "Tuck
//! away at the edge" on, the page winds a docked rail down to a sliver while
//! the pointer is elsewhere and opens it the moment the pointer reaches the
//! sliver; otherwise, and always off the edge, it stays open.
//!
//! **Redrawing.** The rail's middle is at the same place in the window
//! whichever way it lies (`anchor`), and the window is sized for the sweep
//! whether or not the rail is docked. So docking never moves a ring, and a
//! turn to or from the top changes the window only at its bottom, which
//! leaves everything in it where it was: the page turns the rail where it
//! is, animated, and the window need not move for it. A window that moves
//! while the page redraws the rail somewhere else in it — when the rail
//! itself changes size, say — would show it where the move takes that spot
//! until the page caught up. So for that the window is out of sight from
//! just before it moves until the page has drawn the rail where it is now
//! (`hide_until_drawn`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::menu::ContextMenu;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use crate::settings::{RingShows, Settings};
use crate::store::AppState;

pub const LABEL: &str = "panel";

// Budgets in logical pixels. The page lays the rail out from these (they
// travel in `Layout`), so the window and what is drawn in it cannot drift.
const MARGIN: f64 = 12.0;
const RAIL_WIDTH: f64 = 64.0;
const RAIL_PAD_TOP: f64 = 16.0;
const RAIL_PAD_BOTTOM: f64 = 14.0;
/// A ring: 36 and a 4px stroke.
const RING: f64 = 40.0;
/// Ring, a 6px gap, a 16px label.
const ITEM_HEIGHT: f64 = 62.0;
const LABEL_HEIGHT: f64 = 16.0;
/// Between a ring and its figure, under it or beside it.
const LABEL_GAP: f64 = 6.0;
/// Across the top, each figure sits beside its ring, in this much room: the
/// widest, "100%", is 32.6 in Segoe UI Variable, and a fallback font may
/// set it a little wider.
const LABEL_WIDTH: f64 = 36.0;
/// The same with the letter saying which limit it is: "w 100%" is 43.9.
const LETTERED_LABEL_WIDTH: f64 = 46.0;
/// Between a service's two rings, when both limits are stacked.
const PAIR_GAP: f64 = 8.0;
const ITEM_SPACING: f64 = 22.0;
const GAP: f64 = 8.0;
const CARD_WIDTH: f64 = 250.0;
const POINTER_WIDTH: f64 = 20.0;
/// Room for the tallest card: a header and six limits with a footnote.
const CARD_MAX_HEIGHT: f64 = 460.0;
/// How far a docked rail's ends sweep into the screen edge beyond its body,
/// along the edge. The rings do not move for it: it is outside the body.
const FLARE_HEIGHT: f64 = 24.0;
/// How close to an edge of the screen a carried rail has to come to fuse to
/// it — or, for the top, the pointer carrying it. Easy to hit on purpose,
/// tight enough that parking the panel *near* an edge on purpose still works
/// (macOS `PanelPlacement.dockDistance`).
const DOCK_DISTANCE: f64 = 32.0;
/// How far from the edge a rail fused to it has to be pulled, measured the
/// same way, before it lets go of it.
const UNDOCK_DISTANCE: f64 = 96.0;
/// How often a carried rail catches up with the pointer: faster than any
/// screen redraws.
const CARRY_STEP: Duration = Duration::from_millis(4);
/// Where a carried rail's place jumps — onto an edge, off one, turning — it
/// glides there instead, what is left of the way shrinking to about a third
/// in this long.
const GLIDE: Duration = Duration::from_millis(50);
/// Let go of, it has glided the rest of the way within this.
const GLIDE_AT_MOST: Duration = Duration::from_millis(400);
/// How long the window stays out of sight at most, should the page never say
/// it has drawn the layout it was hidden for. It takes some 10-20 ms.
const HIDDEN_AT_MOST: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    /// The rail down the window's left; the card opens to its right.
    Left,
    /// The rail down the window's right; the card opens to its left.
    Right,
    /// The rail across the window's top, which only a rail docked at the top
    /// of the screen is; the card opens below it.
    Top,
}

impl Side {
    /// Whether a rail on this side lies across the window rather than down it.
    fn is_across(self) -> bool {
        self == Side::Top
    }
}

/// How the rail lies, and what it is fused to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pose {
    /// Which way its card opens, and so which way it lies.
    side: Side,
    /// The screen edge it is fused to, which is also `side`; `None` floating.
    dock: Option<Side>,
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
    /// Before the first item, from the rail's top down a side and from its
    /// left end across the top.
    pub pad_start: f64,
    /// One service's extent along the rail: its height down a side, its
    /// width across the top.
    pub item_length: f64,
    pub item_spacing: f64,
    pub gap: f64,
    pub card_width: f64,
    pub pointer_width: f64,
    pub margin: f64,
    /// The part of the window that is on screen, so a card is never placed
    /// where nobody can see it. Window coordinates.
    pub visible: Rect,
    /// The screen edge the rail is fused to, which is also the window side it
    /// is on; `None` floating.
    pub dock: Option<Side>,
    /// Laid out while it is carried, on its way somewhere: the page animates
    /// to it, and the drag goes on.
    pub carried: bool,
    /// How far a docked rail's ends reach beyond `rail`, along the edge:
    /// above and below it down a side, left and right of it across the top.
    pub flare: f64,
    /// The window's size, logical: the page's viewport once it has caught up.
    pub width: f64,
    pub height: f64,
    /// Which layout this is, for the page to say when it has drawn it.
    pub generation: u64,
}

pub struct PanelState {
    side: Mutex<Side>,
    hit_rects: Mutex<Vec<Rect>>,
    /// Window origin (physical) and scale, cached for the pointer thread so
    /// it does not ask the event loop thirty times a second.
    origin: Mutex<(i32, i32, f64)>,
    shown: AtomicBool,
    /// The rail's own menu is up: the pointer is on it, and the rail is not
    /// to be left because of that.
    menu_open: AtomicBool,
    move_generation: AtomicU64,
    layout: Mutex<Option<Layout>>,
    /// The logical size the window was last given.
    sized_to: Mutex<Option<(f64, f64)>>,
    /// The rail is following the pointer (`carry`): it lands when the button
    /// comes up, not once the window stops moving.
    carrying: AtomicBool,
    /// Counts the layouts sent to the page.
    layout_generation: AtomicU64,
    /// The layout the window is out of sight until the page has drawn
    /// (`hide_until_drawn`); 0 in sight.
    hidden_until: Mutex<u64>,
}

impl PanelState {
    pub fn new() -> PanelState {
        PanelState {
            side: Mutex::new(Side::Right),
            hit_rects: Mutex::new(Vec::new()),
            origin: Mutex::new((0, 0, 1.0)),
            shown: AtomicBool::new(false),
            menu_open: AtomicBool::new(false),
            move_generation: AtomicU64::new(0),
            layout: Mutex::new(None),
            sized_to: Mutex::new(None),
            carrying: AtomicBool::new(false),
            layout_generation: AtomicU64::new(0),
            hidden_until: Mutex::new(0),
        }
    }
}

/// What the rail holds: how many services, and how much of the rail each
/// one's item takes down a side of the screen and across its top.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Items {
    count: usize,
    height: f64,
    width: f64,
}

impl Items {
    fn of(settings: &Settings) -> Items {
        // Across the top every figure is beside its ring, and a pair of them
        // one over the other there, wide enough for the letters if they are on.
        let pair = if settings.limit_letters { LETTERED_LABEL_WIDTH } else { LABEL_WIDTH };
        let beside = |label: f64| RING + LABEL_GAP + label;
        let (height, width) = match settings.ring_shows {
            RingShows::Fullest | RingShows::FiveHour | RingShows::Weekly => (ITEM_HEIGHT, beside(LABEL_WIDTH)),
            // The 5-hour figure above the ring, the weekly one below it.
            RingShows::BothSplit => (ITEM_HEIGHT + LABEL_GAP + LABEL_HEIGHT, beside(pair)),
            // Both figures under the ring, one on top of the other.
            RingShows::BothNested => (ITEM_HEIGHT + LABEL_HEIGHT, beside(pair)),
            RingShows::BothStacked => (2.0 * ITEM_HEIGHT + PAIR_GAP, 2.0 * beside(pair) + PAIR_GAP),
        };
        Items { count: settings.enabled.len(), height, width }
    }

    /// One item's extent along a rail on `side`.
    fn length(self, side: Side) -> f64 {
        if side.is_across() { self.width } else { self.height }
    }
}

/// The rail from end to end, whichever way it lies.
fn rail_length(items: Items, side: Side) -> f64 {
    let n = items.count.max(1) as f64;
    RAIL_PAD_TOP + RAIL_PAD_BOTTOM + n * items.length(side) + (n - 1.0) * ITEM_SPACING
}

/// Before the first item on a rail on `side`: down a side as much as ever,
/// and across the top as much as after the last one, so the services sit in
/// the middle of the rail — and a card centred under one lines up with it.
fn pad_start(side: Side) -> f64 {
    if side.is_across() { (RAIL_PAD_TOP + RAIL_PAD_BOTTOM) / 2.0 } else { RAIL_PAD_TOP }
}

/// The rail's width and height, logical pixels.
fn rail_size(items: Items, side: Side) -> (f64, f64) {
    let length = rail_length(items, side);
    if side.is_across() { (length, RAIL_WIDTH) } else { (RAIL_WIDTH, length) }
}

fn window_size(items: Items, side: Side) -> (f64, f64) {
    // Down a side, room for the card on both sides of the rail, though it
    // opens on one. A rail dropped on the other half of a screen then turns
    // its card round without its window moving: moved, the window would show
    // the rail where the page last drew it — a card's width off, until the
    // page caught up.
    let width = MARGIN * 2.0 + RAIL_WIDTH + 2.0 * (GAP + POINTER_WIDTH + CARD_WIDTH);
    let height = (rail_length(items, Side::Right) + FLARE_HEIGHT * 2.0).max(CARD_MAX_HEIGHT) + MARGIN * 2.0;
    if !side.is_across() {
        return (width, height);
    }
    // Across the top, the rail's middle stays where it is down a side, and
    // the window reaches from there down past the card opening below it:
    // turning, the window changes only at the bottom. Above the rail it is
    // off the screen. As wide, a card centred under a ring has room, and one
    // pushed back on screen at a side of it too.
    (width, height / 2.0 + RAIL_WIDTH / 2.0 + GAP + POINTER_WIDTH + CARD_MAX_HEIGHT + MARGIN)
}

/// Where the rail's middle is in the window, logical pixels: the same
/// whichever way it lies, so it turns there without the window moving.
fn anchor(items: Items) -> (f64, f64) {
    let (width, height) = window_size(items, Side::Right);
    (width / 2.0, height / 2.0)
}

/// Where the rail sits inside the window, logical pixels: its middle on the
/// anchor.
fn rail_rect(items: Items, side: Side) -> Rect {
    let (x, y) = anchor(items);
    let (width, height) = rail_size(items, side);
    Rect { x: (x - width / 2.0).round(), y: (y - height / 2.0).round(), width, height }
}

/// The rail's size on a screen of `scale`, physical.
fn physical_size(rail: Rect, scale: f64) -> (i32, i32) {
    ((rail.width * scale) as i32, (rail.height * scale) as i32)
}

/// Where the window goes for the rail at `inside` in it to be at `rail`,
/// physical.
fn origin_for(rail: (i32, i32), inside: Rect, scale: f64) -> (i32, i32) {
    (rail.0 - (inside.x * scale).round() as i32, rail.1 - (inside.y * scale).round() as i32)
}

/// Where the rail at `inside` is with the window at `origin`, physical.
fn rail_at(origin: (i32, i32), inside: Rect, scale: f64) -> (i32, i32) {
    (origin.0 + (inside.x * scale).round() as i32, origin.1 + (inside.y * scale).round() as i32)
}

/// Show, hide, create or re-lay the panel to match the settings.
pub fn sync(app: &AppHandle) {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let wanted = settings.has_chosen && !settings.enabled.is_empty() && settings.panel_visible;
    let items = Items::of(&settings);
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
        None => match create(app, items) {
            Some(window) => window,
            None => return,
        },
    };

    place(app, &window, items, &settings);
    let _ = window.show();
    panel.shown.store(true, Ordering::SeqCst);
    crate::tray::set_panel_checked(app, true);
}

/// Sized down a side for now; `place` gives it the size for where it goes.
fn create(app: &AppHandle, items: Items) -> Option<WebviewWindow> {
    let (width, height) = window_size(items, Side::Right);
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
    #[cfg(windows)]
    keep_frameless(&window);

    let handle = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Moved(position) => {
            let panel = handle.state::<PanelState>();
            {
                let mut origin = panel.origin.lock().unwrap();
                origin.0 = position.x;
                origin.1 = position.y;
            }
            // Moved by us, or by Windows, it settles once the window stops
            // moving, not on every step. Carried, it lands where the pointer
            // lets go of it instead (`carry`).
            let generation = panel.move_generation.fetch_add(1, Ordering::SeqCst) + 1;
            if !panel.carrying.load(Ordering::SeqCst) {
                let app = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    let panel = app.state::<PanelState>();
                    if panel.move_generation.load(Ordering::SeqCst) == generation && !panel.carrying.load(Ordering::SeqCst) {
                        settle(&app);
                    }
                });
            }
        }
        WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
            handle.state::<PanelState>().origin.lock().unwrap().2 = *scale_factor;
        }
        _ => {}
    });
    Some(window)
}

/// Size the window for `items` and put the rail where it was left, fused to
/// the edge it was left against — or docked on the right of the main screen
/// the first time.
fn place(app: &AppHandle, window: &WebviewWindow, items: Items, settings: &Settings) {
    let (rail_x, rail_y, scale, dock) = match settings.rail_position.and_then(|(x, y)| {
        let monitor = window.monitor_from_point(x as f64, y as f64).ok().flatten()?;
        Some((x, y, monitor.scale_factor()))
    }) {
        Some((x, y, scale)) => (x, y, scale, settings.rail_dock),
        None => {
            let Some(monitor) = window.primary_monitor().ok().flatten() else { return };
            let scale = monitor.scale_factor();
            let area = monitor.work_area();
            let x = area.position.x + area.size.width as i32 - (RAIL_WIDTH * scale) as i32;
            let y = area.position.y + ((area.size.height as f64 - rail_length(items, Side::Right) * scale) / 2.0) as i32;
            (x, y, scale, Some(Side::Right))
        }
    };

    let side = dock.unwrap_or_else(|| side_for(window, (rail_x, rail_y), items, scale));
    position_for_rail(app, window, items, Pose { side, dock }, (rail_x, rail_y), scale);
}

/// A floating rail's card opens towards the middle of the screen it is on.
/// Floating, a rail stands down the screen: only the top lays one across it.
fn side_for(window: &WebviewWindow, rail: (i32, i32), items: Items, scale: f64) -> Side {
    let (width, height) = rail_size(items, Side::Right);
    let center_x = rail.0 as f64 + width * scale / 2.0;
    let center_y = rail.1 as f64 + height * scale / 2.0;
    match window.monitor_from_point(center_x, center_y).ok().flatten() {
        Some(monitor) => {
            let middle = monitor.position().x as f64 + monitor.size().width as f64 / 2.0;
            if center_x > middle { Side::Right } else { Side::Left }
        }
        None => Side::Right,
    }
}

/// The screen the point `at` (physical) is on — or, off every screen, the
/// one most of the window is on.
fn monitor_at(window: &WebviewWindow, at: (i32, i32)) -> Option<tauri::Monitor> {
    window
        .monitor_from_point(at.0 as f64, at.1 as f64)
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten())
}

/// The middle of a rail whose top-left is `rail` and whose logical size is
/// `size`, physical.
fn rail_center(rail: (i32, i32), size: (f64, f64), scale: f64) -> (i32, i32) {
    (rail.0 + (size.0 * scale / 2.0) as i32, rail.1 + (size.1 * scale / 2.0) as i32)
}

/// Move and size the window so the rail comes to rest at `rail` (physical)
/// on the screen it is on (`rest`), then tell the page.
fn position_for_rail(app: &AppHandle, window: &WebviewWindow, items: Items, pose: Pose, rail: (i32, i32), scale: f64) {
    let (width, height) = window_size(items, pose.side);
    let inside = rail_rect(items, pose.side);
    let rail = match monitor_at(window, rail_center(rail, (inside.width, inside.height), scale)) {
        Some(monitor) => rest(rail, physical_size(inside, scale), Screen::of(&monitor).area, pose, (FLARE_HEIGHT * scale).round() as i32),
        None => rail,
    };
    let (origin_x, origin_y) = origin_for(rail, inside, scale);

    let panel = app.state::<PanelState>();
    *panel.side.lock().unwrap() = pose.side;
    // Moved only when it has to: our own move settles like a drag would, and
    // one that changed nothing must not start another round.
    let moved = {
        let mut origin = panel.origin.lock().unwrap();
        let moved = (origin.0, origin.1) != (origin_x, origin_y);
        *origin = (origin_x, origin_y, scale);
        moved
    };
    let resized = {
        let mut sized = panel.sized_to.lock().unwrap();
        let resized = *sized != Some((width, height));
        *sized = Some((width, height));
        resized
    };
    let generation = panel.layout_generation.fetch_add(1, Ordering::SeqCst) + 1;
    let shown = panel.shown.load(Ordering::SeqCst);
    // Out of sight before the window moves, if the page has the rail
    // somewhere else in it. Only resized, it has not moved: the window's
    // bottom has, which leaves the rail where it was.
    if shown && moved && panel.layout.lock().unwrap().as_ref().is_some_and(|drawn| drawn.rail != inside) {
        hide_until_drawn(app, window, generation);
    }
    if moved || !shown {
        let _ = window.set_position(PhysicalPosition::new(origin_x, origin_y));
    }
    if resized {
        let _ = window.set_size(LogicalSize::new(width, height));
    }
    // The page first: a window out of sight waits for it, not for the disk.
    publish_layout(window, items, pose, false, (origin_x, origin_y), scale, generation);

    let state = app.state::<AppState>();
    let mut settings = state.settings.lock().unwrap();
    if settings.rail_position != Some(rail) || settings.rail_dock != pose.dock {
        settings.rail_position = Some(rail);
        settings.rail_dock = pose.dock;
        settings.save();
    }
}

/// Where a rail of `size` whose top-left is `rail` comes to rest in a work
/// area spanning `area` (left, top, right, bottom), lying and fused as
/// `pose` has it: all of it on the screen — fused, the sweep of its ends
/// (`flare`) too — and flush against the edge it is fused to. Physical.
fn rest(rail: (i32, i32), size: (i32, i32), area: (i32, i32, i32, i32), pose: Pose, flare: i32) -> (i32, i32) {
    let (left, top, right, bottom) = area;
    let (width, height) = size;
    let flare = if pose.dock.is_some() { flare } else { 0 };
    let (flare_x, flare_y) = if pose.side.is_across() { (flare, 0) } else { (0, flare) };
    let x = rail.0.clamp(left + flare_x, (right - width - flare_x).max(left + flare_x));
    let y = rail.1.clamp(top + flare_y, (bottom - height - flare_y).max(top + flare_y));
    match pose.dock {
        Some(Side::Left) => (left, y),
        Some(Side::Right) => ((right - width).max(left), y),
        Some(Side::Top) => (x, top),
        None => (x, y),
    }
}

fn publish_layout(
    window: &WebviewWindow,
    items: Items,
    pose: Pose,
    carried: bool,
    origin: (i32, i32),
    scale: f64,
    generation: u64,
) {
    let app = window.app_handle();
    let Pose { side, dock } = pose;
    let (width, height) = window_size(items, side);
    let rail = rail_rect(items, side);

    // The window's on-screen part, in its own logical coordinates.
    let mut visible = Rect { x: 0.0, y: 0.0, width, height };
    let center = (origin.0 as f64 + (rail.x + rail.width / 2.0) * scale, origin.1 as f64 + (rail.y + rail.height / 2.0) * scale);
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
        pad_start: pad_start(side),
        item_length: items.length(side),
        item_spacing: ITEM_SPACING,
        gap: GAP,
        card_width: CARD_WIDTH,
        pointer_width: POINTER_WIDTH,
        margin: MARGIN,
        visible,
        dock,
        carried,
        flare: FLARE_HEIGHT,
        width,
        height,
        generation,
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

/// Takes the window out of sight until the page has drawn layout
/// `generation` (`drawn`), or for `HIDDEN_AT_MOST` should it never say so.
fn hide_until_drawn(app: &AppHandle, window: &WebviewWindow, generation: u64) {
    {
        let panel = app.state::<PanelState>();
        let mut hidden = panel.hidden_until.lock().unwrap();
        *hidden = generation;
        cloak(window, true);
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(HIDDEN_AT_MOST).await;
        drawn(&app, generation);
    });
}

/// The page has drawn layout `generation`, and it is on screen: the window
/// is in sight again, if that is what it was hidden for.
pub fn drawn(app: &AppHandle, generation: u64) {
    let panel = app.state::<PanelState>();
    let mut hidden = panel.hidden_until.lock().unwrap();
    if *hidden != generation {
        return;
    }
    *hidden = 0;
    if let Some(window) = app.get_webview_window(LABEL) {
        cloak(&window, false);
    }
}

/// The rail was pulled past a click: it follows the pointer until the button
/// comes up, fusing to an edge it comes near on the way (`fuse`), and comes
/// to rest where it was headed. Carried here rather than dragged by Windows,
/// which stops a window it drags at the top of the screen — and so the rail,
/// in the middle of its window, well short of it.
pub fn carry(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else { return };
    let Some(start) = cursor() else { return };
    let panel = app.state::<PanelState>();
    if panel.carrying.swap(true, Ordering::SeqCst) {
        return;
    }
    let (x, y, scale) = *panel.origin.lock().unwrap();
    let laid = panel.layout.lock().unwrap().as_ref().map(|layout| Pose { side: layout.side, dock: layout.dock });
    let pose = laid.unwrap_or(Pose { side: *panel.side.lock().unwrap(), dock: None });
    let items = Items::of(&app.state::<AppState>().settings.lock().unwrap());
    let app = app.clone();
    std::thread::spawn(move || {
        let screens = window.available_monitors().unwrap_or_default().iter().map(Screen::of).collect();
        let mut carried = Carried {
            app: app.clone(),
            window,
            items,
            screens,
            hold: ((start.0 - x) as f64 / scale, (start.1 - y) as f64 / scale),
            pose,
            scale,
            screen: None,
            headed: (x, y),
            at: (x as f64, y as f64),
            shown: (x, y),
        };
        let mut pointer = start;
        let mut last = Instant::now();
        while primary_button_down() {
            std::thread::sleep(CARRY_STEP);
            pointer = cursor().unwrap_or(pointer);
            let now = Instant::now();
            carried.step(pointer, (now - last).as_secs_f64());
            last = now;
        }
        carried.let_go();
        app.state::<PanelState>().carrying.store(false, Ordering::SeqCst);
    });
}

/// A rail on its way somewhere (`carry`).
struct Carried {
    app: AppHandle,
    window: WebviewWindow,
    items: Items,
    screens: Vec<Screen>,
    /// Where on the window it is held, logical: carried onto a screen of
    /// another scale, it is still held there.
    hold: (f64, f64),
    pose: Pose,
    scale: f64,
    /// The screen the pointer was on a step ago.
    screen: Option<Screen>,
    /// Where the window is headed, and where it has got to on the way:
    /// physical.
    headed: (i32, i32),
    at: (f64, f64),
    /// Where it was last put, to the pixel.
    shown: (i32, i32),
}

impl Carried {
    /// A step, `dt` seconds after the last, with the pointer at `pointer`:
    /// the rail goes where the pointer holds it, or along the edge it has
    /// fused to. Where that jumps, it glides there instead.
    fn step(&mut self, pointer: (i32, i32), dt: f64) {
        let Some(screen) = screen_at(&self.screens, pointer) else { return };
        let scale = self.app.state::<PanelState>().origin.lock().unwrap().2;
        let pose = self.pose_at(pointer, screen, scale);
        if pose.side.is_across() != self.pose.side.is_across() {
            // Turned, it is held by its middle: where it was held was on a
            // rail lying the other way.
            self.hold = anchor(self.items);
        }
        let headed = self.headed_for(pose, pointer, screen, scale);
        let behind = if scale != self.scale {
            // Windows has just drawn it at another scale, in another place:
            // it goes on from where it is headed.
            (0.0, 0.0)
        } else if pose != self.pose || Some(screen) != self.screen {
            (self.at.0 - headed.0 as f64, self.at.1 - headed.1 as f64)
        } else {
            let keep = (-dt / GLIDE.as_secs_f64()).exp();
            ((self.at.0 - self.headed.0 as f64) * keep, (self.at.1 - self.headed.1 as f64) * keep)
        };
        self.scale = scale;
        self.screen = Some(screen);
        self.headed = headed;
        self.at = (headed.0 as f64 + behind.0, headed.1 as f64 + behind.1);
        if pose != self.pose {
            self.pose = pose;
            self.relayout();
        }
        self.show();
    }

    /// How the rail lies and what it is fused to with the pointer at
    /// `pointer`, on `screen`.
    fn pose_at(&self, pointer: (i32, i32), screen: Screen, scale: f64) -> Pose {
        let physical = |logical: f64| (logical * scale).round() as i32;
        // The rail standing where the pointer would hold it: off the top, it
        // stands up held by its middle.
        let down = rail_rect(self.items, Side::Right);
        let left = if self.pose.side.is_across() {
            pointer.0 - physical(down.width / 2.0)
        } else {
            pointer.0 - physical(self.hold.0) + physical(down.x)
        };
        let (area_left, area_top, area_right, _) = screen.area;
        let dock = fuse(
            self.pose.dock,
            pointer,
            (left, left + physical(down.width)),
            (area_left, area_top, area_right),
            (physical(DOCK_DISTANCE), physical(UNDOCK_DISTANCE)),
        );
        let side = match dock {
            Some(edge) => edge,
            // Standing up off the top, its card opens towards the middle of
            // the screen. Which way that is once it lands is for it to say.
            None if self.pose.side.is_across() => {
                if pointer.0 > (screen.bounds.0 + screen.bounds.2) / 2 { Side::Right } else { Side::Left }
            }
            None => self.pose.side,
        };
        Pose { side, dock }
    }

    /// Where the window is headed for the rail to lie and be fused as `pose`
    /// has it, held where it is by the pointer at `pointer` on `screen`: all
    /// of it on the screen, and fused, flush against the edge.
    fn headed_for(&self, pose: Pose, pointer: (i32, i32), screen: Screen, scale: f64) -> (i32, i32) {
        let inside = rail_rect(self.items, pose.side);
        let held = (pointer.0 - (self.hold.0 * scale).round() as i32, pointer.1 - (self.hold.1 * scale).round() as i32);
        let rail = rest(rail_at(held, inside, scale), physical_size(inside, scale), screen.area, pose, (FLARE_HEIGHT * scale).round() as i32);
        origin_for(rail, inside, scale)
    }

    /// It lies or is fused differently now: the window takes the size for
    /// that, and the page the layout, which it animates to.
    fn relayout(&self) {
        let panel = self.app.state::<PanelState>();
        *panel.side.lock().unwrap() = self.pose.side;
        let (width, height) = window_size(self.items, self.pose.side);
        let resized = panel.sized_to.lock().unwrap().replace((width, height)) != Some((width, height));
        if resized {
            let _ = self.window.set_size(LogicalSize::new(width, height));
        }
        let generation = panel.layout_generation.fetch_add(1, Ordering::SeqCst) + 1;
        publish_layout(&self.window, self.items, self.pose, true, self.shown, self.scale, generation);
    }

    /// Puts the window where it has got to, to the pixel.
    fn show(&mut self) {
        let at = (self.at.0.round() as i32, self.at.1.round() as i32);
        if at != self.shown {
            self.shown = at;
            let _ = self.window.set_position(PhysicalPosition::new(at.0, at.1));
        }
    }

    /// Let go of: it glides the rest of the way to where it was headed, and
    /// lands there.
    fn let_go(mut self) {
        let started = Instant::now();
        let mut last = started;
        loop {
            let behind = (self.at.0 - self.headed.0 as f64, self.at.1 - self.headed.1 as f64);
            if behind.0.abs().max(behind.1.abs()) < 0.5 || started.elapsed() >= GLIDE_AT_MOST {
                break;
            }
            std::thread::sleep(CARRY_STEP);
            let now = Instant::now();
            let keep = (-(now - last).as_secs_f64() / GLIDE.as_secs_f64()).exp();
            last = now;
            self.at = (self.headed.0 as f64 + behind.0 * keep, self.headed.1 as f64 + behind.1 * keep);
            self.show();
        }
        self.at = (self.headed.0 as f64, self.headed.1 as f64);
        self.show();

        let rail = rail_at(self.headed, rail_rect(self.items, self.pose.side), self.scale);
        let side = self.pose.dock.unwrap_or_else(|| side_for(&self.window, rail, self.items, self.scale));
        // Anything still waiting to settle an earlier move is too late: this
        // is where the rail lands.
        self.app.state::<PanelState>().move_generation.fetch_add(1, Ordering::SeqCst);
        position_for_rail(&self.app, &self.window, self.items, Pose { side, ..self.pose }, rail, self.scale);
    }
}

/// A screen, physical: all of it, and the part of it windows are given —
/// left, top, right, bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Screen {
    bounds: (i32, i32, i32, i32),
    area: (i32, i32, i32, i32),
}

impl Screen {
    fn of(monitor: &tauri::Monitor) -> Screen {
        let (position, size, area) = (monitor.position(), monitor.size(), monitor.work_area());
        Screen {
            bounds: (position.x, position.y, position.x + size.width as i32, position.y + size.height as i32),
            area: (
                area.position.x,
                area.position.y,
                area.position.x + area.size.width as i32,
                area.position.y + area.size.height as i32,
            ),
        }
    }
}

/// The screen `point` is on, or the one nearest it.
fn screen_at(screens: &[Screen], point: (i32, i32)) -> Option<Screen> {
    screens.iter().copied().min_by_key(|screen| {
        let (left, top, right, bottom) = screen.bounds;
        let dx = i64::from((left - point.0).max(point.0 - (right - 1)).max(0));
        let dy = i64::from((top - point.1).max(point.1 - (bottom - 1)).max(0));
        dx * dx + dy * dy
    })
}

/// The edge a carried rail is fused to, having been fused to `was`: the top
/// of the screen while the pointer is near it, a side while the rail —
/// standing where the pointer holds it, `rail` its left and right — is near
/// it or past it. Near is `distances.0` to fuse, and `distances.1` to stay
/// fused: pulled off an edge it holds on for a while, and parked just off
/// one it does not jump to it. All physical; `screen` is the work area's
/// left, top and right.
///
/// The top is tested against the pointer, not the rail (macOS
/// `FloatingPanelController.carry`): throwing the pointer at the top is the
/// deliberate gesture, and it wins in a corner. A rail fused to a side goes
/// up it to the top of the screen without turning, until the pointer does.
fn fuse(was: Option<Side>, pointer: (i32, i32), rail: (i32, i32), screen: (i32, i32, i32), distances: (i32, i32)) -> Option<Side> {
    let (left, top, right) = screen;
    let reach = |edge: Side| if was == Some(edge) { distances.1 } else { distances.0 };
    if pointer.1 - top <= reach(Side::Top) {
        return Some(Side::Top);
    }
    dock_edge(rail, (left, right), (reach(Side::Left), reach(Side::Right)))
}

/// The window stopped moving, moved by something other than a drag —
/// Windows, when a screen comes or goes. The rail stays fused to the edge
/// it is against, or floats, kept on screen, its card opening towards the
/// middle of whichever screen it is now on. Where it was left is remembered.
fn settle(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else { return };
    let items = Items::of(&app.state::<AppState>().settings.lock().unwrap());
    let Ok(origin) = window.outer_position() else { return };
    let scale = window.scale_factor().unwrap_or(1.0);
    let side = *app.state::<PanelState>().side.lock().unwrap();

    let inside = rail_rect(items, side);
    let rail = rail_at((origin.x, origin.y), inside, scale);
    let Some(monitor) = monitor_at(&window, rail_center(rail, (inside.width, inside.height), scale)) else { return };
    let (left, top, right, _) = Screen::of(&monitor).area;
    let physical = |side| physical_size(rail_rect(items, side), scale);
    let (dock, rail) = land(
        side,
        rail,
        (physical(Side::Right), physical(Side::Top)),
        (left, top, right),
        (DOCK_DISTANCE * scale).round() as i32,
    );
    let new_side = dock.unwrap_or_else(|| side_for(&window, rail, items, scale));
    position_for_rail(app, &window, items, Pose { side: new_side, dock }, rail, scale);
}

/// Where a rail that was on `side`, its top-left at `rail`, comes to rest
/// when something other than a drag moved it, on a screen whose work area
/// spans `screen` (left, top, right): the edge it fuses to, if any, and its
/// top-left laid the way that has it lie. It stays across the top as long
/// as it is there, and moved off it, stands up where it was. `sizes` are the
/// rail down a side and across the top; `distance`, `DOCK_DISTANCE`. All
/// physical.
fn land(side: Side, rail: (i32, i32), sizes: ((i32, i32), (i32, i32)), screen: (i32, i32, i32), distance: i32) -> (Option<Side>, (i32, i32)) {
    let (left, top, right) = screen;
    let (down, across) = sizes;
    if side.is_across() && rail.1 - top <= distance {
        return (Some(Side::Top), rail);
    }
    let rail = if side.is_across() { (rail.0 + across.0 / 2 - down.0 / 2, rail.1 + across.1 / 2 - down.1 / 2) } else { rail };
    (dock_edge((rail.0, rail.0 + down.0), (left, right), (distance, distance)), rail)
}

/// Which side of a screen spanning `screen` (left, right) a rail spanning
/// `rail` fuses to: one it is within `distances` (left, right) of, or past.
/// The nearer, on a screen so narrow it is near both.
fn dock_edge(rail: (i32, i32), screen: (i32, i32), distances: (i32, i32)) -> Option<Side> {
    let to_left = rail.0 - screen.0;
    let to_right = screen.1 - rail.1;
    match (to_left <= distances.0, to_right <= distances.1) {
        (true, true) if to_left <= to_right => Some(Side::Left),
        (true, true) => Some(Side::Right),
        (true, false) => Some(Side::Left),
        (false, true) => Some(Side::Right),
        (false, false) => None,
    }
}

pub fn set_hit_rects(app: &AppHandle, rects: Vec<Rect>) {
    *app.state::<PanelState>().hit_rects.lock().unwrap() = rects;
}

/// Puts the rail's own menu up under the pointer, and returns once it is
/// gone. While it is up the pointer is on it rather than on the rail, which
/// is not to be left because of that: a docked rail would wind down to its
/// sliver under it. And to show it, Windows makes the panel the active
/// window, as it does for any menu — the panel, which a click never makes
/// active. Once the menu is gone and what was chosen from it has been acted
/// on, the window that was active is again, unless another has been made
/// active meanwhile.
pub fn pop_up_menu<M: ContextMenu>(app: &AppHandle, window: &WebviewWindow, menu: &M) -> tauri::Result<()> {
    let panel = app.state::<PanelState>();
    panel.menu_open.store(true, Ordering::SeqCst);
    let active = active_window();
    let shown = window.popup_menu(menu);
    panel.menu_open.store(false, Ordering::SeqCst);
    // Queued behind the choice, which the menu has queued already: a window
    // it opens, such as Settings, comes up while the panel is still the
    // active window, and so in front: Windows can leave a new window behind
    // the active one when that belongs to another app. Queued from another
    // thread, as from this one it would run straight away.
    let (app, window) = (app.clone(), window.clone());
    tauri::async_runtime::spawn(async move {
        let _ = app.run_on_main_thread(move || give_back_activation(&window, active));
    });
    shown
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
            // So does the rail's own menu, which the pointer is on instead.
            let now_inside = over || (inside && primary_button_down()) || panel.menu_open.load(Ordering::SeqCst);
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
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};
    // The state is the physical button's: with the buttons swapped for the
    // left hand, the primary one is on the right.
    // SAFETY: plain queries with no pointers.
    let button = if unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0 { VK_RBUTTON } else { VK_LBUTTON };
    (unsafe { GetAsyncKeyState(button as i32) } as u16 & 0x8000) != 0
}

#[cfg(windows)]
fn active_window() -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    // SAFETY: a plain query.
    unsafe { GetForegroundWindow() as isize }
}

/// Makes `before` the active window again if a menu left the panel so.
#[cfg(windows)]
fn give_back_activation(window: &WebviewWindow, before: isize) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow, SetForegroundWindow};
    let Ok(panel) = window.hwnd() else { return };
    let before = before as HWND;
    // SAFETY: plain queries, and a request on a handle Windows checks.
    unsafe {
        if !before.is_null() && before != panel.0 && GetForegroundWindow() == panel.0 && IsWindow(before) != 0 {
            SetForegroundWindow(before);
        }
    }
}

/// Takes the window out of sight, or brings it back. Cloaked, it is still
/// drawn, and so is the page in it: only not shown.
#[cfg(windows)]
fn cloak(window: &WebviewWindow, cloaked: bool) {
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};
    let Ok(hwnd) = window.hwnd() else { return };
    let value = i32::from(cloaked);
    // SAFETY: the BOOL the attribute takes, read during the call.
    unsafe {
        DwmSetWindowAttribute(
            hwnd.0,
            DWMWA_CLOAK as u32,
            std::ptr::from_ref(&value).cast(),
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// Takes the title bar and frame off the panel's window, for good.
///
/// tao leaves both on a window without decorations, only sized to nothing,
/// and sets them again every time the window turns click-through or back.
/// Windows still draws them, the way it did on Windows 7, over the
/// see-through window whenever it redraws a frame there — when the rail's
/// menu makes the panel the active window, or the pointer comes and goes
/// quickly — and there they stay. Even without them it draws a title bar
/// when the panel is made active or not while it takes clicks, as it is for
/// that menu; that drawing is left out as well.
#[cfg(windows)]
fn keep_frameless(window: &WebviewWindow) {
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, SetWindowLongW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, WS_OVERLAPPEDWINDOW,
    };
    let handle = window.clone();
    // A window can only be subclassed on the thread it belongs to.
    let _ = window.run_on_main_thread(move || {
        let Ok(hwnd) = handle.hwnd() else { return };
        let hwnd = hwnd.0;
        // SAFETY: the window is alive and this is its thread; `frameless`
        // keeps no state of its own.
        unsafe {
            if SetWindowSubclass(hwnd, Some(frameless), FRAMELESS, 0) == 0 {
                return;
            }
            // What tao has set so far goes the same way, and the frame is
            // worked out again without it.
            SetWindowLongW(hwnd, GWL_STYLE, (GetWindowLongW(hwnd, GWL_STYLE) as u32 & !WS_OVERLAPPEDWINDOW) as i32);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    });
}

/// Which of the window's subclasses is `frameless`; tao has one of its own.
#[cfg(windows)]
const FRAMELESS: usize = 1;

/// The theme's own requests to draw a window's title bar and its frame.
/// Windows sends them but leaves them unnamed.
#[cfg(windows)]
const WM_NCUAHDRAWCAPTION: u32 = 0x00AE;
#[cfg(windows)]
const WM_NCUAHDRAWFRAME: u32 = 0x00AF;

/// Takes the title bar, the frame and their buttons out of every style set
/// on the panel's window, and keeps Windows from drawing a title bar there
/// anyway (`keep_frameless`).
#[cfg(windows)]
unsafe extern "system" fn frameless(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    id: usize,
    _data: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_STYLE, STYLESTRUCT, WM_NCACTIVATE, WM_NCDESTROY, WM_STYLECHANGING, WS_OVERLAPPEDWINDOW,
    };
    match message {
        WM_STYLECHANGING if wparam as i32 == GWL_STYLE => {
            // SAFETY: with this message, `lparam` is the style about to be set.
            let style = unsafe { &mut *(lparam as *mut STYLESTRUCT) };
            style.styleNew &= !WS_OVERLAPPEDWINDOW;
        }
        // Passed on, this draws a title bar over the top of the panel, frame
        // or no frame, whenever the panel is made active or not while it
        // takes clicks — as it is for the rail's menu. It is only there to
        // show that state on a frame, so it goes no further, as Chromium does
        // for its windows without one. tao, next in line, would only note
        // the focus, which Tauri takes from the webview instead.
        WM_NCACTIVATE => return 1,
        WM_NCUAHDRAWCAPTION | WM_NCUAHDRAWFRAME => return 0,
        WM_NCDESTROY => {
            // SAFETY: our own subclass, taken off on the window's thread.
            unsafe { RemoveWindowSubclass(hwnd, Some(frameless), id) };
        }
        _ => {}
    }
    // SAFETY: the message passed on down the window's subclasses.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(not(windows))]
fn cursor() -> Option<(i32, i32)> {
    None
}

#[cfg(not(windows))]
fn primary_button_down() -> bool {
    false
}

#[cfg(not(windows))]
fn active_window() -> isize {
    0
}

#[cfg(not(windows))]
fn give_back_activation(_window: &WebviewWindow, _before: isize) {}

#[cfg(not(windows))]
fn cloak(_window: &WebviewWindow, _cloaked: bool) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Provider;

    #[test]
    fn a_rail_near_a_side_fuses_to_it() {
        let screen = (0, 1920);
        let near = (32, 32);
        // At the distance, and past the edge, both count.
        assert_eq!(dock_edge((32, 96), screen, near), Some(Side::Left));
        assert_eq!(dock_edge((-40, 24), screen, near), Some(Side::Left));
        assert_eq!(dock_edge((1824, 1888), screen, near), Some(Side::Right));
        assert_eq!(dock_edge((1900, 1964), screen, near), Some(Side::Right));
        // Parked near an edge on purpose, it floats.
        assert_eq!(dock_edge((33, 97), screen, near), None);
        assert_eq!(dock_edge((900, 964), screen, near), None);
        // A second screen to the right is measured from its own left.
        assert_eq!(dock_edge((1940, 2004), (1920, 3840), near), Some(Side::Left));
    }

    #[test]
    fn on_a_screen_near_both_sides_the_nearer_wins() {
        assert_eq!(dock_edge((10, 74), (0, 100), (32, 32)), Some(Side::Left));
        assert_eq!(dock_edge((30, 94), (0, 100), (32, 32)), Some(Side::Right));
    }

    // A 1920-wide work area; fusing at 32, holding on to 96.
    const AREA: (i32, i32, i32) = (0, 0, 1920);
    const NEAR: (i32, i32) = (32, 96);

    #[test]
    fn a_carried_rail_fuses_to_the_top_when_the_pointer_comes_near_it_and_holds_on() {
        let rail = (900, 964);
        assert_eq!(fuse(None, (932, 32), rail, AREA, NEAR), Some(Side::Top));
        assert_eq!(fuse(None, (932, 33), rail, AREA, NEAR), None);
        assert_eq!(fuse(Some(Side::Top), (932, 96), rail, AREA, NEAR), Some(Side::Top));
        assert_eq!(fuse(Some(Side::Top), (932, 97), rail, AREA, NEAR), None);
        // A second screen's top is its own.
        assert_eq!(fuse(None, (2532, 1110), (2500, 2564), (1920, 1080, 3840), NEAR), Some(Side::Top));
    }

    #[test]
    fn a_carried_rail_fuses_to_a_side_it_comes_near_and_holds_on() {
        // The rail near the side, or past it; not the pointer.
        assert_eq!(fuse(None, (100, 500), (32, 96), AREA, NEAR), Some(Side::Left));
        assert_eq!(fuse(None, (100, 500), (-40, 24), AREA, NEAR), Some(Side::Left));
        assert_eq!(fuse(None, (100, 500), (33, 97), AREA, NEAR), None);
        assert_eq!(fuse(Some(Side::Left), (100, 500), (96, 160), AREA, NEAR), Some(Side::Left));
        assert_eq!(fuse(Some(Side::Left), (100, 500), (97, 161), AREA, NEAR), None);
        assert_eq!(fuse(None, (1850, 500), (1824, 1888), AREA, NEAR), Some(Side::Right));
        // Holding on to one side reaches no further for the other.
        assert_eq!(fuse(Some(Side::Left), (1800, 500), (1790, 1854), AREA, NEAR), None);
    }

    #[test]
    fn in_a_corner_the_pointer_at_the_top_wins() {
        assert_eq!(fuse(None, (10, 10), (-22, 42), AREA, NEAR), Some(Side::Top));
        assert_eq!(fuse(Some(Side::Left), (10, 10), (-22, 42), AREA, NEAR), Some(Side::Top));
        // Fused to a side, it goes up it without turning while the pointer
        // is short of the top.
        assert_eq!(fuse(Some(Side::Left), (10, 40), (-22, 42), AREA, NEAR), Some(Side::Left));
    }

    #[test]
    fn a_rail_comes_to_rest_on_its_screen_flush_against_the_edge_it_is_fused_to() {
        let area = (0, 0, 1920, 1040);
        let (down, across) = ((64, 200), (300, 64));
        let floating = Pose { side: Side::Right, dock: None };
        let left = Pose { side: Side::Left, dock: Some(Side::Left) };
        let top = Pose { side: Side::Top, dock: Some(Side::Top) };
        // Floating, anywhere on the screen, and back on it where it is not.
        assert_eq!(rest((500, 400), down, area, floating, 24), (500, 400));
        assert_eq!(rest((-10, 950), down, area, floating, 24), (0, 840));
        // Fused, flush against the edge, the sweep of its ends on the screen too.
        assert_eq!(rest((20, 400), down, area, left, 24), (0, 400));
        assert_eq!(rest((20, 10), down, area, left, 24), (0, 24));
        assert_eq!(rest((500, 30), across, area, top, 24), (500, 0));
        assert_eq!(rest((1700, 30), across, area, top, 24), (1596, 0));
    }

    fn items(count: usize, shows: RingShows, letters: bool) -> Items {
        let enabled = [Provider::ClaudeCode, Provider::Codex][..count].iter().map(|p| p.id().to_string()).collect();
        Items::of(&Settings { enabled, ring_shows: shows, limit_letters: letters, ..Settings::default() })
    }

    const EVERY_CHOICE: [RingShows; 6] = [
        RingShows::Fullest,
        RingShows::FiveHour,
        RingShows::Weekly,
        RingShows::BothSplit,
        RingShows::BothStacked,
        RingShows::BothNested,
    ];

    #[test]
    fn the_window_has_room_for_a_docked_rails_ends() {
        for count in 1..=2 {
            for shows in [RingShows::Fullest, RingShows::BothStacked] {
                let items = items(count, shows, true);
                let rail = rail_rect(items, Side::Right);
                let (_, height) = window_size(items, Side::Right);
                assert!(rail.y >= FLARE_HEIGHT + MARGIN, "{count} {shows:?}");
                assert!(height - (rail.y + rail.height) >= FLARE_HEIGHT + MARGIN, "{count} {shows:?}");

                // Across the top, they are its left and right.
                let rail = rail_rect(items, Side::Top);
                let (width, _) = window_size(items, Side::Top);
                assert!(rail.x >= FLARE_HEIGHT + MARGIN, "{count} {shows:?}");
                assert!(width - (rail.x + rail.width) >= FLARE_HEIGHT + MARGIN, "{count} {shows:?}");
            }
        }
    }

    #[test]
    fn a_rail_turning_its_card_round_stays_where_it_is_in_its_window() {
        for count in 1..=2 {
            for shows in EVERY_CHOICE {
                let items = items(count, shows, true);
                assert_eq!(rail_rect(items, Side::Left), rail_rect(items, Side::Right), "{count} {shows:?}");
                assert_eq!(window_size(items, Side::Left), window_size(items, Side::Right), "{count} {shows:?}");
                // With room for the card either way.
                let rail = rail_rect(items, Side::Left);
                let (width, _) = window_size(items, Side::Left);
                let card = GAP + POINTER_WIDTH + CARD_WIDTH;
                assert!(rail.x >= card + MARGIN, "{count} {shows:?}");
                assert!(width - (rail.x + rail.width) >= card + MARGIN, "{count} {shows:?}");
            }
        }
    }

    #[test]
    fn a_rail_turns_about_its_middle_and_its_window_changes_only_below_it() {
        let middle = |rail: Rect| (rail.x + rail.width / 2.0, rail.y + rail.height / 2.0);
        for count in 1..=2 {
            for shows in EVERY_CHOICE {
                let items = items(count, shows, true);
                assert_eq!(middle(rail_rect(items, Side::Right)), anchor(items), "{count} {shows:?}");
                assert_eq!(middle(rail_rect(items, Side::Top)), anchor(items), "{count} {shows:?}");
                assert_eq!(window_size(items, Side::Top).0, window_size(items, Side::Right).0, "{count} {shows:?}");
            }
        }
    }

    #[test]
    fn across_the_top_a_card_has_room_below_the_rail_wherever_it_is() {
        for count in 1..=2 {
            for shows in EVERY_CHOICE {
                let items = items(count, shows, true);
                let rail = rail_rect(items, Side::Top);
                let (width, height) = window_size(items, Side::Top);
                assert!(height - (rail.y + rail.height + GAP + POINTER_WIDTH) >= CARD_MAX_HEIGHT + MARGIN, "{count} {shows:?}");
                // Against the left side of a screen, the flare's width from
                // it, a card pushed back on screen still ends in the window;
                // against the right side, it still starts in it.
                assert!(rail.x - FLARE_HEIGHT + CARD_WIDTH <= width - MARGIN, "{count} {shows:?}");
                assert!(rail.x + rail.width + FLARE_HEIGHT - CARD_WIDTH >= MARGIN, "{count} {shows:?}");
            }
        }
    }

    #[test]
    fn across_the_top_the_services_sit_in_the_middle_of_the_rail() {
        for count in 1..=2 {
            for shows in EVERY_CHOICE {
                let items = items(count, shows, true);
                let n = count as f64;
                let services = n * items.length(Side::Top) + (n - 1.0) * ITEM_SPACING;
                let rail = rail_rect(items, Side::Top);
                assert_eq!(2.0 * pad_start(Side::Top) + services, rail.width, "{count} {shows:?}");
            }
        }
    }

    #[test]
    fn across_the_top_only_a_pair_of_figures_makes_room_for_letters() {
        let width = |shows, letters| items(1, shows, letters).width;
        // A figure on its own never carries one.
        assert_eq!(width(RingShows::Fullest, true), width(RingShows::Fullest, false));
        for shows in [RingShows::BothSplit, RingShows::BothNested, RingShows::BothStacked] {
            assert!(width(shows, true) > width(shows, false), "{shows:?}");
        }
        // Stacked, each ring has its own figure beside it.
        assert_eq!(width(RingShows::BothStacked, true), 2.0 * width(RingShows::BothSplit, true) + PAIR_GAP);
    }

    // A 1920-wide work area, and a rail 64 × 200 down a side or 300 × 64 across the top.
    const SCREEN: (i32, i32, i32) = (0, 0, 1920);
    const SIZES: ((i32, i32), (i32, i32)) = ((64, 200), (300, 64));

    #[test]
    fn a_rail_moved_without_a_drag_lies_the_way_it_did() {
        // Put back across the top by us: it stays.
        assert_eq!(land(Side::Top, (500, 0), SIZES, SCREEN, 32), (Some(Side::Top), (500, 0)));
        // Down a side with its top at the top of the screen: it does not turn.
        assert_eq!(land(Side::Left, (0, 24), SIZES, SCREEN, 32), (Some(Side::Left), (0, 24)));
        // Moved off the top by Windows: it stands up where it was.
        assert_eq!(land(Side::Top, (500, 600), SIZES, SCREEN, 32), (None, (618, 532)));
        // And fuses to a side it is near.
        assert_eq!(land(Side::Top, (-100, 600), SIZES, SCREEN, 32), (Some(Side::Left), (18, 532)));
    }
}
