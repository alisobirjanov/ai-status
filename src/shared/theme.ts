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

/** The rail's, and its card's: the same, unless the rail is kept dark. */
export function railScheme(snapshot: Snapshot): Scheme {
  return snapshot.settings.railStaysDark ? "dark" : pageScheme(snapshot);
}
