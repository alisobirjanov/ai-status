// Mirrors of what Rust sends (`src-tauri/src/model.rs`, `store.rs`,
// `panel.rs`). Rust is the source of truth; these only name the fields.

export type Provider = "claudeCode" | "codex";
export type WindowKind = "fiveHour" | "weekly" | "spend" | "other";
export type UsageState = "live" | "stale" | "unavailable";
export type Route = "endpoint" | "appServer";
export type CodexSource = "automatic" | "endpoint" | "tooling";

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
  panelVisible: boolean;
}

export interface Snapshot {
  accounts: AccountView[];
  settings: Settings;
  version: string;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Layout {
  side: "left" | "right";
  rail: Rect;
  padTop: number;
  itemHeight: number;
  itemSpacing: number;
  gap: number;
  cardWidth: number;
  pointerWidth: number;
  margin: number;
  visible: Rect;
}

export interface SettingsPatch {
  enabled?: Provider[];
  codexSource?: CodexSource;
  /** 0 is adaptive. */
  refreshMinutes?: number;
  showsRemaining?: boolean;
  warningAt?: number;
  panelVisible?: boolean;
}
