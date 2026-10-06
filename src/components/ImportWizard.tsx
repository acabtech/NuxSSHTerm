// Phase 2 — the sessions/keys import wizard.
//
// Scan a file (PuTTY .reg, KiTTY .txt, WinSSHTerm XML, KeePass .kdbx) → preview
// tree with per-session overrides → .ppk conversion → merge into the native
// store. Passwords from the source are handed to the encrypted vault (when
// unlocked) and never shown in the preview.

import { useCallback, useEffect, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import {
  convertPpk,
  importSessionsFile,
  puttygenAvailable,
  vaultPutPassword,
  vaultPutPpkImport,
  type ConvertedKey,
  type ImportPreview,
} from "../api";
import type { SessionNode } from "../types";

export interface ImportSummary {
  count: number;
  format: string;
  warnings: string[];
  /** merge=false → replace the current tree */
  merge: boolean;
}

type ScanState =
  | { phase: "idle" }
  | { phase: "scanning" }
  | { phase: "error"; message: string };

interface Row {
  /** stable id (index in the flattened list) — used as React key */
  id: number;
  path: number[];
  depth: number;
  kind: "folder" | "session";
  name: string;
  /** for sessions: the vault key the source stored its password under */
  sourcePassword?: string;
  checked: boolean;
  host: string;
  user: string;
  port: string;
  key: string;
}

export function ImportWizard({
  initialPath,
  vaultUnlocked,
  onClose,
  onImport,
}: {
  initialPath: string;
  vaultUnlocked: boolean;
  onClose: () => void;
  onImport: (nodes: SessionNode[], summary: ImportSummary) => void;
}) {
  const [path, setPath] = useState(initialPath);
  const [scan, setScan] = useState<ScanState>({ phase: "idle" });
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [rows, setRows] = useState<Row[]>([]);
  const [puttygen, setPuttygen] = useState<boolean | null>(null);
  const [merge, setMerge] = useState(true);
  const [importing, setImporting] = useState(false);
  const [note, setNote] = useState("");

  useEffect(() => {
    void puttygenAvailable()
      .then(setPuttygen)
      .catch(() => setPuttygen(false));
  }, []);

  const scanPath = useCallback(
    async (value: string) => {
      const p = value.trim();
      if (!p) {
        setNote(
          "Enter the path to a file to import (PuTTY .reg, KiTTY .txt, WinSSHTerm connections.xml / .settings, KeePass .kdbx), or use Browse….",
        );
        return;
      }
      setScan({ phase: "scanning" });
      setNote("");
      try {
        const res = await importSessionsFile(p);
        setPreview(res);
        setRows(flattenRows(res));
        setScan({ phase: "idle" });
      } catch (e) {
        setScan({ phase: "error", message: String(e) });
        setPreview(null);
        setRows([]);
      }
    },
    [],
  );

  // Native file picker (Phase 2 plan: "file pickers → preview tree → mapping").
  const browse = useCallback(async () => {
    try {
      const selected = await openFileDialog({
        multiple: false,
        directory: false,
        title: "Select a file to import",
        filters: [
          {
            name: "Sessions & keys",
            extensions: ["reg", "txt", "xml", "settings", "kdbx", "ppk"],
          },
          { name: "All files", extensions: ["*"] },
        ],
      });
      if (typeof selected === "string" && selected) {
        setPath(selected);
        setNote("");
        void scanPath(selected);
      }
    } catch (e) {
      setNote(`File picker failed: ${String(e)}`);
    }
  }, [scanPath]);

  const updateRow = useCallback((id: number, patch: Partial<Row>) => {
    setRows((rs) => rs.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  }, []);

  const toggleRow = useCallback((id: number) => {
    setRows((rs) => {
      const row = rs.find((r) => r.id === id);
      if (!row) return rs;
      const next = !row.checked;
      if (row.kind === "folder") {
        // a folder toggle cascades to every descendant row
        return rs.map((r) =>
          r === row || (r.path.length > row.path.length && isPrefix(row.path, r.path))
            ? { ...r, checked: next }
            : r,
        );
      }
      return rs.map((r) => (r.id === id ? { ...r, checked: next } : r));
    });
  }, []);

  const selectedCount = rows.filter((r) => r.kind === "session" && r.checked).length;

  const doImport = useCallback(async () => {
    const chosen = rows.filter((r) => r.kind === "session" && r.checked);
    if (chosen.length === 0 || !preview) {
      setNote("Select at least one session to import.");
      return;
    }
    setImporting(true);
    setNote("");

    // 1) .ppk → .pem conversion (original files are never touched)
    const ppkRows = chosen.filter((r) => r.key.trim().toLowerCase().endsWith(".ppk"));
    const problems: string[] = [];
    const conversions: ConvertedKey[] = [];
    const known: Record<string, string> = {};
    for (const row of ppkRows) {
      const orig = row.key.trim();
      if (known[orig]) {
        row.key = known[orig];
        continue;
      }
      try {
        const res = await convertPpk(orig);
        known[orig] = res.converted;
        row.key = res.converted;
        conversions.push(res);
      } catch (e) {
        problems.push(`Key "${orig}" — ${String(e)}`);
        row.checked = false; // exclude this session from the import
      }
    }
    setRows([...rows]);

    // 2) rebuild nodes from the edited rows
    const nodes = buildFromRows(preview.nodes, rows);

    // 3) passwords → encrypted vault (best effort; skipped when locked)
    if (vaultUnlocked && preview.passwords) {
      for (const [k, pw] of Object.entries(preview.passwords)) {
        void vaultPutPassword(k, pw).catch(() => {});
      }
    }

    // 4) record conversions in the vault (needs the vault unlocked)
    if (vaultUnlocked && conversions.length) {
      for (const c of conversions) {
        void vaultPutPpkImport(c.original, c.converted).catch(() => {});
      }
    }

    const allWarnings = [...(preview.warnings ?? []), ...problems];
    setImporting(false);
    onImport(nodes, {
      count: countSessions(nodes),
      format: preview.format,
      warnings: allWarnings,
      merge,
    });
  }, [rows, preview, vaultUnlocked, merge, onImport]);

  return (
    <div className="modal-backdrop" onMouseDown={importing ? undefined : onClose}>
      <div className="modal import-modal" onMouseDown={(e) => e.stopPropagation()}>
        <h3>Import sessions</h3>
        <div className="modal-body">
          <div className="import-path-row">
            <button className="btn" onClick={() => void browse()} title="Pick a file with the system dialog">
              Browse…
            </button>
            <input
              type="text"
              value={path}
              onChange={(e) => {
                setPath(e.target.value);
                setNote("");
              }}
              onKeyDown={(e) => e.key === "Enter" && scanPath(path)}
              placeholder="/path/to/putty.reg, connections.xml, sessions.txt or vault.kdbx"
            />
            <button
              className="btn primary"
              onClick={() => scanPath(path)}
              disabled={scan.phase === "scanning"}
            >
              {scan.phase === "scanning" ? "Scanning…" : "Scan"}
            </button>
          </div>

          {scan.phase === "error" && <p className="muted import-error">{scan.message}</p>}
          {note && <p className="muted">{note}</p>}

          {preview && (
            <>
              <div className="import-meta">
                <span className="muted">{preview.format}</span>
                <span className="spacer" />
                <span className="muted">{preview.count} sessions</span>
              </div>

              {preview.warnings.length > 0 && (
                <div className="import-warnings">
                  {preview.warnings.slice(0, 8).map((w) => (
                    <div key={w}>⚠ {w}</div>
                  ))}
                  {preview.warnings.length > 8 && (
                    <div className="muted">… and {preview.warnings.length - 8} more</div>
                  )}
                </div>
              )}

              {preview.passwords && Object.keys(preview.passwords).length > 0 && (
                <div className="import-passwords-note">
                  {vaultUnlocked
                    ? `${Object.keys(preview.passwords).length} passwords found — they will be stored in the encrypted vault.`
                    : `${Object.keys(preview.passwords).length} passwords found but the vault is locked — they will NOT be imported.`}
                </div>
              )}

              {puttygen === false && ppkCount(rows) > 0 && (
                <div className="import-warnings">
                  ⚠ puttygen is not installed — .ppk keys will keep their original path
                  (unusable). Install putty-tools to convert keys automatically.
                </div>
              )}

              <div className="import-preview">
                <div className="import-row head">
                  <span className="pbtn" />
                  <span className="import-name">Session / folder</span>
                  <span>Host</span>
                  <span>User</span>
                  <span>Port</span>
                  <span>Private key</span>
                </div>
                {rows.map((r) => (
                  <div key={r.id} className="import-row" style={{ paddingLeft: 8 + r.depth * 18 }}>
                    <input
                      type="checkbox"
                      checked={r.checked}
                      onChange={() => toggleRow(r.id)}
                    />
                    <span className="import-name" title={r.path.join(" / ")}>
                      {r.kind === "folder" ? "📁 " : "🖥 "}
                      {r.name}
                    </span>
                    {r.kind === "session" ? (
                      <>
                        <input
                          value={r.host}
                          placeholder="host"
                          onChange={(e) => updateRow(r.id, { host: e.target.value })}
                        />
                        <input
                          value={r.user}
                          placeholder="user"
                          onChange={(e) => updateRow(r.id, { user: e.target.value })}
                        />
                        <input
                          value={r.port}
                          placeholder="22"
                          onChange={(e) => updateRow(r.id, { port: e.target.value })}
                        />
                        <input
                          value={r.key}
                          placeholder="~/.ssh/key"
                          title={r.key}
                          onChange={(e) => updateRow(r.id, { key: e.target.value })}
                        />
                      </>
                    ) : (
                      <span className="muted" />
                    )}
                  </div>
                ))}
              </div>

              <label className="import-merge">
                <input type="checkbox" checked={merge} onChange={(e) => setMerge(e.target.checked)} />
                Append to the existing session tree (recommended)
              </label>
            </>
          )}
        </div>
        <div className="modal-actions">
          <button className="btn" onClick={onClose} disabled={importing}>
            Cancel
          </button>
          <button
            className="btn primary"
            onClick={() => void doImport()}
            disabled={importing || !preview || selectedCount === 0}
          >
            {importing
              ? "Importing…"
              : `Import ${selectedCount} session${selectedCount === 1 ? "" : "s"}`}
          </button>
        </div>
      </div>
    </div>
  );
}

/* -------------------------- helpers -------------------------- */

function isPrefix(prefix: number[], path: number[]): boolean {
  if (prefix.length >= path.length) return false;
  for (let i = 0; i < prefix.length; i++) {
    if (prefix[i] !== path[i]) return false;
  }
  return true;
}

function flattenRows(preview: ImportPreview): Row[] {
  const rows: Row[] = [];
  const walk = (nodes: SessionNode[], path: number[], depth: number, keyPath: string[]) => {
    nodes.forEach((n, i) => {
      const p = [...path, i];
      const kp = [...keyPath, n.name];
      if (n.type === "Container") {
        rows.push({
          id: rows.length,
          path: p,
          depth,
          kind: "folder",
          name: n.name,
          checked: true,
          host: "",
          user: "",
          port: "",
          key: "",
        });
        walk(n.children, p, depth + 1, kp);
      } else {
        rows.push({
          id: rows.length,
          path: p,
          depth,
          kind: "session",
          name: n.name,
          sourcePassword: preview.passwords[kp.join("/")],
          checked: true,
          host: n.hostname,
          user: n.username,
          port: n.port,
          key: n.private_key,
        });
      }
    });
  };
  walk(preview.nodes, [], 0, []);
  return rows;
}

/** Rebuild a tree from the preview, applying row edits/selection. */
function buildFromRows(nodes: SessionNode[], rows: Row[]): SessionNode[] {
  const byPath = new Map<string, Row>();
  for (const r of rows) byPath.set(r.path.join("."), r);

  const build = (node: SessionNode, path: number[]): SessionNode | null => {
    const row = byPath.get(path.join("."));
    if (row && !row.checked) return null; // unchecked session or folder

    let next = node;
    if (row && node.type === "Connection") {
      next = {
        ...node,
        hostname: row.host.trim(),
        username: row.user.trim(),
        port: row.port.trim(),
        private_key: row.key.trim(),
        password: row.sourcePassword ?? "",
      };
    }
    const kids: SessionNode[] = [];
    node.children.forEach((c, i) => {
      const built = build(c, [...path, i]);
      if (built) kids.push(built);
    });
    next = { ...next, children: kids };
    if (node.type === "Container" && kids.length === 0) return null; // drop empty folders
    return next;
  };

  const out: SessionNode[] = [];
  nodes.forEach((n, i) => {
    const built = build(n, [i]);
    if (built) out.push(built);
  });
  return out;
}

function countSessions(nodes: SessionNode[]): number {
  let n = 0;
  const walk = (xs: SessionNode[]) => {
    for (const x of xs) {
      if (x.type === "Connection") n += 1;
      walk(x.children);
    }
  };
  walk(nodes);
  return n;
}

function ppkCount(rows: Row[]): number {
  let n = 0;
  for (const r of rows) {
    if (r.kind === "session" && r.key.trim().toLowerCase().endsWith(".ppk")) n += 1;
  }
  return n;
}