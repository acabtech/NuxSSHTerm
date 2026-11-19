// Settings page (v0.5.0): Appearance / Terminal persist for real via
// UiSettings; Connections & Security and File Transfer mirror the mockup's
// clearly-labeled simulated sections (per DESIGN.md — don't invent backend
// behavior). Shortcuts & About closes the loop.

import { useState } from "react";
import { accentSoftFromHex, normalizeHexColor } from "../lib/color";
import type { CursorStyle, TransferSettings, UiSettings } from "../types";

type Section = "appearance" | "terminal" | "conn" | "transfer" | "shortcuts";

const SECTIONS: Array<{ id: Section; label: string }> = [
  { id: "appearance", label: "Appearance" },
  { id: "terminal", label: "Terminal" },
  { id: "conn", label: "Connections & Security" },
  { id: "transfer", label: "File Transfer" },
  { id: "shortcuts", label: "Shortcuts & About" },
];

interface Props {
  ui: UiSettings;
  onPatch: (patch: Partial<UiSettings>) => void;
  onPatchTerminal: (patch: Partial<UiSettings["terminal"]>) => void;
  onPatchTransfer: (patch: Partial<TransferSettings>) => void;
  onPatchSidebar: (patch: Partial<UiSettings["sidebar"]>) => void;
  onPatchAuth: (patch: Partial<UiSettings["auth"]>) => void;
  onSave: () => void;
  onBack: () => void;
}

export function SettingsPage({ ui, onPatch, onPatchTerminal, onPatchTransfer, onPatchSidebar, onPatchAuth, onSave, onBack }: Props) {
  const [section, setSection] = useState<Section>("appearance");
  const [saved, setSaved] = useState(false);

  const save = () => {
    onSave();
    setSaved(true);
    window.setTimeout(() => setSaved(false), 1600);
  };

  return (
    <div className="settings-page">
      <div className="set-side">
        <button className="btn ghost" onClick={onBack}>
          ← Back to Workspace
        </button>
        <h2>Settings</h2>
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            className={`set-nav${section === s.id ? " active" : ""}`}
            onClick={() => setSection(s.id)}
          >
            {s.label}
          </button>
        ))}
      </div>

      <div className="set-main">
        {section === "appearance" && (
          <div className="card">
            <h3>Appearance</h3>
            <label>
              Theme
              <select value="dark" disabled title="Light theme lands in v0.6">
                <option value="dark">Dark (modern)</option>
                <option value="light">Light (coming soon)</option>
              </select>
            </label>
            <label>
              Accent color
              <div className="row" style={{ gap: 8, alignItems: "center" }}>
                <input
                  type="color"
                  value={normalizeHexColor(ui.accentColor)}
                  onChange={(e) => onPatch({ accentColor: e.target.value })}
                  aria-label="Pick accent color"
                  style={{ width: 44, minWidth: 44, padding: 2, height: 34 }}
                />
                <input
                  type="text"
                  value={ui.accentColor}
                  onChange={(e) => onPatch({ accentColor: e.target.value })}
                  onBlur={() => onPatch({ accentColor: normalizeHexColor(ui.accentColor) })}
                  spellCheck={false}
                  placeholder="#3b82f6"
                  aria-label="Accent color hex value"
                />
              </div>
              <div className="note">
                Applied live across focus, hover, and selection highlights. Derived soft fill:{" "}
                <span className="mono">{accentSoftFromHex(normalizeHexColor(ui.accentColor))}</span>
              </div>
            </label>
            <label>
              Density
              <select
                value={ui.density}
                onChange={(e) => onPatch({ density: e.target.value as UiSettings["density"] })}
              >
                <option value="comfortable">Comfortable</option>
                <option value="compact">Compact</option>
              </select>
            </label>
            <label>
              Font scale ({Math.round(ui.fontScale * 100)}%)
              <input
                type="range"
                min={0.85}
                max={1.3}
                step={0.05}
                value={ui.fontScale}
                onChange={(e) => onPatch({ fontScale: parseFloat(e.target.value) })}
              />
            </label>
            <label className="row">
              <input
                type="checkbox"
                checked={ui.showConfig}
                onChange={(e) => onPatch({ showConfig: e.target.checked })}
              />
              Show configuration panel under connections
            </label>
            <h3>Sidebar</h3>
            <label>
              Expanded width ({ui.sidebar.width}px — also drag the sidebar edge to resize)
              <input
                type="range"
                min={200}
                max={560}
                step={4}
                value={ui.sidebar.width}
                onChange={(e) => onPatchSidebar({ width: parseInt(e.target.value, 10) })}
              />
            </label>
            <label>
              Collapse behavior
              <select
                value={ui.sidebar.collapseMode}
                onChange={(e) =>
                  onPatchSidebar({ collapseMode: e.target.value as UiSettings["sidebar"]["collapseMode"] })
                }
              >
                <option value="hover">Collapse when the pointer leaves</option>
                <option value="never">Never collapse (pinned open)</option>
              </select>
            </label>
            {ui.sidebar.collapseMode === "hover" && (
              <label>
                Collapse delay (
                {ui.sidebar.collapseDelayMs % 1000 === 0
                  ? `${ui.sidebar.collapseDelayMs / 1000}s`
                  : `${ui.sidebar.collapseDelayMs}ms`}
                )
                <input
                  type="range"
                  min={0}
                  max={20000}
                  step={1000}
                  value={ui.sidebar.collapseDelayMs}
                  onChange={(e) => onPatchSidebar({ collapseDelayMs: parseInt(e.target.value, 10) })}
                />
                <div className="note">0 collapses immediately on pointer leave; up to 20s grace (1s steps).</div>
              </label>
            )}
          </div>
        )}

        {section === "terminal" && (
          <div className="card">
            <h3>Terminal</h3>
            <label>
              Font family
              <input value="JetBrains Mono, monospace" readOnly title="Custom fonts land in v0.6" />
            </label>
            <label>
              Font size
              <input
                type="number"
                min={10}
                max={24}
                value={ui.terminal.fontSize}
                onChange={(e) => onPatchTerminal({ fontSize: clamp(parseInt(e.target.value, 10) || 13, 10, 24) })}
              />
            </label>
            <label className="row">
              <input
                type="checkbox"
                checked={ui.terminal.cursorBlink}
                onChange={(e) => onPatchTerminal({ cursorBlink: e.target.checked })}
              />
              Cursor blink
            </label>
            <label>
              Cursor style
              <select
                value={ui.terminal.cursorStyle}
                onChange={(e) => onPatchTerminal({ cursorStyle: e.target.value as CursorStyle })}
              >
                <option value="block">Block</option>
                <option value="underline">Underline</option>
                <option value="bar">Bar</option>
              </select>
            </label>
            <label>
              Bell notifications
              <select
                value={ui.terminal.bellStyle}
                onChange={(e) =>
                  onPatchTerminal({ bellStyle: e.target.value as UiSettings["terminal"]["bellStyle"] })
                }
              >
                <option value="none">None</option>
                <option value="visual">Visual flash</option>
                <option value="sound">Sound</option>
                <option value="both">Visual + sound</option>
              </select>
            </label>
            <label>
              Scrollback lines
              <input
                type="number"
                min={500}
                max={200000}
                step={500}
                value={ui.terminal.scrollback}
                onChange={(e) =>
                  onPatchTerminal({ scrollback: clamp(parseInt(e.target.value, 10) || 10000, 500, 200000) })
                }
              />
            </label>
            <div className="note">Terminal options apply to newly opened tabs (bell applies to new tabs).</div>
          </div>
        )}

        {section === "conn" && (
          <>
          <div className="card">
            <h3>Connections & Security</h3>
            <label className="row">
              <input
                type="checkbox"
                checked={ui.defaultForwardAgent}
                onChange={(e) => onPatch({ defaultForwardAgent: e.target.checked })}
              />{" "}
              Forward SSH agent by default for new connections
            </label>
            <label>
              Default private key path (new connections)
              <input
                type="text"
                value={ui.defaultPrivateKey}
                onChange={(e) => onPatch({ defaultPrivateKey: e.target.value })}
                placeholder="e.g. ~/.ssh/id_ed25519"
                spellCheck={false}
              />
            </label>
            <label>
              Vault auto-lock
              <select
                value={ui.vaultAutoLockMinutes}
                onChange={(e) => onPatch({ vaultAutoLockMinutes: parseInt(e.target.value, 10) || 0 })}
              >
                <option value={0}>Never</option>
                <option value={5}>After 5 minutes of inactivity</option>
                <option value={15}>After 15 minutes of inactivity</option>
                <option value={30}>After 30 minutes of inactivity</option>
                <option value={60}>After 60 minutes of inactivity</option>
              </select>
            </label>
            <div className="note">
              Forwarding and key defaults are applied when creating new connection entries; edit existing sessions
              in their Configuration panel. Vault auto-lock is applied while the vault is unlocked and any
              mouse/keyboard/touch activity resets the timer. Manage the running agent via Tools → SSH Key Manager
              and the vault via Tools → Master Password…
            </div>
          </div>
          <div className="card">
            <h3>SSH Authentication</h3>
            <label>
              Authentication order
              <select
                value={ui.auth.authOrder}
                onChange={(e) => onPatchAuth({ authOrder: e.target.value as UiSettings["auth"]["authOrder"] })}
              >
                <option value="credential_first">Prefer session credential (key / vault password)</option>
                <option value="agent_first">Prefer ssh-agent identities</option>
                <option value="agent_only">Only ssh-agent identities (ignore stored credential)</option>
              </select>
            </label>
            <label>
              Agent fallback
              <select
                value={ui.auth.agentFallback ? "1" : "0"}
                disabled={ui.auth.authOrder !== "credential_first"}
                onChange={(e) => onPatchAuth({ agentFallback: e.target.value === "1" })}
              >
                <option value="0">Never — use the session credential only</option>
                <option value="1">Offer ssh-agent identities after the credential</option>
              </select>
            </label>
            <div className="note">
              "Prefer session credential" pins the connection's key/password first (IdentitiesOnly while no
              fallback is configured) and offers ssh-agent identities after it when the fallback is enabled.
              Applies to newly opened terminal and SFTP tabs.
            </div>
          </div>
          </>
        )}

        {section === "transfer" && (
          <div className="card">
            <h3>File Transfer</h3>
            <label>
              Concurrent transfers
              <input value="1" readOnly title="Parallel transfer engine lands with the russh driver" />
            </label>
            <label className="row">
              <input
                type="checkbox"
                checked={ui.transfer.confirmBeforeOverwrite}
                onChange={(e) => onPatchTransfer({ confirmBeforeOverwrite: e.target.checked })}
              />
              Confirm before overwrite
            </label>
            <label className="row">
              <input
                type="checkbox"
                checked={ui.transfer.confirmBeforeDelete}
                onChange={(e) => onPatchTransfer({ confirmBeforeDelete: e.target.checked })}
              />
              Confirm before delete
            </label>
            <label>
              Default local path
              <input
                type="text"
                value={ui.transfer.defaultLocalPath}
                onChange={(e) => onPatchTransfer({ defaultLocalPath: e.target.value })}
                placeholder="(use home directory)"
              />
            </label>
            <div className="note">
              Applied to new SFTP commander tabs. Overwrite checks run before copy/move; delete confirmation
              respects the toggle above. Concurrent transfer engine options land with the russh driver.
            </div>
          </div>
        )}

        {section === "shortcuts" && (
          <div className="card">
            <h3>Shortcuts & About</h3>
            <div className="kvs">
              <div><b>Ctrl+,</b> Settings</div>
              <div><b>Ctrl+T</b> New local shell</div>
              <div><b>Ctrl+W</b> Close tab</div>
              <div><b>Shift+Ctrl+C</b> Copy from terminal</div>
              <div><b>Ctrl+V</b> Paste into terminal (Edit menu)</div>
              <div><b>F5–F9</b> Commander copy / move / mkdir / delete / properties</div>
            </div>
            <p className="note">
              NuxSSHTerm v0.5.0 — modernized UI on the real Tauri backend: sessions via system OpenSSH over
              a PTY, SFTP commander, encrypted vault and ssh-agent manager included.
            </p>
          </div>
        )}

        <div className="set-actions">
          <button className="btn primary" onClick={save}>
            Save changes
          </button>
          {saved && <span className="saved">Saved</span>}
        </div>
      </div>
    </div>
  );
}

function clamp(n: number, lo: number, hi: number): number {
  return Math.min(Math.max(n, lo), hi);
}
