import { useCallback } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

/**
 * "Open config folder" — reveals the config directory in the system file
 * manager (opener:default allows reveal, not open-path). Falls back to a
 * toast-style error message when Tauri's opener is unavailable.
 */
export function useRevealConfigDir(
  configDir: string,
  notify: (msg: string, kind?: "info" | "success" | "error") => void,
) {
  return useCallback(() => {
    if (!configDir) {
      notify("Config folder not known yet", "error");
      return;
    }
    revealItemInDir(configDir)
      .then(() => notify(`Opened ${configDir}`))
      .catch((e) => notify(`Could not open config folder: ${String(e)}\nPath: ${configDir}`, "error"));
  }, [configDir, notify]);
}
