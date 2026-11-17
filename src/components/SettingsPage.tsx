// Settings page (v0.5.0): Appearance / Terminal persist for real via
// UiSettings; Connections & Security and File Transfer mirror the mockup's
// clearly-labeled simulated sections (per DESIGN.md — don't invent backend
// behavior). Shortcuts & About closes the loop.

import { useState } from "react";
import type { UiSettings } from "../types";

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
  onSave: () => void;
  onBack: () => void;
}

export function SettingsPage({ ui, onPatch, onPatchTerminal, onSave, onBack }: Props) {
  const [section, setSection] = useState<Section>("appearance");
  const [saved, setSaved] = useState(false);

  const save = () => {
    onSave();
    setSaved(true);
    window.setTimeout(() => setSaved(false), 1600);
  };

  // Simulated (per mockup) — real backend wiring lands with v0.6.
  const [agent] = useState(true);
  const [rememberUnlock] = useState(true);
  const [keyPath] = useState("~/.ssh/id_ed25519");
  const [concurrent] = useState(2);
  const [confirmOverwrite] = useState(true);
  const [localPath] = useState("~/Downloads");

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
              <input value="#3b82f6" readOnly title="Custom accent color lands in v0.6" />
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
            <div className="note">Terminal options apply to newly opened tabs.</div>
          </div>
        )}

        {section === "conn" && (
          <div className="card">
            <h3>Connections & Security</h3>
            <label className="row">
              <input type="checkbox" checked={agent} readOnly /> Use system SSH agent
            </label>
            <label className="row">
              <input type="checkbox" checked={rememberUnlock} readOnly /> Remember vault unlock for session
            </label>
            <label>
              Default key path
              <input value={keyPath} readOnly />
            </label>
            <div className="note">
              Security actions are simulated here — manage the real agent via Tools → SSH Key Manager and the
              vault via Tools → Master Password…
            </div>
          </div>
        )}

        {section === "transfer" && (
          <div className="card">
            <h3>File Transfer</h3>
            <label>
              Concurrent transfers
              <input value={String(concurrent)} readOnly />
            </label>
            <label className="row">
              <input type="checkbox" checked={confirmOverwrite} readOnly /> Confirm before overwrite
            </label>
            <label>
              Default local path
              <input value={localPath} readOnly />
            </label>
            <div className="note">
              Transfer engine options land with the russh driver; transfers currently run through the SFTP
              commander tabs.
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
