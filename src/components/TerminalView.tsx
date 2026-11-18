import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { base64ToBytes, onPtyData, onPtyExit, ptyClose, ptyOpen, ptyResize, ptyWrite } from "../api";
import type { Tab, TerminalSettings } from "../types";

/**
 * One xterm.js terminal bound to one PTY session.
 *
 * v0.5.0 mount model: App keeps exactly one TerminalView mounted per tab and
 * relocates its DOM into a pane slot via `createPortal` (see TabContent.tsx),
 * so the terminal survives tiling, tab drags and page switches. The root is a
 * plain `.term-pane` filling its slot; no `hidden` classes anymore.
 */

/** tab.id -> live Terminal, so menu actions can reach the right instance. */
const terms = new Map<string, Terminal>();

/**
 * Copy the current xterm selection for `tabId` into the clipboard.
 * Returns false when there is no such terminal or no selection active
 * (the caller then falls back to the system-clipboard copy path).
 */
export function termCopySelection(tabId: string): boolean {
  const term = terms.get(tabId);
  if (!term || !term.hasSelection()) return false;
  const sel = term.getSelection();
  if (!sel) return false;
  void navigator.clipboard.writeText(sel).catch(() => {});
  return true;
}

export function TerminalView({ tab, active, term: termSettings }: { tab: Tab; active: boolean; term: TerminalSettings }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [exited, setExited] = useState(false);

  // create terminal + pty (once)
  useEffect(() => {
    const host = hostRef.current;
    if (!host || termRef.current) return;

    const term = new Terminal({
      fontFamily: '"JetBrains Mono", ui-monospace, "DejaVu Sans Mono", Consolas, monospace',
      fontSize: termSettings.fontSize,
      cursorBlink: termSettings.cursorBlink,
      cursorStyle: termSettings.cursorStyle,
      scrollback: termSettings.scrollback,
      allowProposedApi: true,
      theme: {
        background: "#0b1017",
        foreground: "#d4d4d4",
        cursor: "#d4d4d4",
        selectionBackground: "#2b4a68",
        black: "#000000",
        red: "#cd3131",
        green: "#0dbc79",
        yellow: "#e5e510",
        blue: "#2472c8",
        magenta: "#bc3fbc",
        cyan: "#11a8cd",
        white: "#e5e5e5",
        brightBlack: "#666666",
        brightRed: "#f14c4c",
        brightGreen: "#23d18b",
        brightYellow: "#f5f543",
        brightBlue: "#3b8eea",
        brightMagenta: "#d670d6",
        brightCyan: "#29b8db",
        brightWhite: "#ffffff",
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    terms.set(tab.id, term);
    try {
      fit.fit();
    } catch {
      /* pane not measured yet */
    }
    termRef.current = term;
    fitRef.current = fit;

    // wire events BEFORE spawning so no early output is lost
    let unData: (() => void) | undefined;
    let unExit: (() => void) | undefined;
    let disposed = false;

    (async () => {
      unData = await onPtyData((e) => {
        if (e.id === tab.id) term.write(base64ToBytes(e.data));
      });
      unExit = await onPtyExit((e) => {
        if (e.id === tab.id) {
          setExited(true);
          term.write("\r\n\x1b[33m[session closed]\x1b[0m\r\n");
        }
      });
      if (disposed) return;
      try {
        await ptyOpen(tab.id, tab.spec, term.cols, term.rows, tab.password);
      } catch (err) {
        term.write(`\x1b[31mFailed to start session: ${String(err)}\x1b[0m\r\n`);
      }
    })();

    const onDataSub = term.onData((d) => {
      void ptyWrite(tab.id, d).catch(() => {});
    });

    const ro = new ResizeObserver(() => {
      try {
        fit.fit();
        void ptyResize(tab.id, term.cols, term.rows).catch(() => {});
      } catch {
        /* ignore */
      }
    });
    ro.observe(host);

    return () => {
      disposed = true;
      ro.disconnect();
      onDataSub.dispose();
      unData?.();
      unExit?.();
      terms.delete(tab.id);
      void ptyClose(tab.id).catch(() => {});
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
    // tab.id is the session identity; spec/title/settings changes must not respawn the pty
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab.id]);

  // live-apply terminal settings without respawning
  useEffect(() => {
    const t = termRef.current;
    if (!t) return;
    t.options.fontSize = termSettings.fontSize;
    t.options.cursorBlink = termSettings.cursorBlink;
    t.options.cursorStyle = termSettings.cursorStyle;
    t.options.scrollback = termSettings.scrollback;
    try {
      fitRef.current?.fit();
      void ptyResize(tab.id, t.cols, t.rows).catch(() => {});
    } catch {
      /* ignore */
    }
  }, [termSettings.fontSize, termSettings.cursorBlink, termSettings.cursorStyle, termSettings.scrollback, tab.id]);

  // refit + focus when this tab becomes visible
  useEffect(() => {
    if (!active || !termRef.current || !fitRef.current) return;
    const t = window.setTimeout(() => {
      try {
        fitRef.current?.fit();
        const term = termRef.current;
        if (term) void ptyResize(tab.id, term.cols, term.rows).catch(() => {});
        term?.focus();
      } catch {
        /* ignore */
      }
    }, 30);
    return () => window.clearTimeout(t);
  }, [active, tab.id]);

  return (
    <div className="term-pane">
      <div className="term-host" ref={hostRef} />
      {exited && (
        <div className="term-missing">
          Session closed. Press the ✕ on the tab to close it, or reopen the connection.
        </div>
      )}
    </div>
  );
}