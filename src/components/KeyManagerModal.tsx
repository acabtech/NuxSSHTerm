import { useCallback, useEffect, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import {
  agentAdd,
  agentRemove,
  agentRemoveAll,
  agentStart,
  agentStatus,
  agentStop,
  type AddKeyResult,
  type AgentStatus,
} from "../api";

/**
 * Tools → SSH key manager… (Pageant equivalent).
 *
 * Shows the ssh-agent state — external (`SSH_AUTH_SOCK` inherited) or the
 * dedicated agent NuxSSHTerm spawned — lists identities (`ssh-add -l`), and
 * offers add (with an on-demand passphrase prompt via the askpass helper),
 * remove one, and remove all. Start/Stop manage the dedicated agent only.
 */
export function KeyManagerModal({
  onClose,
  notify,
}: {
  onClose: () => void;
  notify: (msg: string, kind?: "info" | "success" | "error") => void;
}) {
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  // While an add needs a passphrase we remember the pending key path so the
  // retry can send the phrase back to the same key.
  const [pendingKey, setPendingKey] = useState<string | null>(null);
  const [passphrase, setPassphrase] = useState("");

  const refresh = useCallback(async () => {
    try {
      const s = await agentStatus();
      setStatus(s);
      setError("");
      if (!s.present) {
        setPendingKey(null);
        setPassphrase("");
      }
    } catch (e) {
      setError(`Agent status failed: ${String(e)}`);
    }
    setBusy(false);
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const notifyAdd = useCallback(
    (r: AddKeyResult) => {
      if (r.added) {
        setPendingKey(null);
        setPassphrase("");
        notify("Key added to the agent", "success");
      } else if (r.needs_passphrase) {
        setPendingKey(null);
        setPassphrase("");
        notify("This key is passphrase-protected — enter its passphrase", "error");
      } else {
        notify(r.message || "Could not add key", "error");
      }
    },
    [notify],
  );

  const start = useCallback(async () => {
    setBusy(true);
    try {
      const s = await agentStart();
      setStatus(s);
      setError("");
      setPendingKey(null);
      setPassphrase("");
      notify(
        s.present
          ? (s.ours ? "Dedicated ssh-agent started" : "Using the available SSH agent")
          : "No SSH agent available",
        "success",
      );
    } catch (e) {
      setError(`Could not start agent: ${String(e)}`);
    }
    setBusy(false);
  }, [notify]);

  const stop = useCallback(async () => {
    setBusy(true);
    try {
      await agentStop();
      setStatus(null);
      setPendingKey(null);
      setPassphrase("");
      notify("Dedicated ssh-agent stopped", "success");
    } catch (e) {
      setError(`Could not stop agent: ${String(e)}`);
    }
    setBusy(false);
    void refresh();
  }, [notify, refresh]);

  const remove = useCallback(
    async (key: string) => {
      setBusy(true);
      try {
        await agentRemove(key);
        setPendingKey(null);
        notify("Key removed", "success");
      } catch (e) {
        setError(`Remove failed: ${String(e)}`);
      }
      setBusy(false);
      void refresh();
    },
    [notify, refresh],
  );

  const removeAll = useCallback(async () => {
    setBusy(true);
    try {
      await agentRemoveAll();
      setPendingKey(null);
      notify("All keys removed", "success");
    } catch (e) {
      setError(`Remove all failed: ${String(e)}`);
    }
    setBusy(false);
    void refresh();
  }, [notify, refresh]);

  const browse = useCallback(async () => {
    try {
      const selected = await openFileDialog({
        multiple: false,
        directory: false,
        title: "Select an OpenSSH private key to add to the agent",
        filters: [
          { name: "SSH private keys", extensions: ["pem", "key", "ed25519", "rsa", "ecdsa", "id_ed25519", "id_rsa"] },
          { name: "All files", extensions: ["*"] },
        ],
      });
      if (typeof selected !== "string" || !selected) return;
      setBusy(true);
      const r = await agentAdd(selected);
      notifyAdd(r);
      if (r.added || r.needs_passphrase) void refresh();
      // Let the UI retry with a passphrase when the key is encrypted.
      if (!r.added && r.needs_passphrase) setPendingKey(selected);
    } catch (e) {
      setError(`Add key failed: ${String(e)}`);
    }
    setBusy(false);
  }, [notifyAdd, refresh]);

  const submitPassphrase = useCallback(async () => {
    if (!pendingKey) return;
    setBusy(true);
    try {
      const r = await agentAdd(pendingKey, passphrase);
      notifyAdd(r);
      if (r.added) {
        setPendingKey(null);
        setPassphrase("");
      } else if (r.needs_passphrase) {
        notify("Wrong passphrase — try again", "error");
      }
      void refresh();
    } catch (e) {
      setError(`Add key failed: ${String(e)}`);
    }
    setBusy(false);
  }, [pendingKey, passphrase, notifyAdd, refresh]);

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="modal modal-wide" onMouseDown={(e) => e.stopPropagation()}>
        <h3>SSH Key Manager (Pageant)</h3>
        <div className="modal-body">
          {!status ? (
            <p className="muted">Querying ssh-agent…</p>
          ) : (
            <>
              <div className="agent-status-line">
                {status.present ? (
                  <>
                    <span className="agent-dot ok" />
                    <b>{status.ours ? "Dedicated agent" : "External agent"}</b>
                    <span className="muted mono" title="Socket">
                      {status.socket ?? ""}
                    </span>
                  </>
                ) : (
                  <>
                    <span className="agent-dot off" />
                    <span>No SSH agent running</span>
                    <span className="muted" style={{ marginLeft: 8 }}>
                      {status.socket ? " (stale socket)" : " — start a dedicated one below"}
                    </span>
                  </>
                )}
              </div>

              <table className="agent-keys">
                <thead>
                  <tr>
                    <th>Bits</th>
                    <th>Fingerprint</th>
                    <th>Comment</th>
                    <th>Type</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {status.keys.length === 0 && (
                    <tr>
                      <td colSpan={5} className="muted">
                        {status.present ? "The agent has no identities." : "Start an agent to add keys."}
                      </td>
                    </tr>
                  )}
                  {status.keys.map((k, i) => (
                    <tr key={`${k.fingerprint}-${i}`}>
                      <td>{k.bits}</td>
                      <td className="mono">{k.fingerprint}</td>
                      <td>{k.comment}</td>
                      <td>{k.key_type}</td>
                      <td>
                        <button
                          className="btn"
                          disabled={busy}
                          title={`Remove ${k.comment || k.fingerprint} from the agent`}
                          onClick={() => void remove(k.fingerprint)}
                        >
                          Remove
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>

              {pendingKey && (
                <div className="agent-passphrase">
                  <p className="muted">This key is passphrase-protected:</p>
                  <input
                    type="password"
                    autoFocus
                    placeholder="Key passphrase"
                    value={passphrase}
                    onChange={(e) => setPassphrase(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && void submitPassphrase()}
                  />
                  <button className="btn primary" onClick={() => void submitPassphrase()}>
                    Add key
                  </button>
                </div>
              )}

              {error && (
                <p className="muted" style={{ color: "#e06c75", marginBottom: 0 }}>
                  {error}
                </p>
              )}
            </>
          )}
        </div>
        <div className="modal-actions">
          <button className="btn" onClick={() => void refresh()}>
            Refresh
          </button>
          {status?.present ? (
            status.ours ? (
              <button className="btn" disabled={busy} onClick={() => void stop()}>
                Stop agent
              </button>
            ) : (
              <span className="muted">external agent — not managed here</span>
            )
          ) : (
            <button className="btn primary" disabled={busy} onClick={() => void start()}>
              Start agent
            </button>
          )}
          {status?.present && (
            <>
              <button className="btn" disabled={busy} onClick={() => void browse()}>
                Add key…
              </button>
              <button className="btn" disabled={busy} onClick={() => void removeAll()}>
                Remove all
              </button>
            </>
          )}
          <button className="btn" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}