// The floating panel: a rail of rings, and a card for the ring under the
// pointer. It draws what Rust sends and reports what it drew (so the window
// takes clicks exactly there) and what was clicked. Where the pointer is,
// Rust decides: this window lets clicks through everywhere else, so it would
// never hear the pointer leave.
//
// Docked against a side of the screen (Rust decides, `layout.dock`), the rail
// winds down to a sliver while the pointer is elsewhere and opens as it
// arrives — the macOS panel's `DockBerthShape`, drawn here as one SVG outline.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { colours, isSpent, percentText, reasonText, relative, resetText, shownFraction, tint, windowName } from "../shared/format";
import { t } from "../shared/i18n";
import { icon } from "../shared/icons";
import { RING, ringItem, shownWindows, svg } from "../shared/rail";
import type { AccountView, Layout, Rect, Snapshot, UsageWindow } from "../shared/types";
import "../shared/rail.css";
import "./panel.css";

/** Leaving is not closing at once: a pointer crossing to the card must not flicker it. */
const CLOSE_DELAY_MS = 140;
const DRAG_THRESHOLD = 4;

// The docked rail, in logical pixels (macOS `DockLayout`).
/** Tucked away: this much of it against the edge. */
const SLIVER_WIDTH = 6;
const SLIVER_HEIGHT = 96;
/** Wider than the sliver, so the pointer need not arrive exactly on it. */
const SLIVER_HIT_WIDTH = 20;
/** The body's corners, and how far along its ends the flare sweeps out to the edge: the rail's width between them. */
const DOCK_CORNER = 26;
const FLARE_WIDTH = 38;
/** Past the screen edge, so the outline's hairline is never drawn along it. */
const OVERHANG = 3;
/** Leaving winds it down only after this: overshooting the edge, or crossing to the card, must not. */
const COLLAPSE_DELAY_MS = 320;
/** Shown, a docked rail stays open this long, so it is seen where it is before it tucks away. */
const INTRO_MS = 2500;
/** A card waits this long after the rail starts opening, rather than landing on a sliver. */
const CARD_AFTER_OPEN_MS = 120;

const surface = document.getElementById("surface") as unknown as SVGSVGElement;
const outline = surface.querySelector("path")!;
const rail = document.getElementById("rail") as HTMLDivElement;
const card = document.getElementById("card") as HTMLDivElement;

let snapshot: Snapshot | null = null;
let layout: Layout | null = null;
/** Index into the rail of the ring whose card is open. */
let hovered: number | null = null;
let closeTimer: number | undefined;
let press: { x: number; y: number; index: number | null; dragging: boolean } | null = null;
let lastHitRects = "";
let lastLookedNote = 0;

/** What Rust last said: the pointer is on something drawn here. */
let pointerInside = false;
/** Just left: still open for `COLLAPSE_DELAY_MS`. */
let lingering = false;
let collapseTimer: number | undefined;
let introUntil = 0;
/** Whether the rail is open — always, unless it is docked and tucking away. */
let expanded = true;
let openedAt = 0;
/** 0 the sliver, 1 the whole rail, sprung between the two. */
let openness = 1;
let velocity = 0;
let frame = 0;

function railAccounts(): AccountView[] {
  return snapshot?.accounts.filter((account) => account.enabled) ?? [];
}

function itemTop(index: number): number {
  const l = layout!;
  return l.padTop + index * (l.itemHeight + l.itemSpacing);
}

// MARK: - The rail

function renderRail() {
  if (!layout || !snapshot) return;
  const { rail: frame } = layout;
  Object.assign(rail.style, {
    left: `${frame.x}px`,
    top: `${frame.y}px`,
    width: `${frame.width}px`,
    height: `${frame.height}px`,
  });
  const settings = snapshot.settings;
  rail.replaceChildren(
    ...railAccounts().map((account, index) => {
      const item = ringItem(account, settings);
      item.style.top = `${itemTop(index)}px`;
      item.style.height = `${layout!.itemHeight}px`;
      return item;
    }),
  );
}

// MARK: - The surface

/** Docked, and not being carried off: carried, it is already a floating capsule. */
function isDocked(): boolean {
  return layout?.dock != null && !press?.dragging;
}

/** Where the surface can reach: docked, the flare sweeps beyond the body above and below it. */
function surfaceBox(): Rect {
  const l = layout!;
  return isDocked() ? { x: l.rail.x, y: l.rail.y - l.flare, width: l.rail.width, height: l.rail.height + 2 * l.flare } : l.rail;
}

function capsule(width: number, height: number): string {
  const r = Math.min(width, height) / 2;
  return `M${r} 0H${width - r}A${r} ${r} 0 0 1 ${width} ${r}V${height - r}A${r} ${r} 0 0 1 ${width - r} ${height}H${r}A${r} ${r} 0 0 1 0 ${height - r}V${r}A${r} ${r} 0 0 1 ${r} 0Z`;
}

/**
 * The docked outline at `open` (0–1), in a `width` × `height` box: flush
 * against the screen edge for its whole height, its body inset from the
 * ends by the flare, which leaves the body's flat end and meets the edge
 * tangentially. Wound down, flare and corners shrink until only the sliver
 * is left — the same shape, not a second one. Drawn facing right, mirrored
 * for the left edge.
 */
function berth(width: number, height: number, flare: number, open: number, left: boolean): string {
  const w = SLIVER_WIDTH + (width - SLIVER_WIDTH) * open;
  const sliver = Math.min(SLIVER_HEIGHT, height);
  const h = sliver + (height - sliver) * open;
  const x0 = width - w;
  const y0 = (height - h) / 2;
  const f = Math.min(flare * open, h / 2);
  const r = Math.max(Math.min(SLIVER_WIDTH + (DOCK_CORNER - SLIVER_WIDTH) * open, w, (h - 2 * f) / 2), 0);
  const fw = Math.max(Math.min(FLARE_WIDTH * open, w - r), 0);
  // Pulls each fillet's control points off its ends: the usual circular-arc
  // approximation, which keeps the sweep full.
  const k = 0.55;

  const at = (x: number, y: number) => {
    const across = x0 + x;
    return `${(left ? width - across : across).toFixed(2)} ${(y0 + y).toFixed(2)}`;
  };
  // A quarter of a superellipse (exponent 4): the corner eases into the
  // straight edges beside it instead of starting abruptly, as a circle would.
  const corner = (cx: number, cy: number, from: [number, number], to: [number, number]) => {
    let d = "";
    for (let step = 1; step <= 16; step++) {
      const angle = (step / 16) * (Math.PI / 2);
      const along = Math.sqrt(Math.max(Math.cos(angle), 0));
      const beside = Math.sqrt(Math.max(Math.sin(angle), 0));
      d += `L${at(cx + r * (from[0] * along + to[0] * beside), cy + r * (from[1] * along + to[1] * beside))}`;
    }
    return d;
  };

  return (
    `M${at(r, f)}L${at(w - fw, f)}` +
    `C${at(w - fw * (1 - k), f)} ${at(w, f * k)} ${at(w, 0)}` +
    `L${at(w + OVERHANG, 0)}L${at(w + OVERHANG, h)}L${at(w, h)}` +
    `C${at(w, h - f * k)} ${at(w - fw * (1 - k), h - f)} ${at(w - fw, h - f)}` +
    `L${at(r, h - f)}` +
    corner(r, h - f - r, [0, 1], [-1, 0]) +
    `L${at(0, f + r)}` +
    corner(r, f + r, [-1, 0], [0, -1]) +
    "Z"
  );
}

/** The worst limit on the rail past the red line: tucked away, the sliver takes its colour. */
function alertColour(): string | null {
  const settings = snapshot?.settings;
  if (!settings) return null;
  let worst: UsageWindow | undefined;
  for (const account of railAccounts()) {
    for (const window of shownWindows(account, settings.ringShows)) {
      if (!window || (!isSpent(window) && window.usedFraction < settings.warningAt / 100)) continue;
      if (!worst || window.usedFraction > worst.usedFraction) worst = window;
    }
  }
  return worst ? tint(worst, settings.warningAt) : null;
}

function drawSurface() {
  if (!layout) return;
  const box = surfaceBox();
  surface.setAttribute("width", String(box.width));
  surface.setAttribute("height", String(box.height));
  Object.assign(surface.style, { left: `${box.x}px`, top: `${box.y}px` });
  const docked = isDocked();
  // Nothing past the screen edge: not the overhang, not the shadow.
  surface.style.clipPath = !docked ? "" : layout.dock === "right" ? "inset(-48px 0 -48px -48px)" : "inset(-48px -48px -48px 0)";
  const open = Math.min(Math.max(openness, 0), 1);
  outline.setAttribute("d", docked ? berth(box.width, box.height, layout.flare, open, layout.dock === "left") : capsule(box.width, box.height));
  outline.style.fill = docked && !expanded ? (alertColour() ?? "") : "";
}

/** Springs `openness` towards the state (macOS: response 0.32s, damping 0.86). Runs only while it moves. */
function animate() {
  cancelAnimationFrame(frame);
  const omega = (2 * Math.PI) / 0.32;
  let last = performance.now();
  const step = (now: number) => {
    const dt = Math.min((now - last) / 1000, 1 / 20);
    last = now;
    const target = expanded ? 1 : 0;
    for (let i = 0; i < 4; i++) {
      velocity += (omega * omega * (target - openness) - 2 * 0.86 * omega * velocity) * (dt / 4);
      openness += velocity * (dt / 4);
    }
    const settled = Math.abs(target - openness) < 0.002 && Math.abs(velocity) < 0.02;
    if (settled) {
      openness = target;
      velocity = 0;
    }
    drawSurface();
    if (!settled) frame = requestAnimationFrame(step);
  };
  frame = requestAnimationFrame(step);
}

/** Open unless docked, tucking away, and nothing is holding it. */
function wantsOpen(): boolean {
  if (!layout?.dock || !snapshot?.settings.autoCollapse) return true;
  return pointerInside || lingering || press != null || Date.now() < introUntil;
}

function updateOpen() {
  const next = wantsOpen();
  if (next === expanded) return;
  expanded = next;
  if (expanded) openedAt = Date.now();
  else hovered = null;
  rail.classList.toggle("tucked", !expanded);
  renderCard();
  animate();
}

/** Straight to the state, no animation: a rail that moved or appeared. */
function settleOpen() {
  expanded = wantsOpen();
  if (expanded) openedAt = 0;
  else hovered = null;
  cancelAnimationFrame(frame);
  openness = expanded ? 1 : 0;
  velocity = 0;
  rail.classList.toggle("tucked", !expanded);
}

/** Where the rail takes the pointer: tucked away, only the sliver's strip against the edge. */
function railHitRect(): Rect {
  const l = layout!;
  if (!isDocked()) return l.rail;
  if (expanded) return surfaceBox();
  const x = l.dock === "right" ? l.rail.x + l.rail.width - SLIVER_HIT_WIDTH : l.rail.x;
  return { x, y: l.rail.y + l.rail.height / 2 - SLIVER_HEIGHT / 2, width: SLIVER_HIT_WIDTH, height: SLIVER_HEIGHT };
}

/** Which ring a point on the rail belongs to: the gaps split between neighbours. */
function indexAt(clientY: number): number | null {
  if (!layout) return null;
  const count = railAccounts().length;
  const y = clientY - layout.rail.y;
  for (let index = 0; index < count; index++) {
    const top = itemTop(index) - layout.itemSpacing / 2;
    const bottom = itemTop(index) + layout.itemHeight + layout.itemSpacing / 2;
    if (y >= top && y < bottom) return index;
  }
  if (count > 0) return y < itemTop(0) ? 0 : count - 1;
  return null;
}

// MARK: - The card

function row(window: UsageWindow): HTMLElement {
  const settings = snapshot!.settings;
  const element = document.createElement("div");
  element.className = "row";

  const title = document.createElement("div");
  title.className = "row-title";
  title.textContent = windowName(window);
  element.appendChild(title);

  const bar = document.createElement("div");
  bar.className = "bar";
  const fill = document.createElement("div");
  const fraction = Math.min(Math.max(shownFraction(window, settings.showsRemaining), 0), 1);
  // Anything at all puts a dot on the bar, as the ring's round cap does.
  fill.style.width = fraction > 0 ? `max(${fraction * 100}%, 6px)` : "0";
  fill.style.backgroundColor = tint(window, settings.warningAt);
  bar.appendChild(fill);
  element.appendChild(bar);

  const facts = document.createElement("div");
  facts.className = "row-facts";
  const figure = document.createElement("span");
  figure.className = "figure";
  // The word follows the figure: "12% Left" when counting down.
  const percent = percentText(window, settings.showsRemaining);
  figure.textContent = settings.showsRemaining ? t("left", percent) : t("used", percent);
  if (isSpent(window)) figure.style.color = colours.exhausted;
  const reset = document.createElement("span");
  reset.className = "reset";
  reset.textContent = resetText(window);
  facts.append(figure, reset);
  element.appendChild(facts);
  return element;
}

function paragraph(className: string, text: string): HTMLElement {
  const element = document.createElement("div");
  element.className = className;
  element.textContent = text;
  return element;
}

function cardBody(account: AccountView): HTMLElement {
  const usage = account.usage;
  const body = document.createElement("div");
  body.className = "card-body";

  const header = document.createElement("div");
  header.className = "card-header";
  header.appendChild(icon(account.provider, 16));
  header.appendChild(document.createTextNode(t("usageTitle", account.name)));
  body.appendChild(header);

  for (const window of usage.windows) body.appendChild(row(window));

  if (usage.windows.length === 0 && usage.creditBalance != null) {
    const value = document.createElement("div");
    value.className = "value-row";
    value.append(document.createTextNode(t("creditBalance")));
    const amount = document.createElement("span");
    amount.className = "value";
    amount.textContent = usage.creditBalance;
    value.appendChild(amount);
    body.appendChild(value);
  }

  if (usage.state === "unavailable" && usage.reason) {
    body.appendChild(paragraph("message", reasonText(usage.reason)));
  } else if (usage.windows.length === 0 && usage.creditBalance == null) {
    // A card with only a title in it reads as one that failed to load.
    body.appendChild(paragraph("message", t("noLimitsReported")));
  }

  // Banked figures carry their date — and, since they are standing in for a
  // check that did not answer, why.
  if (usage.state === "stale" && usage.observedAt != null) {
    let note = t("asOf", relative(usage.observedAt));
    const failed = account.lastCheck?.reason;
    if (failed && failed !== "notChecked") note += ` · ${reasonText(failed)}`;
    body.appendChild(paragraph("footnote", note));
  }
  return body;
}

function tailFor(side: Layout["side"]): SVGSVGElement {
  const tail = svg("svg", { class: "card-tail", width: 20, height: 40, viewBox: "0 0 20 40" });
  // Concave sides meeting at a point aimed at the ring. The base overlaps the
  // body by a pixel so no seam shows.
  const d = side === "right" ? "M-1 0 Q 7 14 19.5 20 Q 7 26 -1 40 Z" : "M21 0 Q 13 14 0.5 20 Q 13 26 21 40 Z";
  tail.appendChild(svg("path", { d }));
  return tail;
}

/** Where the card's pointer aims: the middle of that service's ring, or of its two. */
function ringMiddle(index: number): number {
  const rings = rail.children[index]?.querySelectorAll(".ring");
  if (!rings?.length) return layout!.rail.y + itemTop(index) + RING / 2;
  return (rings[0].getBoundingClientRect().top + rings[rings.length - 1].getBoundingClientRect().bottom) / 2;
}

function renderCard() {
  const account = hovered == null ? undefined : railAccounts()[hovered];
  // Switched off, a ring still counts as looked at; it just opens nothing.
  if (!account || !layout || press?.dragging || !expanded || !snapshot?.settings.showsCard) {
    card.hidden = true;
    card.replaceChildren();
    reportHitRects(null);
    return;
  }

  const l = layout;
  const body = cardBody(account);
  body.style.width = `${l.cardWidth}px`;
  const tail = tailFor(l.side);
  card.hidden = false;
  card.replaceChildren(body, tail);

  const bodyLeft =
    l.side === "right" ? l.rail.x - l.gap - l.pointerWidth - l.cardWidth : l.rail.x + l.rail.width + l.gap + l.pointerWidth;
  const tailLeft = l.side === "right" ? bodyLeft + l.cardWidth : bodyLeft - l.pointerWidth;

  const height = body.offsetHeight;
  const ringCenter = ringMiddle(hovered!);
  const windowHeight = document.documentElement.clientHeight;
  // Centred on the ring, pushed back inside whatever part of the window is
  // on screen.
  const minTop = Math.max(l.visible.y, l.margin);
  const maxTop = Math.min(l.visible.y + l.visible.height, windowHeight - l.margin) - height;
  const top = Math.round(Math.min(Math.max(ringCenter - height / 2, minTop), Math.max(minTop, maxTop)));
  // The pointer keeps aiming at the ring, clear of the rounded corners.
  const tailTop = Math.min(Math.max(ringCenter - 20, top + 14), top + height - 14 - 40);

  body.style.left = `${bodyLeft}px`;
  body.style.top = `${top}px`;
  tail.style.left = `${tailLeft}px`;
  tail.style.top = `${tailTop}px`;

  // The gap between card and rail counts as the card, so crossing it keeps
  // the card open.
  const reach: Rect =
    l.side === "right"
      ? { x: bodyLeft, y: top, width: l.rail.x - bodyLeft, height }
      : { x: l.rail.x + l.rail.width, y: top, width: bodyLeft + l.cardWidth - (l.rail.x + l.rail.width), height };
  reportHitRects(reach);
}

function reportHitRects(cardRect: Rect | null) {
  if (!layout) return;
  const rects = cardRect ? [railHitRect(), cardRect] : [railHitRect()];
  const key = JSON.stringify(rects);
  if (key === lastHitRects) return;
  lastHitRects = key;
  void invoke("set_hit_rects", { rects });
}

function open(index: number | null) {
  window.clearTimeout(closeTimer);
  if (index === hovered) return;
  hovered = index;
  renderCard();
  const now = Date.now();
  if (index != null && now - lastLookedNote > 10_000) {
    lastLookedNote = now;
    void invoke("note_looked");
  }
}

function closeSoon() {
  window.clearTimeout(closeTimer);
  closeTimer = window.setTimeout(() => {
    hovered = null;
    renderCard();
  }, CLOSE_DELAY_MS);
}

// MARK: - Input

rail.addEventListener("mousemove", (event) => {
  if (!press?.dragging && expanded && Date.now() - openedAt >= CARD_AFTER_OPEN_MS) open(indexAt(event.clientY));
});

card.addEventListener("mousemove", () => window.clearTimeout(closeTimer));

rail.addEventListener("mousedown", (event) => {
  if (event.button !== 0) return;
  press = { x: event.screenX, y: event.screenY, index: indexAt(event.clientY), dragging: false };
});

// The flare and the sliver carry the rail too, but a click there is on no ring.
surface.addEventListener("mousedown", (event) => {
  if (event.button !== 0) return;
  press = { x: event.screenX, y: event.screenY, index: null, dragging: false };
});

window.addEventListener("mousemove", (event) => {
  if (!press || press.dragging || (event.buttons & 1) === 0) return;
  if (Math.hypot(event.screenX - press.x, event.screenY - press.y) < DRAG_THRESHOLD) return;
  // Past the threshold it is a drag, not a click. Windows runs the move
  // itself from here, and the button's release never reaches the page.
  press.dragging = true;
  hovered = null;
  // Carried off, it floats: a capsule, whatever edge it was fused to.
  drawSurface();
  renderCard();
  void getCurrentWindow().startDragging();
});

window.addEventListener("mouseup", (event) => {
  const pressed = press;
  press = null;
  if (!pressed || pressed.dragging || event.button !== 0 || pressed.index == null) return;
  // A click on a ring asks that provider now.
  const account = railAccounts()[pressed.index];
  if (account) void invoke("refresh", { provider: account.provider });
});

for (const target of [rail, surface]) {
  target.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    void invoke("panel_menu");
  });
}
// No browser menu anywhere on the panel.
document.addEventListener("contextmenu", (event) => event.preventDefault());

// MARK: - From Rust

function render() {
  renderRail();
  drawSurface();
  renderCard();
}

/** Open for a moment, then tuck away unless the pointer came. */
function intro() {
  introUntil = Date.now() + INTRO_MS;
  window.setTimeout(updateOpen, INTRO_MS + 20);
}

await listen<Snapshot>("snapshot", (event) => {
  snapshot = event.payload;
  if (hovered != null && hovered >= railAccounts().length) hovered = null;
  // Tucking away may just have been switched on or off.
  updateOpen();
  render();
});

// A new layout is the panel shown, resized or dropped somewhere.
await listen<Layout>("layout", (event) => {
  const fused = layout?.dock !== event.payload.dock;
  layout = event.payload;
  lastHitRects = "";
  // A drag ends here: its release never reaches the page.
  if (press?.dragging) press = null;
  intro();
  // Fused to an edge or taken off one, it is simply there: it did not open.
  if (fused) settleOpen();
  else updateOpen();
  render();
});

await listen<boolean>("pointer", (event) => {
  window.clearTimeout(collapseTimer);
  if (event.payload) {
    pointerInside = true;
    lingering = false;
    window.clearTimeout(closeTimer);
  } else {
    press = null;
    pointerInside = false;
    lingering = true;
    closeSoon();
    collapseTimer = window.setTimeout(() => {
      lingering = false;
      updateOpen();
    }, COLLAPSE_DELAY_MS);
  }
  updateOpen();
});

snapshot = await invoke<Snapshot>("get_snapshot");
layout = await invoke<Layout | null>("get_layout");
intro();
settleOpen();
render();

// "As of 5 minutes ago" has to keep up with the clock while a card is open.
window.setInterval(() => {
  if (hovered != null) renderCard();
}, 30_000);
