// What's new: a version's entry in CHANGELOG.md, in the reader's language.
// A new version's notes arrive from the updater, and are the same entry.

import changelog from "../../CHANGELOG.md?raw";
import { locale } from "../shared/i18n";

/** The body of `## <version>`, up to the next `## `. Mirrors `notes` in `scripts/release.mjs`. */
export function entry(version: string): string | null {
  const lines = changelog.split(/\r?\n/);
  const start = lines.findIndex((line) => line.trim() === `## ${version}`);
  if (start < 0) return null;
  const end = lines.findIndex((line, index) => index > start && line.startsWith("## "));
  return lines.slice(start + 1, end < 0 ? undefined : end).join("\n").trim() || null;
}

/** Markdown's marks are noise in a list of plain lines. */
function plain(text: string): string {
  return text
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/\*\*|__|`/g, "")
    .trim();
}

/**
 * The points under **Русский** or **English**, whichever the reader reads —
 * or every point, for notes not written that way.
 */
export function points(notes: string): string[] {
  const wanted = locale.toLowerCase().startsWith("ru") ? "Русский" : "English";
  const sections = new Map<string, string[]>();
  const all: string[] = [];
  let section = "";
  let last: string[] | null = null;
  for (const line of notes.split(/\r?\n/)) {
    const title = line.trim().match(/^\*\*(.+)\*\*$/);
    if (title) {
      section = title[1].trim();
      last = null;
      continue;
    }
    const point = line.match(/^\s*[-*]\s+(.*)$/);
    if (point) {
      const list = sections.get(section) ?? [];
      list.push(point[1]);
      sections.set(section, list);
      all.push(point[1]);
      last = list;
    } else if (line.trim() && last) {
      // A point wrapped onto the next line.
      const joined = `${last[last.length - 1]} ${line.trim()}`;
      last[last.length - 1] = joined;
      all[all.length - 1] = joined;
    }
  }
  const mine = sections.get(wanted);
  return (mine?.length ? mine : all).map(plain).filter(Boolean);
}
