// Shared types mirroring the Rust `Node` model (WinSSHTerm session XML).

export type NodeType = "Container" | "Connection";

export interface SessionNode {
  name: string;
  type: NodeType;
  expanded: boolean;
  children: SessionNode[];

  // WinSSHTerm connection attributes
  descr: string;
  username: string;
  password: string;
  private_key: string;
  hostname: string;
  port: string;
  certificate: string;
  launch_tool: string;
  x11: string;
  cf_prot: string;
  proxy_enabled: boolean;
  proxy_type: string;
  proxy_host: string;
  proxy_port: string;
  proxy_user: string;
  proxy_telnet_cmd: string;

  // UI-local properties (Configuration panel: Login Dir, Login Cmds, …)
  login_dir: string;
  login_cmds: string;
  cmdline_args: string;
  env_color: string;
  custom_id: string;
  custom_type: string;
  /** Forward the ssh-agent to the remote host (`-o ForwardAgent=yes`). */
  forward_agent: boolean;
}

export interface LaunchSpec {
  host: string;
  port: number;
  username: string;
  private_key: string;
  x11: boolean;
  forward_agent: boolean;
  extra_args: string[];
  proxy_enabled: boolean;
  proxy_type: string;
  proxy_host: string;
  proxy_port: string;
  proxy_telnet_cmd: string;
}

export interface Tab {
  /** unique tab id, also the pty/sftp session id */
  id: string;
  title: string;
  /** "terminal" | "commander" */
  kind: "terminal" | "commander";
  spec: LaunchSpec;
  target: string;
  exited: boolean;
  /**
   * Vault password fed to this tab's ssh/sftp child via the askpass helper
   * (memory only, never persisted).
   */
  password?: string;
}

export function emptyNode(name: string, type: NodeType): SessionNode {
  return {
    name,
    type,
    expanded: type === "Container",
    children: [],
    descr: "",
    username: "",
    password: "",
    private_key: "",
    hostname: "",
    port: type === "Connection" ? "22" : "",
    certificate: "",
    launch_tool: "",
    x11: "don't forward",
    cf_prot: type === "Connection" ? "sftp" : "",
    proxy_enabled: false,
    proxy_type: "",
    proxy_host: "",
    proxy_port: "",
    proxy_user: "",
    proxy_telnet_cmd: "",
    login_dir: "",
    login_cmds: "",
    cmdline_args: "",
    env_color: "",
    custom_id: "",
    custom_type: "",
    forward_agent: false,
  };
}

export function specFromNode(n: SessionNode): LaunchSpec {
  return {
    host: n.hostname,
    port: parseInt(n.port || "22", 10) || 22,
    username: n.username,
    private_key: n.private_key,
    x11: n.x11 !== "" && !n.x11.toLowerCase().startsWith("don't"),
    forward_agent: n.forward_agent === true,
    extra_args: n.cmdline_args ? n.cmdline_args.split(/\s+/).filter(Boolean) : [],
    proxy_enabled: n.proxy_enabled,
    proxy_type: n.proxy_type,
    proxy_host: n.proxy_host,
    proxy_port: n.proxy_port,
    proxy_telnet_cmd: n.proxy_telnet_cmd,
  };
}

export function targetOf(n: SessionNode): string {
  const user = n.username || "?";
  return `${user}@${n.hostname || "?"}:${n.port || "22"}`;
}

// ---- v0.5.0: modernized UI settings + tileable layout ---------------------

/** Density token for the chrome (spacing). Mirrors DESIGN.md `spacing.unit`. */
export type Density = "comfortable" | "compact";

/** xterm.js cursor style (mirrors the Rust `TerminalSettings.cursor_style`). */
export type CursorStyle = "block" | "underline" | "bar";

/** xterm.js terminal preferences (mirrors the Rust `TerminalSettings`). */
export interface TerminalSettings {
  fontSize: number;
  scrollback: number;
  cursorBlink: boolean;
  cursorStyle: CursorStyle;
  /** Bell/notification behavior on terminal BEL (xterm.onBell). */
  bellStyle: "none" | "visual" | "sound" | "both";
}

/** Connections sidebar layout preferences (mirrors the Rust `SidebarSettings`). */
export interface SidebarSettings {
  /** Expanded sidebar width in px (drag-resizable), clamped 200–560. */
  width: number;
  /**
   * Collapse behavior: "hover" collapses after `collapseDelayMs` when the
   * pointer leaves the rail; "never" keeps the sidebar pinned open.
   */
  collapseMode: "hover" | "never";
  /** Hover-out collapse delay in ms (0 = collapse immediately). */
  collapseDelayMs: number;
}

/** SFTP commander / file transfer preferences (mirrors the Rust `TransferSettings`). */
export interface TransferSettings {
  confirmBeforeOverwrite: boolean;
  confirmBeforeDelete: boolean;
  /** Initial local working directory; empty falls back to $HOME. */
  defaultLocalPath: string;
}

/** Persisted, non-secret chrome preferences (mirrors the Rust `UiSettings`). */
export interface UiSettings {
  density: Density;
  fontScale: number;
  showConfig: boolean;
  /** Primary accent color as a CSS hex string (e.g. `#3b82f6`). */
  accentColor: string;
  /** Auto-lock the encrypted vault after N minutes of inactivity while unlocked. 0 disables. */
  vaultAutoLockMinutes: number;
  /** Default for new connections: forward the local ssh-agent to the remote host. */
  defaultForwardAgent: boolean;
  /** Default for new connections: private key path (e.g. ~/.ssh/id_ed25519). */
  defaultPrivateKey: string;
  terminal: TerminalSettings;
  transfer: TransferSettings;
  /** Connections sidebar (width / collapse behavior). */
  sidebar: SidebarSettings;
}

export const DEFAULT_UI_SETTINGS: UiSettings = {
  density: "comfortable",
  fontScale: 1,
  showConfig: true,
  accentColor: "#3b82f6",
  vaultAutoLockMinutes: 0,
  defaultForwardAgent: false,
  defaultPrivateKey: "",
  terminal: { fontSize: 13, scrollback: 10000, cursorBlink: true, cursorStyle: "block", bellStyle: "visual" },
  transfer: { confirmBeforeOverwrite: true, confirmBeforeDelete: true, defaultLocalPath: "" },
  sidebar: { width: 320, collapseMode: "hover", collapseDelayMs: 0 },
};

/** A node in the tileable work-area layout tree. */
export type LayoutNode =
  | { kind: "leaf"; id: string; tabId: string | null }
  | { kind: "split"; id: string; dir: "row" | "col"; sizes: number[]; children: LayoutNode[] };

export const DEFAULT_LAYOUT: LayoutNode = {
  kind: "leaf",
  id: "l1",
  tabId: null,
};
