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
