// What the settings pages are built from: elements, icons, switches,
// segmented choices, the arcs of Dipstick's own mark, and figures that count
// their way to a new value.

import { createElement, type IconNode } from "lucide";

import { t } from "../shared/i18n";
import { svg } from "../shared/rail";

/** Nothing slides, counts or draws itself in for somebody who asked Windows for less motion. */
export const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

export const EASE = "cubic-bezier(0.2, 0.7, 0.2, 1)";

export type Child = Node | string | null | undefined | false;

export function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, ...children: Child[]): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  for (const child of children) if (child) node.append(child);
  return node;
}

export function button(className: string, ...children: Child[]): HTMLButtonElement {
  const node = el("button", className, ...children);
  node.type = "button";
  return node;
}

export function icon(node: IconNode, size = 16): SVGElement {
  return createElement(node, { width: size, height: size, "stroke-width": 1.75, "aria-hidden": "true" });
}

// MARK: - Parts every page uses

export function sectionHead(title: string, note?: string): HTMLElement {
  return el("div", "section-head", el("h2", undefined, title), note ? el("p", "section-note", note) : null);
}

export function stageHead(left: string, right: string): HTMLElement {
  return el("div", "stage-head", el("span", undefined, left), el("span", undefined, right));
}

export function soonBadge(): HTMLElement {
  return el("span", "soon-badge", t("soon"));
}

/** A title and what it does, for the left of a row or the top of a card. */
export function heading(title: string | Node, hint?: string | Node): HTMLElement {
  return el("div", "heading", el("h3", undefined, title), hint ? el("p", "hint", hint) : null);
}

/** One line of a card with dividers: what it is on the left, its control on the right. */
export function row(title: string | Node, hint: string | Node | null, control: Node | null): HTMLElement {
  return el("div", "row", heading(title, hint ?? undefined), control);
}

/** Staggers a page's entrance: each part arrives a moment after the one before. */
export function reveal<T extends HTMLElement>(element: T, index: number): T {
  element.classList.add("reveal");
  element.style.setProperty("--i", String(index));
  return element;
}

// MARK: - Switches

export interface Switch {
  element: HTMLButtonElement;
  set(on: boolean): void;
  /** Greyed out: for what is not there yet, or does not apply. */
  disable(disabled: boolean, why?: string): void;
  /** Looks as it is but will not move: a click shakes it and calls `refused`. */
  hold(held: boolean, why: string, refused?: () => void): void;
}

export function toggle(label: string, onChange?: (on: boolean) => void): Switch {
  const control = button("toggle", el("span", "knob"));
  control.setAttribute("role", "switch");
  control.setAttribute("aria-label", label);
  control.setAttribute("aria-checked", "false");
  let held: (() => void) | null = null;
  control.addEventListener("click", () => {
    if (held) {
      control.classList.remove("refused");
      void control.offsetWidth;
      control.classList.add("refused");
      held();
      return;
    }
    const on = control.getAttribute("aria-checked") !== "true";
    control.setAttribute("aria-checked", String(on));
    onChange?.(on);
  });
  return {
    element: control,
    set: (on) => control.setAttribute("aria-checked", String(on)),
    disable(disabled, why) {
      control.disabled = disabled;
      control.title = disabled && why ? why : "";
    },
    hold(on, why, refused = () => undefined) {
      held = on ? refused : null;
      if (on) control.setAttribute("aria-disabled", "true");
      else control.removeAttribute("aria-disabled");
      control.title = on ? why : "";
    },
  };
}

export interface Tile {
  element: HTMLElement;
  switch: Switch;
  hint: HTMLElement;
  disable(disabled: boolean, hint?: string): void;
}

/** A switch with room to say what it does. The whole tile flips it. */
export function toggleTile(glyph: IconNode, title: string, hint: string, onChange?: (on: boolean) => void): Tile {
  const control = toggle(title, onChange);
  const hintLine = el("p", "hint", hint);
  const tile = el(
    "div",
    "card tile",
    el("div", "tile-top", el("span", "tile-icon", icon(glyph, 20)), control.element),
    el("h3", undefined, title),
    hintLine,
  );
  tile.addEventListener("click", (event) => {
    if (!control.element.contains(event.target as Node) && !control.element.disabled) control.element.click();
  });
  return {
    element: tile,
    switch: control,
    hint: hintLine,
    disable(disabled, why) {
      control.disable(disabled);
      tile.classList.toggle("disabled", disabled);
      hintLine.textContent = disabled && why ? why : hint;
    },
  };
}

// MARK: - Choices

/** Arrow keys move along a row of radios or tabs and choose as they go, as Windows' own do. */
export function arrowKeys(container: HTMLElement, items: () => HTMLElement[]) {
  container.addEventListener("keydown", (event) => {
    const list = items().filter((item) => !(item as HTMLButtonElement).disabled);
    const index = list.indexOf(document.activeElement as HTMLElement);
    if (index < 0) return;
    let next: number;
    switch (event.key) {
      case "ArrowRight":
      case "ArrowDown":
        next = (index + 1) % list.length;
        break;
      case "ArrowLeft":
      case "ArrowUp":
        next = (index - 1 + list.length) % list.length;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = list.length - 1;
        break;
      default:
        return;
    }
    event.preventDefault();
    list[next].focus();
    list[next].click();
  });
}

/** Marks one of a set chosen, and only that one reachable by Tab. */
export function check(items: Iterable<HTMLElement>, chosen: HTMLElement | undefined, attribute = "aria-checked") {
  for (const item of items) {
    const on = item === chosen;
    item.setAttribute(attribute, String(on));
    item.tabIndex = on ? 0 : -1;
  }
}

/**
 * A pill that slides under whichever of its row is chosen. It is measured,
 * not guessed, so it fits labels in either language; a row that is not on
 * screen yet is measured when it arrives.
 */
export function slidingThumb(row: HTMLElement, thumb: HTMLElement, chosen: () => HTMLElement | undefined): () => void {
  let placed = false;
  function place() {
    const target = chosen();
    if (!target || !target.offsetWidth) {
      thumb.style.opacity = "0";
      return;
    }
    // The first time it is simply there; after that it travels.
    if (!placed) thumb.classList.add("still");
    thumb.style.opacity = "1";
    thumb.style.width = `${target.offsetWidth}px`;
    thumb.style.transform = `translateX(${target.offsetLeft}px)`;
    if (!placed) {
      void thumb.offsetWidth;
      thumb.classList.remove("still");
      placed = true;
    }
  }
  new ResizeObserver(place).observe(row);
  return place;
}

export interface Choice<T> {
  element: HTMLElement;
  set(value: T): void;
  disable(disabled: boolean): void;
}

export function segmented<T extends string | number>(label: string, options: [T, string][], onChange?: (value: T) => void): Choice<T> {
  const group = el("div", "segmented");
  group.setAttribute("role", "radiogroup");
  group.setAttribute("aria-label", label);
  const thumb = el("span", "segmented-thumb");
  group.append(thumb);

  let value: T | undefined;
  const buttons = new Map<T, HTMLButtonElement>();
  for (const [option, text] of options) {
    const segment = button("segment", text);
    segment.setAttribute("role", "radio");
    segment.addEventListener("click", () => {
      if (value === option) return;
      set(option);
      onChange?.(option);
    });
    buttons.set(option, segment);
    group.append(segment);
  }
  const place = slidingThumb(group, thumb, () => (value === undefined ? undefined : buttons.get(value)));
  arrowKeys(group, () => [...buttons.values()]);

  function set(option: T) {
    value = option;
    check(buttons.values(), buttons.get(option));
    place();
  }
  return {
    element: group,
    set,
    disable(disabled) {
      group.classList.toggle("disabled", disabled);
      for (const segment of buttons.values()) segment.disabled = disabled;
    },
  };
}

// MARK: - Figures

const counting = new WeakMap<HTMLElement, number>();

/**
 * Shows a whole number, counting to it from `from` — or from what the
 * element showed last. The text is always a number the reader could have
 * been shown, never a blur.
 */
export function countTo(element: HTMLElement, to: number, format: (value: number) => string, from?: number) {
  cancelAnimationFrame(counting.get(element) ?? 0);
  const start = from ?? Number(element.dataset.value ?? to);
  element.dataset.value = String(to);
  if (reducedMotion.matches || start === to) {
    element.textContent = format(to);
    return;
  }
  const began = performance.now();
  const duration = 650;
  const step = (now: number) => {
    const progress = Math.min((now - began) / duration, 1);
    const eased = 1 - Math.pow(1 - progress, 3);
    element.textContent = format(Math.round(start + (to - start) * eased));
    if (progress < 1) counting.set(element, requestAnimationFrame(step));
  };
  element.textContent = format(start);
  counting.set(element, requestAnimationFrame(step));
}

// MARK: - Arcs

export interface ArcOptions {
  width: number;
  className?: string;
  /** Turned upside down, so a bottom half fills from the left as a top half does. */
  mirrored?: boolean;
  /** Round ends, as the rail draws; the mark has square ones. */
  round?: boolean;
}

/** Part of a circle: `start` degrees clockwise from three o'clock, `sweep` degrees round. */
export function arc(size: number, radius: number, start: number, sweep: number, options: ArcOptions): SVGCircleElement {
  const middle = size / 2;
  const circumference = 2 * Math.PI * radius;
  let transform = `rotate(${start} ${middle} ${middle})`;
  if (options.mirrored) transform = `translate(0 ${size}) scale(1 -1) ${transform}`;
  const circle = svg("circle", {
    cx: middle,
    cy: middle,
    r: radius,
    fill: "none",
    "stroke-width": options.width,
    "stroke-linecap": options.round === false ? "butt" : "round",
    transform,
    "stroke-dasharray": `${(circumference * sweep) / 360} ${circumference}`,
  });
  if (options.className) circle.setAttribute("class", options.className);
  return circle;
}

export function track(size: number, radius: number, width: number, cx = size / 2, cy = size / 2): SVGCircleElement {
  return svg("circle", { cx, cy, r: radius, fill: "none", "stroke-width": width, class: "arc-track" });
}

/** How much of its circle an arc covers, in the units of its dash. */
export function dashLength(circle: SVGCircleElement): number {
  const dash = circle.getAttribute("stroke-dasharray");
  return dash ? Number(dash.split(/[ ,]+/)[0]) : 2 * Math.PI * Number(circle.getAttribute("r"));
}

/** Draws an arc in from `from` (nothing, unless given) to where it is. */
export function drawIn(circle: SVGCircleElement, from = 0, delay = 0, duration = 750) {
  if (reducedMotion.matches) return;
  const circumference = 2 * Math.PI * Number(circle.getAttribute("r"));
  circle.animate([{ strokeDasharray: `${from} ${circumference}` }, { strokeDasharray: `${dashLength(circle)} ${circumference}` }], {
    duration,
    delay,
    easing: EASE,
    fill: "backwards",
  });
}

let marks = 0;

/**
 * Dipstick's mark: two sticks, Claude's orange and Codex's lilac, each
 * filled to a level. The tray icon is the same mark filled to the figures
 * (`tray.rs`); here it is filled to a fixed 62% and 38%.
 */
export function brandMark(size: number): SVGSVGElement {
  const mark = svg("svg", { class: "brand-mark", width: size, height: size, viewBox: "0 0 100 100", "aria-hidden": "true" });
  const defs = svg("defs", {});
  mark.append(defs);
  const id = `brand-mark-${++marks}`;
  for (const [index, [left, level, className]] of ([[14, 0.62, "stick-claude"], [56, 0.38, "stick-codex"]] as const).entries()) {
    const shape = { x: left, y: 5, width: 30, height: 90, rx: 15 };
    const clip = svg("clipPath", { id: `${id}-${index}` });
    clip.append(svg("rect", shape));
    defs.append(clip);
    const fill = svg("g", { "clip-path": `url(#${id}-${index})` });
    fill.append(svg("rect", { ...shape, y: 5 + 90 * (1 - level), rx: 0, class: `stick-fill ${className}` }));
    mark.append(svg("rect", { ...shape, class: "stick-track" }), fill);
  }
  return mark;
}

/** Fills a stick of the mark up from empty to its level. */
export function fillIn(fill: SVGRectElement, delay = 0, duration = 750) {
  if (reducedMotion.matches) return;
  const empty = 95 - Number(fill.getAttribute("y"));
  fill.animate([{ transform: `translateY(${empty}px)` }, { transform: "translateY(0)" }], {
    duration,
    delay,
    easing: EASE,
    fill: "backwards",
  });
}
