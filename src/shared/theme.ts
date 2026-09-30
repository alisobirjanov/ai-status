// Light or dark, settled from what Rust sends: the theme chosen, and which
// one Windows is in. Neither page asks its WebView instead — the WebView's
// `prefers-color-scheme` is shared by both windows and follows whichever
// last had its theme set.

import type { Scheme, Snapshot } from "./types";

/** Settings' own: the theme chosen, or Windows'. */
export function pageScheme(snapshot: Snapshot): Scheme {
  const { theme } = snapshot.settings;
  return theme === "system" ? snapshot.systemTheme : theme;
}

/**
 * Glass: how opaque the rail's surface is (`--rail-glass`, 1 solid) and
 * `data-glass` for what else changes with it — on the panel's page, or on
 * the rail in Settings' preview.
 */
export function applyGlass(element: HTMLElement, snapshot: Snapshot) {
  const { glass, glassTransparency } = snapshot.settings;
  element.style.setProperty("--rail-glass", glass ? String(1 - glassTransparency / 100) : "1");
  element.toggleAttribute("data-glass", glass);
}

/** The rail's, and its card's: the same, unless the rail is kept dark. */
export function railScheme(snapshot: Snapshot): Scheme {
  return snapshot.settings.railStaysDark ? "dark" : pageScheme(snapshot);
}
