import type { Account } from "../types";
import type { LaunchLogEntry } from "../store";
import { anonymize, type AnonymizeContext } from "./anonymize";
import { diagnosticsSummaryLines, type DiagnosticCheck } from "./diagnostics";

/** O que o backend diz do app e do PC (`get_report_environment`). */
export interface ReportEnvironment {
  version: string;
  edition: string;
  os: string;
}

/** Quantas linhas do Console vão no resumo, no máximo. */
export const REPORT_LOG_LINES = 40;

/** Nomes, IDs e segredos das contas salvas, para a anonimização. */
export function anonymizeContextFor(accounts: Account[]): AnonymizeContext {
  const names: string[] = [];
  const secrets: string[] = [];
  const userIds: number[] = [];
  for (const account of accounts) {
    if (account.Username) names.push(account.Username);
    if (account.Alias) names.push(account.Alias);
    if (account.SecurityToken) secrets.push(account.SecurityToken);
    if (account.Password) secrets.push(account.Password);
    if (account.UserID) userIds.push(account.UserID);
  }
  return { accountNames: names, userIds, secrets };
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

function formatLogLine(entry: LaunchLogEntry, accountIndex: Map<number, number>): string {
  const d = new Date(entry.ts);
  const time = `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  const who =
    entry.userId === null ? "" : ` account ${accountIndex.get(entry.userId) ?? "?"}:`;
  const step = entry.step ? ` [${entry.step}]` : "";
  return `${time} [${entry.level}]${step}${who} ${entry.message}`;
}

/**
 * O resumo que vai no relato. Contas viram "account 1", "account 2"... pela
 * ordem da lista (o número só serve para ligar linhas da mesma conta), e o
 * texto inteiro passa pelo `anonymize` no fim — inclusive as mensagens de erro,
 * que às vezes trazem o que veio do Roblox.
 */
export function buildProblemReport(input: {
  env: ReportEnvironment | null;
  logs: LaunchLogEntry[];
  diagnostics: DiagnosticCheck[] | null;
  accounts: Account[];
  maxLines?: number;
}): string {
  const { env, logs, diagnostics, accounts } = input;
  const maxLines = input.maxLines ?? REPORT_LOG_LINES;
  const accountIndex = new Map<number, number>();
  accounts.forEach((a, i) => accountIndex.set(a.UserID, i + 1));

  const lines: string[] = ["### Diagnostic summary (anonymised by MultiAlt)", ""];
  if (env) {
    lines.push(`- App: MultiAlt ${env.version} (${env.edition} edition)`);
    lines.push(`- OS: ${env.os}`);
  }
  lines.push(`- Accounts saved: ${accounts.length}`);
  lines.push("");

  lines.push("Launch check:");
  if (diagnostics && diagnostics.length > 0) {
    lines.push(...diagnosticsSummaryLines(diagnostics));
  } else {
    lines.push("- (not run)");
  }
  lines.push("");

  const recent = logs.slice(-maxLines);
  lines.push(`Last ${recent.length} console line(s):`);
  lines.push("```text");
  if (recent.length === 0) {
    lines.push("(empty)");
  } else {
    for (const entry of recent) lines.push(formatLogLine(entry, accountIndex));
  }
  lines.push("```");

  return anonymize(lines.join("\n"), anonymizeContextFor(accounts));
}
