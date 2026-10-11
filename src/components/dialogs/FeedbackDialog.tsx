import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowLeft, Bug, Copy, ExternalLink, Lightbulb, X } from "lucide-react";
import { useBackdropClose } from "../../hooks/useBackdropClose";
import { useEscapeStack } from "../../hooks/useEscapeStack";
import { useTr } from "../../i18n/text";
import type { Account } from "../../types";
import type { LaunchLogEntry } from "../../store";
import type { DiagnosticCheck } from "../../utils/diagnostics";
import { buildProblemReport, type ReportEnvironment } from "../../utils/problemReport";

export type FeedbackKind = "bug" | "idea";

/** De onde o resumo tira o Console e as contas (para anonimizar). */
export interface ReportSource {
  logs: LaunchLogEntry[];
  accounts: Account[];
}

interface FeedbackDialogProps {
  open: boolean;
  onClose: () => void;
  /** Sem isto o relato vai sem a opção de resumo. */
  reportSource?: () => ReportSource;
}

/**
 * Reportar problema ou sugerir ideia. O app **não envia nada**: abre o
 * formulário do GitHub no navegador (`open_feedback_form`, com os modelos de
 * `.github/ISSUE_TEMPLATE/`), e quem escreve e envia é a pessoa. Por isso o
 * texto diz que precisa de conta no GitHub e que nada sai do app sozinho.
 *
 * "Reportar problema" (ideia 28) pode levar um resumo de diagnóstico: versão,
 * edição, sistema, o resultado da checagem do launch e as últimas linhas do
 * Console, **anonimizados** (`utils/anonymize.ts`). O resumo só é montado se a
 * pessoa marcar a caixa, aparece inteiro na prévia e só sai do app pelo botão
 * de copiar — ela cola no formulário. Nada vai na URL.
 */
export function FeedbackDialog({ open, onClose, reportSource }: FeedbackDialogProps) {
  const t = useTr();
  const backdropClose = useBackdropClose(onClose);
  const [step, setStep] = useState<"choose" | "report">("choose");
  const [attach, setAttach] = useState(false);
  const [summary, setSummary] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const buildId = useRef(0);
  // Em ref: quem chama passa uma função nova a cada render, e ela não pode
  // disparar uma nova montagem do resumo (seria um laço).
  const sourceRef = useRef(reportSource);
  sourceRef.current = reportSource;
  const hasSource = !!reportSource;

  // Pela pilha de Escape: só o topo responde, então a página atrás (que volta
  // para a lista de contas com Esc) não fecha junto.
  useEscapeStack(open, onClose);

  useEffect(() => {
    if (open) return;
    buildId.current++;
    setStep("choose");
    setAttach(false);
    setSummary(null);
    setCopied(false);
  }, [open]);

  useEffect(() => {
    if (!open || step !== "report" || !attach || !sourceRef.current) return;
    const id = ++buildId.current;
    setSummary(null);
    setCopied(false);
    const source = sourceRef.current();
    void Promise.all([
      invoke<ReportEnvironment>("get_report_environment").catch(() => null),
      invoke<DiagnosticCheck[]>("run_launch_diagnostics").catch(() => null),
    ]).then(([env, diagnostics]) => {
      if (buildId.current !== id) return;
      setSummary(
        buildProblemReport({
          env: env ?? null,
          diagnostics: Array.isArray(diagnostics) ? diagnostics : null,
          logs: source.logs,
          accounts: source.accounts,
        })
      );
    });
  }, [open, step, attach, hasSource]);

  if (!open) return null;

  function openForm(kind: FeedbackKind) {
    onClose();
    void invoke("open_feedback_form", { kind }).catch(() => {});
  }

  async function copySummary() {
    if (!summary) return;
    try {
      await navigator.clipboard.writeText(summary);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }

  const option =
    "flex items-start gap-3 w-full px-3 py-3 text-left rounded-lg border theme-border hover:bg-[var(--panel-soft)] transition-colors outline-none focus-visible:shadow-[0_0_0_2px_var(--input-focus)]";
  const action =
    "flex items-center gap-1.5 rounded-lg border theme-border px-3 py-1.5 text-[12px] text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] transition-colors disabled:opacity-40 disabled:cursor-not-allowed";

  return (
    <div
      className="fixed inset-0 z-[80] flex items-center justify-center bg-black/60 backdrop-blur-sm animate-fade-in"
      {...backdropClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t("Send feedback")}
        className={`theme-modal-scope theme-panel theme-border border rounded-2xl shadow-2xl ${step === "report" ? "w-[560px]" : "w-[440px]"} max-w-[calc(100vw-24px)] max-h-[calc(100vh-24px)] overflow-y-auto animate-scale-in`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b theme-border">
          <h2 className="text-sm font-semibold text-[var(--panel-fg)]">{t("Send feedback")}</h2>
          <button onClick={onClose} className="theme-muted hover:opacity-100 transition-opacity" aria-label={t("Close")}>
            <X size={16} strokeWidth={2} />
          </button>
        </div>

        {step === "choose" ? (
          <div className="px-4 py-3 space-y-2">
            <button type="button" onClick={() => setStep("report")} className={option}>
              <Bug size={16} strokeWidth={1.75} className="mt-0.5 shrink-0 theme-muted" />
              <span className="min-w-0">
                <span className="block text-[13px] font-medium text-[var(--panel-fg)]">{t("Report a problem")}</span>
                <span className="block mt-0.5 text-[12px] theme-muted">{t("Something broke or did not work as expected.")}</span>
              </span>
            </button>
            <button type="button" onClick={() => openForm("idea")} className={option}>
              <Lightbulb size={16} strokeWidth={1.75} className="mt-0.5 shrink-0 theme-muted" />
              <span className="min-w-0">
                <span className="block text-[13px] font-medium text-[var(--panel-fg)]">{t("Suggest an idea")}</span>
                <span className="block mt-0.5 text-[12px] theme-muted">{t("Something you would like MultiAlt to do.")}</span>
              </span>
            </button>
            <p className="pt-1 text-[11.5px] leading-relaxed theme-muted">
              {t(
                "Opens a form on this project's GitHub page in your browser. You need a free GitHub account to send it. The app sends nothing on its own, and nothing about your accounts goes along."
              )}
            </p>
          </div>
        ) : (
          <div className="px-4 py-3 space-y-3">
            <button
              type="button"
              onClick={() => setStep("choose")}
              className="flex items-center gap-1 text-[12px] theme-muted hover:text-[var(--panel-fg)] transition-colors"
            >
              <ArrowLeft size={13} strokeWidth={1.8} />
              {t("Back")}
            </button>
            <p className="text-[13px] font-medium text-[var(--panel-fg)]">{t("Report a problem")}</p>

            {hasSource && (
              <label className="flex items-start gap-2 text-[12.5px] text-[var(--panel-fg)] cursor-pointer select-none">
                <input
                  type="checkbox"
                  checked={attach}
                  onChange={(e) => setAttach(e.target.checked)}
                  className="mt-0.5 accent-[var(--accent-color)]"
                />
                <span>
                  {t("Add a diagnostic summary")}
                  <span className="block text-[12px] theme-muted">
                    {t(
                      "App version, edition, Windows version, the launch check and the last console lines. Account names, user IDs, cookies, passwords, your Windows user name, server IDs and IP addresses are removed."
                    )}
                  </span>
                </span>
              </label>
            )}

            {attach && (
              <div className="space-y-1.5">
                <p className="text-[12px] theme-muted">
                  {t("This is everything that would go along. Read it before copying.")}
                </p>
                <textarea
                  readOnly
                  aria-label={t("Summary preview")}
                  value={summary ?? t("Building the summary...")}
                  className="theme-input w-full h-56 rounded-lg p-2.5 font-mono text-[11.5px] leading-relaxed resize-y"
                />
              </div>
            )}

            <p className="text-[11.5px] leading-relaxed theme-muted">
              {attach
                ? t(
                    "Copy the summary, open the form and paste it into the description. Nothing leaves your PC until you submit the form on GitHub."
                  )
                : t(
                    "Opens a form on this project's GitHub page in your browser. You need a free GitHub account to send it. The app sends nothing on its own, and nothing about your accounts goes along."
                  )}
            </p>

            <div className="flex flex-wrap items-center justify-end gap-2 pt-1">
              {attach && (
                <button type="button" onClick={() => void copySummary()} disabled={!summary} className={action}>
                  <Copy size={13} strokeWidth={1.8} />
                  {copied ? t("Copied") : t("Copy summary")}
                </button>
              )}
              <button type="button" onClick={() => openForm("bug")} className={action}>
                <ExternalLink size={13} strokeWidth={1.8} />
                {t("Open the form")}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
