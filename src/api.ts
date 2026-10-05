// Thin wrappers over the Tauri command surface (src-tauri/src/lib.rs).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { LaunchSpec, SessionNode } from "./types";

export const loadTree = () => invoke<SessionNode[]>("load_tree");
export const saveTree = (tree: SessionNode[]) => invoke<void>("save_tree", { tree });
export const getConfigDir = () => invoke<string>("config_dir");
export const importConnectionsFile = (path: string) =>
  invoke<SessionNode[]>("import_connections_file", { path });
export const exportConnectionsFile = (path: string, tree: SessionNode[]) =>
  invoke<void>("export_connections_file", { path, tree });

// ---- Phase 2: sessions + keys import wizard ----
export interface ImportPreview {
  format: string;
  count: number;
  /** preview tree — passwords already stripped (they travel in `passwords`) */
  nodes: SessionNode[];
  /** path key (ancestor names joined by "/") -> password, for the vault */
  passwords: Record<string, string>;
  warnings: string[];
}
export interface ConvertedKey {
  original: string;
  converted: string;
  /** true when the target .pem already existed (no puttygen run) */
  was_already: boolean;
}
export const importSessionsFile = (path: string) =>
  invoke<ImportPreview>("import_sessions_file", { path });
export const puttygenAvailable = () => invoke<boolean>("puttygen_available");
export const convertPpk = (source: string) =>
  invoke<ConvertedKey>("convert_ppk", { source });
export const keepassxcAvailable = () => invoke<boolean>("keepassxc_available");

// ppk conversion log stored inside the vault
// export const vaultGetPpkMap = () => invoke<Record<string, string>>("vault_get_ppk_map");
export const vaultPutPpkImport = (original: string, converted: string) =>
  invoke<void>("vault_put_ppk_import", { original, converted });

// ---- Encrypted vault (Phase 1) ----
export interface VaultStatus {
  initialized: boolean;
  unlocked: boolean;
}
export const getVaultStatus = () => invoke<VaultStatus>("vault_status");
export const vaultInit = (master: string) => invoke<void>("vault_init", { master });
export const vaultUnlock = (master: string) => invoke<void>("vault_unlock", { master });
export const vaultLock = () => invoke<void>("vault_lock");
export const vaultReset = () => invoke<void>("vault_reset");
export const vaultGetPasswords = () =>
  invoke<Record<string, string>>("vault_get_passwords");
export const vaultPutPassword = (path: string, password: string) =>
  invoke<void>("vault_put_password", { path, password });
export const vaultRemovePassword = (path: string) =>
  invoke<void>("vault_remove_password", { path });

export const ptyOpen = (
  id: string,
  spec: LaunchSpec,
  cols: number,
  rows: number,
) => invoke<void>("pty_open", { id, spec, cols, rows });
export const ptyWrite = (id: string, data: string) =>
  invoke<void>("pty_write", { id, data });
export const ptyResize = (id: string, cols: number, rows: number) =>
  invoke<void>("pty_resize", { id, cols, rows });
export const ptyClose = (id: string) => invoke<void>("pty_close", { id });

export interface PtyDataEvent {
  id: string;
  /** base64-encoded raw PTY bytes (binary-safe across chunk boundaries). */
  data: string;
}
export interface PtyExitEvent {
  id: string;
  status: string;
}

export const onPtyData = (cb: (e: PtyDataEvent) => void): Promise<UnlistenFn> =>
  listen<PtyDataEvent>("pty-data", (ev) => cb(ev.payload));
export const onPtyExit = (cb: (e: PtyExitEvent) => void): Promise<UnlistenFn> =>
  listen<PtyExitEvent>("pty-exit", (ev) => cb(ev.payload));

/** Decode a base64 string (from `pty-data`) into raw bytes for xterm.js. */
export function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}
