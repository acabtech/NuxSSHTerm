import { useCallback, useEffect, useRef, useState } from "react";
import { getUiSettings, setUiSettings } from "../api";
import { normalizeHexColor } from "../lib/color";
import {
  DEFAULT_UI_SETTINGS,
  type CursorStyle,
  type TransferSettings,
  type UiSettings,
} from "../types";

const CURSOR_STYLES: readonly CursorStyle[] = ["block", "underline", "bar"];
const BELL_STYLES: readonly UiSettings["terminal"]["bellStyle"][] = ["none", "visual", "sound", "both"];

function normalizeCursorStyle(v: unknown): CursorStyle {
  return CURSOR_STYLES.includes(v as CursorStyle) ? (v as CursorStyle) : DEFAULT_UI_SETTINGS.terminal.cursorStyle;
}

function normalizeBellStyle(v: unknown): UiSettings["terminal"]["bellStyle"] {
  return BELL_STYLES.includes(v as UiSettings["terminal"]["bellStyle"]) ? (v as UiSettings["terminal"]["bellStyle"]) : DEFAULT_UI_SETTINGS.terminal.bellStyle;
}

/**
 * Owns the persisted UI chrome settings (v0.5.0 modernization): density,
 * font scale, accent color, configuration-rail visibility, and terminal preferences.
 *
 * On mount the settings load from the Tauri backend; a failing load (e.g. when
 * the frontend runs outside Tauri) silently falls back to defaults so the UI
 * still works. Changes are written through to disk, debounced, so rapid edits
 * (sliders, typing) don't spam the backend.
 */
export function useSettings(notify: (msg: string, kind?: "info" | "success" | "error") => void) {
  const [ui, setUi] = useState<UiSettings>(DEFAULT_UI_SETTINGS);
  const loaded = useRef(false);

  useEffect(() => {
    (async () => {
      try {
        const s = await getUiSettings();
        const merged = {
          ...DEFAULT_UI_SETTINGS,
          ...s,
          terminal: { ...DEFAULT_UI_SETTINGS.terminal, ...(s.terminal ?? {}) },
          transfer: { ...DEFAULT_UI_SETTINGS.transfer, ...(s.transfer ?? {}) },
        };
        merged.accentColor = normalizeHexColor(merged.accentColor);
        merged.terminal.cursorStyle = normalizeCursorStyle(merged.terminal.cursorStyle);
        merged.terminal.bellStyle = normalizeBellStyle((merged.terminal as UiSettings["terminal"]).bellStyle);
        const lockMins = Number((merged as UiSettings).vaultAutoLockMinutes);
        merged.vaultAutoLockMinutes =
          Number.isFinite(lockMins) && lockMins >= 0
            ? Math.round(lockMins)
            : DEFAULT_UI_SETTINGS.vaultAutoLockMinutes;
        merged.defaultForwardAgent = Boolean((merged as UiSettings).defaultForwardAgent);
        merged.defaultPrivateKey =
          typeof (merged as UiSettings).defaultPrivateKey === "string"
            ? (merged as UiSettings).defaultPrivateKey
            : DEFAULT_UI_SETTINGS.defaultPrivateKey;
        setUi(merged);
      } catch {
        // not running under Tauri (vite dev alone) — keep defaults
      } finally {
        loaded.current = true;
      }
    })();
  }, []);

  // Debounced write-through whenever settings change (after initial load).
  useEffect(() => {
    if (!loaded.current) return;
    const h = window.setTimeout(() => {
      void setUiSettings(ui).catch(() => {});
    }, 500);
    return () => window.clearTimeout(h);
  }, [ui]);

  const update = useCallback((patch: Partial<UiSettings>) => {
    setUi((prev) => ({ ...prev, ...patch }));
  }, []);

  const updateTerminal = useCallback((patch: Partial<UiSettings["terminal"]>) => {
    setUi((prev) => ({ ...prev, terminal: { ...prev.terminal, ...patch } }));
  }, []);

  const updateTransfer = useCallback((patch: Partial<TransferSettings>) => {
    setUi((prev) => ({ ...prev, transfer: { ...prev.transfer, ...patch } }));
  }, []);

  const save = useCallback(async () => {
    try {
      await setUiSettings(ui);
      notify("Settings saved", "success");
    } catch (e) {
      notify(`Could not save settings: ${String(e)}`, "error");
    }
  }, [ui, notify]);

  return { ui, update, updateTerminal, updateTransfer, save };
}
