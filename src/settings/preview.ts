// The live preview: the rail itself, drawn by the code that draws the panel,
// at a size you can read — and a legend saying what each ring is showing.
// It follows every setting as it changes, so its arcs travel to their new
// length and its figures count there.

import { MousePointerClick } from "lucide";

import { colours, isSpent, percentValue, shownFraction, tint, windowName } from "../shared/format";
import { locale, t } from "../shared/i18n";
import { ITEM_SPACING, RAIL_PAD_BOTTOM, RAIL_PAD_TOP, RAIL_WIDTH, itemHeight, ringItem, shownWindows } from "../shared/rail";
import { applyGlass } from "../shared/theme";
import type { AccountView, RingShows, Snapshot, UsageWindow } from "../shared/types";
import { countTo, dashLength, drawIn, el, icon, stageHead } from "./dom";

/** Large enough to read, small enough that two services still fit. */
const MAX_ZOOM = 2.4;

function duration(ms: number): string {
  const minutes = Math.max(1, Math.round(ms / 60_000));
  return minutes < 60 ? t("durationMinutes", String(minutes)) : t("durationHours", String(Math.floor(minutes / 60)), String(minutes % 60));
}

/** "resets in 2h 14m" within a day, "resets Mon 09:00" beyond one. */
function resetLine(window: UsageWindow): string {
  if (window.resetsAt == null) return "";
  const ms = window.resetsAt - Date.now();
  if (ms <= 0) return "";
  if (ms < 86_400_000) return t("resetsIn", duration(ms));
  const when = new Intl.DateTimeFormat(locale, { weekday: "short", hour: "numeric", minute: "2-digit" }).format(new Date(window.resetsAt));
  return t("resetsOn", when);
}

interface LegendRow {
  element: HTMLElement;
  swatch: HTMLElement;
  name: HTMLElement;
  reset: HTMLElement;
  figure: HTMLElement;
}

export interface Preview {
  element: HTMLElement;
  apply(snapshot: Snapshot): void;
  /** Keeps "resets in 2h 14m" in step with the clock. */
  tick(): void;
}

export function preview(): Preview {
  const rail = el("div", "preview-rail rail-face");
  rail.setAttribute("role", "list");
  const note = el("p", "preview-note");
  const box = el("div", "preview-box", rail, note);
  const legend = el("div", "legend");
  legend.setAttribute("aria-live", "polite");
  const hint = el("div", "stage-hint", icon(MousePointerClick, 16), el("span", undefined, t("previewHint")));
  const element = el("aside", "stage preview-stage", stageHead(t("livePreview"), t("previewUpdates")), box, legend, hint);

  let railHeight = 0;
  let lastShows: RingShows | null = null;
  let last: Snapshot | null = null;
  const rows = new Map<string, LegendRow>();
  const captions = new Map<string, HTMLElement>();

  function fit() {
    if (!railHeight || !box.clientHeight) return;
    const room = Math.min((box.clientHeight - 24) / railHeight, (box.clientWidth - 24) / RAIL_WIDTH);
    rail.style.zoom = String(Math.max(1, Math.min(MAX_ZOOM, room)));
  }
  new ResizeObserver(fit).observe(box);

  function renderRail(accounts: AccountView[], snapshot: Snapshot) {
    const settings = snapshot.settings;
    const height = itemHeight(settings.ringShows);

    // Where each arc and figure stood, so they travel from there rather
    // than from nothing. A different kind of ring starts over.
    const before = new Map<string, number>();
    if (lastShows === settings.ringShows) {
      for (const circle of rail.querySelectorAll<SVGCircleElement>("circle.usage")) {
        if (circle.dataset.key) before.set(circle.dataset.key, dashLength(circle));
      }
      for (const figure of rail.querySelectorAll<HTMLElement>("[data-figure]")) {
        before.set(figure.dataset.figure!, Number(figure.dataset.value));
      }
    }
    lastShows = settings.ringShows;

    rail.replaceChildren(
      ...accounts.map((account, index) => {
        const item = ringItem(account, settings);
        item.style.top = `${RAIL_PAD_TOP + index * (height + ITEM_SPACING)}px`;
        item.style.height = `${height}px`;
        item.querySelectorAll(".ring").forEach((ring, ringIndex) => {
          for (const circle of ring.querySelectorAll<SVGCircleElement>("circle.usage")) {
            const key = `${account.provider}:${ringIndex}:${circle.getAttribute("transform")}`;
            circle.dataset.key = key;
            const from = before.get(key) ?? 0;
            if (Math.abs(from - dashLength(circle)) > 0.5) drawIn(circle, from, 0, 800);
          }
        });
        // The figure under a ring is its text after any letter; it counts
        // in step with its arc.
        item.querySelectorAll(".label").forEach((label, labelIndex) => {
          const text = label.lastChild;
          const value = text?.nodeType === Node.TEXT_NODE ? /^(\d+)%$/.exec(text.textContent ?? "") : null;
          if (!text || !value) return;
          const figure = el("span");
          const key = `${account.provider}:${labelIndex}`;
          figure.dataset.figure = key;
          text.replaceWith(figure);
          countTo(figure, Number(value[1]), (shown) => `${shown}%`, before.get(key) ?? 0);
        });
        return item;
      }),
    );
    railHeight = RAIL_PAD_TOP + accounts.length * height + Math.max(accounts.length - 1, 0) * ITEM_SPACING + RAIL_PAD_BOTTOM;
    rail.style.height = `${railHeight}px`;
    fit();
  }

  function legendRow(key: string): LegendRow {
    let row = rows.get(key);
    if (!row) {
      const swatch = el("span", "swatch");
      const name = el("div", "legend-name");
      const reset = el("div", "legend-reset");
      const figure = el("div", "legend-figure");
      row = { element: el("div", "legend-row", swatch, el("div", "legend-text", name, reset), figure), swatch, name, reset, figure };
      rows.set(key, row);
    }
    return row;
  }

  function fill(row: LegendRow, window: UsageWindow, snapshot: Snapshot) {
    const settings = snapshot.settings;
    row.swatch.style.background = isSpent(window) ? colours.exhausted : tint(window, settings.warningAt);
    row.name.textContent = windowName(window);
    row.reset.textContent = resetLine(window);
    const figure = percentValue(isSpent(window) ? 1 : shownFraction(window, settings.showsRemaining));
    countTo(row.figure, figure, (value) => `${value}%`, row.figure.dataset.value ? undefined : 0);
    row.figure.classList.toggle("spent", isSpent(window));
  }

  function renderLegend(accounts: AccountView[], snapshot: Snapshot) {
    const children: HTMLElement[] = [];
    const used = new Set<string>();
    for (const account of accounts) {
      const windows = shownWindows(account, snapshot.settings.ringShows).filter((window): window is UsageWindow => !!window);
      if (accounts.length > 1) {
        const caption = captions.get(account.provider) ?? el("div", "legend-caption");
        caption.textContent = account.name;
        captions.set(account.provider, caption);
        children.push(caption);
      }
      for (const window of windows) {
        const key = `${account.provider}:${window.id}`;
        used.add(key);
        const row = legendRow(key);
        fill(row, window, snapshot);
        children.push(row.element);
      }
    }
    for (const key of [...rows.keys()]) if (!used.has(key)) rows.delete(key);
    legend.replaceChildren(...children);
  }

  function apply(snapshot: Snapshot) {
    last = snapshot;
    const settings = snapshot.settings;
    const accounts = settings.hasChosen ? snapshot.accounts.filter((account) => account.enabled) : [];
    element.classList.toggle("empty", accounts.length === 0);
    element.classList.toggle("hidden-panel", accounts.length > 0 && !settings.panelVisible);
    note.textContent = accounts.length === 0 ? t("previewEmpty") : settings.panelVisible ? "" : t("previewHidden");
    // The page's theme, or dark when the rail is kept dark. Left to follow
    // the page, it changes with it rather than a moment before.
    if (settings.railStaysDark) rail.dataset.railTheme = "dark";
    else delete rail.dataset.railTheme;
    applyGlass(rail, snapshot);
    renderRail(accounts, snapshot);
    renderLegend(accounts, snapshot);
  }

  return {
    element,
    apply,
    tick() {
      if (!last) return;
      for (const account of last.accounts) {
        for (const window of account.usage.windows) {
          const row = rows.get(`${account.provider}:${window.id}`);
          if (row) row.reset.textContent = resetLine(window);
        }
      }
    },
  };
}
