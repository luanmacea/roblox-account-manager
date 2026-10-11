import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, CheckCircle2, RefreshCw, X, XCircle } from "lucide-react";
import { useBackdropClose } from "../../hooks/useBackdropClose";
import { useEscapeStack } from "../../hooks/useEscapeStack";
import { useTr } from "../../i18n/text";
import { diagnosticText, overallStatus, type DiagnosticCheck, type DiagnosticStatus } from "../../utils/diagnostics";

interface DiagnosticsDialogProps {
  open: boolean;
  onClose: () => void;
}

const STATUS_ICON: Record<DiagnosticStatus, { icon: typeof CheckCircle2; className: string }> = {
  ok: { icon: CheckCircle2, className: "text-emerald-400" },
  warn: { icon: AlertTriangle, className: "text-amber-400" },
  problem: { icon: XCircle, className: "text-red-400" },
};

/**
 * "O launch não faz nada" (ideia 16): roda as checagens do backend
 * (`run_launch_diagnostics`) e mostra uma linha por item, com o que fazer. Só
 * lê — nenhum botão aqui fecha cliente ou mexe em arquivo do Roblox.
 */
export function DiagnosticsDialog({ open, onClose }: DiagnosticsDialogProps) {
  const t = useTr();
  const backdropClose = useBackdropClose(onClose);
  const [checks, setChecks] = useState<DiagnosticCheck[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const runId = useRef(0);

  useEscapeStack(open, onClose);

  const run = useCallback(() => {
    const id = ++runId.current;
    setRunning(true);
    setError(null);
    invoke<DiagnosticCheck[]>("run_launch_diagnostics")
      .then((result) => {
        if (runId.current === id) setChecks(Array.isArray(result) ? result : []);
      })
      .catch((e) => {
        if (runId.current === id) setError(String(e));
      })
      .finally(() => {
        if (runId.current === id) setRunning(false);
      });
  }, []);

  useEffect(() => {
    if (!open) {
      runId.current++;
      setChecks(null);
      setRunning(false);
      return;
    }
    run();
  }, [open, run]);

  if (!open) return null;

  const overall = checks ? overallStatus(checks) : null;

  return (
    <div
      className="fixed inset-0 z-[80] flex items-center justify-center bg-black/60 backdrop-blur-sm animate-fade-in"
      {...backdropClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t("Launch check")}
        className="theme-modal-scope theme-panel theme-border border rounded-2xl shadow-2xl w-[520px] max-w-[calc(100vw-24px)] max-h-[calc(100vh-24px)] overflow-y-auto animate-scale-in"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b theme-border">
          <h2 className="text-sm font-semibold text-[var(--panel-fg)]">{t("Launch check")}</h2>
          <button onClick={onClose} className="theme-muted hover:opacity-100 transition-opacity" aria-label={t("Close")}>
            <X size={16} strokeWidth={2} />
          </button>
        </div>

        <div className="px-4 py-3 space-y-3">
          <p className="text-[12px] theme-muted">
            {t("Checks the usual reasons a launch does nothing. It only looks; it never closes a Roblox window.")}
          </p>

          {overall && !running && (
            <p role="status" className="text-[13px] font-medium text-[var(--panel-fg)]">
              {overall === "ok"
                ? t("Nothing wrong found.")
                : overall === "warn"
                  ? t("Something here may be in the way.")
                  : t("Found a problem that stops launches.")}
            </p>
          )}

          {running && (
            <p role="status" className="text-[12px] theme-muted animate-pulse">
              {t("Checking...")}
            </p>
          )}

          {error && (
            <p className="rounded-lg border border-red-500/20 bg-red-500/10 px-3 py-2 text-[12px] text-red-400">
              {t("The check could not run: {{error}}", { error })}
            </p>
          )}

          {checks && (
            <ul className="space-y-1.5" aria-label={t("Check results")}>
              {checks.map((check) => {
                const { icon: Icon, className } = STATUS_ICON[check.status] ?? STATUS_ICON.warn;
                const text = diagnosticText(check, t);
                return (
                  <li
                    key={check.id}
                    data-status={check.status}
                    className="flex items-start gap-2.5 rounded-lg border theme-border px-3 py-2"
                  >
                    <Icon size={15} strokeWidth={1.9} className={`mt-0.5 shrink-0 ${className}`} aria-hidden="true" />
                    <span className="min-w-0">
                      <span className="block text-[13px] text-[var(--panel-fg)]">{text.title}</span>
                      {text.detail && <span className="block mt-0.5 text-[12px] theme-muted">{text.detail}</span>}
                    </span>
                  </li>
                );
              })}
            </ul>
          )}

          <div className="flex justify-end pt-1">
            <button
              type="button"
              onClick={run}
              disabled={running}
              className="flex items-center gap-1.5 rounded-lg border theme-border px-3 py-1.5 text-[12px] text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] transition-colors disabled:opacity-50"
            >
              <RefreshCw size={13} strokeWidth={1.8} className={running ? "animate-spin" : undefined} />
              {t("Check again")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
