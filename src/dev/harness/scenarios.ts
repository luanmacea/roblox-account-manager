/**
 * Cenários do harness de UI.
 *
 * Cada cenário monta um backend de mentira com dados realistas e, quando faz
 * sentido, **manda os eventos aos poucos** — é justamente o que só aparece com
 * a tela montada: lista que chega em páginas, progresso, estado final.
 *
 * Escolha pela URL: `http://localhost:1420/?scenario=servers-big-game&accounts=6`.
 * Cada agente abre a sua própria URL e olha um cenário diferente. `&lang=pt`
 * (ou `de`, `es`) sobe a tela naquele idioma, sem passar pelo seletor de Settings.
 *
 * Um cenário **nunca** inventa o comportamento que está sendo testado: ele
 * entrega os mesmos dados que a API do Roblox entregaria (inclusive na ordem
 * ruim), para a UI ter que se virar.
 */
import realPlacePages from "./fixtures/place-15101393044.json";
import {
  harnessCalls,
  harnessEmit,
  resetHarnessCalls,
  setInvokeHandler,
  type InvokeHandler,
} from "./bus";
import { GAME_FIXTURES, fixtureIcon, iconForGame } from "./games";
import { seedTourStorage, tourHandler } from "./tour";
import { HARNESS_RELEASES, kindBody } from "./releases";
import { clipReport } from "./clipReport";

const params = new URLSearchParams(window.location.search);
const scenarioName = params.get("scenario") || "default";
const accountCount = Math.max(1, Math.min(Number(params.get("accounts") ?? 6) || 6, 16));
const language = params.get("lang");
// `&names=demo`: nomes inventados com cara de conta de verdade, para gravar os
// vídeos de divulgação sem "TestAccount1" na tela. O padrão
// continua TestAccountN, que é o que os cenários e testes esperam.
const demoNames = params.get("names") === "demo";
const DEMO_NAMES = [
  "Nebula_Main", "NebulaTrades", "FruitFarm_01", "FruitFarm_02", "FruitFarm_03", "FruitFarm_04",
  "FruitFarm_05", "PetGrinder_A", "PetGrinder_B", "PetGrinder_C", "AfkBank_01", "AfkBank_02",
  "BuilderAlt", "EventAlt_01", "EventAlt_02", "SpareAlt",
];

function account(index: number) {
  return {
    UserID: 1000 + index,
    Username: demoNames ? DEMO_NAMES[index - 1] : `TestAccount${index}`,
    Alias: "",
    Description: "",
    Group: demoNames ? (index <= 2 ? "Main" : "Squad alts") : "",
    SecurityToken: `cookie-${index}`,
    Password: "",
    Fields: {},
    Valid: true,
    LastUse: new Date().toISOString(),
    DateAdded: new Date().toISOString(),
    Region: "",
    Order: index,
    Moderated: false,
    Banned: false,
  };
}

const accounts = Array.from({ length: accountCount }, (_, i) => account(i + 1));

const settings: Record<string, Record<string, string>> = {
  General: {
    ...(language ? { Language: language } : {}),
    RestrictedBackgroundStyle: "waves",
    ServerPreference: "bestfit",
    ServerRegionFilter: "",
    ServerScanPages: "30",
    MaxRecentGames: "8",
    MaxRecentJobs: "12",
  },
};

/** Base comum: o app sobe destrancado, com contas e settings. */
/** O `RAMGameLists.json` do harness, em memória. */
let harnessGameLists: Record<string, unknown[]> | null = null;

const baseHandler: InvokeHandler = (cmd, args) => {
  switch (cmd) {
    case "needs_password":
      return false;
    case "is_accounts_encrypted":
      return false;
    case "get_accounts":
      return accounts;
    case "get_all_settings":
      return settings;
    case "get_platform_capabilities":
      return {
        isWindows: true,
        isMacos: false,
        supportsIsolation: true,
        supportsMultiRoblox: true,
        // Mostra no dev:ui as opções que só existem com a feature `live-audio`.
        supportsLiveAudio: true,
      };
    case "remembered_unlock_state":
      return { supported: true, active: false, defaultHours: 24 };
    case "get_launch_queue":
      return { entries: [], active: false, placeId: 0, jobId: "" };
    case "get_running_instances":
      return [];
    case "get_unidentified_clients":
      return [];
    // Modo AFK parado, como o backend responde sem sessão. O `[]` do fallback
    // derrubava a página Session (`afkStatus.accounts` não existe num array).
    case "get_afk_mode_status":
      return {
        active: false,
        startedAtMs: null,
        intervalSeconds: 0,
        key: "",
        mode: "key",
        clickX: 50,
        clickY: 50,
        accounts: [],
      };
    // Auto Rejoin parado, idem (`bottingStatus.userIds`).
    case "get_botting_mode_status":
      return {
        active: false,
        startedAtMs: null,
        placeId: 0,
        jobId: "",
        launchData: "",
        intervalMinutes: 19,
        launchDelaySeconds: 20,
        playerGraceMinutes: 15,
        playerUserIds: [],
        userIds: [],
        accounts: [],
      };
    case "batched_get_avatar_headshots":
      return [];
    case "update_setting": {
      // Guarda de verdade (em memória): sem isso, qualquer tela que releia as
      // settings depois de gravar volta a ver o valor antigo e parece bug.
      const { section, key, value } = (args ?? {}) as {
        section?: string;
        key?: string;
        value?: string;
      };
      if (section && key) {
        settings[section] = { ...(settings[section] ?? {}), [key]: String(value ?? "") };
      }
      return null;
    }
    case "get_place_details": {
      const ids = ((args?.placeIds as number[] | undefined) ?? []).map(Number);
      return ids
        .filter((id) => GAME_FIXTURES[id])
        .map((id) => ({
          placeId: id,
          universeId: GAME_FIXTURES[id].universeId,
          name: GAME_FIXTURES[id].name,
          description: "Fixture do harness.",
          sourceName: GAME_FIXTURES[id].name,
          sourceDescription: "",
          url: `https://www.roblox.com/games/${id}`,
        }));
    }
    case "batched_get_game_icon": {
      const id = Number(args?.placeId ?? 0);
      const game = GAME_FIXTURES[id];
      return game ? iconForGame(id, game.name) : null;
    }
    case "batched_get_game_info": {
      const id = Number(args?.placeId ?? 0);
      const game = GAME_FIXTURES[id];
      return {
        placeId: id,
        universeId: game?.universeId ?? null,
        name: game?.name ?? null,
        iconUrl: game ? iconForGame(id, game.name) : null,
      };
    }
    // Cópia de credencial: o backend devolve quantas linhas copiou e em quanto
    // tempo apaga. O harness não toca na área de transferência de verdade.
    case "copy_account_secret": {
      const ids = ((args?.userIds as number[] | undefined) ?? []).map(Number);
      return { count: ids.length, clearsInSecs: 30 };
    }
    // Sem atualização: o diálogo de update não pode tapar a tela em teste.
    case "check_for_updates_with_channels":
      return null;
    // Sem aviso da chave do vault: o backend devolve `null` quando está tudo em
    // ordem. Cair no `[]` do fallback abaixo pintava a faixa vermelha de "faça
    // backup agora" em todo cenário, porque `[]` é truthy. Os cenários
    // `vault-key-warning-*` sobrescrevem isto quando querem a faixa.
    case "vault_key_warning":
      return null;
    case "get_theme":
      return null;
    // Checagem "o launch não faz nada" (ideia 16): um aviso para a tela ter o
    // que mostrar; a ordem é a do backend.
    // Adicionar por Quick Login (ideia 12): o código fica esperando aprovação.
    case "add_by_quick_login_start":
      return { code: "HAR123", expiresAt: null };
    case "add_by_quick_login_poll":
      return { kind: "pending" };
    case "add_by_quick_login_cancel":
      return null;
    // Tela trancada por inatividade (ideia 27): qualquer senha não vazia.
    case "verify_app_password":
      if (!String(args?.password ?? "")) throw "Wrong password.";
      return null;
    // Resumo do "Reportar problema" (ideia 28).
    case "get_report_environment":
      return { version: "0.0.0-harness", edition: "standard", os: "Windows 11 (harness)" };
    case "run_launch_diagnostics":
      return [
        { id: "robloxInstall", status: "ok", reason: "found" },
        { id: "dataFolder", status: "ok", reason: "writable" },
        { id: "versionsFolder", status: "ok", reason: "writable" },
        { id: "internet", status: "ok", reason: "reachable" },
        { id: "stuckProcesses", status: "warn", reason: "stuck", count: 1 },
        { id: "multiRoblox", status: "ok", reason: "held" },
      ];
    // Favoritos e recentes (`RAMGameLists.json`). Começa sem arquivo (`null`):
    // a UI migra o que o `localStorage` tem — inclusive o que `seedTourStorage`
    // semeou — e as gravações ficam em memória. Cair no `[]` do fallback seria
    // uma resposta que o backend nunca dá.
    case "get_game_lists":
      return harnessGameLists;
    case "save_game_lists":
      harnessGameLists = (args?.lists as typeof harnessGameLists) ?? harnessGameLists;
      return null;
    default:
      // Comando sem resposta no cenário: `[]` é o que a maioria das telas
      // espera, e o aviso no console diz ao agente o que ainda falta cobrir.
      console.warn("[harness] comando sem resposta:", cmd);
      return [];
  }
};

/**
 * Jogo grande, servidores de 13 lugares: exatamente a forma que a API devolve
 * com `sortOrder=Desc` — páginas e páginas de servidores quase cheios antes de
 * aparecer qualquer um que caiba um lote grande.
 */
function bigGamePages(): { id: string; playing: number; maxPlayers: number; ping: number }[][] {
  const pages: { id: string; playing: number; maxPlayers: number; ping: number }[][] = [];
  let serial = 0;
  const make = (playing: number) => ({
    id: `job-${String(++serial).padStart(4, "0")}`,
    playing,
    maxPlayers: 13,
    ping: 20 + (serial % 180),
  });

  // 3 páginas só de servidores cheios demais para um lote de 6.
  for (let page = 0; page < 3; page++) {
    pages.push(Array.from({ length: 100 }, (_, i) => make(12 - (i % 3))));
  }
  // A página em que finalmente aparecem servidores utilizáveis, misturados.
  pages.push([
    ...Array.from({ length: 60 }, () => make(9)),
    make(5), // folga 8 — cabe, mas quase vazio
    make(6), // folga 7 — cabe com folga 1: o ideal
    make(7), // folga 6 — cabe exatamente, sem folga
    ...Array.from({ length: 37 }, () => make(10)),
  ]);
  return pages;
}

/**
 * Emite a varredura do place real página a página, como o backend faz.
 *
 * Fica separado porque dois cenários usam a mesma entrega: o que existe para
 * conferir a ordem da lista e o `tour`, que só precisa da aba cheia.
 */
function emitRealPlaceScan(
  pages: { id: string; playing: number; maxPlayers: number; ping: number | null }[][],
  placeId: number
): void {
  const all: (typeof pages)[number] = [];
  pages.forEach((page, index) => {
    setTimeout(() => {
      all.push(...page);
      harnessEmit("server-scan", {
        scanId: 1,
        placeId,
        servers: [
          ...all.filter((s) => s.playing + accountCount <= s.maxPlayers),
          ...all.filter((s) => s.playing + accountCount > s.maxPlayers),
        ].slice(0, 150),
        scanned: all.length,
        fitting: all.filter((s) => s.playing + accountCount <= s.maxPlayers).length,
        done: index === pages.length - 1,
        stoppedAtLimit: false,
        error: null,
      });
    }, 250 * (index + 1));
  });
}

// ─── AFK mode ────────────────────────────────────────────────────────────────

/** A lista fechada de `AFK_KEYS` (`commands/afk.rs`), na ordem em que o backend a entrega. */
const AFK_KEYS = ["Space", "W", "A", "S", "D", "E", "F", "R", "Q", "1", "2", "3", "4", "5"];

/** `AfkSendError::message()`, por código. A tela escolhe a frase pelo código. */
const AFK_SEND_ERRORS = {
  noWindow: "No Roblox window for this account",
  focusDenied:
    "Windows did not bring this account's Roblox window to the front, so nothing was sent",
  keyRefused: "Windows refused the synthetic key",
  clickRefused: "Windows refused the synthetic click",
} as const;

type AfkSendErrorCode = keyof typeof AFK_SEND_ERRORS;

/** Uma conta no modo, como o `AfkAccountRuntime` do backend. */
interface AfkRuntime {
  userId: number;
  /** Último envio — ou a entrada no modo, enquanto não houve envio. */
  lastSendAtMs: number;
  sends: number;
  lastErrorCode: AfkSendErrorCode | null;
}

interface AfkSessionFields {
  startedAtMs: number;
  /** Segundos entre dois envios da mesma conta (`clamp_afk_interval_seconds`). */
  intervalSeconds: number;
  key: string;
  /** `AfkMode::as_str()`. */
  mode: "key" | "click";
  /** Ponto padrão do modo clique, em %. */
  clickX: number;
  clickY: number;
  accounts: Map<number, AfkRuntime>;
}

/**
 * O que o Windows faria, fixo por cenário — é o dado que o cenário entrega, como
 * a lista de servidores é o dado dos cenários de Servers.
 */
interface AfkWorld {
  /** Contas com cliente aberto **por este app** (o tracker sabe o PID delas). */
  withClient: Set<number>;
  /**
   * Contas cuja janela o Windows mantém atrás no ciclo **automático**: é o caso
   * normal enquanto o gerenciador não é a janela em uso (docs/features/afk-mode.md,
   * "Consequência honesta"). No envio manual o usuário acabou de clicar no app, e
   * aí o Windows deixa a janela vir para frente.
   */
  focusDeniedWhenScheduled: Set<number>;
  /**
   * O que o Marcar (`afk_capture_point`) acha embaixo do mouse: a janela de uma
   * conta e o ponto em %, ou o código do erro — fixo por cenário.
   */
  underMouse: { userId: number; xPct: number; yPct: number } | string;
}

/** As quatro primeiras contas com cliente aberto; a janela da 2ª fica presa atrás. */
function afkWorld(): AfkWorld {
  return {
    withClient: new Set(accounts.slice(0, 4).map((a) => a.UserID)),
    focusDeniedWhenScheduled: new Set(accounts.slice(1, 2).map((a) => a.UserID)),
    underMouse: accounts[0]
      ? { userId: accounts[0].UserID, xPct: 37.5, yPct: 62.5 }
      : "notAnAccountWindow",
  };
}

function afkJoined(userId: number, atMs: number): AfkRuntime {
  return { userId, lastSendAtMs: atMs, sends: 0, lastErrorCode: null };
}

/**
 * Sessão que já estava rodando quando a tela carregou: o backend guarda a sessão
 * enquanto o app está aberto, e a tela a lê em `get_afk_mode_status`. É o estado
 * que o agendador deixa depois de `sinceMinutes` com intervalo de 10 min e a
 * tecla Space.
 *
 * No modo: as três primeiras contas com cliente e, quando existir, uma cujo
 * cliente fechou depois do primeiro ciclo (os seguintes a marcaram `noWindow`).
 * A janela da 2ª conta o Windows nunca deixou vir para frente: zero envios e
 * `focusDenied`.
 */
function afkRunningSession(world: AfkWorld, sinceMinutes: number): AfkSessionFields {
  const intervalSeconds = 600;
  const startedAtMs = Date.now() - sinceMinutes * 60_000;
  const cycles = Math.floor((sinceMinutes * 60) / intervalSeconds);
  // Todo ciclo remarca o relógio de quem ele visitou — com ou sem envio. (O fim
  // de cada ciclo empurra o seguinte uns segundos; aqui não faz diferença.)
  const lastVisitAtMs = startedAtMs + cycles * intervalSeconds * 1000;
  const inMode = [
    ...accounts.filter((a) => world.withClient.has(a.UserID)).slice(0, 3),
    ...accounts.filter((a) => !world.withClient.has(a.UserID)).slice(0, 1),
  ];
  const entries = inMode.map((a): AfkRuntime => {
    const userId = a.UserID;
    if (cycles === 0) return afkJoined(userId, startedAtMs);
    if (!world.withClient.has(userId)) {
      return {
        userId,
        lastSendAtMs: lastVisitAtMs,
        sends: 1,
        lastErrorCode: cycles > 1 ? "noWindow" : null,
      };
    }
    const denied = world.focusDeniedWhenScheduled.has(userId);
    return {
      userId,
      lastSendAtMs: lastVisitAtMs,
      sends: denied ? 0 : cycles,
      lastErrorCode: denied ? "focusDenied" : null,
    };
  });
  return {
    startedAtMs,
    intervalSeconds,
    key: "Space",
    mode: "key",
    clickX: 50,
    clickY: 50,
    accounts: new Map(entries.map((entry) => [entry.userId, entry])),
  };
}

/**
 * O AFK mode do backend (`src-tauri/src/commands/afk.rs`), em memória.
 *
 * Sem isto todo comando do AFK caía no `[]` do `baseHandler`: a lista de teclas
 * vinha vazia, o modo não ligava e nada da tela podia ser visto funcionando.
 *
 * Reproduz o **contrato**: as mesmas validações e mensagens de erro; o mesmo
 * status (`afk_status_from_parts`: contas em ordem de user id, `nextSendAtMs` =
 * último envio + intervalo); os mesmos eventos (`afk-status`, `afk-cycle
 * { sent }`, `afk-stopped`); o agendador de 1 s; um ciclo por vez; e a duração
 * de um ciclo (150 ms de folga + 40 ms de tecla + 250 ms por janela).
 *
 * O que ele não tem é Windows: quem decide se a janela vem para frente é o
 * `AfkWorld`. E não faz nada que é da tela — não conta o relógio, não formata
 * "rodando há", não escolhe frase: manda o código do erro, como o backend.
 */
function afkHandler(
  fallback: InvokeHandler,
  world: AfkWorld,
  initial: AfkSessionFields | null
): InvokeHandler {
  type Session = AfkSessionFields & { id: number; stopping: boolean };
  type Outcome = [number, AfkSendErrorCode | null][];

  let nextSessionId = 1;
  let session: Session | null = null;
  let loopTimer: number | undefined;
  let cycleQueue: Promise<unknown> = Promise.resolve();

  const dedupe = (raw: unknown): number[] => [
    ...new Set(((raw as unknown[] | undefined) ?? []).map(Number)),
  ];

  /** `clamp_afk_percent`. */
  const clampPercent = (raw: unknown): number => {
    const value = Number(raw);
    return Number.isFinite(value) ? Math.min(100, Math.max(0, value)) : 50;
  };

  /** `validate_afk_start`: o modo clique não usa tecla. */
  function startRefusal(mode: "key" | "click", key: string, userIds: number[]): string | null {
    if (
      mode === "key" &&
      !AFK_KEYS.some((allowed) => allowed.toLowerCase() === key.toLowerCase())
    ) {
      return "Choose one of the AFK mode keys before starting";
    }
    if (userIds.length === 0) return "Put at least one account in AFK mode before starting";
    return null;
  }

  /** `afk_status_from_parts`; sem sessão, o `AfkStatusPayload::default()`. */
  function status() {
    if (!session) {
      return {
        active: false,
        startedAtMs: null,
        intervalSeconds: 0,
        key: "",
        mode: "key",
        clickX: 50,
        clickY: 50,
        accounts: [],
      };
    }
    const intervalMs = session.intervalSeconds * 1000;
    return {
      active: true,
      startedAtMs: session.startedAtMs,
      intervalSeconds: session.intervalSeconds,
      key: session.key,
      mode: session.mode,
      clickX: session.clickX,
      clickY: session.clickY,
      accounts: [...session.accounts.values()]
        .sort((a, b) => a.userId - b.userId)
        .map((entry) => ({
          userId: entry.userId,
          lastSendAtMs: entry.lastSendAtMs,
          nextSendAtMs: entry.lastSendAtMs + intervalMs,
          sends: entry.sends,
          lastError: entry.lastErrorCode ? AFK_SEND_ERRORS[entry.lastErrorCode] : null,
          lastErrorCode: entry.lastErrorCode,
        })),
    };
  }

  const publish = () => harnessEmit("afk-status", status());

  /**
   * `run_afk_cycle_blocking` sem Windows: o resultado de cada conta sai do
   * mundo e o tempo é o do ciclo de verdade. Um ciclo por vez, como o
   * `AFK_CYCLE_LOCK` faz com o agendador e o envio manual.
   */
  function runCycle(targets: number[], scheduled: boolean): Promise<Outcome> {
    const run = cycleQueue.then(() => {
      const outcome: Outcome = targets.map((userId): [number, AfkSendErrorCode | null] => [
        userId,
        !world.withClient.has(userId)
          ? "noWindow"
          : scheduled && world.focusDeniedWhenScheduled.has(userId)
            ? "focusDenied"
            : null,
      ]);
      // Conta sem janela é pulada na hora; foco negado não chega a teclar.
      const durationMs = outcome.reduce(
        (sum, [, error]) =>
          sum + (error === "noWindow" ? 0 : error === "focusDenied" ? 400 : 440),
        0
      );
      return new Promise<Outcome>((resolve) =>
        window.setTimeout(() => resolve(outcome), durationMs)
      );
    });
    cycleQueue = run.catch(() => undefined);
    return run;
  }

  /** Remarca o relógio de quem o ciclo visitou, com ou sem envio. */
  function apply(target: Session, outcome: Outcome, attemptedAtMs: number) {
    for (const [userId, error] of outcome) {
      const entry = target.accounts.get(userId);
      if (!entry) continue;
      entry.lastSendAtMs = attemptedAtMs;
      if (error === null) entry.sends += 1;
      entry.lastErrorCode = error;
    }
  }

  const sentIn = (outcome: Outcome) => outcome.filter(([, error]) => error === null).length;

  /** `run_afk_session`: acorda a cada segundo e visita quem venceu o intervalo. */
  function tick(current: Session) {
    loopTimer = window.setTimeout(async () => {
      if (session !== current || current.stopping) return;
      const intervalMs = current.intervalSeconds * 1000;
      const now = Date.now();
      const due = [...current.accounts.values()]
        .filter((entry) => now - entry.lastSendAtMs >= intervalMs)
        .map((entry) => entry.userId)
        .sort((a, b) => a - b);
      if (due.length > 0) {
        const outcome = await runCycle(due, true);
        if (session !== current || current.stopping) return;
        // Marcado com o fim do ciclo, como o `finished_at` do laço: a espera
        // conta de quando o ciclo acabou.
        apply(current, outcome, Date.now());
        publish();
        const sent = sentIn(outcome);
        if (sent > 0) harnessEmit("afk-cycle", { sent });
      }
      tick(current);
    }, 1_000);
  }

  function begin(fields: AfkSessionFields) {
    session = { ...fields, id: nextSessionId++, stopping: false };
    tick(session);
  }

  /**
   * `stop_afk_session`: o laço confere a parada a cada 250 ms e sai emitindo
   * `afk-stopped` e o status vazio. Ciclo que ainda estava em andamento é
   * abandonado.
   */
  function stopSession(): Promise<void> {
    const current = session;
    if (!current) return Promise.resolve();
    current.stopping = true;
    window.clearTimeout(loopTimer);
    return new Promise((resolve) => {
      window.setTimeout(() => {
        if (session === current) session = null;
        harnessEmit("afk-stopped", null);
        publish();
        resolve();
      }, 250);
    });
  }

  if (initial) begin(initial);

  return (cmd, args) => {
    switch (cmd) {
      case "get_afk_keys":
        return [...AFK_KEYS];
      case "get_afk_mode_status":
        return status();
      case "start_afk_mode": {
        const userIds = dedupe(args.userIds);
        const key = String(args.key ?? "");
        // `AfkMode::parse`: qualquer outra coisa é tecla.
        const mode = String(args.mode ?? "").trim().toLowerCase() === "click" ? "click" : "key";
        const refusal = startRefusal(mode, key, userIds);
        if (refusal) return Promise.reject(refusal);
        return stopSession().then(() => {
          const now = Date.now();
          begin({
            startedAtMs: now,
            // `clamp_afk_interval_seconds`
            intervalSeconds: Math.min(7_200, Math.max(5, Math.trunc(Number(args.intervalSeconds) || 0))),
            key,
            mode,
            clickX: clampPercent(args.clickX),
            clickY: clampPercent(args.clickY),
            // Cada conta entra com o relógio marcando agora: o prazo do primeiro
            // envio já vai no status do start.
            accounts: new Map(userIds.map((userId) => [userId, afkJoined(userId, now)])),
          });
          publish();
          return status();
        });
      }
      case "stop_afk_mode":
        return stopSession().then(() => {
          publish();
          return null;
        });
      case "set_afk_accounts": {
        const userIds = dedupe(args.userIds);
        const current = session;
        if (!current) return Promise.reject("AFK mode is not running");
        if (userIds.length === 0) {
          return stopSession().then(() => {
            publish();
            return status();
          });
        }
        const now = Date.now();
        for (const userId of [...current.accounts.keys()]) {
          if (!userIds.includes(userId)) current.accounts.delete(userId);
        }
        for (const userId of userIds) {
          if (!current.accounts.has(userId)) current.accounts.set(userId, afkJoined(userId, now));
        }
        publish();
        return status();
      }
      case "afk_trigger_now": {
        const current = session;
        if (!current) return Promise.reject("Start AFK mode before sending the key by hand");
        // `afk_manual_targets`: só quem está no modo, na ordem pedida.
        const targets = dedupe(args.userIds).filter((userId) => current.accounts.has(userId));
        if (targets.length === 0) return Promise.reject("None of those accounts is in AFK mode");
        // Tecla ou clique é o da sessão ligada.
        const refusal = startRefusal(current.mode, current.key, targets);
        if (refusal) return Promise.reject(refusal);
        return runCycle(targets, false).then((outcome) => {
          apply(current, outcome, Date.now());
          publish();
          const sent = sentIn(outcome);
          if (sent > 0) harnessEmit("afk-cycle", { sent });
          return sent;
        });
      }
      // `afk_capture_point`: o que o mundo diz que está embaixo do mouse.
      case "afk_capture_point":
        return typeof world.underMouse === "string"
          ? Promise.reject(world.underMouse)
          : { ...world.underMouse };
      // O ouvinte do `afk-cycle` relê `Afk.BeepOnCycle` na hora de bipar.
      case "get_setting": {
        const { section, key } = args as { section?: string; key?: string };
        return (section && key ? settings[section]?.[key] : undefined) ?? null;
      }
      // A forma do backend (`RunningInstance`, sem rename): snake_case.
      case "get_running_instances":
        return [...world.withClient].map((userId, index) => ({
          pid: 8120 + index * 4,
          user_id: userId,
          browser_tracker_id: `${userId}0001`,
        }));
      default:
        return fallback(cmd, args);
    }
  };
}

/** Place em que as contas com cliente aberto estão jogando (presença). */
const BOTTING_WORLD_PLACE = 606849621;
/** O outro jogo de `&games=mixed`. */
const BOTTING_OTHER_PLACE = 920587237;

/**
 * Auto Rejoin do lado do backend (`commands/botting.rs`), só o que a tela lê:
 * status, start (com `adoptRunning`), add, stop, mains e ações por conta. A
 * presença (`get_account_game_location`) diz que as contas com cliente aberto
 * estão no place {@link BOTTING_WORLD_PLACE}; as outras, fora de jogo.
 *
 * `active` liga uma sessão que já rodava quando a tela carregou, com as duas
 * primeiras contas com cliente.
 */
function bottingHandler(fallback: InvokeHandler, world: AfkWorld, active: boolean): InvokeHandler {
  type Row = {
    userId: number;
    isPlayer: boolean;
    disconnected: boolean;
    phase: string;
    retryCount: number;
    nextRestartAtMs: number | null;
    playerGraceUntilMs: number | null;
    lastError: string | null;
  };
  let session: {
    startedAtMs: number;
    placeId: number;
    jobId: string;
    launchData: string;
    intervalMinutes: number;
    launchDelaySeconds: number;
    playerGraceMinutes: number;
    playerUserIds: number[];
    userIds: number[];
  } | null = null;

  const rowFor = (userId: number, index: number): Row => {
    const s = session!;
    const isPlayer = s.playerUserIds.includes(userId);
    return {
      userId,
      isPlayer,
      disconnected: false,
      phase: isPlayer ? "player" : "waiting-rejoin",
      retryCount: 0,
      nextRestartAtMs: isPlayer
        ? null
        : s.startedAtMs + s.intervalMinutes * 60_000 + index * s.launchDelaySeconds * 1000,
      playerGraceUntilMs: null,
      lastError: null,
    };
  };

  const status = () =>
    session
      ? { active: true, ...session, accounts: session.userIds.map(rowFor) }
      : {
          active: false,
          startedAtMs: null,
          placeId: 0,
          jobId: "",
          launchData: "",
          intervalMinutes: 19,
          launchDelaySeconds: 20,
          playerGraceMinutes: 15,
          playerUserIds: [],
          userIds: [],
          accounts: [],
        };

  const publish = () => harnessEmit("botting-status", status());
  const ids = (raw: unknown): number[] => [...new Set(((raw as unknown[]) ?? []).map(Number))];

  if (active) {
    const running = [...world.withClient].slice(0, 2);
    if (running.length >= 2) {
      session = {
        startedAtMs: Date.now() - 7 * 60_000,
        placeId: BOTTING_WORLD_PLACE,
        jobId: "",
        launchData: "",
        intervalMinutes: 19,
        launchDelaySeconds: 20,
        playerGraceMinutes: 15,
        playerUserIds: [],
        userIds: running,
      };
    }
  }

  return (cmd, args) => {
    switch (cmd) {
      case "get_botting_mode_status":
        return status();
      case "get_account_game_location": {
        const userId = Number(args?.userId);
        const inGame = world.withClient.has(userId);
        // `&games=mixed`: a 2ª conta com cliente está em outro jogo (Adopt Me!).
        const elsewhere = params.get("games") === "mixed" && [...world.withClient][1] === userId;
        const placeId = inGame ? (elsewhere ? BOTTING_OTHER_PLACE : BOTTING_WORLD_PLACE) : null;
        return { userId, inGame, placeId, jobId: null };
      }
      case "start_botting_mode": {
        // `start_botting_mode`: Multi Roblox ligado e duas contas no mínimo.
        if (settings.General?.EnableMultiRbx !== "true") {
          return Promise.reject("Auto Rejoin currently requires Multi Roblox to be enabled");
        }
        const userIds = ids(args.userIds);
        if (userIds.length < 2) return Promise.reject("Select at least 2 accounts");
        session = {
          startedAtMs: Date.now(),
          placeId: Number(args.placeId),
          jobId: String(args.jobId ?? ""),
          launchData: String(args.launchData ?? ""),
          intervalMinutes: Number(args.intervalMinutes) || 19,
          launchDelaySeconds: Number(args.launchDelaySeconds) || 20,
          playerGraceMinutes: Number(args.playerGraceMinutes) || 15,
          playerUserIds: ids(args.playerUserIds),
          userIds,
        };
        publish();
        return status();
      }
      case "add_botting_accounts": {
        if (!session) return Promise.reject("Auto Rejoin is not running");
        session.userIds = [...new Set([...session.userIds, ...ids(args.userIds)])];
        publish();
        return status();
      }
      case "set_botting_player_accounts": {
        if (!session) return status();
        session.playerUserIds = ids(args.userIds);
        publish();
        return status();
      }
      case "botting_account_action":
        return status();
      case "stop_botting_mode":
        session = null;
        harnessEmit("botting-stopped", null);
        publish();
        return null;
      default:
        return fallback(cmd, args);
    }
  };
}

/** Item do catálogo grátis como o backend serializa (`FreeCatalogItem`). */
interface HarnessCatalogItem {
  id: number;
  kind: "Asset" | "Bundle";
  typeId: number;
  name: string;
  collectibleItemId: string;
}

function catalogItem(id: number, kind: "Asset" | "Bundle", typeId: number, name: string): HarnessCatalogItem {
  return { id, kind, typeId, name, collectibleItemId: `harness-${kind.toLowerCase()}-${id}` };
}

/**
 * Catálogo grátis oficial (`avatar_free_catalog`), com nomes do catálogo do
 * Roblox e todas as categorias: cabelo (41), chapéu (8), acessórios (42–47),
 * camisa (11), calça (12), camiseta (2), corpo (bundle 1) e cabeça (bundle 4).
 * Um asset e um bundle têm o mesmo número de propósito: a tela tem de separar
 * os dois.
 */
const FREE_CATALOG: HarnessCatalogItem[] = [
  catalogItem(62724852, "Asset", 41, "Chestnut Bun"),
  catalogItem(4819740796, "Asset", 41, "Robox"),
  catalogItem(6340213, "Asset", 41, "Brown Charmer Hair"),
  catalogItem(376524487, "Asset", 41, "Blonde Spiked Hair"),
  catalogItem(1374269, "Asset", 8, "Kitty Ears"),
  catalogItem(607702162, "Asset", 8, "Roblox Baseball Cap"),
  catalogItem(1031429, "Asset", 8, "Bucket Hat"),
  catalogItem(4819722776, "Asset", 42, "Black Aviators"),
  catalogItem(4819625858, "Asset", 43, "Gold Chain"),
  catalogItem(4819743519, "Asset", 44, "Shoulder Owl"),
  catalogItem(4819763009, "Asset", 46, "Bear Backpack"),
  catalogItem(4819753478, "Asset", 47, "Utility Belt"),
  catalogItem(144076358, "Asset", 11, "Blue and Black Motorcycle Shirt"),
  catalogItem(398633584, "Asset", 11, "Guitar Tee with Black Jacket"),
  catalogItem(382537569, "Asset", 11, "Pink Hoodie with Bear"),
  catalogItem(144076760, "Asset", 12, "Dark Green Jeans"),
  catalogItem(398633812, "Asset", 12, "Black Jeans with Sneakers"),
  catalogItem(382538503, "Asset", 12, "Jean Shorts with White Sneakers"),
  catalogItem(607785314, "Asset", 2, "ROBLOX Logo Tee"),
  catalogItem(239, "Bundle", 1, "Rthro Boy"),
  catalogItem(240, "Bundle", 1, "Rthro Girl"),
  catalogItem(109, "Bundle", 1, "Robloxian 2.0"),
  catalogItem(192, "Bundle", 1, "Woman"),
  catalogItem(6340213, "Bundle", 4, "Classic Male v2 Head"),
  catalogItem(4920, "Bundle", 4, "Classic Female v2 Head"),
];

/** Cor da miniatura por tipo, para a grade não virar um bloco de uma cor só. */
const THUMB_COLORS: Record<string, string> = {
  "Asset:41": "#92400e",
  "Asset:8": "#be123c",
  "Asset:11": "#1d4ed8",
  "Asset:12": "#334155",
  "Asset:2": "#0f766e",
  "Bundle:1": "#7c3aed",
  "Bundle:4": "#c2410c",
};

function catalogThumb(item: HarnessCatalogItem): string {
  const color = THUMB_COLORS[`${item.kind}:${item.typeId}`] ?? "#15803d";
  return fixtureIcon((item.name.trim()[0] || "?").toUpperCase(), color);
}

interface HarnessAvatarResult {
  userId: number;
  avatarId: string;
  status: "ok" | "skipped" | "failed";
  reason: string | null;
  claimed: number;
  missing: number;
}

interface HarnessAvatarBatch {
  running: boolean;
  total: number;
  done: number;
  currentUserId: number | null;
  accounts: HarnessAvatarResult[];
}

/**
 * Avatares grátis (`commands/avatars.rs`) em memória. Só entrega dados: o
 * montador, o sorteio e a validação são da tela. O lote espelha o backend —
 * uma conta por vez, o retrato inteiro em `avatar-batch-state` a cada passo, o
 * `avatar_apply_batch` só responde no fim, e o cancelamento vale da próxima
 * conta em diante. Resultado fixo: a 1ª conta resgata 3 peças, a 2ª esbarra
 * na verificação do Roblox (`challenge`), as demais vestem sem resgatar nada.
 */
function avatarsHandler(fallback: InvokeHandler): InvokeHandler {
  const byKey = new Map(FREE_CATALOG.map((item) => [`${item.kind}:${item.id}`, item]));
  const find = (kind: "Asset" | "Bundle", id: number) => byKey.get(`${kind}:${id}`)!;
  let saved = [
    {
      id: "av_harness_street",
      name: "Street",
      items: [
        find("Asset", 6340213),
        find("Asset", 607702162),
        find("Asset", 398633584),
        find("Asset", 398633812),
        find("Bundle", 239),
      ],
      skinColor: 18,
    },
    {
      id: "av_harness_classic",
      name: "Classic",
      items: [
        find("Asset", 62724852),
        find("Asset", 4819763009),
        find("Asset", 382537569),
        find("Asset", 382538503),
        find("Bundle", 240),
        find("Bundle", 4920),
      ],
      skinColor: 1030,
    },
  ];
  let batch: HarnessAvatarBatch = { running: false, total: 0, done: 0, currentUserId: null, accounts: [] };
  let cancel = false;
  const publish = (change: Partial<HarnessAvatarBatch>) => {
    batch = { ...batch, ...change };
    harnessEmit("avatar-batch-state", batch);
    return batch;
  };
  const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

  return (cmd, args) => {
    switch (cmd) {
      case "avatar_free_catalog":
        return wait(400).then(() => FREE_CATALOG);
      case "batch_thumbnails": {
        const requests = (args?.requests as { requestId: string; type: string; targetId: number }[]) ?? [];
        return wait(250).then(() =>
          requests.map((r) => {
            const item = byKey.get(`${r.type === "Asset" ? "Asset" : "Bundle"}:${r.targetId}`);
            return {
              targetId: r.targetId,
              requestId: r.requestId,
              state: "Completed",
              errorCode: 0,
              imageUrl: item ? catalogThumb(item) : null,
            };
          })
        );
      }
      case "avatar_list_saved":
        return saved;
      case "avatar_save": {
        const avatar = args.avatar as (typeof saved)[number];
        saved = saved.some((a) => a.id === avatar.id)
          ? saved.map((a) => (a.id === avatar.id ? avatar : a))
          : [...saved, avatar];
        return avatar;
      }
      case "avatar_delete": {
        const before = saved.length;
        saved = saved.filter((a) => a.id !== args.id);
        return saved.length !== before;
      }
      case "get_avatar_batch_state":
        return batch;
      case "avatar_cancel_batch":
        cancel = true;
        return null;
      case "invalidate_avatar_headshots":
        return null;
      case "avatar_apply_batch": {
        // Mesmas recusas que o backend devolve (em inglês: a tela passa pelo t()).
        if (batch.running) return Promise.reject("An avatar batch is already running");
        const userIds = (args.userIds as number[] | undefined) ?? [];
        const avatarIds = ((args.avatarIds as string[] | undefined) ?? []).filter((id) =>
          saved.some((a) => a.id === id)
        );
        if (userIds.length === 0) return Promise.reject("No account selected");
        if (avatarIds.length === 0) return Promise.reject("No saved avatar selected");
        cancel = false;
        publish({ running: true, total: userIds.length, done: 0, currentUserId: null, accounts: [] });
        return (async () => {
          for (const [index, userId] of userIds.entries()) {
            const avatarId = avatarIds[index % avatarIds.length];
            let result: HarnessAvatarResult;
            if (cancel) {
              result = { userId, avatarId, status: "skipped", reason: "cancelled", claimed: 0, missing: 0 };
            } else {
              publish({ currentUserId: userId });
              await wait(1400);
              result =
                index === 0
                  ? { userId, avatarId, status: "ok", reason: null, claimed: 3, missing: 0 }
                  : index === 1
                    ? { userId, avatarId, status: "skipped", reason: "challenge", claimed: 0, missing: 0 }
                    : { userId, avatarId, status: "ok", reason: null, claimed: 0, missing: 0 };
            }
            const accountsSoFar = [...batch.accounts, result];
            publish({ accounts: accountsSoFar, done: accountsSoFar.length, currentUserId: null });
          }
          return publish({ running: false, currentUserId: null });
        })();
      }
      default:
        return fallback(cmd, args);
    }
  };
}

interface HarnessGroup {
  id: number;
  name: string;
  description: string;
  memberCount: number;
  publicEntryAllowed: boolean;
  hasVerifiedBadge: boolean;
  isLocked: boolean;
}

const HARNESS_GROUPS: HarnessGroup[] = [
  ["Pet Simulator Fans", 1_284_311, true, true],
  ["Pet Traders United", 94_200, false, false],
  ["Pet Sim Speedrunners", 3_120, true, false],
  ["Pets & Plushies — Official Community With A Very Long Name That Should Truncate", 512_000, true, true],
  ["Pet Builders", 860, false, false],
  ["Pet Lovers BR", 41_000, true, false],
].map(([name, memberCount, publicEntryAllowed, hasVerifiedBadge], index) => ({
  id: 4_100_000 + index,
  name: name as string,
  description: "",
  memberCount: memberCount as number,
  publicEntryAllowed: publicEntryAllowed as boolean,
  hasVerifiedBadge: hasVerifiedBadge as boolean,
  isLocked: false,
}));

const HARNESS_GROUPS_PAGE_2: HarnessGroup[] = [
  { id: 4_100_100, name: "Pet Collectors Guild", description: "", memberCount: 12_000, publicEntryAllowed: true, hasVerifiedBadge: false, isLocked: false },
  { id: 4_100_101, name: "Retired Pet Club", description: "", memberCount: 77, publicEntryAllowed: true, hasVerifiedBadge: false, isLocked: true },
];

/**
 * "Grupos populares" (campo vazio): nomes inventados, membros na casa dos
 * milhões, fora de ordem de propósito — quem ordena é a tela.
 */
const HARNESS_POPULAR_GROUPS: HarnessGroup[] = [
  ["Obby Makers Guild", 18_621_888, true, true],
  ["Sky Tycoon Studio", 138_130_377, true, true],
  ["Pet Collectors HQ", 29_176_487, true, true],
  ["Racing Club Official", 8_642_738, true, true],
  ["Avatar Creators", 44_070_059, true, true],
  ["Tower Climbers", 15_448_056, false, true],
  ["Builders Legion", 10_092_230, true, true],
  ["Roleplay Town Community With A Very Long Name That Should Truncate", 12_487_223, true, false],
].map(([name, memberCount, publicEntryAllowed, hasVerifiedBadge], index) => ({
  id: 4_200_000 + index,
  name: name as string,
  description: "",
  memberCount: memberCount as number,
  publicEntryAllowed: publicEntryAllowed as boolean,
  hasVerifiedBadge: hasVerifiedBadge as boolean,
  isLocked: false,
}));

const GROUP_COLORS = ["#1d4ed8", "#be123c", "#0f766e", "#7c3aed", "#c2410c", "#15803d"];

interface HarnessGroupJoin {
  running: boolean;
  groupId: number | null;
  groupName: string;
  total: number;
  done: number;
  currentUserId: number | null;
  accounts: {
    userId: number;
    status: string;
    reason: string | null;
    challengeType?: string | null;
    detail?: string | null;
  }[];
}

/**
 * Página Groups (`commands/groups.rs`). Só entrega dados: a busca devolve uma
 * página com cursor e depois uma segunda; um número ou link devolve um grupo.
 * O lote espelha o backend — uma conta por vez, o retrato inteiro em
 * `groups-join-state` a cada passo, `groups_join_batch` só responde no fim — e
 * o resultado de cada conta é fixo pela posição: a 1ª entra, a 2ª esbarra no
 * captcha, a 3ª já é membro, a 4ª fica pendente, a 5ª falha (limite de
 * grupos), a 6ª esbarra num desafio que **não** é captcha (`proofofwork`, o
 * caso dos termos de uso que o dono viu), as demais entram. A conferência
 * devolve "notMember" na primeira vez e "joined" depois, como se a pessoa
 * tivesse entrado pelo navegador; o "Tentar de novo" devolve "joined". Com o
 * campo vazio, `groups_popular` entrega os populares em 700 ms
 * (`&popular=fail` = falha, e a tela volta para a dica).
 */
function groupsHandler(fallback: InvokeHandler): InvokeHandler {
  const all = [...HARNESS_GROUPS, ...HARNESS_GROUPS_PAGE_2, ...HARNESS_POPULAR_GROUPS];
  let batch: HarnessGroupJoin = {
    running: false,
    groupId: null,
    groupName: "",
    total: 0,
    done: 0,
    currentUserId: null,
    accounts: [],
  };
  let cancel = false;
  const checks = new Map<number, number>();
  const publish = (change: Partial<HarnessGroupJoin>) => {
    batch = { ...batch, ...change };
    harnessEmit("groups-join-state", batch);
    return batch;
  };
  const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
  const OUTCOMES: HarnessGroupJoin["accounts"][number][] = [
    { userId: 0, status: "joined", reason: null },
    {
      userId: 0,
      status: "challenge",
      reason: "Challenge is required to authorize the request",
      challengeType: "captcha",
      detail: "HTTP 403 · challenge captcha · code 0",
    },
    { userId: 0, status: "alreadyMember", reason: null },
    { userId: 0, status: "pending", reason: null },
    {
      userId: 0,
      status: "failed",
      reason: "You are already in the maximum number of groups.",
      detail: "HTTP 403 · code 6",
    },
    {
      userId: 0,
      status: "challenge",
      reason: "Challenge is required to authorize the request",
      challengeType: "proofofwork",
      detail: "HTTP 403 · challenge proofofwork · code 0",
    },
  ];

  return (cmd, args) => {
    switch (cmd) {
      case "groups_search": {
        const query = String(args.query ?? "").trim();
        const byId = /^\d+$/.test(query) ? Number(query) : Number(/(?:groups|communities)\/(\d+)/.exec(query)?.[1] ?? NaN);
        if (Number.isFinite(byId)) {
          const found = all.find((g) => g.id === byId);
          return wait(300).then(() =>
            found ? { groups: [found], nextCursor: null } : Promise.reject("Group is invalid or does not exist.")
          );
        }
        if (query.length < 2) return Promise.reject("Type at least 2 characters");
        if (args.cursor === "page-2") return wait(500).then(() => ({ groups: HARNESS_GROUPS_PAGE_2, nextCursor: null }));
        return wait(500).then(() => ({ groups: HARNESS_GROUPS, nextCursor: "page-2" }));
      }
      case "groups_icons": {
        const ids = (args.groupIds as number[] | undefined) ?? [];
        return wait(250).then(() =>
          ids.map((id, index) => {
            const group = all.find((g) => g.id === id);
            return {
              targetId: id,
              // A 3ª fica sem imagem: o cartão tem que mostrar o ícone padrão.
              imageUrl:
                group && index !== 2
                  ? fixtureIcon((group.name.trim()[0] || "?").toUpperCase(), GROUP_COLORS[id % GROUP_COLORS.length])
                  : null,
              thumbnailType: "GroupIcon",
            };
          })
        );
      }
      case "groups_popular":
        return wait(700).then(() =>
          params.get("popular") === "fail"
            ? Promise.reject("Could not load the popular groups")
            : HARNESS_POPULAR_GROUPS
        );
      case "get_groups_join_state":
        return batch;
      case "groups_join_retry": {
        if (batch.running) return Promise.reject("A group join is already running");
        const userId = Number(args.userId);
        const row = { userId, status: "joined", reason: null, challengeType: null, detail: null };
        const has = batch.accounts.some((r) => r.userId === userId);
        publish({
          accounts: has
            ? batch.accounts.map((r) => (r.userId === userId ? { ...r, status: "joining" } : r))
            : [...batch.accounts, { ...row, status: "joining" }],
        });
        return wait(900).then(() =>
          publish({ accounts: batch.accounts.map((r) => (r.userId === userId ? row : r)) })
        );
      }
      case "groups_cancel_join":
        cancel = true;
        return null;
      case "open_account_browser":
        return null;
      case "groups_check_membership": {
        const userId = Number(args.userId);
        const seen = (checks.get(userId) ?? 0) + 1;
        checks.set(userId, seen);
        const status = seen === 1 ? "notMember" : "joined";
        if (batch.groupId === args.groupId) {
          publish({
            accounts: batch.accounts.map((row) =>
              row.userId === userId ? { ...row, status, reason: null, challengeType: null, detail: null } : row
            ),
          });
        }
        return wait(600).then(() => status);
      }
      case "groups_join_batch": {
        if (batch.running) return Promise.reject("A group join is already running");
        const userIds = (args.userIds as number[] | undefined) ?? [];
        const group = all.find((g) => g.id === args.groupId);
        if (userIds.length === 0) return Promise.reject("No account selected");
        if (!group) return Promise.reject("Pick a group first");
        if (group.isLocked) return Promise.reject("This group is locked and does not accept new members");
        cancel = false;
        publish({
          running: true,
          groupId: group.id,
          groupName: group.name,
          total: userIds.length,
          done: 0,
          currentUserId: null,
          accounts: userIds.map((userId) => ({ userId, status: "waiting", reason: null })),
        });
        return (async () => {
          for (const [index, userId] of userIds.entries()) {
            if (cancel) {
              publish({
                accounts: batch.accounts.map((row) => (row.status === "waiting" ? { ...row, status: "cancelled" } : row)),
              });
              break;
            }
            publish({
              currentUserId: userId,
              accounts: batch.accounts.map((row) => (row.userId === userId ? { ...row, status: "joining" } : row)),
            });
            await wait(1200);
            const outcome = OUTCOMES[index] ?? OUTCOMES[0];
            publish({
              done: index + 1,
              currentUserId: null,
              accounts: batch.accounts.map((row) => (row.userId === userId ? { ...row, ...outcome, userId } : row)),
            });
          }
          return publish({ running: false, currentUserId: null });
        })();
      }
      default:
        return fallback(cmd, args);
    }
  };
}

/**
 * Moderação como o backend devolve (`check_account_moderation`): o mesmo
 * resumo que `usermoderation/v1/not-approved` vira em `commands/moderation.rs`.
 * Conta 2 banida por 3 dias com nota, 3 advertida, 4 encerrada, 5 com o Roblox
 * limitando (429), o resto limpa.
 */
function harnessModeration(userId: number): { status?: Record<string, unknown>; error?: string } {
  const slot = userId - 1000;
  if (slot === 2) {
    return {
      status: {
        state: "banned",
        until: new Date(Date.now() + 3 * 86400000).toISOString(),
        note: "Exploiting",
        punishment: "Ban 3 Days",
      },
    };
  }
  if (slot === 3) {
    return { status: { state: "warned", until: null, note: "Be respectful in chat", punishment: "Warn" } };
  }
  if (slot === 4) {
    return { status: { state: "terminated", until: null, note: null, punishment: "Delete" } };
  }
  if (slot === 5) return { error: "Roblox is limiting requests right now. Try again in a minute." };
  return { status: { state: "clean", until: null, note: null, punishment: null } };
}

/** O texto que `auth.rs` devolve quando o Roblox pede verificação (ideia 10). */
const HARNESS_CHALLENGE_ERROR =
  "Roblox wants to verify this account (2-step verification). Open it in the browser (account panel › Tools › Browser), finish the check there, then try again.";

const SCENARIOS: Record<string, () => void> = {
  default() {
    setInvokeHandler(baseHandler);
  },

  /**
   * Histórico de sessões (ideia 6): a conta 1 tem duas semanas de sessões em
   * três jogos — em jogo agora, quedas com motivo, teleporte, saída, o app
   * fechado no meio e um aviso de moderação; a conta 2 não tem nada. Abra o
   * painel da conta (selecione TestAccount1). O backend mandaria a lista já
   * montada, da mais nova para a mais velha: é o que vai aqui.
   */
  history() {
    const now = Date.now();
    const min = 60_000;
    const day = 86_400_000;
    const s = (
      startAgo: number,
      minutes: number | null,
      placeId: number | null,
      end: string,
      extra: Record<string, unknown> = {}
    ) => ({
      startedAt: now - startAgo,
      endedAt: minutes === null ? null : now - startAgo + minutes * min,
      placeId,
      jobId: placeId ? `job-${Math.round(startAgo / min)}` : null,
      end,
      dropKind: null,
      reason: null,
      code: null,
      message: null,
      ...extra,
    });
    const first = [
      s(25 * min, null, 6516141723, "ongoing"),
      s(95 * min, 62, 6516141723, "dropped", { dropKind: "disconnected", reason: "connectionLost", code: 277 }),
      s(3 * 60 * min, 40, 606849621, "teleported"),
      s(day + 2 * 60 * min, 120, 606849621, "dropped", { dropKind: "kicked", code: 267, message: "You were AFK for too long" }),
      s(day + 5 * 60 * min, 0, null, "moderated"),
      s(2 * day, 180, 15101393044, "left"),
      s(3 * day, 95, 6516141723, "appClosed"),
      s(5 * day, 30, 6516141723, "dropped", { dropKind: "serverShutdown", code: 274 }),
      s(9 * day, 240, 606849621, "closed"),
      s(20 * day, 300, 15101393044, "left"),
    ];
    setInvokeHandler((cmd, args) => {
      if (cmd === "get_session_history") return Number(args?.userId) === accounts[0].UserID ? first : [];
      if (cmd === "save_history_export") return "C:/Users/you/AppData/Local/Roblox Account Manager/exports/history.csv";
      return baseHandler(cmd, args);
    });
  },

  /**
   * Presets de launch (ideia 13): três presets como o backend devolve — um com
   * horário de abrir e fechar e dois clientes ainda abertos, um com VIP dos
   * favoritos e um com uma conta que saiu do app. Salvar e apagar ficam em
   * memória, como o arquivo `RAMLaunchPresets.json` ficaria. Abra pela Toolbar
   * (Presets) ou pela Choose Game (Save as preset).
   */
  presets() {
    const hour = 3_600_000;
    let presets: Record<string, unknown>[] = [
      {
        id: "preset-1",
        name: "Morning farm",
        userIds: accounts.slice(0, 4).map((a) => a.UserID),
        placeId: 6516141723,
        jobId: "",
        gameName: "Blox Fruits",
        vipName: null,
        arrangeGrid: true,
        schedule: { openEnabled: true, openAt: "08:00", days: [0, 1, 2, 3, 4], closeEnabled: true, closeAt: "18:00" },
        nextOpenAt: Date.now() + 14 * hour,
        nextCloseAt: Date.now() + 4 * hour,
        openClients: 2,
        openUserIds: accounts.slice(0, 2).map((a) => a.UserID),
      },
      {
        id: "preset-2",
        name: "Squad VIP",
        userIds: accounts.slice(0, 2).map((a) => a.UserID),
        placeId: 606849621,
        jobId: "vip:ABC123",
        gameName: "Jailbreak",
        vipName: "Squad",
        arrangeGrid: false,
        schedule: null,
        nextOpenAt: null,
        nextCloseAt: null,
        openClients: 0,
      },
      {
        id: "preset-3",
        name: "Old alts",
        userIds: [accounts[accounts.length - 1].UserID, 999_999],
        placeId: 15101393044,
        jobId: "",
        gameName: "Steal a Brainrot",
        vipName: null,
        arrangeGrid: false,
        schedule: null,
        nextOpenAt: null,
        nextCloseAt: null,
        openClients: 0,
      },
    ];
    let serial = 10;
    setInvokeHandler((cmd, args) => {
      if (cmd === "get_launch_presets") return presets;
      if (cmd === "save_launch_preset") {
        const preset = { ...(args?.preset as Record<string, unknown>) };
        if (!preset.id) preset.id = `preset-${++serial}`;
        const view = { nextOpenAt: null, nextCloseAt: null, openClients: 0, openUserIds: [], ...preset };
        presets = presets.some((p) => p.id === preset.id)
          ? presets.map((p) => (p.id === preset.id ? { ...p, ...view } : p))
          : [...presets, view];
        return preset;
      }
      if (cmd === "delete_launch_preset") {
        presets = presets.filter((p) => p.id !== args?.id);
        return true;
      }
      if (cmd === "launch_preset") return 2;
      if (cmd === "close_preset_clients") {
        presets = presets.map((p) => (p.id === args?.id ? { ...p, openClients: 0, openUserIds: [] } : p));
        return 2;
      }
      return baseHandler(cmd, args);
    });
    // Favoritos com um VIP, para o editor oferecer "Jogo — VIP: Squad".
    harnessGameLists = {
      favorites: [
        {
          placeId: 606849621,
          name: "Jailbreak",
          iconUrl: null,
          addedAt: 1,
          vipServers: [{ id: "v1", name: "Squad", link: "vip:ABC123" }],
        },
        { placeId: 6516141723, name: "Blox Fruits", iconUrl: null, addedAt: 2, vipServers: [] },
      ],
      recentGames: [],
      recentJobs: [],
    };
  },

  /**
   * Contas banidas/advertidas/encerradas (ideia 8) e uma que pede verificação
   * ao abrir (ideia 10, conta 6). Ver `harnessModeration`.
   */
  moderation() {
    const deadAfterCheck = new Set<number>();
    setInvokeHandler((cmd, args) => {
      if (cmd === "check_account_moderation") {
        const userId = Number(args?.userId);
        const result = harnessModeration(userId);
        if (result.error) throw result.error;
        harnessEmit("account-moderation", { userId, status: result.status });
        return result.status;
      }
      if (cmd === "check_accounts") {
        // O que o backend devolveria (`commands/account_check.rs`), aos poucos:
        // progresso a cada conta, moderação de quem respondeu e o resumo. A
        // conta 6 tem o cookie morto (401), a 5 pegou o limite do Roblox.
        const ids = ((args?.userIds as number[] | undefined) ?? []).map(Number);
        return new Promise((resolve) => {
          const summary = { total: 0, ok: 0, warned: 0, invalid: 0, banned: 0, unknown: 0, results: [] as unknown[] };
          harnessEmit("account-check-progress", { done: 0, total: ids.length });
          ids.forEach((userId, index) => {
            setTimeout(() => {
              const result = harnessModeration(userId);
              let outcome: "ok" | "warned" | "invalid" | "banned" | "unknown";
              if (userId === 1006) {
                outcome = "invalid";
                deadAfterCheck.add(userId);
              }
              else if (result.error) outcome = "unknown";
              else {
                harnessEmit("account-moderation", { userId, status: result.status });
                const state = result.status?.state;
                outcome = state === "banned" || state === "terminated" ? "banned" : state === "warned" ? "warned" : "ok";
              }
              summary.total += 1;
              summary[outcome] += 1;
              summary.results.push({ userId, outcome });
              harnessEmit("account-check-progress", { done: summary.total, total: ids.length });
              if (summary.total === ids.length) resolve(summary);
            }, 500 * (index + 1));
          });
          if (ids.length === 0) resolve(summary);
        });
      }
      if (cmd === "get_accounts") {
        // O backend grava `Valid = false` em quem respondeu 401 no check.
        return accounts.map((a) => (deadAfterCheck.has(a.UserID) ? { ...a, Valid: false } : a));
      }
      if (cmd === "launch_roblox") {
        const userId = Number(args?.userId);
        if (userId === 1006) throw HARNESS_CHALLENGE_ERROR;
        const result = harnessModeration(userId);
        const state = result.status?.state;
        if (state === "banned" || state === "terminated") {
          harnessEmit("account-moderation", { userId, status: result.status });
          throw state === "terminated"
            ? "Skipped: Roblox terminated this account."
            : "Skipped: this account is banned until " +
                new Date(String(result.status?.until)).toLocaleString() +
                '. Moderator note: "Exploiting"';
        }
        return null;
      }
      return baseHandler(cmd, args);
    });
  },

  /**
   * O app inteiro com conteúdo: favoritos, recentes, busca de jogos, scripts,
   * versões, backups, contas em jogo. É o cenário da revisão de usabilidade —
   * tela vazia esconde onde o botão está e se o rótulo explica o que ele faz.
   */
  tour() {
    seedTourStorage();
    const pages = realPlacePages as { id: string; playing: number; maxPlayers: number; ping: number | null }[][];
    const withTour = tourHandler(groupsHandler(baseHandler), accounts.map((a) => a.UserID));
    setInvokeHandler((cmd, args) => {
      if (cmd === "start_server_scan") {
        emitRealPlaceScan(pages, Number(args.placeId) || 0);
        return 1;
      }
      if (cmd === "stop_server_scan") return null;
      return withTour(cmd, args);
    });
  },

  /**
   * Aba Servers de um jogo grande. Valida a pergunta que sempre volta: o topo
   * da lista é mesmo o melhor encaixe para o lote?
   */
  "servers-big-game"() {
    const pages = bigGamePages();
    setInvokeHandler((cmd, args) => {
      if (cmd === "start_server_scan") {
        const scanId = 1;
        const all: ReturnType<typeof bigGamePages>[number] = [];
        pages.forEach((page, index) => {
          setTimeout(() => {
            all.push(...page);
            const fitting = all.filter(
              (s) => s.playing + accountCount <= s.maxPlayers
            ).length;
            harnessEmit("server-scan", {
              scanId,
              placeId: Number(args.placeId) || 0,
              // Como o backend faz: quem cabe o lote vai na frente **antes**
              // do corte dos 150, senão o recorte esconde justamente os bons.
              servers: [
                ...all.filter((s) => s.playing + accountCount <= s.maxPlayers),
                ...all.filter((s) => s.playing + accountCount > s.maxPlayers),
              ].slice(0, 150),
              scanned: all.length,
              fitting,
              done: index === pages.length - 1,
              stoppedAtLimit: false,
              error: null,
            });
          }, 250 * (index + 1));
        });
        return scanId;
      }
      if (cmd === "stop_server_scan") return null;
      if (cmd === "get_server_regions") {
        const jobIds = (args.jobIds as string[]) || [];
        return jobIds.map((jobId, i) => ({
          jobId,
          region: {
            ip: `1.2.3.${i}`,
            city: i % 2 === 0 ? "São Paulo" : "Ashburn",
            region: "",
            country: i % 2 === 0 ? "Brazil" : "United States",
            countryCode: i % 2 === 0 ? "BR" : "US",
          },
          label: i % 2 === 0 ? "São Paulo, BR" : "Ashburn, US",
          error: null,
        }));
      }
      return baseHandler(cmd, args);
    });
  },

  /**
   * Réplica com **dados reais** do place 15101393044 (o do relato): 4 páginas
   * capturadas da API do Roblox, entregues página a página como a varredura
   * faz. É o cenário para conferir a ordem da lista contra o mundo real.
   */
  "servers-real-place"() {
    const pages = realPlacePages as { id: string; playing: number; maxPlayers: number; ping: number | null }[][];
    setInvokeHandler((cmd, args) => {
      if (cmd === "start_server_scan") {
        emitRealPlaceScan(pages, Number(args.placeId) || 0);
        return 1;
      }
      if (cmd === "stop_server_scan") return null;
      return baseHandler(cmd, args);
    });
  },

  /**
   * O caso ruim: o backend diz que existem servidores que cabem, mas a lista
   * enviada foi cortada antes deles. A tela não pode fingir que os primeiros
   * servem — tem que dizer que os bons ainda não chegaram.
   */
  "servers-truncated"() {
    setInvokeHandler((cmd, args) => {
      if (cmd === "start_server_scan") {
        setTimeout(() => {
          harnessEmit("server-scan", {
            scanId: 1,
            placeId: Number(args.placeId) || 0,
            servers: Array.from({ length: 20 }, (_, i) => ({
              id: `job-cheio-${i}`,
              playing: 10,
              maxPlayers: 13,
              ping: 40,
            })),
            scanned: 1700,
            fitting: 68,
            done: true,
            stoppedAtLimit: false,
            error: null,
          });
        }, 200);
        return 1;
      }
      if (cmd === "stop_server_scan") return null;
      return baseHandler(cmd, args);
    });
  },

  /** Nenhum servidor cabe o lote: a lista tem que abrir pelos que levam mais contas. */
  "servers-no-fit"() {
    setInvokeHandler((cmd, args) => {
      if (cmd === "start_server_scan") {
        setTimeout(() => {
          harnessEmit("server-scan", {
            scanId: 1,
            placeId: Number(args.placeId) || 0,
            servers: [
              { id: "uma-vaga", playing: 12, maxPlayers: 13, ping: 40 },
              { id: "tres-vagas", playing: 10, maxPlayers: 13, ping: 40 },
              { id: "duas-vagas", playing: 11, maxPlayers: 13, ping: 40 },
              { id: "quatro-vagas", playing: 9, maxPlayers: 13, ping: 40 },
            ],
            scanned: 4,
            fitting: 0,
            done: true,
            stoppedAtLimit: false,
            error: null,
          });
        }, 200);
        return 1;
      }
      if (cmd === "stop_server_scan") return null;
      return baseHandler(cmd, args);
    });
  },

  /** Varredura interrompida pelo limite de páginas. */
  "servers-page-limit"() {
    setInvokeHandler((cmd, args) => {
      if (cmd === "start_server_scan") {
        setTimeout(() => {
          harnessEmit("server-scan", {
            scanId: 1,
            placeId: Number(args.placeId) || 0,
            servers: [{ id: "job-1", playing: 11, maxPlayers: 13, ping: 30 }],
            scanned: 3000,
            fitting: 0,
            done: true,
            stoppedAtLimit: true,
            error: null,
          });
        }, 200);
        return 1;
      }
      if (cmd === "stop_server_scan") return null;
      return baseHandler(cmd, args);
    });
  },

  /**
   * Amigos online de cada conta, com uma conta falhando.
   *
   * Entregue como o backend entrega: uma conta por vez, com a pausa do rate
   * limit (`delayMs`) entre elas, um `friends-online-progress` com a entrada de
   * cada conta e o lote inteiro só no fim. Quem tem que mostrar cada conta
   * assim que ela chega é a UI.
   */
  "friends-online"() {
    setInvokeHandler((cmd, args) => {
      if (cmd === "get_online_friends_for_accounts") {
        const ids = [...new Set(((args.userIds as number[]) || []).filter((id) => id > 0))];
        const requestId = (args.requestId as number | undefined) ?? null;
        const pause = Number(args.delayMs ?? 0) || 0;
        const rows = friendsFor(ids);
        const total = rows.length;
        harnessEmit("friends-online-progress", { done: 0, total, requestId, entry: null });
        return new Promise((resolve) => {
          rows.forEach((entry, index) => {
            // ~400 ms da consulta de cada conta + a pausa entre elas.
            setTimeout(() => {
              harnessEmit("friends-online-progress", { done: index + 1, total, requestId, entry });
              if (index === total - 1) resolve(rows);
            }, (index + 1) * 400 + index * pause);
          });
          if (total === 0) resolve([]);
        });
      }
      return baseHandler(cmd, args);
    });

    function friendsFor(ids: number[]) {
      return ids.map((userId, index) => ({
        userId,
        error: index === 1 ? "Failed to get online friends (status 429)" : null,
        friends:
          index === 1
            ? []
            : [
                {
                  userId: 7000 + index,
                  name: `friend${index}`,
                  displayName: `Friend ${index}`,
                  presenceType: 2,
                  lastLocation: "Some Game",
                  placeId: 606849621,
                  rootPlaceId: 606849621,
                  gameId: `job-friend-${index}`,
                },
                {
                  userId: 8000 + index,
                  name: `website${index}`,
                  displayName: `On Site ${index}`,
                  presenceType: 1,
                  lastLocation: "Website",
                  placeId: null,
                  rootPlaceId: null,
                  gameId: null,
                },
              ],
      }));
    }
  },

  /**
   * Contas espalhadas em grupos nomeados, para ver a lista com cabeçalhos e
   * arrastar a ordem. Os nomes são escolhidos de propósito: um com prefixo
   * numérico (ordena e some do rótulo), um com vírgula (o motivo de a ordem ser
   * guardada em JSON) e um sem nada.
   */
  /**
   * Faixa de aviso do `AccountData.key` **na tela de senha**.
   *
   * A faixa é a única rede contra o lockout de quem não tem senha, e ela passou a
   * ser desenhada nestas telas de `return` antecipado — que têm altura de viewport
   * inteiro, num `body` com `overflow: hidden`. jsdom prova que o texto existe;
   * só o navegador mostra se o rodapé continua na janela e se a pílula de
   * minimizar/fechar está por cima do texto. O cenário só entrega os dados que o
   * backend entregaria.
   */
  "vault-key-warning-locked"() {
    setInvokeHandler((cmd, args) => {
      switch (cmd) {
        case "needs_password":
          return true;
        case "try_remembered_unlock":
          return false;
        case "vault_key_warning":
          return {
            code: "writeFailed",
            path: String.raw`C:\Users\luanm\AppData\Local\Roblox Account Manager\AccountData.key`,
            detail: "O processo nao pode acessar o arquivo porque ele esta sendo usado por outro processo. (os error 32)",
          };
        default:
          return baseHandler(cmd, args);
      }
    });
  },

  /**
   * A mesma faixa na **tela de criptografia** (primeira execução): é para ela que
   * o backend manda o usuário olhar quando a chave não pôde ser criada, e o rodapé
   * dela são os botões Continue/Cancel — justamente o que sai da janela se a
   * geometria estiver errada.
   */
  "vault-key-warning-setup"() {
    accounts.length = 0;
    setInvokeHandler((cmd, args) => {
      switch (cmd) {
        case "get_accounts":
          return [];
        case "get_all_settings":
          return { ...settings, General: { ...settings.General, EncryptionOnboardingState: "pending" } };
        case "vault_key_warning":
          return {
            code: "migrationFailed",
            path: String.raw`C:\Users\luanm\AppData\Local\Roblox Account Manager\AccountData.json`,
            detail: "Acesso negado. (os error 5)",
          };
        default:
          return baseHandler(cmd, args);
      }
    });
  },

  /**
   * O aviso nascendo **com a tela aberta**, sem ninguém clicar em nada: é o
   * ciclo do Auto Rejoin da madrugada gravando num `.key` que ficou ruim. Depois
   * de 3 s o backend publica `vault-key-warning-changed` e a leitura
   * (`vault_key_warning`) passa a devolver o mesmo aviso — a faixa tem que
   * aparecer sozinha. `&clearAfter=<s>` publica a resolução (`null`) depois.
   */
  "vault-key-warning-background"() {
    let current: unknown = null;
    setInvokeHandler((cmd, args) =>
      cmd === "vault_key_warning" ? current : baseHandler(cmd, args)
    );
    window.setTimeout(() => {
      current = {
        code: "writeFailed",
        path: String.raw`C:\Users\luanm\AppData\Local\Roblox Account Manager\AccountData.key`,
        detail: "Acesso negado. (os error 5)",
      };
      harnessEmit("vault-key-warning-changed", current);
    }, 3000);
    const clearAfter = Number(params.get("clearAfter") ?? 0);
    if (Number.isFinite(clearAfter) && clearAfter > 0) {
      window.setTimeout(() => {
        current = null;
        harnessEmit("vault-key-warning-changed", null);
      }, 3000 + clearAfter * 1000);
    }
  },

  groups() {
    const nomes = ["5 Mains", "20 Bots", "Alts, velhas", "Zeta"];
    accounts.forEach((account, index) => {
      account.Group = nomes[index % nomes.length];
    });
    setInvokeHandler(baseHandler);
  },

  /**
   * Console como histórico geral: linhas de launch, de Auto Rejoin e do Watcher
   * chegando aos poucos, como o backend manda. Serve para ver se dá para
   * distinguir a origem de cada linha e se o log ainda é legível cheio.
   */
  "console-history"() {
    setInvokeHandler(baseHandler);
    const linhas: { userId: number | null; level: string; step: string; message: string }[] = [
      { userId: null, level: "info", step: "rejoin", message: "Auto Rejoin iniciado — 4 conta(s), place 606849621, ciclo de 19 min, 20s entre launches" },
      { userId: accounts[0].UserID, level: "info", step: "start", message: "Iniciando launch — place 606849621" },
      { userId: accounts[0].UserID, level: "success", step: "pid", message: "Cliente detectado (pid 8124)" },
      { userId: accounts[1].UserID, level: "success", step: "rejoin", message: "Entrou no jogo pelo ciclo do Auto Rejoin" },
      { userId: accounts[2].UserID, level: "warn", step: "rejoin-retry", message: "Rate limit do Roblox (tentativa 2) — nova tentativa em 45s: 429 Too Many Requests" },
      { userId: accounts[1].UserID, level: "warn", step: "watcher", message: "Cliente fechado pelo Watcher: sem conexao por 30s" },
      { userId: accounts[1].UserID, level: "info", step: "rejoin", message: "Reiniciando a conta (reinicio #1 nesta sessao)" },
      { userId: accounts[3 % accountCount].UserID, level: "error", step: "rejoin-retry", message: "Falha no ciclo (tentativa 1) — nova tentativa em 8s: auth ticket vazio" },
      { userId: accounts[0].UserID, level: "warn", step: "watcher", message: "Cliente do Roblox fechou (o processo morreu)" },
      { userId: null, level: "info", step: "rejoin", message: "Auto Rejoin parado" },
    ];
    linhas.forEach((linha, index) => {
      setTimeout(() => harnessEmit("launch-log", linha), 400 * (index + 1));
    });
  },

  /**
   * Make Friends em andamento: o backend manda o retrato completo a cada
   * mudança, incluindo uma conta que falha. Serve para ver se dá para
   * acompanhar conta por conta e se o erro aparece onde deveria.
   */
  "friend-link"() {
    const ids = accounts.map((a) => a.UserID);
    let estado = {
      active: false,
      phase: "idle",
      processed: 0,
      total: 0,
      accounts: [] as { userId: number; state: string; error: string | null }[],
      mode: "mesh",
      mainUserId: null as number | null,
    };

    function publica(mudanca: Partial<typeof estado>) {
      estado = { ...estado, ...mudanca };
      estado.processed = estado.accounts.filter(
        (a) => a.state === "done" || a.state === "failed"
      ).length;
      harnessEmit("friend-link-state", estado);
    }

    setInvokeHandler((cmd, args) => {
      if (cmd === "get_friend_link_state") return estado;
      if (cmd === "make_selected_friends") {
        publica({
          active: true,
          phase: "checking",
          total: ids.length,
          accounts: ids.map((userId) => ({ userId, state: "pending", error: null })),
        });
        ids.forEach((userId, index) => {
          setTimeout(() => {
            publica({
              phase: index === 0 ? "checking" : "linking",
              accounts: estado.accounts.map((a) =>
                a.userId === userId
                  ? {
                      ...a,
                      state: index === ids.length - 1 ? "failed" : "done",
                      error: index === ids.length - 1 ? "429 Too Many Requests" : null,
                    }
                  : a.userId === ids[index + 1]
                    ? { ...a, state: "processing" }
                    : a
              ),
            });
          }, 700 * (index + 1));
        });
        setTimeout(
          () => publica({ active: false, phase: "done" }),
          700 * (ids.length + 1)
        );
        return {
          pairsTotal: ids.length,
          alreadyFriends: 0,
          attempted: ids.length,
          verifiedOk: ids.length - 1,
          failed: 1,
          requestsSent: ids.length * 2,
          errors: [],
        };
      }
      return baseHandler(cmd, args);
    });
  },

  /** Fila de launch e contas em jogo (Painel de Sessão). */
  "launch-queue"() {
    setInvokeHandler((cmd, args) => {
      if (cmd === "get_launch_queue") {
        return {
          entries: accounts.map((a, i) => ({
            userId: a.UserID,
            state: i === 0 ? "done" : i === 1 ? "launching" : "queued",
            error: null,
            updatedAtMs: Date.now(),
          })),
          active: true,
          placeId: 606849621,
          jobId: "",
        };
      }
      if (cmd === "get_running_instances") {
        return [{ userId: accounts[0].UserID, pid: 4242, placeId: 606849621, jobId: "job-1" }];
      }
      return baseHandler(cmd, args);
    });
  },

  /**
   * Clientes abertos pelo site (Painel de Sessão → Em jogo). A 1ª conta foi
   * lançada pelo app; a 2ª foi aberta pelo site e reconhecida pelo log do
   * Roblox (`adopted`). O PID 9100 ainda não entrou num jogo, então o backend
   * não sabe de quem é: ele aparece como "não identificado", com Mostrar
   * janela e Identificar. Identificar move o PID para a conta escolhida, como o
   * `identify_external_client` faz (se a conta já tinha cliente, o antigo vira
   * não identificado).
   */
  "external-clients"() {
    const running = new Map<number, { pid: number; adopted: boolean }>([
      [accounts[0].UserID, { pid: 8120, adopted: false }],
    ]);
    if (accounts[1]) running.set(accounts[1].UserID, { pid: 8124, adopted: true });
    let unidentified: { pid: number; reason: string; userId: number | null }[] = [
      { pid: 9100, reason: "waitingForGame", userId: null },
    ];
    setInvokeHandler((cmd, args) => {
      switch (cmd) {
        case "get_running_instances":
          return [...running].map(([userId, row]) => ({
            pid: row.pid,
            user_id: userId,
            browser_tracker_id: `${userId}0001`,
            adopted: row.adopted,
          }));
        case "get_unidentified_clients":
          return unidentified.map((c) => ({
            ...c,
            placeId: null,
            jobId: null,
            startedAtMs: Date.now() - 120_000,
          }));
        case "focus_client_window":
          return unidentified.some((c) => c.pid === Number(args?.pid));
        case "identify_external_client": {
          const pid = Number(args?.pid);
          const userId = Number(args?.userId);
          if (!unidentified.some((c) => c.pid === pid)) {
            return Promise.reject("That Roblox client is no longer running.");
          }
          if (!accounts.some((a) => a.UserID === userId)) {
            return Promise.reject("That account is not in your account list.");
          }
          unidentified = unidentified.filter((c) => c.pid !== pid);
          const previous = running.get(userId);
          if (previous) unidentified.push({ pid: previous.pid, reason: "accountBusy", userId });
          running.set(userId, { pid, adopted: true });
          return true;
        }
        default:
          return baseHandler(cmd, args);
      }
    });
  },

  /**
   * Quedas lidas do log do Roblox (Painel de Sessão → Em jogo, e o painel da
   * conta). Todas as contas começam em jogo; as quedas chegam aos poucos, como
   * o monitor do backend (`commands/client_health.rs`) as manda: o evento
   * `roblox-client-health` e o mesmo dado no polling de `get_running_instances`.
   * A 4ª conta foi aberta pelo site (adotada); a 5ª fica "Não respondendo".
   * Depois de um tempo a 1ª volta a um jogo e o aviso some. O cenário só
   * entrega dados.
   *
   * Reconexão automática (`commands/reconnect.rs`, evento `auto-reconnect`),
   * na ordem em que o backend a mandaria: a 1ª espera 10 s, tenta, confere e
   * volta ao jogo; a 2ª está na 2ª tentativa (a 1ª não entrou no jogo); a 3ª
   * espera a internet voltar; a 6ª (fora do "Em jogo": o cliente fechou
   * sozinho) desistiu depois de 5 tentativas. A 4ª, do site, nunca reconecta.
   *
   * Chaves de reconexão do "Em jogo": o padrão (`General.AutoReconnect`) está
   * ligado, a 2ª conta desligou a sua (campo `AutoReconnect`), e a 3ª tem o
   * AutoRelaunch do Nexus (`get_nexus_accounts`). `update_account` guarda em
   * memória o que a chave e o lote gravam.
   *
   * Sessão de agora de cada linha do "Em jogo" (`get_current_sessions`): jogo,
   * servidor público/privado (a 4ª, do site, sem tipo) e tempo em jogo. Quem
   * cai perde a sessão, e a 1ª ganha uma nova ao voltar (com
   * `session-history-changed`, como o backend). Com `&accounts=9`, três contas
   * a mais jogam sem cair — uma num jogo de nome comprido, outra num place sem
   * nome conhecido.
   */
  "client-drops"() {
    // Reconexão: padrão ligado; a 2ª conta desligou a sua (escolha própria).
    settings.General = { ...settings.General, AutoReconnect: "true" };
    if (accounts[1]) accounts[1].Fields = { ...accounts[1].Fields, AutoReconnect: "false" };
    type Drop = {
      kind: string;
      reason: string | null;
      code: number | null;
      message: string | null;
      sinceMs: number;
    };
    const rows = accounts.slice(0, Math.min(accountCount, 5)).map((a, i) => ({
      userId: a.UserID,
      pid: 8200 + i,
      adopted: i === 3,
      drop: null as Drop | null,
    }));
    // Com `&accounts=9` (ou mais), da 7ª à 9ª conta jogam sem cair: é onde se
    // vê a sessão de agora inteira (jogo, servidor, tempo em jogo). A 6ª fica
    // de fora (o cliente fechou e a reconexão desistiu, abaixo).
    accounts.slice(6, 9).forEach((a, i) => {
      rows.push({ userId: a.UserID, pid: 8300 + i, adopted: false, drop: null });
    });
    // Sessão de agora (`get_current_sessions`): onde e desde quando, como o
    // observador do histórico guarda. Nome comprido para ver o corte; um place
    // sem nome conhecido para ver o Place ID no lugar.
    const LONG_NAME_PLACE = 4924922222;
    const LONG_NAME = "[UPDATE 12] Brookhaven Super Mega Tycoon Simulator: Build Your Empire Edition";
    const started = Date.now();
    const sessionOf = new Map<number, { placeId: number; jobId: string; sinceMs: number; privateServer: boolean | null }>();
    const playing = (index: number, placeId: number, minutesAgo: number, privateServer: boolean | null) => {
      const row = rows[index];
      if (row) {
        sessionOf.set(row.userId, {
          placeId,
          jobId: `job-${row.userId}`,
          sinceMs: started - minutesAgo * 60_000,
          privateServer,
        });
      }
    };
    playing(0, 606849621, 47, false);
    playing(1, 6516141723, 130, true);
    playing(2, 606849621, 8, false);
    playing(3, 15101393044, 20, null);
    playing(4, 6516141723, 3, false);
    playing(5, LONG_NAME_PLACE, 65, true);
    playing(6, 606849621, 12, false);
    playing(7, 1234567890, 0.5, false);
    const drops: [number, Omit<Drop, "sinceMs">][] = [
      [0, { kind: "disconnected", reason: "connectionLost", code: 277, message: null }],
      [1, { kind: "kicked", reason: null, code: 267, message: "You have been kicked for being AFK too long" }],
      [2, { kind: "serverShutdown", reason: null, code: 274, message: null }],
      [3, { kind: "disconnected", reason: "joinedElsewhere", code: 273, message: null }],
    ];
    drops.forEach(([index, drop], order) => {
      const row = rows[index];
      if (!row) return;
      setTimeout(() => {
        row.drop = { ...drop, sinceMs: Date.now() };
        harnessEmit("roblox-client-health", { userId: row.userId, drop: row.drop, adopted: row.adopted });
        // A queda fecha a sessão no histórico (evento `dropped`).
        sessionOf.delete(row.userId);
        harnessEmit("session-history-changed", { userIds: [row.userId] });
      }, 1200 * (order + 1));
    });
    setTimeout(() => {
      if (!rows[0]) return;
      rows[0].drop = null;
      harnessEmit("roblox-client-health", { userId: rows[0].userId, drop: null });
      // Voltou: sessão nova, contando de agora.
      sessionOf.set(rows[0].userId, {
        placeId: 606849621,
        jobId: "job-back",
        sinceMs: Date.now(),
        privateServer: false,
      });
      harnessEmit("session-history-changed", { userIds: [rows[0].userId] });
    }, 15_000);
    // A 5ª trava: a janela fica "Não respondendo" (o backend só avisa depois
    // de 30 s; aqui chega em 6 s para não esperar).
    let hungUserId: number | null = null;
    setTimeout(() => {
      if (!rows[4]) return;
      hungUserId = rows[4].userId;
      harnessEmit("roblox-client-health", { userId: hungUserId, notResponding: true });
    }, 6_000);
    type Reconnect = {
      userId: number;
      phase: string;
      attempt: number;
      maxAttempts: number;
      nextAttemptAtMs: number | null;
      reason: string | null;
      error: string | null;
      drop: Drop;
    };
    let reconnect: Reconnect[] = [];
    const sendReconnect = (reconnected: number[] = []) =>
      harnessEmit("auto-reconnect", { entries: reconnect, reconnected });
    const setReconnect = (userId: number, patch: Partial<Reconnect> | null, reconnected: number[] = []) => {
      const others = reconnect.filter((r) => r.userId !== userId);
      const current = reconnect.find((r) => r.userId === userId);
      reconnect =
        patch === null
          ? others
          : [
              ...others,
              {
                userId,
                phase: "waiting",
                attempt: 1,
                maxAttempts: 5,
                nextAttemptAtMs: null,
                reason: null,
                error: null,
                drop: { ...drops[0][1], sinceMs: Date.now() },
                ...current,
                ...patch,
              },
            ].sort((a, b) => a.userId - b.userId);
      sendReconnect(reconnected);
    };
    const first = rows[0];
    if (first) {
      setTimeout(() => setReconnect(first.userId, { nextAttemptAtMs: Date.now() + 10_000 }), 1_400);
      setTimeout(() => setReconnect(first.userId, { phase: "launching", nextAttemptAtMs: null }), 11_400);
      setTimeout(() => setReconnect(first.userId, { phase: "checking" }), 13_000);
      setTimeout(() => setReconnect(first.userId, null, [first.userId]), 17_000);
    }
    const second = rows[1];
    if (second) {
      setTimeout(
        () =>
          setReconnect(second.userId, {
            attempt: 2,
            nextAttemptAtMs: Date.now() + 30_000,
            error: "It did not get into the game in 2 minutes",
            drop: { ...drops[1][1], sinceMs: Date.now() },
          }),
        2_600
      );
    }
    const third = rows[2];
    if (third) {
      setTimeout(
        () =>
          setReconnect(third.userId, {
            phase: "waitingForInternet",
            drop: { ...drops[2][1], sinceMs: Date.now() },
          }),
        3_800
      );
    }
    const sixth = accounts[5];
    if (sixth) {
      setTimeout(
        () =>
          setReconnect(sixth.UserID, {
            phase: "gaveUp",
            attempt: 5,
            error: "The Roblox client did not start",
            drop: { kind: "crashed", reason: null, code: null, message: null, sinceMs: Date.now() },
          }),
        500
      );
    }
    setInvokeHandler((cmd, args) => {
      if (cmd === "get_running_instances") {
        return rows.map((row) => ({
          pid: row.pid,
          user_id: row.userId,
          browser_tracker_id: `${row.userId}0001`,
          adopted: row.adopted,
          health: {
            pid: row.pid,
            logFound: true,
            drop: row.drop,
            windowTitle: `${row.userId} — Roblox`,
            notResponding: row.userId === hungUserId,
            inGame: !row.drop,
          },
        }));
      }
      if (cmd === "get_auto_reconnect_status") return { entries: reconnect };
      if (cmd === "get_current_sessions") {
        return [...sessionOf.entries()]
          .map(([userId, s]) => ({ userId, ...s }))
          .sort((a, b) => a.userId - b.userId);
      }
      if (cmd === "batched_get_game_info" && Number(args?.placeId) === LONG_NAME_PLACE) {
        return { placeId: LONG_NAME_PLACE, universeId: 1, name: LONG_NAME, iconUrl: null };
      }
      // A chave de reconexão por conta (Sessão → Em jogo) grava a conta inteira.
      if (cmd === "update_account") {
        const next = args?.account as (typeof accounts)[number] | undefined;
        const index = next ? accounts.findIndex((a) => a.UserID === next.UserID) : -1;
        if (next && index >= 0) accounts[index] = next;
        return null;
      }
      // A 3ª conta tem o AutoRelaunch do Nexus ligado: a chave dela fica travada.
      if (cmd === "get_nexus_accounts") {
        return rows[2] ? [{ username: accounts[2].Username, auto_relaunch: true }] : [];
      }
      // Os botões só chegam ao backend: quem muda o estado é ele, e aqui não há backend.
      if (cmd === "stop_auto_reconnect" || cmd === "retry_auto_reconnect") return true;
      return baseHandler(cmd, args);
    });
  },

  /**
   * AFK mode desligado, como num INI novo: sem tecla escolhida (`Afk.Key` nasce
   * vazia e nem chega ao INI), intervalo 10, bipe desligado. As quatro primeiras
   * contas têm cliente aberto por este app. No ciclo automático o Windows mantém
   * a janela da 2ª atrás e ela volta com `focusDenied`; o "Enviar a tecla agora",
   * que vem de um clique no app, passa para todas. Para ver um ciclo automático
   * sem esperar 10 min, ligue com 1 min. O Marcar do modo clique acha a janela
   * da 1ª conta, no ponto 37,5% × 62,5%. Multi Roblox ligado: o Auto Rejoin do
   * Modo AFK liga, e a presença diz que as contas com cliente estão no place
   * 606849621 (é o que o "Em jogo" → Modo AFK detecta; com `&games=mixed`, a 2ª conta com cliente está em outro
   * jogo). Os favoritos são os do tour, um deles com servidor VIP salvo.
   */
  "afk-mode"() {
    settings.Afk = {
      IntervalMinutes: "10",
      IntervalSeconds: "0",
      BeepOnCycle: "false",
      Mode: "key",
      ClickX: "50",
      ClickY: "50",
    };
    settings.General = { ...settings.General, EnableMultiRbx: "true", BottingEnabled: "true" };
    // Favoritos (um com servidor VIP salvo) para o "um jogo que eu escolher".
    seedTourStorage();
    const world = afkWorld();
    setInvokeHandler(afkHandler(bottingHandler(baseHandler, world, false), world, null));
  },

  /**
   * O mesmo mundo com uma sessão que já estava rodando quando a tela carregou:
   * há 65 min (ou `&afkSince=<min>`), tecla Space, intervalo 10. A 2ª conta com
   * `focusDenied` e zero envios; uma conta cujo cliente fechou, com `noWindow`.
   */
  "afk-mode-running"() {
    settings.Afk = {
      IntervalMinutes: "10",
      IntervalSeconds: "0",
      Key: "Space",
      BeepOnCycle: "false",
      Mode: "key",
      ClickX: "50",
      ClickY: "50",
    };
    const since = Number(params.get("afkSince") ?? 65);
    const world = afkWorld();
    settings.General = { ...settings.General, EnableMultiRbx: "true", BottingEnabled: "true" };
    // `&rejoin=1`: o Auto Rejoin também já rodava (duas contas com cliente).
    const rejoinRunning = params.get("rejoin") === "1";
    setInvokeHandler(
      afkHandler(
        bottingHandler(baseHandler, world, rejoinRunning),
        world,
        afkRunningSession(world, Number.isFinite(since) ? Math.max(0, Math.min(since, 24 * 60)) : 65)
      )
    );
  },

  /**
   * Avatares grátis: catálogo que chega em 400 ms, miniaturas embutidas, dois
   * avatares já salvos e um lote em que a 2ª conta esbarra na verificação do
   * Roblox. Selecione contas na lista principal antes de abrir Distribuir.
   */
  avatars() {
    setInvokeHandler(avatarsHandler(baseHandler));
  },

  /**
   * Página Groups: grupos populares com o campo vazio, busca "pet" (duas
   * páginas, uma com um grupo trancado), link/id colado, e um lote em que a 2ª
   * conta esbarra no captcha, a 3ª já é membro, a 4ª fica pendente, a 5ª falha
   * e a 6ª esbarra num desafio que não é captcha. Use `&accounts=6`. (O
   * cenário `groups` é outro: os grupos da lista de contas.)
   */
  "roblox-groups"() {
    setInvokeHandler(groupsHandler(baseHandler));
  },

  /**
   * Página "What's new": as releases chegam pelo mesmo `fetch` em
   * `api.github.com` que o app usa — aqui respondido por um dublê, sem rede —,
   * depois de 900 ms (`&delay=<ms>`), com os corpos no formato real: as duas
   * mais novas com a marca do tipo (0.2.1 correção, 0.2.0 geral), a 0.1.10
   * sem marca (lista simples, detalhes técnicos recolhidos — o tipo sai da
   * lista: novidades), as antigas com a lista de títulos de PR e
   * `[skip release]` (sem selo), uma sem "What's Changed" e um rascunho. O app
   * diz que é a 0.1.9 (`&current=<versão>`).
   *
   * - `&update=1`: o updater acha a mais nova, 0.2.1 (a janela abre na
   *   partida; feche e use o "Atualização disponível" da página).
   * - `&fail=offline` ou `&fail=rate`: o primeiro pedido falha (sem internet ou
   *   limite do GitHub); o "Tentar de novo" recebe a lista.
   */
  changelog() {
    const delay = Math.max(0, Number(params.get("delay") ?? 900) || 0);
    const current = params.get("current") || "0.1.9";
    let failuresLeft = params.get("fail") ? 1 : 0;
    const failKind = params.get("fail");
    const realFetch = window.fetch.bind(window);
    window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
      if (!url.startsWith("https://api.github.com/")) return realFetch(input, init);
      await new Promise((resolve) => setTimeout(resolve, delay));
      const isList = /\/releases\?/.test(url);
      if (isList && failuresLeft > 0) {
        failuresLeft -= 1;
        if (failKind === "rate") {
          return new Response(JSON.stringify({ message: "API rate limit exceeded" }), {
            status: 403,
            headers: { "content-type": "application/json", "x-ratelimit-remaining": "0" },
          });
        }
        throw new TypeError("Failed to fetch");
      }
      if (!isList) return new Response(JSON.stringify({ message: "Not Found" }), { status: 404 });
      return new Response(JSON.stringify(HARNESS_RELEASES), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    };
    setInvokeHandler((cmd, args) => {
      if (cmd === "plugin:app|version") return current;
      if (cmd === "check_for_updates_with_channels" && params.get("update") === "1") {
        return {
          version: HARNESS_RELEASES[0].tag_name.replace(/^v|-beta$/g, ""),
          currentVersion: current,
          date: "",
          body: HARNESS_RELEASES[0].body,
          releaseChannel: "beta",
          featureChannel: "standard",
        };
      }
      return baseHandler(cmd, args);
    });
  },

  /**
   * Atualização disponível: o download chega em pedaços (~3 s, 18 MB) pelo
   * `update-download-progress`, como o backend manda. A instalação nunca
   * responde — no app de verdade ele fecha nessa hora —, então a tela
   * "Instalando" fica à vista para inspeção.
   *
   * `&kind=fix|feature|mixed` (padrão `mixed`): o tipo da release, com a marca
   * e o selo na primeira linha do texto, como o workflow escreve; `&kind=none`
   * é uma release antiga, sem tipo nem selo.
   */
  update() {
    const total = 18 * 1024 * 1024;
    const kind = params.get("kind") || "mixed";
    const items =
      kind === "fix"
        ? ["Fixed: the account list no longer jumps when a game opens", "Fixed: VIP servers stay saved after a restore"]
        : kind === "feature"
          ? ["Free avatars for your alts", "Desktop shortcut stays in place on updates"]
          : ["Free avatars for your alts", "Fixed: VIP servers stay saved after a restore"];
    const body =
      kind === "fix" || kind === "feature" || kind === "mixed"
        ? kindBody("0.1.7", kind, items)
        : "## What's new\n- Free avatars for your alts\n- Desktop shortcut stays in place on updates";
    setInvokeHandler((cmd, args) => {
      if (cmd === "check_for_updates_with_channels") {
        return {
          version: "0.1.7",
          currentVersion: "0.1.6",
          date: "",
          body,
          releaseChannel: "beta",
          featureChannel: "standard",
        };
      }
      if (cmd === "plugin:app|version") return "0.1.6";
      if (cmd === "download_selected_update") {
        return new Promise<void>((resolve) => {
          let downloaded = 0;
          const timer = setInterval(() => {
            downloaded = Math.min(total, downloaded + total / 30);
            harnessEmit("update-download-progress", { downloaded: Math.round(downloaded), total });
            if (downloaded >= total) {
              clearInterval(timer);
              resolve();
            }
          }, 100);
        });
      }
      if (cmd === "install_selected_update") return new Promise<void>(() => {});
      return baseHandler(cmd, args);
    });
  },
};

(SCENARIOS[scenarioName] ?? SCENARIOS.default)();

/** Deixa o agente inspecionar o cenário pelo console do navegador. */
(window as unknown as Record<string, unknown>).__harness = {
  scenario: scenarioName,
  accounts: accountCount,
  emit: harnessEmit,
  calls: harnessCalls,
  resetCalls: resetHarnessCalls,
  scenarios: Object.keys(SCENARIOS),
  clipReport,
};
