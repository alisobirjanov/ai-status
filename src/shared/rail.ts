// One service on the rail: its ring or rings and their figures. The floating
// panel draws these, and so does the live preview in Settings, so what the
// preview shows is the rail itself rather than a picture of it.

import { colours, isSpent, percentText, shownFraction, tint, windowName } from "./format";
import { t } from "./i18n";
import { icon } from "./icons";
import type { AccountView, RingShows, Settings, UsageWindow, WindowKind } from "./types";

const SVG_NS = "http://www.w3.org/2000/svg";
export const RING = 40;
const RADIUS = 18;
const STROKE = 4;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

// The rail's budgets in logical pixels. They mirror `panel.rs`, which sizes
// the panel window from the same numbers.
export const RAIL_WIDTH = 64;
export const RAIL_PAD_TOP = 16;
export const RAIL_PAD_BOTTOM = 14;
export const ITEM_SPACING = 22;
const ITEM_HEIGHT = 62;
const LABEL_HEIGHT = 16;
const LABEL_GAP = 6;
const PAIR_GAP = 8;

/** How tall one service is on the rail. Mirrors `Items::of` in `panel.rs`. */
export function itemHeight(shows: RingShows): number {
  switch (shows) {
    case "bothSplit":
      return ITEM_HEIGHT + LABEL_GAP + LABEL_HEIGHT;
    case "bothNested":
      return ITEM_HEIGHT + LABEL_HEIGHT;
    case "bothStacked":
      return 2 * ITEM_HEIGHT + PAIR_GAP;
    default:
      return ITEM_HEIGHT;
  }
}

/** Where on the ring a limit is drawn. */
interface Shape {
  radius: number;
  stroke: number;
  /** Where the arc begins, in degrees clockwise from three o'clock. */
  start: number;
  /** How far round it goes when full. */
  span: number;
  /** Turned upside down, so the bottom half fills from the left as the top half does. */
  mirrored?: boolean;
}

/** Between the halves of a split ring, round caps included. */
const SPLIT_GAP = 36;
const WHOLE: Shape = { radius: RADIUS, stroke: STROKE, start: -90, span: 360 };
const TOP_HALF: Shape = { radius: RADIUS, stroke: STROKE, start: 180 + SPLIT_GAP / 2, span: 180 - SPLIT_GAP };
const BOTTOM_HALF: Shape = { ...TOP_HALF, mirrored: true };
const INNER: Shape = { radius: 11.5, stroke: 3, start: -90, span: 360 };

/** The limit closest to biting, or the fullest of one kind. */
export function fullest(account: AccountView, kind?: WindowKind): UsageWindow | undefined {
  return account.usage.windows
    .filter((window) => !kind || window.kind === kind)
    .reduce<UsageWindow | undefined>(
      (fullest, window) => (!fullest || window.usedFraction > fullest.usedFraction ? window : fullest),
      undefined,
    );
}

/**
 * What the ring shows, top or outer first: the limit chosen, falling back to
 * the fullest; or for both, the 5-hour and the weekly limit, either of which
 * may be missing. Mirrors `RingShows::windows` in `settings.rs`.
 */
export function shownWindows(account: AccountView, shows: RingShows): (UsageWindow | undefined)[] {
  const headline = fullest(account);
  switch (shows) {
    case "fullest":
      return [headline];
    case "fiveHour":
    case "weekly":
      return [fullest(account, shows) ?? headline];
    default: {
      const pair = [fullest(account, "fiveHour"), fullest(account, "weekly")];
      // Something other than either is shown on its own rather than hidden.
      return !pair[0] && !pair[1] && headline ? [headline] : pair;
    }
  }
}

export function svg<K extends keyof SVGElementTagNameMap>(tag: K, attributes: Record<string, string | number>): SVGElementTagNameMap[K] {
  const element = document.createElementNS(SVG_NS, tag);
  for (const [name, value] of Object.entries(attributes)) element.setAttribute(name, String(value));
  return element;
}

/** A track, or the part of it a limit has used, on a circle round the mark. */
function arc(shape: Shape, attributes: Record<string, string | number>, fraction = 1): SVGCircleElement {
  const circumference = 2 * Math.PI * shape.radius;
  let transform = `rotate(${shape.start} 20 20)`;
  if (shape.mirrored) transform = `translate(0 ${RING}) scale(1 -1) ${transform}`;
  const circle = svg("circle", {
    cx: 20,
    cy: 20,
    r: shape.radius,
    fill: "none",
    "stroke-width": shape.stroke,
    "stroke-linecap": "round",
    transform,
    ...attributes,
  });
  // A whole circle has no dash, or its round caps would meet in a seam.
  if (fraction * shape.span < 360) {
    circle.setAttribute("stroke-dasharray", `${(circumference * shape.span * fraction) / 360} ${circumference}`);
  }
  return circle;
}

/** A gauge for each limit given, in its own part of the ring. */
function ring(account: AccountView, settings: Settings, arcs: [UsageWindow | undefined, Shape][], markSize = 16): HTMLElement {
  const ring = document.createElement("div");
  ring.className = account.refreshing ? "ring refreshing" : "ring";

  const gauge = svg("svg", { class: "gauge", width: RING, height: RING, viewBox: `0 0 ${RING} ${RING}` });
  for (const [window, shape] of arcs) {
    gauge.appendChild(arc(shape, { class: "track" }));
    if (!window) continue;
    // Spent fills the ring whichever way the figure is counted.
    const fraction = isSpent(window) ? 1 : Math.min(Math.max(shownFraction(window, settings.showsRemaining), 0), 1);
    if (fraction > 0) gauge.appendChild(arc(shape, { class: "usage", style: `stroke: ${tint(window, settings.warningAt)}` }, fraction));
  }

  if (account.refreshing) {
    gauge.appendChild(
      svg("circle", {
        class: "turning",
        cx: 20,
        cy: 20,
        r: RADIUS,
        fill: "none",
        "stroke-width": STROKE,
        "stroke-linecap": "round",
        "stroke-dasharray": `${CIRCUMFERENCE * 0.18} ${CIRCUMFERENCE}`,
      }),
    );
  }
  ring.appendChild(gauge);

  const mark = document.createElement("div");
  mark.className = "mark";
  mark.appendChild(icon(account.provider, markSize));
  ring.appendChild(mark);
  return ring;
}

/** A ring's figure, after a letter saying which limit it is when there are two. */
function label(window: UsageWindow | undefined, settings: Settings, letter?: string): HTMLElement {
  const label = document.createElement("div");
  label.className = "label";
  label.textContent = window ? percentText(window, settings.showsRemaining) : "–";
  if (letter) {
    const tag = document.createElement("span");
    tag.className = "letter";
    tag.textContent = letter;
    label.prepend(tag);
  }
  if (isSpent(window)) label.style.color = colours.exhausted;
  return label;
}

/** One service's rings and figures, laid out as `ringShows` asks. Placing it is the caller's. */
export function ringItem(account: AccountView, settings: Settings): HTMLElement {
  const shown = shownWindows(account, settings.ringShows);

  const item = document.createElement("div");
  item.className = "item";
  item.setAttribute("role", "listitem");

  const [first, second] = shown;
  if (shown.length === 1) {
    item.append(ring(account, settings, [[first, WHOLE]]), label(first, settings));
    item.setAttribute("aria-label", first ? `${account.name}: ${percentText(first, settings.showsRemaining)}` : account.name);
    return item;
  }

  const [firstLabel, secondLabel] = settings.limitLetters
    ? [label(first, settings, t("fiveHourLetter")), label(second, settings, t("weeklyLetter"))]
    : [label(first, settings), label(second, settings)];
  switch (settings.ringShows) {
    case "bothSplit":
      item.classList.add("split");
      item.append(firstLabel, ring(account, settings, [[first, TOP_HALF], [second, BOTTOM_HALF]]), secondLabel);
      break;
    case "bothNested":
      item.classList.add("nested");
      item.append(ring(account, settings, [[first, WHOLE], [second, INNER]], 14), firstLabel, secondLabel);
      break;
    default:
      item.classList.add("stacked");
      item.append(ring(account, settings, [[first, WHOLE]]), firstLabel, ring(account, settings, [[second, WHOLE]]), secondLabel);
  }
  const figures = shown.filter((window) => window).map((window) => `${windowName(window!)} ${percentText(window!, settings.showsRemaining)}`);
  item.setAttribute("aria-label", [account.name, ...figures].join(", "));
  return item;
}
