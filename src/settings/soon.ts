// Alerts and Shortcuts are not built yet. Their pages show how they will
// look, dimmed and out of reach of pointer and keyboard, under a note that
// says so — nothing on them pretends to work.

import {
  ArrowRight,
  BellRing,
  Check,
  CircleCheck,
  CircleDashed,
  CirclePause,
  ClipboardCopy,
  Globe,
  Moon,
  MousePointer2,
  PanelRight,
  Plus,
  RotateCw,
  SlidersHorizontal,
  Sparkles,
  Sun,
  Undo2,
  Volume2,
  type IconNode,
} from "lucide";

import { locale, t } from "../shared/i18n";
import { svg } from "../shared/rail";
import { arc, button, el, heading, icon, reveal, sectionHead, stageHead, toggle, toggleTile, track } from "./dom";

function banner(message: string): HTMLElement {
  return el("div", "soon-banner", el("span", "soon-banner-icon", icon(Sparkles, 18)), heading(t("soonTitle"), message));
}

function page(stage: HTMLElement, message: string, parts: HTMLElement[]): HTMLElement {
  stage.inert = true;
  const body = el("div", "soon-body", ...parts);
  body.inert = true;
  parts.forEach((part, index) => reveal(part, index + 2));
  return el("section", "page soon", reveal(stage, 0), el("div", "controls", reveal(banner(message), 1), body));
}

function tiles(className: string, ...items: [IconNode, string, string, boolean][]): HTMLElement {
  return el(
    "div",
    className,
    ...items.map(([glyph, title, hint, on]) => {
      const tile = toggleTile(glyph, title, hint);
      tile.switch.set(on);
      return tile.element;
    }),
  );
}

function ringMark(size: number, sweep: number, width: number): SVGSVGElement {
  const mark = svg("svg", { class: "ring-glyph", width: size, height: size, viewBox: `0 0 ${size} ${size}`, "aria-hidden": "true" });
  const radius = (size - width) / 2;
  mark.append(track(size, radius, width), arc(size, radius, -90, sweep, { width, className: "arc-five" }));
  return mark;
}

// MARK: - Alerts

function chip(text: string, tone: "" | "five" | "week"): HTMLElement {
  return el("span", tone ? `chip on ${tone}` : "chip", tone ? icon(Check, 12) : null, text);
}

function days(): HTMLElement {
  // 1 January 2024 was a Monday.
  const format = new Intl.DateTimeFormat(locale, { weekday: "short" });
  return el(
    "div",
    "days",
    ...Array.from({ length: 7 }, (_, index) => {
      const name = format.format(new Date(2024, 0, 1 + index));
      return el("span", index < 5 ? "day on" : "day", name.charAt(0).toUpperCase() + name.slice(1));
    }),
  );
}

export function alertsPage(): HTMLElement {
  const toast = el(
    "div",
    "toast",
    ringMark(34, 270, 4),
    el(
      "div",
      "toast-text",
      el("div", "toast-top", el("span", undefined, "Dipstick"), el("span", undefined, t("now"))),
      el("div", "toast-title", t("toastTitle")),
      el("div", "toast-body", t("toastBody")),
    ),
  );
  const history = (time: string, tone: string, text: string) =>
    el("div", "history-row", el("span", "history-time", time), el("span", `history-dot ${tone}`), el("span", undefined, text));
  const stage = el(
    "aside",
    "stage alerts-stage",
    stageHead(t("livePreview"), t("previewWhat")),
    el("div", "toast-block", el("p", "caption", t("toastLabel")), toast),
    el("div", "flash", el("div", "flash-ring", ringMark(96, 290, 9)), el("p", "flash-caption", t("flashCaption"))),
    el(
      "div",
      "history",
      el("p", "caption", t("today")),
      history("11:02", "five", t("historyCrossed", t("ringFiveHour"), "75%")),
      history("09:14", "five", t("historyCrossed", t("ringFiveHour"), "50%")),
      history("07:00", "week", t("historyReset", t("ringWeekly"))),
    ),
  );

  const notifyRow = (name: string, tone: "five" | "week", on: number[]) =>
    el(
      "div",
      "notify-row",
      el("span", `swatch ${tone}`),
      el("span", "notify-name", name),
      el("div", "chips", ...[50, 75, 90, 100].map((value) => chip(`${value}%`, on.includes(value) ? tone : ""))),
      el("span", "add", icon(Plus, 14), t("custom")),
    );
  const notify = el(
    "article",
    "card",
    heading(t("notifyAt"), t("notifyAtHint")),
    el("div", "notify-rows", notifyRow(t("ringFiveHour"), "five", [75, 90, 100]), notifyRow(t("ringWeekly"), "week", [90, 100])),
  );

  const quietSwitch = toggle(t("quietHours"));
  quietSwitch.set(true);
  const quiet = el(
    "article",
    "card quiet",
    el(
      "div",
      "quiet-top",
      el("span", "time-pill", icon(Moon, 16), "23:00"),
      el("span", "quiet-arrow", icon(ArrowRight, 16)),
      el("span", "time-pill", icon(Sun, 16), "08:00"),
      el("span", "quiet-span", t("quietSpan")),
      quietSwitch.element,
    ),
    days(),
  );

  return page(stage, t("alertsSoon"), [
    el("section", "section", sectionHead(t("alerts"), t("alertsNote")), notify),
    tiles(
      "tiles",
      [BellRing, t("windowResets"), t("windowResetsHint"), true],
      [Sparkles, t("flashRing"), t("flashRingHint"), true],
      [Volume2, t("sound"), t("soundHint"), false],
    ),
    el("section", "section", sectionHead(t("quietHours"), t("quietNote")), quiet),
  ]);
}

// MARK: - Shortcuts

function keys(...names: string[]): HTMLElement {
  return el("span", "keys-small", ...names.map((name) => el("kbd", undefined, name)));
}

export function shortcutsPage(): HTMLElement {
  const bigKey = (name: string, live = false) => el("span", live ? "big-key live" : "big-key", name);
  const plus = () => el("span", "key-plus", "+");
  const tip = (key: string, text: string) => el("div", "tip", el("kbd", undefined, key), el("span", undefined, text));
  const stage = el(
    "aside",
    "stage shortcuts-stage",
    stageHead(t("recording"), t("scTogglePanel").toLowerCase()),
    el(
      "div",
      "key-capture",
      el("div", "big-keys", bigKey("Ctrl"), plus(), bigKey("Alt"), plus(), bigKey("P", true)),
      el("p", "listening", el("span", "listening-dot"), t("listening")),
    ),
    el("div", "conflict", icon(CircleCheck, 16), el("span", undefined, t("noConflict", "Ctrl + Alt + P"))),
    el("div", "tips", tip("Esc", t("tipEsc")), tip("Backspace", t("tipBackspace")), tip("Enter", t("tipEnter"))),
  );

  const shortcut = (glyph: IconNode, title: string, hint: string, right: HTMLElement, recording = false) =>
    el("div", recording ? "shortcut recording" : "shortcut", el("span", "shortcut-glyph", icon(glyph, 16)), heading(title, hint), right);
  const list = el(
    "article",
    "card shortcut-list",
    shortcut(PanelRight, t("scTogglePanel"), t("scTogglePanelHint"), el("span", "press-keys", el("span", "listening-dot"), t("pressKeys")), true),
    shortcut(RotateCw, t("scRefresh"), t("scRefreshHint"), keys("Ctrl", "Alt", "R")),
    shortcut(CircleDashed, t("scCycle"), t("scCycleHint"), keys("Ctrl", "Alt", "S")),
    shortcut(ClipboardCopy, t("scCopy"), t("scCopyHint"), keys("Ctrl", "Alt", "C")),
    shortcut(CirclePause, t("scPause"), t("scPauseHint"), el("span", "set-shortcut", icon(Plus, 14), t("setShortcut"))),
    shortcut(SlidersHorizontal, t("scSettings"), t("scSettingsHint"), keys("Ctrl", ",")),
  );

  return page(stage, t("shortcutsSoon"), [
    el("section", "section", sectionHead(t("shortcuts"), t("shortcutsNote")), list),
    tiles("tiles two", [Globe, t("globalShortcuts"), t("globalShortcutsHint"), true], [MousePointer2, t("keyHints"), t("keyHintsHint"), false]),
    el("div", "reset-defaults", button("link-button", icon(Undo2, 14), el("span", undefined, t("resetDefaults")))),
  ]);
}
