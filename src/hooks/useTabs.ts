import { useCallback, useState } from "react";
import { ptyWrite } from "../api";
import { emptyNode, specFromNode, targetOf, type LaunchSpec, type SessionNode, type Tab } from "../types";
import { uid } from "../lib/tree";

export interface OpenResult {
  ok: boolean;
  reason?: "no-host";
  name?: string;
}

/**
 * Owns the tab strip state and the operations that open/close tabs. `notify`
 * is wired to toasts. `openSession` returns a result so the caller can show a
 * richer modal when a host isn't configured.
 */
export function useTabs(notify: (msg: string, kind?: "info" | "success" | "error") => void) {
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeTab, setActiveTab] = useState<string | null>(null);

  const pushTab = useCallback((tab: Tab) => {
    setTabs((t) => [...t, tab]);
    setActiveTab(tab.id);
  }, []);

  const openSession = useCallback(
    (node: SessionNode): OpenResult => {
      if (node.type === "Container") return { ok: false };
      if (!node.hostname) {
        notify(`"${node.name}" has no Host/IP configured`, "error");
        return { ok: false, reason: "no-host", name: node.name };
      }
      const tab: Tab = {
        id: uid(),
        title: node.name,
        kind: "terminal",
        spec: specFromNode(node),
        target: targetOf(node),
        exited: false,
      };
      pushTab(tab);
      notify(`Opening ${tab.target}`);
      return { ok: true };
    },
    [notify, pushTab],
  );

  /** Open the SFTP commander for a session (WinSSHTerm "Copy Files"). */
  const openCommander = useCallback(
    (node: SessionNode): OpenResult => {
      if (node.type === "Container") return { ok: false };
      if (!node.hostname) {
        notify(`"${node.name}" has no Host/IP configured`, "error");
        return { ok: false, reason: "no-host", name: node.name };
      }
      const prot = (node.cf_prot || "sftp").toLowerCase();
      if (prot !== "sftp") {
        notify(
          `"${node.name}" uses cfProt "${node.cf_prot}" — only sftp is supported in this build`,
          "error",
        );
        return { ok: false };
      }
      const tab: Tab = {
        id: uid(),
        title: `${node.name} · SFTP`,
        kind: "commander",
        spec: specFromNode(node),
        target: targetOf(node),
        exited: false,
        // vault-in-memory password, used by the askpass helper for sftp auth
        password: node.password || undefined,
      };
      pushTab(tab);
      notify(`Opening SFTP ${tab.target}`);
      return { ok: true };
    },
    [notify, pushTab],
  );

  /** Terminal tab for an existing spec (commander Ctrl+T: same host). */
  const openTerminalFromSpec = useCallback(
    (spec: LaunchSpec, title: string, target: string) => {
      pushTab({ id: uid(), title, kind: "terminal", spec, target, exited: false });
    },
    [pushTab],
  );

  const openLocalShell = useCallback(() => {
    const tab: Tab = {
      id: uid(),
      title: "Local shell",
      kind: "terminal",
      spec: specFromNode(emptyNode("Local shell", "Connection")),
      target: `${navigator.platform || "local"} shell`,
      exited: false,
    };
    pushTab(tab);
  }, [pushTab]);

  const closeTab = useCallback((id: string) => {
    setTabs((prev) => {
      const idx = prev.findIndex((t) => t.id === id);
      const next = prev.filter((t) => t.id !== id);
      setActiveTab((cur) => {
        if (cur !== id) return cur;
        const fallback = next[Math.min(idx, next.length - 1)];
        return fallback ? fallback.id : null;
      });
      return next;
    });
  }, []);

  const closeOthers = useCallback((id: string) => {
    setTabs((prev) => prev.filter((t) => t.id === id));
    setActiveTab(id);
  }, []);

  const closeAllTabs = useCallback(() => {
    setTabs([]);
    setActiveTab(null);
  }, []);

  const currentTab = tabs.find((t) => t.id === activeTab) ?? null;

  const pasteIntoActive = useCallback(async () => {
    if (!currentTab) {
      notify("No active terminal to paste into", "error");
      return;
    }
    try {
      const text = await navigator.clipboard.readText();
      if (text) await ptyWrite(currentTab.id, text);
      notify(`Pasted ${text.length} chars`);
    } catch (e) {
      notify(`Clipboard unavailable: ${String(e)}`, "error");
    }
  }, [currentTab, notify]);

  return {
    tabs,
    setTabs,
    activeTab,
    setActiveTab,
    currentTab,
    openSession,
    openCommander,
    openTerminalFromSpec,
    openLocalShell,
    closeTab,
    closeOthers,
    closeAllTabs,
    pasteIntoActive,
  };
}
