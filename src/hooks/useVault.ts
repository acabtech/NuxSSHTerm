import { useCallback, useEffect, useState } from "react";
import {
  getVaultStatus,
  vaultGetPasswords,
  vaultInit,
  vaultLock,
  vaultUnlockAuto,
  vaultPutPassword,
  vaultRemovePassword,
  vaultReset,
  vaultUnlock,
  type VaultStatus,
} from "../api";
/**
 * Owns the encrypted-vault lifecycle: whether it is initialized and unlocked,
 * plus the operations to create/unlock/lock/reset it and to read/write session
 * passwords. `notify` is wired to toasts for status/error messages.
 */
export function useVault(notify: (msg: string, kind?: "info" | "success" | "error") => void) {
  const [status, setStatus] = useState<VaultStatus>({ initialized: false, unlocked: false });
  const [checking, setChecking] = useState(true);

  // initial status probe
  useEffect(() => {
    (async () => {
      try {
        let status = await getVaultStatus();
        // Opt-in auto-unlock (issue #22): try the configured API key before the
        // interactive prompt. A missing/revoked key silently falls back to the
        // existing master-password modal — no startup error spam.
        if (status.initialized && !status.unlocked) {
          try {
            await vaultUnlockAuto();
            status = await getVaultStatus();
          } catch {
            // fall through to the normal unlock prompt
          }
        }
        setStatus(status);
      } catch (e) {
        notify(`Vault status failed: ${String(e)}`, "error");
      } finally {
        setChecking(false);
      }
    })();
  }, [notify]);

  const init = useCallback(async (master: string) => {
    await vaultInit(master);
    setStatus({ initialized: true, unlocked: true });
  }, []);

  const unlock = useCallback(async (master: string) => {
    await vaultUnlock(master);
    setStatus((s) => ({ ...s, unlocked: true }));
  }, []);

  const lock = useCallback(async () => {
    await vaultLock();
    setStatus((s) => ({ ...s, unlocked: false }));
  }, []);

  const reset = useCallback(async () => {
    await vaultReset();
    setStatus({ initialized: false, unlocked: false });
  }, []);

  const getPasswords = useCallback(async () => vaultGetPasswords(), []);

  const putPassword = useCallback(async (path: string, password: string) => {
    await vaultPutPassword(path, password);
  }, []);

  const removePassword = useCallback(async (path: string) => {
    await vaultRemovePassword(path);
  }, []);

  const getPassword = useCallback(
    async (path: string) => (await vaultGetPasswords())[path],
    [],
  );

  return {
    status,
    checking,
    init,
    unlock,
    lock,
    reset,
    getPasswords,
    putPassword,
    removePassword,
    getPassword,
  };
}