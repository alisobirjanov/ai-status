// The floating panel: a rail of rings, and a card for the ring under the
// pointer. It draws what Rust sends and reports what it drew (so the window
// takes clicks exactly there) and what was clicked. Where the pointer is,
// Rust decides: this window lets clicks through everywhere else, so it would
// never hear the pointer leave.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { colours, isSpent, percentText, reasonText, relative, resetText, shownFraction, tint, windowName } from "../shared/format";
import { t } from "../shared/i18n";
import { icon } from "../shared/icons";
import type { AccountView, Layout, Rect, Snapshot, UsageWindow } from "../shared/types";
import "./panel.css";

const SVG_NS = "http://www.w3.org/2000/svg";
const RING = 40;
const RADIUS = 18;
const STROKE = 4;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;
/** Leaving is not closing at once: a pointer crossing to the card must not flicker it. */
const CLOSE_DELAY_MS = 140;
const DRAG_THRESHOLD = 4;

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

function railAccounts(): AccountView[] {
  return snapshot?.accounts.filter((account) => account.enabled) ?? [];
}

function itemTop(index: number): number {
  const l = layout!;
  return l.padTop + index * (l.itemHeight + l.itemSpacing);
}

/** The limit closest to biting — the one the ring shows. */
function headline(account: AccountView): UsageWindow | undefined {
  return account.usage.windows.reduce<UsageWindow | undefined>(
    (fullest, window) => (!fullest || window.usedFraction > fullest.usedFraction ? window : fullest),
    undefined,
  );
}

// MARK: - The rail

function svg<K extends keyof SVGElementTagNameMap>(tag: K, attributes: Record<string, string | number>): SVGElementTagNameMap[K] {
  const element = document.createElementNS(SVG_NS, tag);
  for (const [name, value] of Object.entries(attributes)) element.setAttribute(name, String(value));
  return element;
}

function ringItem(account: AccountView, index: number): HTMLElement {
  const settings = snapshot!.settings;
  const window = headline(account);
  const spent = isSpent(window);

  const item = document.createElement("div");
  item.className = "item";
  item.style.top = `${itemTop(index)}px`;
  item.setAttribute("role", "listitem");

  const ring = document.createElement("div");
  ring.className = account.refreshing ? "ring refreshing" : "ring";

  const gauge = svg("svg", { class: "gauge", width: RING, height: RING, viewBox: `0 0 ${RING} ${RING}` });
  gauge.appendChild(svg("circle", { cx: 20, cy: 20, r: RADIUS, fill: "none", stroke: "rgba(255,255,255,0.14)", "stroke-width": STROKE }));

  if (window) {
    // Spent fills the ring whichever way the figure is counted.
    const fraction = spent ? 1 : Math.min(Math.max(shownFraction(window, settings.showsRemaining), 0), 1);
    if (fraction > 0) {
      const arc = svg("circle", {
        class: "usage",
        cx: 20,
        cy: 20,
        r: RADIUS,
        fill: "none",
        stroke: tint(window, settings.warningAt),
        "stroke-width": STROKE,
        "stroke-linecap": "round",
        transform: "rotate(-90 20 20)",
      });
      if (fraction < 1) arc.setAttribute("stroke-dasharray", `${CIRCUMFERENCE * fraction} ${CIRCUMFERENCE}`);
      gauge.appendChild(arc);
    }
  }

  if (account.refreshing) {
    gauge.appendChild(
      svg("circle", {
        class: "turning",
        cx: 20,
        cy: 20,
        r: RADIUS,
        fill: "none",
        stroke: "rgba(255,255,255,0.85)",
        "stroke-width": STROKE,
        "stroke-linecap": "round",
        "stroke-dasharray": `${CIRCUMFERENCE * 0.18} ${CIRCUMFERENCE}`,
      }),
    );
  }
  ring.appendChild(gauge);

  const mark = document.createElement("div");
  mark.className = "mark";
  mark.appendChild(icon(account.provider, 16));
  ring.appendChild(mark);
  item.appendChild(ring);

  const label = document.createElement("div");
  label.className = "label";
  label.textContent = window ? percentText(window, settings.showsRemaining) : "–";
  if (spent) label.style.color = colours.exhausted;
  item.appendChild(label);

  item.setAttribute(
    "aria-label",
    window ? `${account.name}: ${percentText(window, settings.showsRemaining)}` : account.name,
  );
  return item;
}

function renderRail() {
  if (!layout || !snapshot) return;
  const { rail: frame } = layout;
  Object.assign(rail.style, {
    left: `${frame.x}px`,
    top: `${frame.y}px`,
    width: `${frame.width}px`,
    height: `${frame.height}px`,
  });
  rail.replaceChildren(...railAccounts().map(ringItem));
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

function renderCard() {
  const account = hovered == null ? undefined : railAccounts()[hovered];
  if (!account || !layout || press?.dragging) {
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
  const ringCenter = l.rail.y + itemTop(hovered!) + RING / 2;
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
  const rects = cardRect ? [layout.rail, cardRect] : [layout.rail];
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
  if (!press?.dragging) open(indexAt(event.clientY));
});

card.addEventListener("mousemove", () => window.clearTimeout(closeTimer));

rail.addEventListener("mousedown", (event) => {
  if (event.button !== 0) return;
  press = { x: event.screenX, y: event.screenY, index: indexAt(event.clientY), dragging: false };
});

window.addEventListener("mousemove", (event) => {
  if (!press || press.dragging || (event.buttons & 1) === 0) return;
  if (Math.hypot(event.screenX - press.x, event.screenY - press.y) < DRAG_THRESHOLD) return;
  // Past the threshold it is a drag, not a click. Windows runs the move
  // itself from here, and the button's release never reaches the page.
  press.dragging = true;
  hovered = null;
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

rail.addEventListener("contextmenu", (event) => {
  event.preventDefault();
  void invoke("panel_menu");
});
// No browser menu anywhere on the panel.
document.addEventListener("contextmenu", (event) => event.preventDefault());

// MARK: - From Rust

function render() {
  renderRail();
  renderCard();
}

await listen<Snapshot>("snapshot", (event) => {
  snapshot = event.payload;
  if (hovered != null && hovered >= railAccounts().length) hovered = null;
  render();
});

await listen<Layout>("layout", (event) => {
  layout = event.payload;
  lastHitRects = "";
  render();
});

await listen<boolean>("pointer", (event) => {
  if (event.payload) {
    window.clearTimeout(closeTimer);
  } else {
    press = null;
    closeSoon();
  }
});

snapshot = await invoke<Snapshot>("get_snapshot");
layout = await invoke<Layout | null>("get_layout");
render();

// "As of 5 minutes ago" has to keep up with the clock while a card is open.
window.setInterval(() => {
  if (hovered != null) renderCard();
}, 30_000);
