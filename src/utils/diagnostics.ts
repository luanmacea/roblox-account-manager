/**
 * Diagnóstico "o launch não faz nada" (ideia 16). O backend
 * (`run_launch_diagnostics`, commands/diagnostics.rs) devolve só `id` +
 * `reason` (+ `count`); a frase que a pessoa lê sai daqui, traduzida. Assim
 * nenhum caminho, nome de conta ou PID chega à tela — e o resumo do "Reportar
 * problema" leva o resultado sem precisar anonimizar.
 */

export type DiagnosticStatus = "ok" | "warn" | "problem";

export interface DiagnosticCheck {
  id: string;
  status: DiagnosticStatus;
  reason: string;
  count?: number;
}

export interface DiagnosticText {
  title: string;
  /** O que fazer, numa frase. Vazio quando está tudo bem. */
  detail: string;
}

type T = (text: string, options?: Record<string, unknown>) => string;

/** A frase de cada resultado. Combinação desconhecida cai num texto genérico. */
export function diagnosticText(check: DiagnosticCheck, t: T): DiagnosticText {
  const count = check.count ?? 0;
  switch (`${check.id}.${check.reason}`) {
    case "robloxInstall.found":
      return { title: t("Roblox is installed"), detail: "" };
    case "robloxInstall.missing":
      return {
        title: t("No Roblox build found on this PC"),
        detail: t(
          "MultiAlt downloads it on the first launch. If launching still does nothing, check the internet line below or install Roblox from roblox.com."
        ),
      };
    case "dataFolder.writable":
      return { title: t("MultiAlt can save its data"), detail: "" };
    case "dataFolder.notWritable":
      return {
        title: t("MultiAlt can't write to its data folder"),
        detail: t(
          "Accounts and settings can't be saved. Check that the folder isn't read-only or blocked by another program, then restart MultiAlt."
        ),
      };
    case "dataFolder.unknown":
      return {
        title: t("Couldn't find MultiAlt's data folder"),
        detail: t("Restart the PC and open MultiAlt again."),
      };
    case "versionsFolder.writable":
      return { title: t("Roblox builds can be downloaded to disk"), detail: "" };
    case "versionsFolder.notWritable":
      return {
        title: t("MultiAlt can't write to the Roblox builds folder"),
        detail: t(
          "It can't download the Roblox build it needs. Free some disk space and check that the folder isn't read-only or blocked by another program."
        ),
      };
    case "versionsFolder.unknown":
      return {
        title: t("Couldn't find the Roblox builds folder"),
        detail: t("Windows didn't say where local app data lives. Restart the PC and try again."),
      };
    case "internet.reachable":
      return { title: t("Roblox can be reached"), detail: "" };
    case "internet.partial":
      return {
        title: t("Only part of Roblox answered"),
        detail: t(
          "Roblox may be having trouble, or something on your network blocks part of it. Try again in a few minutes, or without a VPN or proxy."
        ),
      };
    case "internet.unreachable":
      return {
        title: t("Roblox can't be reached"),
        detail: t("Check your internet connection, VPN or proxy, then try again."),
      };
    case "stuckProcesses.none":
      return { title: t("No stuck Roblox processes"), detail: "" };
    case "stuckProcesses.stuck":
      return {
        title: t("{{count}} Roblox process(es) running with no window", { count }),
        detail: t(
          "They have had no window for over 2 minutes and can block new launches. If you aren't playing on them, end RobloxPlayerBeta.exe in Task Manager. MultiAlt doesn't close them for you."
        ),
      };
    case "multiRoblox.off":
      return {
        title: t("Multi Roblox is off"),
        detail: t("Only one Roblox window at a time. Turn it on in Settings › General to play several accounts at once."),
      };
    case "multiRoblox.offWithClients":
      return {
        title: t("Multi Roblox is off and Roblox is already open"),
        detail: t(
          "Opening another account closes the one that's open, or doesn't start at all. Turn on Multi Roblox in Settings › General."
        ),
      };
    case "multiRoblox.legacyRam":
      return {
        title: t("The old Roblox Account Manager is holding Roblox's lock"),
        detail: t("Close the old Roblox Account Manager, then launch again."),
      };
    case "multiRoblox.held":
    case "multiRoblox.free":
      return { title: t("Multi Roblox is ready"), detail: "" };
    case "multiRoblox.clientOpen":
      return {
        title: t("Multi Roblox is ready"),
        detail: t("A Roblox window is already open; the next account opens next to it without closing it."),
      };
    default:
      return { title: `${check.id}: ${check.reason}`, detail: "" };
  }
}

/** O pior estado da lista — o que a tela diz no topo. */
export function overallStatus(checks: DiagnosticCheck[]): DiagnosticStatus {
  if (checks.some((c) => c.status === "problem")) return "problem";
  if (checks.some((c) => c.status === "warn")) return "warn";
  return "ok";
}

/**
 * Uma linha por checagem, em inglês e sem tradução: é o que vai no resumo do
 * "Reportar problema" (quem lê é o dono do projeto, no GitHub).
 */
export function diagnosticsSummaryLines(checks: DiagnosticCheck[]): string[] {
  return checks.map(
    (c) => `- ${c.id}: ${c.status} (${c.reason}${c.count !== undefined ? `, ${c.count}` : ""})`
  );
}
