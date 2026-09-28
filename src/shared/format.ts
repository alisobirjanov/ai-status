// How a reading is said and coloured. Ports of `UsageWindow.name`,
// `percentText`, `UsageTint` and the card's reset line.

import { locale, t } from "./i18n";
import type { Reason, UsageWindow } from "./types";

/**
 * A whole percentage that never rounds away the fact that there is *some*,
 * or that there is *not all*. The ring's round cap already puts a dot of
 * colour on screen for the smallest reading; "0%" beside it would be the
 * same number disagreeing with itself.
 */
export function percentValue(fraction: number): number {
  if (!Number.isFinite(fraction)) return 0;
  const percent = Math.min(Math.max(fraction, 0), 1) * 100;
  if (percent <= 0) return 0;
  if (percent >= 100) return 100;
  return Math.min(Math.max(Math.round(percent), 1), 99);
}

export function remainingFraction(window: UsageWindow): number {
  return Math.min(Math.max(1 - window.usedFraction, 0), 1);
}

/** The figure shown, counted whichever way the reader chose. */
export function shownFraction(window: UsageWindow, remaining: boolean): number {
  return remaining ? remainingFraction(window) : window.usedFraction;
}

export function percentText(window: UsageWindow, remaining: boolean): string {
  return `${percentValue(shownFraction(window, remaining))}%`;
}

/** Spent is the provider's word — or a figure past the whole of it. */
export function isSpent(window: UsageWindow | undefined): boolean {
  return !!window && (window.isExhausted || window.usedFraction >= 1);
}

export const colours = {
  good: "rgb(0, 230, 140)",
  caution: "rgb(255, 194, 38)",
  warning: "rgb(255, 79, 66)",
  exhausted: "rgb(217, 23, 33)",
};

/** Colour means how close this limit is to running out — nothing else. */
export function tint(window: UsageWindow, warningAt: number): string {
  if (isSpent(window)) return colours.exhausted;
  if (window.usedFraction < 0.5) return colours.good;
  if (window.usedFraction < warningAt / 100) return colours.caution;
  return colours.warning;
}

export function windowName(window: UsageWindow): string {
  let base: string;
  switch (window.kind) {
    case "fiveHour":
      base = t("fiveHour");
      break;
    case "weekly":
      base = t("weekly");
      break;
    case "spend":
      base = t("spend");
      break;
    default: {
      // Days only when it is a whole number of them; never below an hour.
      const seconds = window.windowSeconds;
      base =
        seconds >= 86_400 && seconds % 86_400 === 0
          ? t("nDays", String(seconds / 86_400))
          : t("nHours", String(Math.max(Math.round(seconds / 3600), 1)));
    }
  }
  return window.scope ? `${base} · ${window.scope}` : base;
}

function isToday(date: Date): boolean {
  const now = new Date();
  return date.getFullYear() === now.getFullYear() && date.getMonth() === now.getMonth() && date.getDate() === now.getDate();
}

/**
 * "Resets 14:30" today, "Resets 30 Sep, 14:30" otherwise. With no reset,
 * only a length the provider stated — never a sort key passed off as one.
 */
export function resetText(window: UsageWindow): string {
  if (window.resetsAt == null) {
    if (window.windowSeconds <= 0) return "";
    const hours = window.windowSeconds / 3600;
    return hours >= 24 ? t("lengthDays", String(Math.round(hours / 24))) : t("lengthHours", String(Math.round(hours)));
  }
  const date = new Date(window.resetsAt);
  const options: Intl.DateTimeFormatOptions = isToday(date)
    ? { hour: "numeric", minute: "2-digit" }
    : { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" };
  return t("resets", new Intl.DateTimeFormat(locale, options).format(date));
}

/** "5 minutes ago", "yesterday". */
export function relative(ms: number): string {
  const seconds = Math.round((ms - Date.now()) / 1000);
  const format = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  const abs = Math.abs(seconds);
  if (abs < 45) return format.format(0, "second");
  if (abs < 3600) return format.format(Math.round(seconds / 60), "minute");
  if (abs < 86_400) return format.format(Math.round(seconds / 3600), "hour");
  return format.format(Math.round(seconds / 86_400), "day");
}

export function reasonText(reason: Reason): string {
  return t(reason);
}
