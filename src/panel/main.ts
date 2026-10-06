// The floating panel: a rail of rings, and a card for the ring under the
// pointer. It draws what Rust sends and reports what it drew (so the window
// takes clicks exactly there) and what was clicked. Where the pointer is,
// Rust decides: this window lets clicks through everywhere else, so it would
// never hear the pointer leave.
//
// Docked against an edge of the screen (Rust decides, `layout.dock`), and with
// `tucksAway` on, the rail winds down to a sliver while the pointer is
// elsewhere and opens as it arrives — the macOS panel's `DockBerthShape`, drawn here as one SVG outline.
// Docked at the top, it lies across the screen with its figures beside its
// rings, and the card opens below it.
//
// It docks while it is carried, and what changes then is animated rather
// than swapped: the capsule fuses into the edge and comes off it again, and
// to or from the top, the rail turns a quarter round about its middle, its
// rings and figures going round with it, upright. It comes off an edge before
// it turns, and fuses to one once it lies along it.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { TURN_MS, claudeAccounts, railAccounts as onRail, takesTurns } from "../shared/accounts";
import { colours, isSpent, percentText, reasonText, relative, resetText, shortWindowName, shownFraction, tint, untilText, windowName } from "../shared/format";
import { t } from "../shared/i18n";
import { icon } from "../shared/icons";
import { RING, ringItem, shownWindows, svg } from "../shared/rail";
import { applyGlass, railScheme } from "../shared/theme";
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
/** The screen edge a docked rail is against. */
type Edge = NonNullable<Layout["dock"]>;
/** Leaving winds it down only after this: overshooting the edge, or crossing to the card, must not. */
const COLLAPSE_DELAY_MS = 320;
/** Shown, a docked rail stays open this long, so it is seen where it is before it tucks away. */
const INTRO_MS = 2500;
/** A card waits this long after the rail starts opening, rather than landing on a sliver. */
const CARD_AFTER_OPEN_MS = 120;
/** The card grows out of its pointer, moves from one service to the next and fades away: quickly, easing to a stop. */
const CARD_EASE = "cubic-bezier(0.2, 0.9, 0.3, 1)";
const CARD_OPEN_MS = 180;
const CARD_MOVE_MS = 180;
const CARD_CLOSE_MS = 110;
/** Nothing grows or slides for somebody who asked Windows for less motion: the card only fades. */
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
/** The springs, as macOS gives them: how long in seconds, and how damped. */
const OPEN_SPRING = [0.32, 0.86] as const;
const TURN_SPRING = [0.34, 0.8] as const;
const FUSE_SPRING = [0.26, 1] as const;

const surface = document.getElementById("surface") as unknown as SVGSVGElement;
const outlinePath = surface.querySelector<SVGPathElement>(":scope > path")!;
const edgeClip = surface.querySelector<SVGRectElement>("clipPath rect")!;
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
/** Where the open card's body and pointer are, for moving them on from there; null closed. */
let cardAt: { body: Point; tail: Point } | null = null;
/** The card fading away (`hideCard`). */
let cardLeaving: Animation | null = null;

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
let openVelocity = 0;
/** The edge the outline is fused to, or last was while it comes off it; and how far, 0 a capsule and 1 fused. */
let fusedTo: Edge | null = null;
let fused = 0;
let fuseVelocity = 0;
/** A turn to or from the top under way (`beginTurn`). */
let turning: Turning | null = null;
/** How far the turn has got, 0 to 1. */
let turn = 1;
let turnVelocity = 0;
let frame = 0;

function railAccounts(): AccountView[] {
  return snapshot ? onRail(snapshot) : [];
}

/** The accounts a ring stands for: a Claude ring, every Claude account. */
function behind(account: AccountView): AccountView[] {
  return account.provider === "claudeCode" && snapshot ? claudeAccounts(snapshot) : [account];
}

/** Whether the rail lies across the top of the screen rather than down a side. */
function isAcross(): boolean {
  return layout?.side === "top";
}

/** Where an item starts along the rail: down it from its top, or across it from its left end. */
function itemStart(index: number): number {
  const l = layout!;
  return l.padStart + index * (l.itemLength + l.itemSpacing);
}

// MARK: - The rail

function renderRail() {
  if (!layout || !snapshot) return;
  const { rail: frame } = layout;
  const across = isAcross();
  Object.assign(rail.style, {
    left: `${frame.x}px`,
    top: `${frame.y}px`,
    width: `${frame.width}px`,
    height: `${frame.height}px`,
  });
  rail.classList.toggle("across", across);
  const settings = snapshot.settings;
  const before = Array.from(rail.children, (item) => (item as HTMLElement).dataset.account);
  rail.replaceChildren(
    ...railAccounts().map((account, index) => {
      const item = ringItem(account, settings);
      item.dataset.account = account.id;
      // Another account's turn: it comes in rather than being swapped in.
      if (before[index] && before[index] !== account.id) item.classList.add("turn-in");
      const start = `${itemStart(index)}px`;
      const length = `${layout!.itemLength}px`;
      Object.assign(item.style, across ? { left: start, width: length } : { top: start, height: length });
      return item;
    }),
  );
  // Redrawn while it turns, it goes on turning.
  placeParts();
  nextTurn();
}

/** Accounts taking turns on the rail: the next one's, on the turn of the clock. */
let turnTimer: number | undefined;
function nextTurn() {
  window.clearTimeout(turnTimer);
  if (!snapshot || !takesTurns(snapshot)) return;
  turnTimer = window.setTimeout(() => {
    renderRail();
    drawSurface();
  }, TURN_MS - (Date.now() % TURN_MS) + 20);
}

// MARK: - The turn

type Point = [number, number];

/**
 * A rail turning to or from the top: turning a quarter round about its
 * middle, as it began — its length, thickness and middle then, and how far
 * round it was from the way the layout has it lie now, in degrees (a rail
 * down a side is one across the top turned clockwise, first ring to the
 * top). Each ring and figure goes round with it, upright, from where it was
 * to where it now is: both from the rail's middle, `was` laid the way the
 * rail now lies.
 */
interface Turning {
  angle: number;
  long: number;
  thick: number;
  middle: Point;
  was: Point[][];
  now: Point[][];
}

/** How the rail's body is drawn at the moment: its middle, length and thickness, and how far round it is from the way the layout has it lie. */
interface Body {
  middle: Point;
  long: number;
  thick: number;
  angle: number;
}

function middleOf(box: Rect): Point {
  return [box.x + box.width / 2, box.y + box.height / 2];
}

/** A rail's box from its middle, length and thickness, lying across or down. */
function boxAround([x, y]: Point, long: number, thick: number, across: boolean): Rect {
  return across
    ? { x: x - long / 2, y: y - thick / 2, width: long, height: thick }
    : { x: x - thick / 2, y: y - long / 2, width: thick, height: long };
}

/** `point` turned `degrees` clockwise about the origin, as the page's y runs down. */
function rotate([x, y]: Point, degrees: number): Point {
  const angle = (degrees * Math.PI) / 180;
  return [x * Math.cos(angle) - y * Math.sin(angle), x * Math.sin(angle) + y * Math.cos(angle)];
}

function bodyNow(): Body {
  const l = layout!;
  const across = isAcross();
  const middle = middleOf(l.rail);
  const long = across ? l.rail.width : l.rail.height;
  const thick = across ? l.rail.height : l.rail.width;
  if (!turning) return { middle, long, thick, angle: 0 };
  return {
    middle: [mix(turning.middle[0], middle[0], turn), mix(turning.middle[1], middle[1], turn)],
    long: mix(turning.long, long, turn),
    thick: mix(turning.thick, thick, turn),
    angle: turning.angle * (1 - turn),
  };
}

/** Where each ring and figure on the rail is drawn now, by service and part. */
function partCentres(): Point[][] {
  return Array.from(rail.children, (item) =>
    Array.from(item.children, (part): Point => {
      const box = part.getBoundingClientRect();
      return [box.left + box.width / 2, box.top + box.height / 2];
    }),
  );
}

/** Each ring and figure where the turn has taken it: gone round the rail's middle with it, on its way from where it was to where it now is. */
function placeParts() {
  const body = turning ? bodyNow() : null;
  const laid = layout ? middleOf(layout.rail) : [0, 0];
  Array.from(rail.children).forEach((item, i) =>
    Array.from(item.children as HTMLCollectionOf<HTMLElement>).forEach((part, j) => {
      const was = turning?.was[i]?.[j];
      const now = turning?.now[i]?.[j];
      if (!body || !was || !now) {
        part.style.transform = "";
        return;
      }
      const [x, y] = rotate([mix(was[0], now[0], turn), mix(was[1], now[1], turn)], body.angle);
      const dx = body.middle[0] + x - (laid[0] + now[0]);
      const dy = body.middle[1] + y - (laid[1] + now[1]);
      part.style.transform = `translate(${dx.toFixed(2)}px, ${dy.toFixed(2)}px)`;
    }),
  );
}

/**
 * The rail turns, to the way the layout now has it lie, from `body` — how it
 * was drawn, the other way — and its rings and figures from `parts`, where
 * they were. Part way through a turn back, it turns back from there.
 */
function beginTurn(body: Body, parts: Point[][]) {
  const angle = body.angle + (isAcross() ? 90 : -90);
  // Turned back before it had got going: it is as it was.
  if (Math.abs(angle) < 0.5) {
    turning = null;
    return;
  }
  const laid = middleOf(layout!.rail);
  turning = {
    angle,
    long: body.long,
    thick: body.thick,
    middle: body.middle,
    was: parts.map((item) => item.map(([x, y]) => rotate([x - body.middle[0], y - body.middle[1]], -angle))),
    now: partCentres().map((item) => item.map(([x, y]): Point => [x - laid[0], y - laid[1]])),
  };
  turn = 0;
  turnVelocity = 0;
  placeParts();
  drawSurface();
}

// MARK: - The surface

/** Fused to an edge of the screen — carried along it too. */
function isDocked(): boolean {
  return layout?.dock != null;
}

/** Where the surface can reach: docked, the flare sweeps beyond the body along the edge — above and below it, or either end of it across the top. */
function surfaceBox(): Rect {
  const l = layout!;
  if (!isDocked()) return l.rail;
  return isAcross()
    ? { x: l.rail.x - l.flare, y: l.rail.y, width: l.rail.width + 2 * l.flare, height: l.rail.height }
    : { x: l.rail.x, y: l.rail.y - l.flare, width: l.rail.width, height: l.rail.height + 2 * l.flare };
}

function mix(from: number, to: number, amount: number): number {
  return from + (to - from) * amount;
}

/**
 * The rail's outline round `body`, in the window: a capsule at `fused` 0,
 * and at 1 fused to `edge` — flush against the screen edge all along it, its
 * body inset from the ends by the flare, which leaves the body's flat end
 * and meets the edge tangentially. In between, the corners by the edge
 * straighten and sweep out into it: each point of the outline is on its way
 * from where the capsule has it to where the fused outline does. Wound down
 * (`open` 0), flare and corners shrink until only the sliver is left — the
 * same shape, not a second one. Worked out facing right, mirrored for the
 * left edge and turned a quarter for the top.
 */
function outline(body: Rect, edge: Edge, fused: number, open: number, flare: number): string {
  // Facing right, x runs across the rail to the edge and y along it, from the body's end.
  const thick = edge === "top" ? body.height : body.width;
  const long = edge === "top" ? body.width : body.height;
  const at = (x: number, y: number) => {
    const [px, py] =
      edge === "right" ? [body.x + x, body.y + y] : edge === "left" ? [body.x + thick - x, body.y + y] : [body.x + y, body.y + thick - x];
    return `${px.toFixed(2)} ${py.toFixed(2)}`;
  };

  // Fused, as far open as `open` has it.
  const w = SLIVER_WIDTH + (thick - SLIVER_WIDTH) * open;
  const sliver = Math.min(SLIVER_HEIGHT, long + 2 * flare);
  const h = sliver + (long + 2 * flare - sliver) * open;
  const x0 = thick - w;
  const y0 = (long - h) / 2;
  const f = Math.min(flare * open, h / 2);
  const r = Math.max(Math.min(SLIVER_WIDTH + (DOCK_CORNER - SLIVER_WIDTH) * open, w, (h - 2 * f) / 2), 0);
  const fw = Math.max(Math.min(FLARE_WIDTH * open, w - r), 0);
  // Floating, its round ends.
  const c = Math.min(thick, long) / 2;
  // Pulls each fillet's control points off its ends: the usual circular-arc
  // approximation, which keeps the sweep full. And a quarter circle's.
  const k = 0.55;
  const q = 1 - 0.5523;

  const point = (fusedAt: [number, number], floatingAt: [number, number]) =>
    at(mix(floatingAt[0], fusedAt[0], fused), mix(floatingAt[1], fusedAt[1], fused));
  // Fused, a quarter of a superellipse (exponent 4): the corner eases into
  // the straight edges beside it instead of starting abruptly, as a circle
  // would. Floating, a quarter circle.
  const corner = (fusedCentre: [number, number], floatingCentre: [number, number], from: [number, number], to: [number, number]) => {
    let d = "";
    for (let step = 1; step <= 16; step++) {
      const angle = (step / 16) * (Math.PI / 2);
      const cos = Math.max(Math.cos(angle), 0);
      const sin = Math.max(Math.sin(angle), 0);
      const along = Math.sqrt(cos);
      const beside = Math.sqrt(sin);
      d += `L${point(
        [fusedCentre[0] + r * (from[0] * along + to[0] * beside), fusedCentre[1] + r * (from[1] * along + to[1] * beside)],
        [floatingCentre[0] + c * (from[0] * cos + to[0] * sin), floatingCentre[1] + c * (from[1] * cos + to[1] * sin)],
      )}`;
    }
    return d;
  };

  const end = y0 + h;
  return (
    `M${point([x0 + r, y0 + f], [c, 0])}L${point([thick - fw, y0 + f], [thick - c, 0])}` +
    `C${point([thick - fw * (1 - k), y0 + f], [thick - c * q, 0])} ${point([thick, y0 + f * k], [thick, c * q])} ${point([thick, y0], [thick, c])}` +
    `L${point([thick + OVERHANG, y0], [thick, c])}L${point([thick + OVERHANG, end], [thick, long - c])}L${point([thick, end], [thick, long - c])}` +
    `C${point([thick, end - f * k], [thick, long - c * q])} ${point([thick - fw * (1 - k), end - f], [thick - c * q, long])} ${point([thick - fw, end - f], [thick - c, long])}` +
    `L${point([x0 + r, end - f], [c, long])}` +
    corner([x0 + r, end - f - r], [c, long - c], [0, 1], [-1, 0]) +
    `L${point([x0, y0 + f + r], [0, c])}` +
    corner([x0 + r, y0 + f + r], [c, c], [-1, 0], [0, -1]) +
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
  // Not before the first snapshot either: until then the theme isn't known.
  if (!layout || !snapshot) return;
  const { middle, long, thick, angle: turned } = bodyNow();
  const edge = fusedTo ?? "right";
  const amount = fusedTo ? Math.min(Math.max(fused, 0), 1) : 0;
  // Fused, it is drawn laid along the edge, and turned from there: a rail
  // coming off an edge to turn, or fusing to one as it finishes turning.
  const across = amount > 0 ? edge === "top" : isAcross();
  const angle = across === isAcross() ? turned : turned - (isAcross() ? 90 : -90);
  const body = boxAround(middle, long, thick, across);
  const open = Math.min(Math.max(openness, 0), 1);
  outlinePath.setAttribute("d", outline(body, edge, amount, open, layout.flare));
  if (angle) outlinePath.setAttribute("transform", `rotate(${angle.toFixed(3)} ${middle[0].toFixed(2)} ${middle[1].toFixed(2)})`);
  else outlinePath.removeAttribute("transform");
  // Nothing past the screen edge — where the body's side against it is —
  // not the overhang, nor its hairline.
  if (amount > 0) {
    const far = 100_000;
    const [x, y, width, height] =
      edge === "top"
        ? [-far, body.y, 2 * far, 2 * far]
        : edge === "left"
          ? [body.x, -far, 2 * far, 2 * far]
          : [-far, -far, far + body.x + body.width, 2 * far];
    for (const [name, value] of Object.entries({ x, y, width, height })) edgeClip.setAttribute(name, String(value));
    outlinePath.setAttribute("clip-path", "url(#edge-clip)");
  } else {
    outlinePath.removeAttribute("clip-path");
  }
  outlinePath.style.fill = isDocked() && !expanded ? (alertColour() ?? "") : "";
}

/** One step of a spring from `value` towards `target`, `dt` seconds on: the value and velocity it has then. */
function spring(value: number, velocity: number, target: number, dt: number, [response, damping]: readonly [number, number]): [number, number] {
  const omega = (2 * Math.PI) / response;
  for (let i = 0; i < 4; i++) {
    velocity += (omega * omega * (target - value) - 2 * damping * omega * velocity) * (dt / 4);
    value += velocity * (dt / 4);
  }
  return [value, velocity];
}

function isSettled(value: number, velocity: number, target: number): boolean {
  return Math.abs(target - value) < 0.002 && Math.abs(velocity) < 0.02;
}

/** Springs the rail towards its state — open or tucked away, fused or floating, turned — while any of it still moves. */
function animate() {
  cancelAnimationFrame(frame);
  let last = performance.now();
  const step = (now: number) => {
    const dt = Math.max(Math.min((now - last) / 1000, 1 / 20), 0);
    last = now;
    // The turn before the fuse: a turn that ends lets the rail fuse in the same step.
    const moving = [stepOpen(dt), stepTurn(dt), stepFuse(dt)].includes(true);
    drawSurface();
    placeParts();
    if (moving) frame = requestAnimationFrame(step);
  };
  frame = requestAnimationFrame(step);
}

/** Opens or tucks away: whether it is still on its way. */
function stepOpen(dt: number): boolean {
  const target = expanded ? 1 : 0;
  [openness, openVelocity] = spring(openness, openVelocity, target, dt, OPEN_SPRING);
  if (!isSettled(openness, openVelocity, target)) return true;
  openness = target;
  openVelocity = 0;
  return false;
}

/** Fuses to the edge the layout has it docked to, or comes off it — off one edge before onto another, and not while it turns. */
function stepFuse(dt: number): boolean {
  const edge = layout?.dock ?? null;
  if (edge !== fusedTo && fused < 0.02) fusedTo = edge;
  // Turning, it fuses as the turn comes to an end, not once it is quite still.
  const target = edge != null && edge === fusedTo && (!turning || turn > 0.9) ? 1 : 0;
  [fused, fuseVelocity] = spring(fused, fuseVelocity, target, dt, FUSE_SPRING);
  if (!isSettled(fused, fuseVelocity, target)) return true;
  fused = target;
  fuseVelocity = 0;
  // Off one edge, and on its way to another.
  return edge !== fusedTo;
}

/** Turns: once it has mostly come off the edge it was fused to, if it was, and done once it is quite off it. */
function stepTurn(dt: number): boolean {
  if (!turning) return false;
  const leaving = fusedTo != null && fusedTo !== (layout?.dock ?? null);
  if (leaving && fused >= 0.3) return true;
  [turn, turnVelocity] = spring(turn, turnVelocity, 1, dt, TURN_SPRING);
  if (!isSettled(turn, turnVelocity, 1) || leaving) return true;
  turning = null;
  turn = 1;
  turnVelocity = 0;
  return false;
}

/** Open unless docked, tucking away, and nothing is holding it — carrying it included, until it has landed. */
function wantsOpen(): boolean {
  if (!layout?.dock || !snapshot?.settings.tucksAway) return true;
  return pointerInside || lingering || press != null || layout.carried || Date.now() < introUntil;
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

/** Straight to the state, no animation: a rail that was put somewhere, or appeared. */
function settle() {
  expanded = wantsOpen();
  if (expanded) openedAt = 0;
  else hovered = null;
  cancelAnimationFrame(frame);
  openness = expanded ? 1 : 0;
  openVelocity = 0;
  rail.classList.toggle("tucked", !expanded);
  fusedTo = layout?.dock ?? null;
  fused = fusedTo ? 1 : 0;
  fuseVelocity = 0;
  turning = null;
  turn = 1;
  turnVelocity = 0;
}

/** Where the rail takes the pointer: tucked away, only the sliver's strip against the edge. */
function railHitRect(): Rect {
  const l = layout!;
  if (!isDocked()) return l.rail;
  if (expanded) return surfaceBox();
  if (isAcross()) {
    return { x: l.rail.x + l.rail.width / 2 - SLIVER_HEIGHT / 2, y: l.rail.y, width: SLIVER_HEIGHT, height: SLIVER_HIT_WIDTH };
  }
  const x = l.dock === "right" ? l.rail.x + l.rail.width - SLIVER_HIT_WIDTH : l.rail.x;
  return { x, y: l.rail.y + l.rail.height / 2 - SLIVER_HEIGHT / 2, width: SLIVER_HIT_WIDTH, height: SLIVER_HEIGHT };
}

/** Which ring a point on the rail belongs to: the gaps split between neighbours. */
function indexAt(event: MouseEvent): number | null {
  if (!layout) return null;
  const count = railAccounts().length;
  const along = isAcross() ? event.clientX - layout.rail.x : event.clientY - layout.rail.y;
  for (let index = 0; index < count; index++) {
    const start = itemStart(index) - layout.itemSpacing / 2;
    const end = itemStart(index) + layout.itemLength + layout.itemSpacing / 2;
    if (along >= start && along < end) return index;
  }
  if (count > 0) return along < itemStart(0) ? 0 : count - 1;
  return null;
}

// MARK: - The card

function row(window: UsageWindow): HTMLElement {
  const settings = snapshot!.settings;
  const element = document.createElement("div");
  element.className = "row";

  // What the limit is, and how long until it resets.
  const head = document.createElement("div");
  head.className = "row-head";
  const title = document.createElement("span");
  title.className = "row-title";
  title.textContent = windowName(window);
  const until = document.createElement("span");
  until.className = "until";
  until.textContent = untilText(window);
  head.append(title, until);
  element.appendChild(head);

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

/** Which account a card is, once there is more than one it could be. */
function whoText(account: AccountView): string | null {
  const several = account.provider === "claudeCode" && (snapshot?.settings.claudeAccounts.length ?? 0) > 0;
  if (!several && !account.label) return null;
  return [account.label, account.email ?? (account.label ? null : account.title)].filter(Boolean).join(" · ");
}

/** One account among several on a card: what it is called, and a line for each limit. */
function accountBlock(account: AccountView): HTMLElement {
  const settings = snapshot!.settings;
  const block = document.createElement("div");
  block.className = "account-block";
  const head = document.createElement("div");
  head.className = "account-head";
  const name = document.createElement("span");
  name.className = "account-name";
  name.textContent = account.title;
  head.appendChild(name);
  if (account.inClaudeCode) {
    const badge = document.createElement("span");
    badge.className = "account-badge";
    badge.textContent = t("inClaudeCode");
    head.appendChild(badge);
  }
  block.appendChild(head);

  const usage = account.usage;
  if (usage.state === "unavailable") {
    if (usage.reason) block.appendChild(paragraph("message", reasonText(usage.reason)));
    return block;
  }
  const limits = usage.windows.filter((window) => window.kind === "fiveHour" || window.kind === "weekly");
  for (const window of limits.length ? limits : usage.windows) {
    const line = document.createElement("div");
    line.className = "limit-line";
    const label = document.createElement("span");
    label.className = "limit-name";
    label.textContent = shortWindowName(window);
    const bar = document.createElement("div");
    bar.className = "bar";
    const fill = document.createElement("div");
    const fraction = Math.min(Math.max(shownFraction(window, settings.showsRemaining), 0), 1);
    fill.style.width = fraction > 0 ? `max(${fraction * 100}%, 4px)` : "0";
    fill.style.backgroundColor = tint(window, settings.warningAt);
    bar.appendChild(fill);
    const figure = document.createElement("span");
    figure.className = "figure";
    figure.textContent = percentText(window, settings.showsRemaining);
    if (isSpent(window)) figure.style.color = colours.exhausted;
    const until = document.createElement("span");
    until.className = "until";
    until.textContent = untilText(window);
    line.append(label, bar, figure, until);
    block.appendChild(line);
  }
  return block;
}

function cardBody(account: AccountView): HTMLElement {
  const usage = account.usage;
  const body = document.createElement("div");
  body.className = "card-body";

  // A Claude ring with several accounts behind it: every one of them.
  const accounts = behind(account);
  if (accounts.length > 1) {
    const header = document.createElement("div");
    header.className = "card-header";
    header.appendChild(icon(account.provider, 16));
    header.appendChild(document.createTextNode(t("usageTitle", account.name)));
    body.classList.add("several");
    body.append(header, ...accounts.map(accountBlock));
    return body;
  }

  const header = document.createElement("div");
  header.className = "card-header";
  header.appendChild(icon(account.provider, 16));
  header.appendChild(document.createTextNode(t("usageTitle", account.name)));
  body.appendChild(header);
  const who = whoText(account);
  if (who) body.appendChild(paragraph("card-account", who));

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
  // Concave sides meeting at a point aimed at the ring. The base overlaps the
  // body by a pixel so no seam shows.
  if (side === "top") {
    const tail = svg("svg", { class: "card-tail up", width: 40, height: 20, viewBox: "0 0 40 20" });
    tail.appendChild(svg("path", { d: "M0 21 Q 14 13 20 0.5 Q 26 13 40 21 Z" }));
    return tail;
  }
  const tail = svg("svg", { class: "card-tail", width: 20, height: 40, viewBox: "0 0 20 40" });
  const d = side === "right" ? "M-1 0 Q 7 14 19.5 20 Q 7 26 -1 40 Z" : "M21 0 Q 13 14 0.5 20 Q 13 26 21 40 Z";
  tail.appendChild(svg("path", { d }));
  return tail;
}

/**
 * Where the card's pointer aims along the rail, and the card is centred.
 * Down a side, the middle of that service's ring, or of its two, the figures
 * under them. Across the top, the middle of the service, its figures beside
 * its rings: CSS centres them in the room the item has.
 */
function aimAlong(index: number): number {
  const l = layout!;
  if (isAcross()) return l.rail.x + itemStart(index) + l.itemLength / 2;
  const rings = rail.children[index]?.querySelectorAll(".ring");
  if (!rings?.length) return l.rail.y + itemStart(index) + RING / 2;
  return (rings[0].getBoundingClientRect().top + rings[rings.length - 1].getBoundingClientRect().bottom) / 2;
}

function renderCard() {
  const account = hovered == null ? undefined : railAccounts()[hovered];
  // Switched off, a ring still counts as looked at; it just opens nothing.
  if (!account || !layout || layout.carried || press?.dragging || !expanded || !snapshot?.settings.showsCard) {
    // Carried off, the rail takes it along at once; otherwise it fades.
    hideCard(layout != null && !layout.carried && !press?.dragging);
    reportHitRects(null);
    return;
  }

  const l = layout;
  const was = cardAt;
  // On its way out, it comes back from however far it had faded.
  const faded = cardLeaving ? Number(getComputedStyle(card).opacity) : null;
  cardLeaving?.cancel();
  cardLeaving = null;
  const body = cardBody(account);
  body.style.width = `${l.cardWidth}px`;
  const tail = tailFor(l.side);
  card.hidden = false;
  card.replaceChildren(body, tail);

  if (l.side === "top") {
    // Below the rail, centred under the service, pushed back inside whatever
    // part of the window is on screen.
    const top = l.rail.y + l.rail.height + l.gap + l.pointerWidth;
    const aim = aimAlong(hovered!);
    const windowWidth = document.documentElement.clientWidth;
    const minLeft = Math.max(l.visible.x, l.margin);
    const maxLeft = Math.min(l.visible.x + l.visible.width, windowWidth - l.margin) - l.cardWidth;
    const left = Math.round(Math.min(Math.max(aim - l.cardWidth / 2, minLeft), Math.max(minLeft, maxLeft)));
    // The pointer keeps aiming at the service, clear of the rounded corners.
    const tailLeft = Math.min(Math.max(aim - 20, left + 14), left + l.cardWidth - 14 - 40);
    const tailTop = top - l.pointerWidth;

    body.style.left = `${left}px`;
    body.style.top = `${top}px`;
    tail.style.left = `${tailLeft}px`;
    tail.style.top = `${tailTop}px`;
    arrive(tail, { body: [left, top], tail: [tailLeft, tailTop] }, [tailLeft + 20, tailTop], was, faded);

    // The gap between rail and card counts as the card, so crossing it
    // keeps the card open.
    const railBottom = l.rail.y + l.rail.height;
    reportHitRects({ x: left, y: railBottom, width: l.cardWidth, height: top + body.offsetHeight - railBottom });
    return;
  }

  const bodyLeft =
    l.side === "right" ? l.rail.x - l.gap - l.pointerWidth - l.cardWidth : l.rail.x + l.rail.width + l.gap + l.pointerWidth;
  const tailLeft = l.side === "right" ? bodyLeft + l.cardWidth : bodyLeft - l.pointerWidth;

  const height = body.offsetHeight;
  const aim = aimAlong(hovered!);
  const windowHeight = document.documentElement.clientHeight;
  // Centred on the ring, pushed back inside whatever part of the window is
  // on screen.
  const minTop = Math.max(l.visible.y, l.margin);
  const maxTop = Math.min(l.visible.y + l.visible.height, windowHeight - l.margin) - height;
  const top = Math.round(Math.min(Math.max(aim - height / 2, minTop), Math.max(minTop, maxTop)));
  // The pointer keeps aiming at the ring, clear of the rounded corners.
  const tailTop = Math.min(Math.max(aim - 20, top + 14), top + height - 14 - 40);

  body.style.left = `${bodyLeft}px`;
  body.style.top = `${top}px`;
  tail.style.left = `${tailLeft}px`;
  tail.style.top = `${tailTop}px`;
  const tip: Point = [l.side === "right" ? tailLeft + l.pointerWidth : tailLeft, tailTop + 20];
  arrive(tail, { body: [bodyLeft, top], tail: [tailLeft, tailTop] }, tip, was, faded);

  // The gap between card and rail counts as the card, so crossing it keeps
  // the card open.
  const reach: Rect =
    l.side === "right"
      ? { x: bodyLeft, y: top, width: l.rail.x - bodyLeft, height }
      : { x: l.rail.x + l.rail.width, y: top, width: bodyLeft + l.cardWidth - (l.rail.x + l.rail.width), height };
  reportHitRects(reach);
}

/**
 * The card has been put `at` its place, its `tail` pointing at `tip`. Closed
 * before, it grows out of its pointer; open, it moves there from where it
 * was (`was`), the pointer along it too where a corner pushed it; and on its
 * way out, it comes back from however far it had faded.
 */
function arrive(tail: SVGSVGElement, at: NonNullable<typeof cardAt>, tip: Point, was: typeof cardAt, faded: number | null) {
  cardAt = at;
  const timing = { duration: CARD_OPEN_MS, easing: CARD_EASE };
  if (faded != null) {
    card.animate([{ opacity: faded }, { opacity: 1 }], timing);
    return;
  }
  if (!was) {
    for (const animation of card.getAnimations()) animation.cancel();
    card.style.transformOrigin = `${tip[0]}px ${tip[1]}px`;
    card.animate(
      reducedMotion.matches ? [{ opacity: 0 }, { opacity: 1 }] : [{ opacity: 0, transform: "scale(0.94)" }, { opacity: 1, transform: "none" }],
      timing,
    );
    return;
  }
  if (reducedMotion.matches) return;
  const slide = (element: Element, [x, y]: Point, composite: CompositeOperation) => {
    if (x || y) element.animate([{ transform: `translate(${x}px, ${y}px)` }, { transform: "none" }], { duration: CARD_MOVE_MS, easing: CARD_EASE, composite });
  };
  const moved: Point = [was.body[0] - at.body[0], was.body[1] - at.body[1]];
  // Added to whatever it is doing already: still growing in, or on its way
  // from the service before.
  slide(card, moved, "add");
  slide(tail, [was.tail[0] - at.tail[0] - moved[0], was.tail[1] - at.tail[1] - moved[1]], "replace");
}

/** Takes the card away: fading out, or at once when the rail is carried off. */
function hideCard(fade: boolean) {
  cardAt = null;
  if (card.hidden || (fade && cardLeaving)) return;
  const gone = () => {
    cardLeaving = null;
    card.hidden = true;
    card.replaceChildren();
    for (const animation of card.getAnimations()) animation.cancel();
  };
  if (!fade) {
    gone();
    return;
  }
  // From however far in it has come.
  const leaving = card.animate([{ opacity: 0 }], { duration: CARD_CLOSE_MS, easing: "ease-in", fill: "forwards" });
  cardLeaving = leaving;
  leaving.onfinish = () => {
    if (cardLeaving === leaving) gone();
  };
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
  if (!press?.dragging && expanded && Date.now() - openedAt >= CARD_AFTER_OPEN_MS) open(indexAt(event));
});

card.addEventListener("mousemove", () => window.clearTimeout(closeTimer));

rail.addEventListener("mousedown", (event) => {
  if (event.button !== 0) return;
  press = { x: event.screenX, y: event.screenY, index: indexAt(event), dragging: false };
});

// The flare and the sliver carry the rail too, but a click there is on no ring.
surface.addEventListener("mousedown", (event) => {
  if (event.button !== 0) return;
  press = { x: event.screenX, y: event.screenY, index: null, dragging: false };
});

window.addEventListener("mousemove", (event) => {
  if (!press || press.dragging || (event.buttons & 1) === 0) return;
  if (Math.hypot(event.screenX - press.x, event.screenY - press.y) < DRAG_THRESHOLD) return;
  // Past the threshold it is a drag, not a click: Rust carries the rail from
  // here, fusing it to an edge it comes near and pulling it off one, and
  // lands it where the pointer lets go (`panel::carry`).
  press.dragging = true;
  hovered = null;
  renderCard();
  void invoke("panel_drag");
});

window.addEventListener("mouseup", (event) => {
  const pressed = press;
  press = null;
  if (!pressed || pressed.dragging || event.button !== 0 || pressed.index == null) return;
  // A click on a ring asks every account it stands for now.
  const account = railAccounts()[pressed.index];
  if (account) for (const each of behind(account)) void invoke("refresh", { account: each.id });
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
  // The whole page is the rail, card included.
  if (snapshot) {
    document.documentElement.dataset.railTheme = railScheme(snapshot);
    applyGlass(document.documentElement, snapshot);
  }
  renderRail();
  drawSurface();
  renderCard();
}

/** Open for a moment, then tuck away unless the pointer came. */
function intro() {
  introUntil = Date.now() + INTRO_MS;
  window.setTimeout(updateOpen, INTRO_MS + 20);
}

/**
 * Tells Rust once `drawn` is on screen: a window that moved under a rail now
 * drawn somewhere else in it is out of sight until then. That is two frames
 * after the page has the window's size, as a frame is not always up by the
 * next one.
 */
function reportDrawn(drawn: Layout) {
  let frames = 0;
  let sized = 0;
  const next = () => {
    if (Math.abs(innerWidth - drawn.width) <= 1 && Math.abs(innerHeight - drawn.height) <= 1) sized++;
    if (sized === 2) void invoke("layout_drawn", { generation: drawn.generation });
    // Never that size, and Rust stops waiting on its own.
    else if (++frames < 60) requestAnimationFrame(next);
  };
  requestAnimationFrame(next);
}

await listen<Snapshot>("snapshot", (event) => {
  snapshot = event.payload;
  if (hovered != null && hovered >= railAccounts().length) hovered = null;
  // Tucking away may just have been switched on or off.
  updateOpen();
  render();
});

// A new layout is the panel shown, resized, or carried somewhere and landed.
await listen<Layout>("layout", (event) => {
  const next = event.payload;
  const turned = layout != null && (layout.side === "top") !== (next.side === "top");
  const refused = layout?.dock !== next.dock;
  // Carried, it turns from wherever it is drawn now, part way through
  // another turn included.
  const from = turned && next.carried ? { body: bodyNow(), parts: partCentres() } : null;
  layout = next;
  lastHitRects = "";
  if (!next.carried) {
    // A drag ends where it lands, whether or not its release reached the page.
    if (press?.dragging) press = null;
    intro();
  }
  // Put somewhere rather than carried there, it is simply there: it did not
  // open, fuse or turn. The window may be out of sight until it is drawn so.
  if (!next.carried && (refused || turned)) settle();
  else updateOpen();
  if (from) turning = null;
  render();
  if (from) beginTurn(from.body, from.parts);
  animate();
  reportDrawn(layout);
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
settle();
render();
if (layout) reportDrawn(layout);

// "As of 5 minutes ago" has to keep up with the clock while a card is open.
window.setInterval(() => {
  if (hovered != null) renderCard();
}, 30_000);
