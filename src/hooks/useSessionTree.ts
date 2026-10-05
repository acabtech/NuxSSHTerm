import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { exportConnectionsFile, getConfigDir, importConnectionsFile, loadTree, saveTree } from "../api";
import { emptyNode, type SessionNode } from "../types";
import { countConnections, getAt, insertAt, mapAll, removeAt, updateAt } from "../lib/tree";

export interface ImportResult {
  ok: boolean;
  count?: number;
  path?: string;
  error?: string;
}

export interface ExportResult {
  ok: boolean;
  path?: string;
  error?: string;
}

/**
 * Owns the session tree, selection, config-dir discovery, and the debounced
 * persistence. `notify` is called for status/error messages (wired to toasts).
 */
export function useSessionTree(notify: (msg: string, kind?: "info" | "success" | "error") => void) {
  const [tree, setTree] = useState<SessionNode[]>([]);
  const [selectedPath, setSelectedPath] = useState<number[] | null>(null);
  const [configDir, setConfigDir] = useState("");
  const treeLoaded = useRef(false);

  // initial load
  useEffect(() => {
    (async () => {
      try {
        const [t, dir] = await Promise.all([loadTree(), getConfigDir()]);
        setTree(t);
        setConfigDir(dir);
        treeLoaded.current = true;
        notify(`Loaded ${t.length} top-level entries`);
      } catch (e) {
        notify(`Load failed: ${String(e)}`, "error");
      }
    })();
  }, [notify]);

  // persist (skip the initial load round-trip)
  useEffect(() => {
    if (!treeLoaded.current) return;
    const h = window.setTimeout(() => {
      void saveTree(tree).catch((e) => notify(`Save failed: ${String(e)}`, "error"));
    }, 400);
    return () => window.clearTimeout(h);
  }, [tree, notify]);

  const selected = useMemo(
    () => (selectedPath ? getAt(tree, selectedPath) : null),
    [tree, selectedPath],
  );

  const connCount = useMemo(() => countConnections(tree), [tree]);

  const updateNode = useCallback(
    (patch: Partial<SessionNode>) => {
      if (!selectedPath) return;
      setTree((t) => updateAt(t, selectedPath, patch));
    },
    [selectedPath],
  );

  const toggleNode = useCallback((path: number[]) => {
    setTree((t) => {
      const n = getAt(t, path);
      if (n?.type === "Container") return updateAt(t, path, { expanded: !n.expanded });
      return t;
    });
  }, []);

  const addSession = useCallback(
    (asContainer: boolean) => {
      const parentPath = selectedPath && selected?.type === "Container" ? selectedPath : [];
      const node = emptyNode(
        asContainer ? "New folder" : "New session",
        asContainer ? "Container" : "Connection",
      );
      setTree((t) => insertAt(t, parentPath, node));
      notify(asContainer ? "Folder added" : "Session added");
    },
    [selected, selectedPath, notify],
  );

  const duplicateSelected = useCallback(() => {
    if (!selectedPath) return;
    const node = getAt(tree, selectedPath);
    if (!node) return;
    const copy: SessionNode = JSON.parse(JSON.stringify(node));
    copy.name = `${node.name} (copy)`;
    const parentPath = selectedPath.slice(0, -1);
    setTree((t) => insertAt(t, parentPath, copy));
    notify(`Duplicated "${node.name}"`);
  }, [selectedPath, tree, notify]);

  const deleteSelected = useCallback(() => {
    if (!selectedPath) return;
    const node = getAt(tree, selectedPath);
    if (!node) return;
    setTree((t) => removeAt(t, selectedPath));
    setSelectedPath(null);
    notify(`Deleted "${node.name}"`);
  }, [selectedPath, tree, notify]);

  const expandAll = useCallback(
    (expanded: boolean) =>
      setTree((t) => mapAll(t, (n) => (n.type === "Container" ? { ...n, expanded } : n))),
    [],
  );

  const runImport = useCallback(
    async (path: string): Promise<ImportResult> => {
      try {
        const imported = await importConnectionsFile(path);
        setTree(imported);
        setSelectedPath(null);
        notify(`Imported ${imported.length} top-level entries from settings`);
        return { ok: true, count: imported.length, path };
      } catch (e) {
        notify(`Import failed: ${String(e)}`, "error");
        return { ok: false, error: String(e) };
      }
    },
    [notify],
  );

  const runExport = useCallback(
    async (path: string): Promise<ExportResult> => {
      try {
        await exportConnectionsFile(path, tree);
        notify(`Exported connections to ${path}`);
        return { ok: true, path };
      } catch (e) {
        notify(`Export failed: ${String(e)}`, "error");
        return { ok: false, error: String(e) };
      }
    },
    [tree, notify],
  );

  return {
    tree,
    setTree,
    selectedPath,
    setSelectedPath,
    selected,
    connCount,
    configDir,
    updateNode,
    toggleNode,
    addSession,
    duplicateSelected,
    deleteSelected,
    expandAll,
    runImport,
    runExport,
  };
}