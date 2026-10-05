import { useState } from "react";

export type VaultModalKind = "vault-init" | "vault-unlock" | "vault-reset";

/**
 * Modal for the encrypted-vault lifecycle: first-run "set master password"
 * wizard, the unlock dialog, and the forgot-password reset confirmation.
 */
export function VaultModal({
  kind,
  onClose,
  onSubmit,
  onForgot,
}: {
  kind: VaultModalKind;
  onClose: () => void;
  onSubmit: (master: string) => void;
  onForgot?: () => void;
}) {
  const [pw, setPw] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState("");

  const isInit = kind === "vault-init";
  const isReset = kind === "vault-reset";

  const submit = () => {
    if (isReset) {
      onSubmit("");
      return;
    }
    if (isInit) {
      if (pw.length < 4) {
        setError("Master password must be at least 4 characters.");
        return;
      }
      if (pw !== confirm) {
        setError("Passwords do not match.");
        return;
      }
    } else if (!pw) {
      setError("Enter your master password.");
      return;
    }
    onSubmit(pw);
  };

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
        <h3>{isInit ? "Set master password" : isReset ? "Reset vault" : "Unlock vault"}</h3>
        <div className="modal-body">
          {isReset ? (
            <p className="muted">
              This wipes the encrypted vault and all stored session passwords. Your connections
              (hosts, users, keys) are kept. This cannot be undone.
            </p>
          ) : (
            <>
              <p className="muted">
                {isInit
                  ? "Create a master password to encrypt stored session passwords (Argon2id + AES-256-GCM)."
                  : "Enter your master password to unlock stored session passwords."}
              </p>
              <input
                type="password"
                autoFocus
                placeholder="Master password"
                value={pw}
                onChange={(e) => {
                  setPw(e.target.value);
                  setError("");
                }}
                onKeyDown={(e) => e.key === "Enter" && submit()}
              />
              {isInit && (
                <input
                  type="password"
                  placeholder="Confirm master password"
                  value={confirm}
                  onChange={(e) => {
                    setConfirm(e.target.value);
                    setError("");
                  }}
                  onKeyDown={(e) => e.key === "Enter" && submit()}
                />
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
          <button className="btn" onClick={onClose}>
            {isInit ? "Skip" : "Cancel"}
          </button>
          {!isReset && onForgot && (
            <button className="btn" onClick={onForgot}>
              Forgot password?
            </button>
          )}
          <button className="btn primary" onClick={submit}>
            {isInit ? "Set password" : isReset ? "Reset vault" : "Unlock"}
          </button>
        </div>
      </div>
    </div>
  );
}