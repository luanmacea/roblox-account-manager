import { useEffect, useMemo, useState } from "react";
import {
  AppWindow,
  Check,
  Coffee,
  Crosshair,
  Gamepad2,
  Globe,
  ListX,
  MemoryStick,
  PowerOff,
  RotateCw,
  SquareStop,
  UserCheck,
  UserPlus,
  X,
} from "lucide-react";
import { useConfirm } from "../../hooks/usePrompt";
import { useTr } from "../../i18n/text";
import { useStore } from "../../store";
import { accountLabel } from "../../utils/accountName";
import { ClientHealthNote } from "./ClientHealthNote";
import { ReconnectSwitch } from "./ReconnectSwitch";
import { MemoryLimitSelect, MemoryReading, type MemoryLimitValue } from "./MemoryLimitControl";
import { MEMORY_LIMIT_SETTING, fieldsWithMemoryLimit, memoryLimitChoice } from "../../utils/memoryLimit";
import {
  autoReconnectLabel,
  fieldsWithReconnect,
  reconnectChoice,
  reconnectErrorText,
  type ReconnectChoice,
} from "../../utils/autoReconnect";
import { useNexusAutoRelaunch } from "../../hooks/useNexusAutoRelaunch";
import { isWindowsPlatform } from "../../utils/platform";
import { useCurrentSessions } from "../../hooks/useCurrentSessions";
import { CurrentSessionLine } from "./CurrentSessionLine";
import type {
  AutoReconnectEntry,
  ClientHealth,
  CurrentSession,
  FriendLinkAccountState,
  LaunchQueueEntry,
  LaunchQueueState,
  UnidentifiedClient,
} from "../../types";

/**
 * Painel de Sessão: o que está entrando agora e o que já está em jogo.
 *
 * O mesmo componente é montado em dois lugares (aba Console da Choose Game e
 * `SessionDialog`, aberto pela barra principal). Todo o estado vem da store
 * (`launchQueue` do evento `launch-queue` e `launchedByProgram` do polling de
 * `get_running_instances`), então os dois mostram exatamente a mesma coisa.
 *
 * Regra de produto que não pode se perder: **cancelar nunca fecha um cliente
 * já aberto**. Cancelar só tira a conta da fila; fechar é uma ação separada,
 * explícita, na seção "In game".
 */

type Translate = ReturnType<typeof useTr>;

function stateLabel(state: LaunchQueueState, t: Translate): string {
  switch (state) {
    case "queued":
      return t("Queued");
    case "launching":
      return t("Joining");
    case "done":
      return t("Joined");
    case "failed":
      return t("Failed");
    case "cancelled":
      return t("Cancelled");
    default:
      return state;
  }
}

const STATE_STYLES: Record<LaunchQueueState, { dot: string; text: string }> = {
  queued: { dot: "bg-[var(--panel-muted)]", text: "theme-muted" },
  launching: { dot: "bg-[var(--accent-color)] animate-pulse", text: "text-[var(--accent-color)]" },
  done: { dot: "bg-emerald-500", text: "text-emerald-400" },
  failed: { dot: "bg-red-500", text: "text-red-400" },
  cancelled: { dot: "bg-zinc-600", text: "theme-muted" },
};

/**
 * Estados de uma conta no Make Friends. Cores iguais às da fila: a bolinha quer
 * dizer a mesma coisa nas duas listas do painel.
 */
const FRIEND_STATE_STYLES: Record<FriendLinkAccountState, { dot: string; text: string }> = {
  pending: { dot: "bg-[var(--panel-muted)]", text: "theme-muted" },
  processing: { dot: "bg-[var(--accent-color)] animate-pulse", text: "text-[var(--accent-color)]" },
  done: { dot: "bg-emerald-500", text: "text-emerald-400" },
  failed: { dot: "bg-red-500", text: "text-red-400" },
};

function friendStateLabel(state: FriendLinkAccountState, t: Translate): string {
  switch (state) {
    case "pending":
      return t("Waiting");
    case "processing":
      return t("Processing");
    case "done":
      return t("Linked");
    case "failed":
      return t("Failed");
    default:
      return state;
  }
}

/** O que a operação está fazendo agora — as três fases custam tempo diferente. */
function friendPhaseLabel(phase: string, t: Translate): string {
  switch (phase) {
    case "checking":
      return t("Checking who is already friends");
    case "linking":
      return t("Sending friend requests");
    case "verifying":
      return t("Verifying the friendships");
    case "done":
      return t("Finished");
    default:
      return "";
  }
}

/**
 * Por que o cliente aberto fora do app não foi reconhecido sozinho. `name` é o
 * nome (já mascarado) da conta que o log citou, quando citou.
 */
function unidentifiedReasonLabel(client: UnidentifiedClient, name: string, t: Translate): string {
  switch (client.reason) {
    case "waitingForGame":
      return t("It hasn't joined a game yet, so the account is still unknown");
    case "noLog":
      return t("No Roblox log matched this client");
    case "unknownAccount":
      return t("Its account is not in your list");
    case "accountBusy":
      return t("{{name}} already has a client open", { name });
    default:
      return "";
  }
}

/** Estados em que a conta ainda pode sair da fila. */
function isPending(entry: LaunchQueueEntry): boolean {
  return entry.state === "queued" || entry.state === "launching";
}

/** Uma queda de uma conta: a mesma conta caindo de novo é outra chave. */
function dropKey(entry: AutoReconnectEntry): string {
  return `${entry.userId}:${entry.drop?.sinceMs ?? 0}`;
}

/**
 * Estado de uma conta do "Em jogo", curto: reconectando (âmbar), caiu ou não
 * responde (o `ClientHealthNote`, com o motivo), jogando (verde) ou ainda sem
 * jogo. Sem log achado e sem sessão, não diz nada (não sabe).
 */
function RunningState({
  session,
  health,
  reconnecting,
}: {
  session: CurrentSession | undefined;
  health: ClientHealth | undefined;
  reconnecting: boolean;
}) {
  const t = useTr();
  const note = <ClientHealthNote health={health} className="shrink" />;
  if (reconnecting) {
    return (
      <>
        <span className="shrink-0 inline-flex items-center gap-1 text-amber-400">
          <RotateCw size={11} strokeWidth={1.75} aria-hidden="true" />
          {t("Reconnecting")}
        </span>
        {note}
      </>
    );
  }
  if (health?.drop || health?.notResponding) return note;
  if (session) {
    return (
      <span className="shrink-0 inline-flex items-center gap-1 text-emerald-400">
        <span aria-hidden="true" className="w-1.5 h-1.5 rounded-full bg-emerald-400" />
        {t("Playing")}
      </span>
    );
  }
  if (health?.logFound && !health.inGame) {
    return <span className="min-w-0 truncate theme-muted">{t("Not in a game yet")}</span>;
  }
  return null;
}

interface SessionPanelProps {
  className?: string;
}

export function SessionPanel({ className = "" }: SessionPanelProps) {
  const t = useTr();
  const store = useStore();
  const confirm = useConfirm();
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [checked, setChecked] = useState<Set<number>>(new Set());
  // Cliente não identificado com o seletor de conta aberto, e a conta escolhida.
  const [identifying, setIdentifying] = useState<{ pid: number; userId: string } | null>(null);

  const entries = store.launchQueue?.entries ?? [];
  const pending = entries.filter(isPending);
  // Só aparece depois que houve um Make Friends: uma seção vazia permanente
  // roubaria altura de um painel que já tem teto de 45% na aba Console.
  const friendLink = store.friendLinkState;
  const showFriendLink = !!friendLink && friendLink.total > 0;

  // Contas com cliente rodando agora, na ordem da lista de contas (estável
  // entre polls). IDs sem conta correspondente ainda aparecem, pelo ID.
  const runningIds = useMemo(() => {
    const running = store.launchedByProgram;
    const known = store.accounts.filter((a) => running.has(a.UserID)).map((a) => a.UserID);
    const extras = [...running].filter((id) => !known.includes(id));
    return [...known, ...extras];
  }, [store.accounts, store.launchedByProgram]);

  // Clientes abertos fora do app que o backend não reconheceu (ver
  // docs/features/external-clients.md). Ficam fora da contagem e do lote de
  // fechar: não têm conta, e o app nunca fecha cliente que não sabe de quem é.
  const unidentified = store.unidentifiedClients;

  // A seleção é derivada: contas que fecharam sozinhas somem do lote.
  const selected = runningIds.filter((id) => checked.has(id));

  // Reconexão automática (commands/reconnect.rs). A contagem regressiva
  // ("em 30 s") só anda enquanto alguma conta espera a próxima tentativa.
  // "Dispensar" esconde a linha na hora, sem esperar o evento do backend. A
  // chave é a queda (conta + início): se a conta cair de novo, a linha volta.
  const [dismissed, setDismissed] = useState<Set<string>>(() => new Set());
  // Contas com um comando de reconexão em andamento (botões desabilitados).
  const [reconnectBusy, setReconnectBusy] = useState<Set<number>>(() => new Set());
  const reconnecting = (store.autoReconnect ?? []).filter((entry) => !dismissed.has(dropKey(entry)));
  const countingDown = reconnecting.some((entry) => entry.phase === "waiting");
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!countingDown) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [countingDown]);

  // Reconexão automática por conta (commands/reconnect.rs lê o log do Roblox,
  // então só no Windows). O campo da conta manda; sem ele, o padrão
  // `General.AutoReconnect`; o AutoRelaunch do Nexus liga por cima.
  const isWindows = isWindowsPlatform(store.platformCapabilities);
  const reconnectDefault = store.settings?.General?.AutoReconnect === "true";
  const nexusRelaunch = useNexusAutoRelaunch(isWindows, runningIds.join(","));
  // Jogo e início da sessão de cada conta (o mesmo do histórico). Só no
  // Windows: é lá que o log do Roblox é lido.
  const sessions = useCurrentSessions(isWindows, runningIds.join(","));
  const reconnectingIds = new Set(
    (store.autoReconnect ?? [])
      .filter((entry) => entry.phase !== "gaveUp" && entry.phase !== "stopped")
      .map((entry) => entry.userId)
  );
  const [reconnectSaving, setReconnectSaving] = useState(false);

  // Teto de memória (commands/memory_ceiling.rs): só com a feature
  // `memory-trim` no binário, e só nos clientes que o app abriu. O campo
  // `MemoryLimit` da conta manda; sem ele, `Optimization.MemoryLimit`.
  const memoryTrim = isWindows && store.platformCapabilities?.supportsMemoryTrim === true;
  const memoryDefaultRaw = store.settings?.[MEMORY_LIMIT_SETTING.section]?.[MEMORY_LIMIT_SETTING.key];
  const memoryDefaultMb = memoryLimitChoice(undefined, memoryDefaultRaw).defaultMb;
  const [memorySaving, setMemorySaving] = useState(false);
  const memoryBulkIds = selected.filter((id) => !store.adoptedClients.has(id));
  const memoryBulkLabel =
    memoryBulkIds.length === 1
      ? t("Memory limit for 1 selected")
      : t("Memory limit for {{count}} selected", { count: memoryBulkIds.length });

  /**
   * Grava o limite (`null` = volta ao padrão) no `Fields.MemoryLimit` de cada
   * conta, uma por vez. Vale na próxima passada do backend (2 s), sem relançar
   * e sem fechar nada.
   */
  async function saveMemoryLimit(userIds: number[], value: MemoryLimitValue) {
    const targets = store.accounts.filter((a) => userIds.includes(a.UserID));
    if (targets.length === 0) return;
    setError(null);
    setMemorySaving(true);
    try {
      for (const account of targets) {
        await store.updateAccount({ ...account, Fields: fieldsWithMemoryLimit(account.Fields, value) });
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setMemorySaving(false);
    }
  }

  function nameFor(userId: number): string {
    const account = store.accounts.find((a) => a.UserID === userId);
    return accountLabel(account, store, userId);
  }

  function choiceFor(userId: number): ReconnectChoice | null {
    const account = store.accounts.find((a) => a.UserID === userId);
    if (!account) return null;
    const nexusForced = nexusRelaunch.has((account.Username ?? "").toLowerCase());
    return reconnectChoice(account.Fields, reconnectDefault, nexusForced);
  }

  /**
   * Grava a escolha (`null` = volta ao padrão) no `Fields.AutoReconnect` de
   * cada conta, uma por vez. Não fecha nem abre cliente nenhum: o backend lê o
   * campo na próxima queda.
   */
  async function saveReconnect(userIds: number[], value: boolean | null) {
    const targets = store.accounts.filter((a) => userIds.includes(a.UserID));
    if (targets.length === 0) return;
    setError(null);
    setReconnectSaving(true);
    try {
      for (const account of targets) {
        await store.updateAccount({ ...account, Fields: fieldsWithReconnect(account.Fields, value) });
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setReconnectSaving(false);
    }
  }

  function toggleChecked(userId: number) {
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(userId)) next.delete(userId);
      else next.add(userId);
      return next;
    });
  }

  function toggleAll() {
    setChecked(selected.length === runningIds.length ? new Set() : new Set(runningIds));
  }

  async function handleCancel(userId: number) {
    setError(null);
    try {
      await store.cancelAccountLaunch(userId);
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleStopQueue() {
    setError(null);
    try {
      await store.stopLaunchQueue();
    } catch (e) {
      setError(String(e));
    }
  }

  /**
   * "Tentar agora" / "Parar" / dispensar. `false` do backend: a conta já não
   * está na reconexão (a lista da tela estava velha) — avisa em vez de calar.
   */
  async function handleReconnectAction(entry: AutoReconnectEntry, action: "retry" | "stop" | "dismiss") {
    const { userId } = entry;
    if (reconnectBusy.has(userId)) return;
    const key = dropKey(entry);
    setError(null);
    setReconnectBusy((prev) => new Set(prev).add(userId));
    if (action === "dismiss") setDismissed((prev) => new Set(prev).add(key));
    try {
      const ok =
        action === "retry" ? await store.retryAutoReconnect(userId) : await store.stopAutoReconnect(userId);
      if (!ok) {
        store.addToast(t("{{name}} is no longer being reconnected.", { name: nameFor(userId) }), "info");
      }
    } catch (e) {
      if (action === "dismiss") {
        setDismissed((prev) => {
          const next = new Set(prev);
          next.delete(key);
          return next;
        });
      }
      setError(String(e));
    } finally {
      setReconnectBusy((prev) => {
        const next = new Set(prev);
        next.delete(userId);
        return next;
      });
    }
  }

  async function handleFocus(userId: number) {
    setError(null);
    try {
      const ok = await store.focusRobloxClient(userId);
      if (!ok) setError(t("Could not bring that Roblox window to the front."));
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleShowWindow(pid: number) {
    setError(null);
    try {
      const ok = await store.focusClientWindow(pid);
      if (!ok) setError(t("Could not bring that Roblox window to the front."));
    } catch (e) {
      setError(String(e));
    }
  }

  function startIdentify(client: UnidentifiedClient) {
    setError(null);
    // Se o log citou uma conta salva (ocupada), ela já vem escolhida.
    const named =
      client.userId !== null && store.accounts.some((a) => a.UserID === client.userId)
        ? String(client.userId)
        : "";
    setIdentifying({ pid: client.pid, userId: named });
  }

  async function confirmIdentify() {
    if (!identifying || !identifying.userId) return;
    setError(null);
    setBusy(true);
    try {
      await store.identifyExternalClient(identifying.pid, Number(identifying.userId));
      setIdentifying(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  /**
   * Abre o Modo AFK com as contas em jogo (as marcadas, ou todas). Não liga
   * nada: o Start de lá é que adota as contas no Auto Rejoin **sem fechar
   * nenhum cliente** — e antes dele a pessoa vê e ajusta o tempo do ciclo e as
   * contas main. Ligar direto daqui escondia onde configurar e onde parar.
   *
   * Abre nos cliques AFK (a aba padrão), a não ser que só o Auto Rejoin esteja
   * ligado.
   */
  function handleOpenAfkMode() {
    const alvo = selected.length > 0 ? selected : runningIds;
    if (alvo.length === 0) return;
    const onlyRejoinActive = !!store.bottingStatus?.active && !store.afkStatus?.active;
    store.openAfkMode({
      tab: onlyRejoinActive ? "rejoin" : "clicks",
      targetUserIds: alvo,
      adoptRunning: true,
    });
  }

  /**
   * Uma confirmação só para o lote inteiro; fechar uma conta é imediato. Fecha
   * só as contas da lista (`closeRobloxClients`), nunca cliente de fora dela.
   */
  async function handleClose(userIds: number[]) {
    if (userIds.length === 0) return;
    setError(null);
    if (userIds.length > 1) {
      const ok = await confirm(t("Close {{count}} clients?", { count: userIds.length }), true);
      if (!ok) return;
    }
    setBusy(true);
    try {
      await store.closeRobloxClients(userIds);
      setChecked(new Set());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className={`flex flex-col gap-3 min-h-0 ${className}`} data-testid="session-panel">
      {error && (
        <div
          role="alert"
          className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-[12px] text-red-400"
        >
          {error}
        </div>
      )}

      {/* ── Entrando ───────────────────────────────────────────────────── */}
      <section data-tour="session-joining" className="rounded-lg border theme-border bg-[var(--panel-soft)]">
        <header className="flex items-center justify-between gap-2 px-3 py-2 border-b theme-border">
          <div className="flex items-baseline gap-2 min-w-0">
            <h3 className="text-[12px] font-semibold text-[var(--panel-fg)]">{t("Joining")}</h3>
            <span className="text-[12px] theme-muted truncate">
              {pending.length > 0
                ? t("{{count}} waiting", { count: pending.length })
                : t("idle")}
            </span>
          </div>
          <button
            onClick={() => void handleStopQueue()}
            disabled={pending.length === 0}
            className="sidebar-btn-sm flex items-center gap-1.5 shrink-0 disabled:opacity-40 disabled:cursor-not-allowed"
            title={t("Remove every waiting account from the queue")}
          >
            <SquareStop size={13} strokeWidth={1.5} />
            {t("Stop queue")}
          </button>
        </header>

        {entries.length === 0 ? (
          <div className="flex flex-col items-center justify-center gap-1.5 px-3 py-6 theme-muted">
            <ListX size={20} strokeWidth={1.5} />
            <p className="text-[12px] text-center">
              {t("Nothing in the launch queue. Pick a game to start joining accounts.")}
            </p>
          </div>
        ) : (
          <ul className="max-h-48 overflow-y-auto py-1">
            {entries.map((entry) => {
              const style = STATE_STYLES[entry.state] ?? STATE_STYLES.queued;
              const name = nameFor(entry.userId);
              // A tentativa da reconexão automática também passa pela fila.
              const error = entry.error ? reconnectErrorText(entry.error, t) : null;
              return (
                <li
                  key={entry.userId}
                  data-testid={`session-queue-${entry.userId}`}
                  className="flex items-center gap-2 px-3 py-1.5 text-[12px]"
                >
                  <span className={`shrink-0 w-1.5 h-1.5 rounded-full ${style.dot}`} />
                  <span className="text-[var(--panel-fg)] truncate max-w-[40%]">{name}</span>
                  <span className={`shrink-0 ${style.text}`}>{stateLabel(entry.state, t)}</span>
                  {error && (
                    <span className="text-red-400 truncate" title={error}>
                      {error}
                    </span>
                  )}
                  <span className="ml-auto shrink-0">
                    {isPending(entry) && (
                      <button
                        onClick={() => void handleCancel(entry.userId)}
                        aria-label={t("Cancel {{name}}", { name })}
                        title={t("Take this account out of the queue (does not close any client)")}
                        className="p-1 rounded-md theme-muted hover:text-red-400 hover:bg-[var(--panel-soft)] transition-colors"
                      >
                        <X size={13} strokeWidth={2} />
                      </button>
                    )}
                  </span>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      {/* ── Make Friends ───────────────────────────────────────────────── */}
      {showFriendLink && friendLink && (
        <section
          data-testid="friend-link-panel"
          className="rounded-lg border theme-border bg-[var(--panel-soft)]"
        >
          <header className="flex items-center justify-between gap-2 px-3 py-2 border-b theme-border">
            <div className="flex items-baseline gap-2 min-w-0">
              <h3 className="text-[12px] font-semibold text-[var(--panel-fg)] flex items-center gap-1.5">
                <UserPlus size={13} strokeWidth={1.5} />
                {t("Make Friends")}
              </h3>
              <span className="text-[12px] theme-muted truncate">
                {t("{{done}} / {{total}} accounts processed", {
                  done: friendLink.processed,
                  total: friendLink.total,
                })}
              </span>
            </div>
            <span className="text-[12px] theme-muted shrink-0 truncate">
              {friendPhaseLabel(friendLink.phase, t)}
            </span>
          </header>

          <ul className="max-h-48 overflow-y-auto py-1">
            {friendLink.accounts.map((entry) => {
              const style = FRIEND_STATE_STYLES[entry.state] ?? FRIEND_STATE_STYLES.pending;
              return (
                <li
                  key={entry.userId}
                  data-testid={`friend-link-${entry.userId}`}
                  className="flex items-center gap-2 px-3 py-1.5 text-[12px]"
                >
                  <span className={`shrink-0 w-1.5 h-1.5 rounded-full ${style.dot}`} />
                  <span className="text-[var(--panel-fg)] truncate max-w-[40%]">
                    {nameFor(entry.userId)}
                  </span>
                  {friendLink.mainUserId === entry.userId && (
                    <span className="shrink-0 theme-muted">{t("main")}</span>
                  )}
                  <span className={`shrink-0 ${style.text}`}>
                    {friendStateLabel(entry.state, t)}
                  </span>
                  {entry.error && (
                    <span className="text-red-400 truncate" title={entry.error}>
                      {entry.error}
                    </span>
                  )}
                </li>
              );
            })}
          </ul>
        </section>
      )}

      {/* ── Reconectando ───────────────────────────────────────────────── */}
      {reconnecting.length > 0 && (
        <section data-testid="auto-reconnect-panel" className="rounded-lg border theme-border bg-[var(--panel-soft)]">
          <header className="flex items-center gap-2 px-3 py-2 border-b theme-border min-w-0">
            <h3 className="text-[12px] font-semibold text-[var(--panel-fg)] flex items-center gap-1.5 shrink-0">
              <RotateCw size={13} strokeWidth={1.5} />
              {t("Reconnecting")}
            </h3>
            <span className="text-[12px] theme-muted truncate">
              {t("Accounts that dropped go back to the same game on their own")}
            </span>
          </header>
          <ul className="max-h-48 overflow-y-auto py-1">
            {reconnecting.map((entry) => {
              const info = autoReconnectLabel(entry, now, t);
              const name = nameFor(entry.userId);
              const pending = info.tone === "pending";
              const busy = reconnectBusy.has(entry.userId);
              const turnedOff = choiceFor(entry.userId)?.effective === false;
              return (
                <li
                  key={entry.userId}
                  data-testid={`session-reconnect-${entry.userId}`}
                  data-phase={entry.phase}
                  aria-busy={busy || undefined}
                  className="flex items-center gap-2 px-3 py-1.5 text-[12px]"
                >
                  <span
                    className={`shrink-0 w-1.5 h-1.5 rounded-full ${pending ? "bg-amber-400 animate-pulse" : "bg-red-500"}`}
                  />
                  <span className="text-[var(--panel-fg)] truncate shrink-0 max-w-[40%]">{name}</span>
                  {/* A causa (erro da última tentativa) fica à vista, embaixo. */}
                  <span className="min-w-0 flex flex-col">
                    <span
                      className={`truncate ${pending ? "text-amber-400" : "text-red-400"}`}
                      title={info.label}
                    >
                      {info.label}
                    </span>
                    {info.detail && (
                      <span
                        data-testid={`session-reconnect-detail-${entry.userId}`}
                        className="truncate text-[11px] theme-muted"
                        title={info.detail}
                      >
                        {info.detail}
                      </span>
                    )}
                    {/* Desligada com a tentativa em andamento: o backend só a solta depois dela. */}
                    {pending && turnedOff && (
                      <span className="truncate text-[11px] theme-muted">
                        {t("Turned off: it stops after the attempt in progress.")}
                      </span>
                    )}
                  </span>
                  <span className="ml-auto flex items-center gap-1.5 shrink-0">
                    {(info.canTryNow || info.canTryAgain) && (
                      <button
                        onClick={() => void handleReconnectAction(entry, "retry")}
                        disabled={busy}
                        className="sidebar-btn-sm flex items-center gap-1.5 disabled:opacity-50 disabled:cursor-default"
                        title={t("Reopen this account now, in the same game")}
                      >
                        <RotateCw size={12} strokeWidth={1.5} className={busy ? "animate-spin" : undefined} />
                        {info.canTryNow ? t("Try now") : t("Try again")}
                      </button>
                    )}
                    {pending ? (
                      <button
                        onClick={() => void handleReconnectAction(entry, "stop")}
                        disabled={busy}
                        className="sidebar-btn-sm flex items-center gap-1.5 disabled:opacity-50 disabled:cursor-default"
                        title={t("Stop reconnecting this account (does not close any client)")}
                      >
                        <SquareStop size={12} strokeWidth={1.5} />
                        {t("Stop")}
                      </button>
                    ) : (
                      <button
                        onClick={() => void handleReconnectAction(entry, "dismiss")}
                        disabled={busy}
                        aria-label={t("Dismiss {{name}}", { name })}
                        title={t("Hide this line")}
                        className="p-1 rounded-md theme-muted hover:text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] transition-colors disabled:opacity-50"
                      >
                        <X size={13} strokeWidth={2} />
                      </button>
                    )}
                  </span>
                </li>
              );
            })}
          </ul>
        </section>
      )}

      {/* ── Em jogo ────────────────────────────────────────────────────── */}
      <section data-tour="session-in-game" className="rounded-lg border theme-border bg-[var(--panel-soft)]">
        <header className="flex items-center justify-between gap-2 px-3 py-2 border-b theme-border">
          <div className="flex items-center gap-2 min-w-0">
            {runningIds.length > 0 && (
              <input
                type="checkbox"
                aria-label={t("Select all running clients")}
                checked={selected.length === runningIds.length}
                onChange={toggleAll}
                className="accent-[var(--accent-color)]"
              />
            )}
            <h3 className="text-[12px] font-semibold text-[var(--panel-fg)]">{t("In game")}</h3>
            <span className="text-[12px] theme-muted truncate">
              {t("{{count}} running", { count: runningIds.length })}
            </span>
            {unidentified.length > 0 && (
              <span className="text-[12px] text-amber-400 truncate">
                {t("{{count}} unidentified", { count: unidentified.length })}
              </span>
            )}
          </div>
          {/* Alvo do tutorial só com botões: vazio, ele cairia num ponto sem nada. */}
          <div
            data-tour={runningIds.length > 0 ? "session-in-game-actions" : undefined}
            className="flex items-center gap-1.5 shrink-0"
          >
            {runningIds.length > 0 && (
              <>
                <button
                  onClick={handleOpenAfkMode}
                  title={t(
                    "Opens AFK Mode with these accounts: Auto Rejoin keeps them in the cycle without closing the clients that are already open."
                  )}
                  className="sidebar-btn-sm flex items-center gap-1.5 shrink-0"
                >
                  <Coffee size={13} strokeWidth={1.5} />
                  {t("AFK Mode")}
                </button>
                {/* Sem marcação vale para todas as da lista; mais de uma pergunta. */}
                <button
                  onClick={() => void handleClose(selected.length > 0 ? selected : runningIds)}
                  disabled={busy}
                  title={
                    selected.length > 0
                      ? undefined
                      : t("Closes the Roblox client of every account in this list")
                  }
                  className="sidebar-btn-sm flex items-center gap-1.5 shrink-0 text-red-400 disabled:opacity-40 disabled:cursor-not-allowed"
                >
                  <PowerOff size={13} strokeWidth={1.5} />
                  {selected.length > 0
                    ? t("Close accounts ({{count}})", { count: selected.length })
                    : t("Close accounts")}
                </button>
              </>
            )}
          </div>
        </header>

        {/* Lote da reconexão: só com contas marcadas, numa faixa própria para
            o cabeçalho não estourar a largura da aba Console. */}
        {isWindows && selected.length > 0 && (
          <div
            data-testid="session-reconnect-bulk"
            className="flex flex-wrap items-center gap-1.5 px-3 py-1.5 border-b theme-border text-[12px]"
          >
            <span className="flex items-center gap-1 theme-muted mr-auto min-w-0">
              <RotateCw size={12} strokeWidth={1.5} className="shrink-0" aria-hidden="true" />
              <span className="truncate">
                {selected.length === 1
                  ? t("Auto-reconnect for 1 selected")
                  : t("Auto-reconnect for {{count}} selected", { count: selected.length })}
              </span>
            </span>
            <button
              onClick={() => void saveReconnect(selected, true)}
              disabled={reconnectSaving}
              className="sidebar-btn-sm shrink-0 disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {t("Reconnect on")}
            </button>
            <button
              onClick={() => void saveReconnect(selected, false)}
              disabled={reconnectSaving}
              className="sidebar-btn-sm shrink-0 disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {t("Reconnect off")}
            </button>
            <button
              onClick={() => void saveReconnect(selected, null)}
              disabled={reconnectSaving}
              title={
                reconnectDefault
                  ? t("Use the default from Settings › General (on)")
                  : t("Use the default from Settings › General (off)")
              }
              className="sidebar-btn-sm shrink-0 disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {t("Use default")}
            </button>
          </div>
        )}

        {/* Lote do teto de memória: a mesma faixa, sem os clientes do site. */}
        {memoryTrim && memoryBulkIds.length > 0 && (
          <div
            data-testid="session-memory-bulk"
            className="flex flex-wrap items-center gap-1.5 px-3 py-1.5 border-b theme-border text-[12px]"
          >
            <span className="flex items-center gap-1 theme-muted mr-auto min-w-0">
              <MemoryStick size={12} strokeWidth={1.5} className="shrink-0" aria-hidden="true" />
              <span className="truncate">{memoryBulkLabel}</span>
            </span>
            <MemoryLimitSelect
              ariaLabel={memoryBulkLabel}
              choice={null}
              defaultMb={memoryDefaultMb}
              disabled={memorySaving}
              onChange={(value) => void saveMemoryLimit(memoryBulkIds, value)}
            />
          </div>
        )}

        {runningIds.length === 0 && unidentified.length === 0 ? (
          <div className="flex flex-col items-center justify-center gap-1.5 px-3 py-6 theme-muted">
            <Gamepad2 size={20} strokeWidth={1.5} />
            <p className="text-[12px] text-center">
              {t("No Roblox client is running. Accounts you launch show up here.")}
            </p>
          </div>
        ) : (
          <ul className="max-h-56 overflow-y-auto py-1">
            {runningIds.map((userId) => {
              const name = nameFor(userId);
              const choice = isWindows ? choiceFor(userId) : null;
              const account = store.accounts.find((a) => a.UserID === userId);
              const memoryChoice =
                memoryTrim && account && !store.adoptedClients.has(userId)
                  ? memoryLimitChoice(account.Fields, memoryDefaultRaw)
                  : null;
              return (
                <li
                  key={userId}
                  data-testid={`session-running-${userId}`}
                  className="flex items-center gap-2 px-3 py-1.5 text-[12px]"
                >
                  <input
                    type="checkbox"
                    aria-label={t("Select {{name}}", { name })}
                    checked={checked.has(userId)}
                    onChange={() => toggleChecked(userId)}
                    className="accent-[var(--accent-color)]"
                  />
                  {/* Duas linhas: conta e estado em cima; jogo, servidor e
                      tempo em jogo embaixo. Cabe em 820 px sem rolar de lado. */}
                  <span className="min-w-0 flex-1 flex flex-col">
                    <span className="flex items-center gap-2 min-w-0">
                      {/* O nome vem primeiro: o aviso de queda encolhe antes dele. */}
                      <span className="text-[var(--panel-fg)] truncate shrink-0 max-w-[45%]" title={name}>
                        {name}
                      </span>
                      <RunningState
                        session={sessions.get(userId)}
                        health={store.clientHealth?.get(userId)}
                        reconnecting={reconnectingIds.has(userId)}
                      />
                      {memoryChoice && <MemoryReading memory={store.clientMemory?.get(userId)} />}
                      {store.adoptedClients.has(userId) && (
                        <span
                          className="min-w-0 flex items-center gap-1 theme-muted"
                          title={t("Opened from the Roblox website and recognized by the Roblox log")}
                        >
                          <Globe size={11} strokeWidth={1.5} className="shrink-0" />
                          <span className="truncate">{t("Opened outside the app")}</span>
                        </span>
                      )}
                    </span>
                    {sessions.has(userId) && <CurrentSessionLine session={sessions.get(userId)!} />}
                  </span>
                  <span className="ml-auto flex items-center gap-1.5 shrink-0">
                    {memoryChoice && (
                      <MemoryLimitSelect
                        ariaLabel={t("Memory limit for {{name}}", { name })}
                        choice={memoryChoice}
                        defaultMb={memoryDefaultMb}
                        disabled={memorySaving}
                        onChange={(value) => void saveMemoryLimit([userId], value)}
                      />
                    )}
                    {choice && (
                      <ReconnectSwitch
                        name={name}
                        choice={choice}
                        disabled={reconnectSaving}
                        onChange={(value) => void saveReconnect([userId], value)}
                      />
                    )}
                    <button
                      onClick={() => void handleFocus(userId)}
                      title={t("Bring this client's window to the front")}
                      className="sidebar-btn-sm flex items-center gap-1.5"
                    >
                      <Crosshair size={12} strokeWidth={1.5} />
                      {t("Focus")}
                    </button>
                    <button
                      onClick={() => void handleClose([userId])}
                      disabled={busy}
                      className="sidebar-btn-sm flex items-center gap-1.5 text-red-400 disabled:opacity-40 disabled:cursor-not-allowed"
                    >
                      <PowerOff size={12} strokeWidth={1.5} />
                      {t("Close")}
                    </button>
                  </span>
                </li>
              );
            })}
            {unidentified.map((client) => {
              const namedAccount =
                client.userId !== null
                  ? store.accounts.find((a) => a.UserID === client.userId)
                  : undefined;
              const namedLabel =
                client.userId !== null ? accountLabel(namedAccount, store, client.userId) : "";
              const picking = identifying?.pid === client.pid ? identifying : null;
              return (
                <li
                  key={`pid-${client.pid}`}
                  data-testid={`session-unidentified-${client.pid}`}
                  data-tour="session-unidentified"
                  className="flex flex-col gap-1.5 px-3 py-1.5 text-[12px]"
                >
                  <div className="flex items-center gap-2 min-w-0">
                    <span className="shrink-0 w-1.5 h-1.5 rounded-full bg-amber-400" />
                    <span className="text-[var(--panel-fg)] shrink-0">{t("Unidentified client")}</span>
                    <span className="theme-muted shrink-0">{t("PID {{pid}}", { pid: client.pid })}</span>
                    <span className="theme-muted truncate" title={unidentifiedReasonLabel(client, namedLabel, t)}>
                      {unidentifiedReasonLabel(client, namedLabel, t)}
                    </span>
                    <span className="ml-auto flex items-center gap-1.5 shrink-0">
                      <button
                        onClick={() => void handleShowWindow(client.pid)}
                        title={t("Bring this client's window to the front")}
                        className="sidebar-btn-sm flex items-center gap-1.5"
                      >
                        <AppWindow size={12} strokeWidth={1.5} />
                        {t("Show window")}
                      </button>
                      {!picking && (
                        <button
                          onClick={() => startIdentify(client)}
                          title={t("Tell the app which account is playing in this client")}
                          className="sidebar-btn-sm flex items-center gap-1.5"
                        >
                          <UserCheck size={12} strokeWidth={1.5} />
                          {t("Identify")}
                        </button>
                      )}
                    </span>
                  </div>
                  {picking && (
                    <div className="flex items-center gap-1.5 pl-3.5">
                      <select
                        aria-label={t("Account for this client")}
                        value={picking.userId}
                        onChange={(e) => setIdentifying({ pid: client.pid, userId: e.target.value })}
                        className="min-w-0 flex-1 rounded-md border theme-border bg-[var(--panel-bg)] px-2 py-1 text-[12px] text-[var(--panel-fg)]"
                      >
                        <option value="">{t("Pick an account")}</option>
                        {store.accounts.map((a) => {
                          const label = accountLabel(a, store, a.UserID);
                          return (
                            <option key={a.UserID} value={String(a.UserID)}>
                              {store.launchedByProgram.has(a.UserID)
                                ? t("{{name}} (already in game)", { name: label })
                                : label}
                            </option>
                          );
                        })}
                      </select>
                      <button
                        onClick={() => void confirmIdentify()}
                        disabled={busy || !picking.userId}
                        className="sidebar-btn-sm flex items-center gap-1.5 shrink-0 disabled:opacity-40 disabled:cursor-not-allowed"
                      >
                        <Check size={12} strokeWidth={1.5} />
                        {t("Confirm")}
                      </button>
                      <button
                        onClick={() => setIdentifying(null)}
                        aria-label={t("Cancel")}
                        className="p-1 rounded-md theme-muted hover:text-[var(--panel-fg)] transition-colors shrink-0"
                      >
                        <X size={13} strokeWidth={2} />
                      </button>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </section>
    </div>
  );
}
