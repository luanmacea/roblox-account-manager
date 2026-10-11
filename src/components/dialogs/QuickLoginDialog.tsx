import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw, Smartphone, X } from "lucide-react";
import { useBackdropClose } from "../../hooks/useBackdropClose";
import { useEscapeStack } from "../../hooks/useEscapeStack";
import { useTr } from "../../i18n/text";
import { useStore } from "../../store";
import { maskAccountName } from "../../utils/accountName";

/** De quanto em quanto tempo o app pergunta ao Roblox se o código foi aprovado. */
export const QUICK_LOGIN_POLL_MS = 3000;

type Poll =
  | { kind: "pending" }
  | { kind: "linked"; accountName?: string | null }
  | { kind: "cancelled" }
  | { kind: "expired" }
  | { kind: "added"; userId: number; username: string; alreadySaved: boolean };

type Phase =
  | { kind: "starting" }
  | { kind: "waiting"; code: string; linkedName: string | null; linked: boolean }
  | { kind: "ended"; reason: "cancelled" | "expired" }
  | { kind: "error"; message: string };

interface QuickLoginDialogProps {
  open: boolean;
  onClose: () => void;
}

/**
 * Adicionar conta por Quick Login (ideia 12): mostra o código, a pessoa aprova
 * num aparelho já logado e a conta entra sozinha. A chave do login nunca sai do
 * backend; a tela só vê o código e o estado. Fechar cancela o login.
 */
export function QuickLoginDialog({ open, onClose }: QuickLoginDialogProps) {
  const t = useTr();
  const store = useStore();
  const backdropClose = useBackdropClose(onClose);
  const [phase, setPhase] = useState<Phase>({ kind: "starting" });
  const runId = useRef(0);
  const storeRef = useRef(store);
  storeRef.current = store;

  useEscapeStack(open, onClose);

  const start = useCallback(() => {
    const id = ++runId.current;
    setPhase({ kind: "starting" });
    invoke<{ code: string; expiresAt?: string | null }>("add_by_quick_login_start")
      .then((started) => {
        if (runId.current !== id) return;
        setPhase({ kind: "waiting", code: started.code, linkedName: null, linked: false });
      })
      .catch((e) => {
        if (runId.current === id) setPhase({ kind: "error", message: String(e) });
      });
  }, []);

  // Abre → pede o código; fecha → cancela o login no backend.
  useEffect(() => {
    if (!open) return;
    start();
    return () => {
      runId.current++;
      void invoke("add_by_quick_login_cancel").catch(() => {});
    };
  }, [open, start]);

  const waiting = phase.kind === "waiting";
  useEffect(() => {
    if (!open || !waiting) return;
    const id = runId.current;
    let busy = false;
    const timer = window.setInterval(() => {
      if (busy) return;
      busy = true;
      invoke<Poll>("add_by_quick_login_poll")
        .then(async (poll) => {
          if (runId.current !== id) return;
          switch (poll.kind) {
            case "pending":
              return;
            case "linked":
              setPhase((prev) =>
                prev.kind === "waiting" ? { ...prev, linked: true, linkedName: poll.accountName ?? null } : prev
              );
              return;
            case "cancelled":
            case "expired":
              setPhase({ kind: "ended", reason: poll.kind });
              return;
            case "added": {
              runId.current++;
              const s = storeRef.current;
              await s.loadAccounts();
              const name = maskAccountName(poll.username, s.hideUsernames, s.hiddenNameLetters);
              s.addToast(t(poll.alreadySaved ? "Updated {{name}}" : "Added {{name}}", { name }), "success");
              onClose();
              return;
            }
          }
        })
        .catch((e) => {
          if (runId.current === id) setPhase({ kind: "error", message: String(e) });
        })
        .finally(() => {
          busy = false;
        });
    }, QUICK_LOGIN_POLL_MS);
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, waiting]);

  if (!open) return null;

  const button =
    "flex items-center gap-1.5 rounded-lg border theme-border px-3 py-1.5 text-[12px] text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] transition-colors";

  return (
    <div
      className="fixed inset-0 z-[80] flex items-center justify-center bg-black/60 backdrop-blur-sm animate-fade-in"
      {...backdropClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t("Add with Quick Login")}
        className="theme-modal-scope theme-panel theme-border border rounded-2xl shadow-2xl w-[440px] max-w-[calc(100vw-24px)] max-h-[calc(100vh-24px)] overflow-y-auto animate-scale-in"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b theme-border">
          <h2 className="text-sm font-semibold text-[var(--panel-fg)]">{t("Add with Quick Login")}</h2>
          <button onClick={onClose} className="theme-muted hover:opacity-100 transition-opacity" aria-label={t("Close")}>
            <X size={16} strokeWidth={2} />
          </button>
        </div>

        <div className="px-4 py-4 space-y-3">
          {phase.kind === "starting" && (
            <p role="status" className="text-[12px] theme-muted animate-pulse">
              {t("Asking Roblox for a code...")}
            </p>
          )}

          {phase.kind === "waiting" && (
            <>
              <div className="flex flex-col items-center gap-1 py-2">
                <span className="text-[11px] uppercase tracking-wide theme-muted">{t("Your code")}</span>
                <span
                  data-testid="quick-login-code"
                  className="font-mono text-3xl font-semibold tracking-[0.3em] text-[var(--panel-fg)] select-all"
                >
                  {phase.code}
                </span>
              </div>
              <ol className="list-decimal pl-5 space-y-1 text-[12.5px] text-[var(--panel-fg)]">
                <li>
                  {t(
                    "On a phone or computer already signed in to the account, open roblox.com/crossdevicelogin (in the Roblox app: Settings › Quick Log In)."
                  )}
                </li>
                <li>{t("Type the code above and confirm.")}</li>
                <li>{t("Keep this window open: the account is added by itself.")}</li>
              </ol>
              <p role="status" className="flex items-center gap-2 text-[12px] theme-muted">
                <Smartphone size={13} strokeWidth={1.8} />
                {phase.linked
                  ? phase.linkedName
                    ? t("Code entered for {{name}}. Confirm it on the other device.", {
                        name: maskAccountName(phase.linkedName, store.hideUsernames, store.hiddenNameLetters),
                      })
                    : t("Code entered. Confirm it on the other device.")
                  : t("Waiting for the code...")}
              </p>
              <p className="text-[11.5px] theme-muted">
                {t("No password or cookie is typed here. Only approve a code you see in this window.")}
              </p>
            </>
          )}

          {phase.kind === "ended" && (
            <p className="text-[12.5px] text-[var(--panel-fg)]">
              {phase.reason === "cancelled"
                ? t("The sign-in was cancelled on the other device.")
                : t("This code expired before it was approved.")}
            </p>
          )}

          {phase.kind === "error" && (
            <p role="alert" className="rounded-lg border border-red-500/20 bg-red-500/10 px-3 py-2 text-[12px] text-red-400">
              {phase.message}
            </p>
          )}

          {(phase.kind === "ended" || phase.kind === "error") && (
            <div className="flex justify-end">
              <button type="button" onClick={start} className={button}>
                <RefreshCw size={13} strokeWidth={1.8} />
                {t("Get a new code")}
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
