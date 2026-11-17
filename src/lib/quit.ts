import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * Quit the app. Closes the main Tauri window, which brings down the whole
 * process. Falls back to window.close() when running outside Tauri (e.g.
 * plain `vite` in a browser).
 */
export function quitApp(): void {
  try {
    void getCurrentWindow()
      .close()
      .catch(() => {
        window.close();
      });
  } catch {
    window.close();
  }
}
