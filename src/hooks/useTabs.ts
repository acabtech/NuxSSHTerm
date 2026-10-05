import { useCallback, useState } from "react";
import { ptyWrite } from "../api";
import { emptyNode, specFromNode, targetOf, type SessionNode, type Tab } from "../types";
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
      setTabs((t) => [...t, tab]);
      setActiveTab(tab.id);
      notify(`Opening ${tab.target}`);
      return { ok: true };
    },
    [notify],
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
    setTabs((t) => [...t, tab]);
    setActiveTab(tab.id);
  }, []);

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
    openLocalShell,
    closeTab,
    closeOthers,
    closeAllTabs,
    pasteIntoActive,
  };
}