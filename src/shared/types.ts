// Mirrors of what Rust sends (`src-tauri/src/model.rs`, `store.rs`,
// `panel.rs`). Rust is the source of truth; these only name the fields.

export type Provider = "claudeCode" | "codex";
export type WindowKind = "fiveHour" | "weekly" | "spend" | "other";
export type UsageState = "live" | "stale" | "unavailable";
export type Route = "endpoint" | "appServer";
export type CodexSource = "automatic" | "endpoint" | "tooling";
export type RingShows = "fullest" | "fiveHour" | "weekly" | "bothSplit" | "bothStacked" | "bothNested";
export type Theme = "system" | "light" | "dark";
/** A theme settled: what System stands for at the moment. */
export type Scheme = Exclude<Theme, "system">;

export type Reason =
  | "notChecked"
  | "noLimitsReported"
  | "claudeSignInRequired"
  | "claudeLoginExpired"
  | "codexSignInRequired"
  | "codexServerFailed"
  | "unreachable"
  | "unreadableReply"
  | "rateLimited"
  | "serverError";

export interface UsageWindow {
  id: string;
  kind: WindowKind;
  scope: string | null;
  usedFraction: number;
  windowSeconds: number;
  /** Unix milliseconds. */
  resetsAt: number | null;
  isExhausted: boolean;
}

export interface ProviderUsage {
  provider: Provider;
  windows: UsageWindow[];
  observedAt: number | null;
  state: UsageState;
  reason: Reason | null;
  plan: string | null;
  creditBalance: string | null;
  origin: Route | null;
  isCached: boolean;
}

export interface Check {
  reason: Reason | null;
  origin: Route | null;
  checkedAt: number;
}

export interface AccountView {
  provider: Provider;
  name: string;
  enabled: boolean;
  detected: boolean;
  /** The login it is read with, the home folder written `~`. */
  credentials: string;
  usage: ProviderUsage;
  refreshing: boolean;
  lastCheck: Check | null;
}

export interface Settings {
  enabled: Provider[];
  hasChosen: boolean;
  codexSource: CodexSource;
  refreshMinutes: number | null;
  showsRemaining: boolean;
  warningAt: number;
  ringShows: RingShows;
  limitLetters: boolean;
  panelVisible: boolean;
  tucksAway: boolean;
  showsCard: boolean;
  theme: Theme;
  railStaysDark: boolean;
  glass: boolean;
  /** Percent: 25, 50 or 75. */
  glassTransparency: number;
  checksForUpdates: boolean;
}

export interface Snapshot {
  accounts: AccountView[];
  settings: Settings;
  version: string;
  /** Light or dark as Windows has it, for a theme left to follow it. */
  systemTheme: Scheme;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Layout {
  /** Where the rail is in the window: down its left or right, or across its top. */
  side: "left" | "right" | "top";
  rail: Rect;
  /** Before the first item, from the rail's top down a side and its left end across the top. */
  padStart: number;
  /** One service's extent along the rail: its height down a side, its width across the top. */
  itemLength: number;
  itemSpacing: number;
  gap: number;
  cardWidth: number;
  pointerWidth: number;
  margin: number;
  visible: Rect;
  /** The screen edge the rail is fused to; null floating. Only a rail docked at the top lies across. */
  dock: "left" | "right" | "top" | null;
  /** Laid out while it is carried, on its way somewhere: animated to, and the drag goes on. */
  carried: boolean;
  /** How far a docked rail's ends reach beyond `rail`, along the edge. */
  flare: number;
  /** The window's size: the viewport, once it has caught up with a new one. */
  width: number;
  height: number;
  /** Which layout this is, for saying when it has been drawn. */
  generation: number;
}

export interface SettingsPatch {
  enabled?: Provider[];
  codexSource?: CodexSource;
  /** 0 is adaptive. */
  refreshMinutes?: number;
  showsRemaining?: boolean;
  warningAt?: number;
  ringShows?: RingShows;
  limitLetters?: boolean;
  panelVisible?: boolean;
  tucksAway?: boolean;
  showsCard?: boolean;
  theme?: Theme;
  railStaysDark?: boolean;
  glass?: boolean;
  glassTransparency?: number;
  checksForUpdates?: boolean;
}

/** A new version of Pulse itself (`src-tauri/src/updater.rs`). */
export type UpdateStatus =
  | "idle"
  | "checking"
  | "upToDate"
  | "available"
  | "downloading"
  | "installing"
  | "failed"
  | "unsupported";

export interface UpdateInfo {
  status: UpdateStatus;
  current: string;
  version: string | null;
  notes: string | null;
  progress: number | null;
  checkedAt: number | null;
  error: string | null;
}
