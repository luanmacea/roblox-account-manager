import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useStore } from "../../../store";
import {
  clampAfkPercent,
  readAfkSettingsPoint,
  writeAfkPoint,
  type AfkMode,
  type AfkPoint,
} from "../../../afkClickPoint";
import { useTr } from "../../../i18n/text";
import { useAccountLabel } from "../../../hooks/useAccountLabel";
import { useRecordings } from "../recordings/useRecordings";
import { recordingForAccount } from "../../../recordings";
import { recordingErrorText } from "../RecordingsTab";

/**
 * Tempo até o próximo envio, no formato `m:ss` — nunca acima do intervalo.
 *
 * O prazo que chega do backend é "agora + intervalo" no relógio dele, e o
 * `nowMs` da tela só anda no tique de 1 s: comparado com um relógio de até 1 s
 * atrás, a contagem nascia em "10:01" (no start e depois de cada envio manual).
 * Faltar mais que um intervalo não existe, então o teto é o intervalo — na
 * hora do render, sem piscar um quadro com o valor errado.
 */
export function formatAfkCountdown(targetMs: number | null, nowMs: number, intervalMs: number): string {
  if (targetMs === null) return "--";
  if (targetMs <= nowMs) return "0:00";
  const remainingMs = intervalMs > 0 ? Math.min(targetMs - nowMs, intervalMs) : targetMs - nowMs;
  const secs = Math.ceil(remainingMs / 1000);
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

/**
 * Tempo desde que a sessão ligou: `<1m`, `12m`, `1h 5m`. É o que responde "isso
 * está funcionando?" sem esperar o próximo ciclo.
 */
export function formatAfkElapsed(startedAtMs: number | null, nowMs: number): string {
  if (startedAtMs === null) return "--";
  const minutes = Math.floor(Math.max(0, nowMs - startedAtMs) / 60_000);
  if (minutes < 1) return "<1m";
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

/** O modo do INI ou da sessão; valor desconhecido é tecla, como no backend. */
export function parseAfkMode(raw: string | undefined | null): AfkMode {
  return raw === "click" ? "click" : raw === "recording" ? "recording" : "key";
}

/** Piso do intervalo, o mesmo do `clamp_afk_interval_seconds` do backend. */
export const AFK_MIN_INTERVAL_SECONDS = 5;

/**
 * O intervalo gravado no INI: `Afk.IntervalMinutes` + `Afk.IntervalSeconds`.
 * Sem os segundos (quem configurou antes deles existirem) vale `0`, e o
 * intervalo continua o de antes. `"0"` minuto é zero, não "ausente": senão
 * 0 min 10 s virava 10 min 10 s.
 */
export function readAfkInterval(afk: Record<string, string> | undefined): {
  minutes: number;
  seconds: number;
} {
  const minutes = parseInt(afk?.IntervalMinutes ?? "", 10);
  const seconds = parseInt(afk?.IntervalSeconds ?? "", 10);
  return {
    minutes: Number.isFinite(minutes) ? Math.min(120, Math.max(0, minutes)) : 10,
    seconds: Number.isFinite(seconds) ? Math.min(59, Math.max(0, seconds)) : 0,
  };
}

export interface ClicksTabOptions {
  /**
   * Contas que chegam já marcadas (aberto pelo "Em jogo" do Painel de Sessão).
   * Só valem com o modo parado: com sessão, quem manda é a sessão.
   */
  targetUserIds?: number[];
}

/**
 * Estado e ações dos cliques AFK (antes o `AfkDialog`): o app manda uma tecla,
 * ou um clique, de tempo em tempo, para a janela de cada conta que está no
 * modo — para o jogo não contar a conta como parada e não precisar de rejoin.
 */
export function useClicksController({ targetUserIds }: ClicksTabOptions = {}) {
  const t = useTr();
  const store = useStore();
  const accountLabel = useAccountLabel();
  const { payload: recordings } = useRecordings();

  const status = store.afkStatus;
  const running = status?.active === true;

  const [intervalMinutes, setIntervalMinutes] = useState(10);
  const [intervalSecondsPart, setIntervalSecondsPart] = useState(0);
  const [key, setKey] = useState("");
  // Quem abriu pelo "Em jogo" já traz as contas marcadas.
  const [draftUserIds, setDraftUserIds] = useState<number[]>(() => targetUserIds ?? []);
  const [beepOnCycle, setBeepOnCycle] = useState(false);
  const [busy, setBusy] = useState(false);
  const [sendingNow, setSendingNow] = useState(false);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [mode, setMode] = useState<AfkMode>("key");
  const [defaultPoint, setDefaultPoint] = useState<AfkPoint>(() => readAfkSettingsPoint(undefined));
  /** Marcar em andamento: de quem é o ponto e quantos segundos faltam. */
  const [capture, setCapture] = useState<{ target: "default" | number; secondsLeft: number } | null>(
    null
  );
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Intervalo e tecla vêm do INI ao abrir; com sessão em andamento, o que vale
  // é o que a sessão está usando.
  useEffect(() => {
    const afk = store.settings?.Afk ?? {};
    const interval = readAfkInterval(afk);
    setIntervalMinutes(interval.minutes);
    setIntervalSecondsPart(interval.seconds);
    setKey(afk.Key || "");
    setBeepOnCycle(afk.BeepOnCycle === "true");
    setMode(parseAfkMode(afk.Mode));
    setDefaultPoint(readAfkSettingsPoint(afk));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Com sessão em andamento, o que vale é o que a sessão está usando — não o
  // rascunho local, que o efeito de abertura relê do INI a qualquer momento.
  const effectiveKey = running ? status?.key ?? "" : key;
  /** Intervalo total em segundos: o da sessão, ou minutos + segundos da tela. */
  const effectiveInterval = running
    ? status?.intervalSeconds ?? 0
    : intervalMinutes * 60 + intervalSecondsPart;
  const intervalTooShort = !running && effectiveInterval < AFK_MIN_INTERVAL_SECONDS;
  const effectiveMode: AfkMode = running ? parseAfkMode(status?.mode) : mode;
  const effectivePoint: AfkPoint = running
    ? { x: clampAfkPercent(status?.clickX ?? 50), y: clampAfkPercent(status?.clickY ?? 50) }
    : defaultPoint;
  const clickMode = effectiveMode === "click";
  const recordingMode = effectiveMode === "recording";

  // O tique não depende de `running`: o tempo decorrido tem de andar sempre que
  // existe sessão, inclusive no intervalo em que a tela ainda não recebeu o
  // status novo — senão o contador congela e parece que o modo morreu.
  useEffect(() => {
    setNowMs(Date.now());
    const timer = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  const sessionUserIds = useMemo(
    () => (running ? (status?.accounts ?? []).map((a) => a.userId) : []),
    [running, status]
  );
  /** Quem está no modo: a sessão manda quando há sessão; senão, o rascunho. */
  const inAfk = running ? sessionUserIds : draftUserIds;

  // Com sessão, o rascunho acompanha quem está nela. Quando a sessão acaba —
  // Parar, ou por qualquer outro caminho —, a tela continua marcando quem
  // estava no modo, inclusive a conta que entrou com a sessão ligada, e religar
  // leva as mesmas contas. Sem isto a seleção voltava à de antes do start, e a
  // conta acrescentada ficava de fora do próximo start sem aviso.
  useEffect(() => {
    if (running) setDraftUserIds(sessionUserIds);
  }, [running, sessionUserIds]);

  /**
   * Só conta com cliente aberto **por este app** pode receber tecla: é o tracker
   * que sabe qual PID é de qual conta. Quem está no modo continua na lista mesmo
   * se o cliente caiu, senão não daria para tirá-la de lá.
   */
  const candidates = useMemo(() => {
    const ids = new Set<number>([...store.launchedByProgram, ...inAfk]);
    return store.accounts.filter((a) => ids.has(a.UserID));
  }, [store.accounts, store.launchedByProgram, inAfk]);

  const focusDenied = (status?.accounts ?? []).some((a) => a.lastErrorCode === "focusDenied");
  const keyAllowed = store.afkKeys.includes(effectiveKey);
  /** A gravação que cada conta toca no modo gravação (a própria ou a de todas). */
  function recordingNameFor(userId: number): string | null {
    const rec = recordingForAccount(recordings, userId);
    return rec && rec.steps.length > 0 ? rec.name : null;
  }
  const anyRecording = inAfk.some((id) => recordingNameFor(id) !== null);
  /**
   * O modo clique não usa tecla; o modo tecla não liga sem uma da lista; o modo
   * gravação precisa de pelo menos uma conta marcada com gravação para tocar.
   */
  const sendReady = clickMode || (recordingMode ? running || anyRecording : keyAllowed);
  const canStart = sendReady && inAfk.length > 0 && !intervalTooShort && !busy;
  const statusByUserId = useMemo(
    () => new Map((status?.accounts ?? []).map((a) => [a.userId, a])),
    [status]
  );
  const totalSends = [...statusByUserId.values()].reduce((sum, a) => sum + a.sends, 0);

  /** A frase que explica por que uma conta não recebeu a tecla. */
  function sendErrorText(code: string | null, raw: string | null, name: string): string {
    switch (code) {
      case "noRecording":
      case "focusLost":
      case "stopped":
        return recordingErrorText(t, code, raw, name);
      case "focusDenied":
        return t(
          "{{name}}: Windows did not let this account's window come to the front, so nothing was sent.",
          { name }
        );
      case "noWindow":
        return t("{{name}}: has no Roblox client open right now.", { name });
      case "keyRefused":
        return t("{{name}}: Windows refused the key.", { name });
      case "clickRefused":
        return t("{{name}}: Windows refused the click.", { name });
      default:
        return `${name}: ${raw ?? ""}`.trim();
    }
  }

  /** A frase de um Marcar que não deu certo, pelo código que o backend devolve. */
  function captureErrorText(code: string): string {
    switch (code) {
      case "noCursor":
        return t("Could not read where the mouse is");
      case "noWindow":
        return t("There is no window under the mouse");
      case "notAnAccountWindow":
        return t("That window is not a Roblox client this app opened");
      case "outsideGameArea":
        return t("Put the mouse inside the game area, not on the border or the title bar");
      default:
        return code;
    }
  }

  /** Nome da conta na tela e nos avisos, com "Names hidden" aplicado. */
  function accountName(userId: number): string {
    const account = store.accounts.find((it) => it.UserID === userId);
    return accountLabel(account, `${t("User ID")}: ${userId}`);
  }

  function persist(settingKey: string, value: string) {
    void invoke("update_setting", { section: "Afk", key: settingKey, value }).catch(() => {});
  }

  /**
   * Marcar: 3 s para o usuário parar o mouse em cima do ponto numa janela de
   * conta, e aí o backend lê a posição **uma vez**. O ponto vira o padrão ou o
   * próprio da conta `target`. Qualquer janela de conta serve: o ponto é
   * relativo, então cai no mesmo lugar nas outras.
   */
  async function startMark(target: "default" | number) {
    if (capture) return;
    for (let seconds = 3; seconds > 0; seconds--) {
      if (!mountedRef.current) return;
      setCapture({ target, secondsLeft: seconds });
      await new Promise((resolve) => window.setTimeout(resolve, 1000));
    }
    if (!mountedRef.current) return;
    setCapture(null);
    try {
      const got = await store.captureAfkPoint();
      const point = { x: clampAfkPercent(got.xPct), y: clampAfkPercent(got.yPct) };
      if (target === "default") {
        setDefaultPoint(point);
        persist("ClickX", String(point.x));
        persist("ClickY", String(point.y));
      } else {
        const account = store.accounts.find((it) => it.UserID === target);
        if (account) {
          await store.updateAccount({ ...account, Fields: writeAfkPoint(account.Fields, point) });
        }
      }
      store.addToast(t("Point marked on {{name}}'s window", { name: accountName(got.userId) }));
    } catch (e) {
      store.addToast(captureErrorText(String(e)), "error");
    }
  }

  /** Tira o ponto próprio da conta: ela volta a usar o padrão no ciclo seguinte. */
  async function clearOwnPoint(userId: number) {
    const account = store.accounts.find((it) => it.UserID === userId);
    if (!account) return;
    try {
      await store.updateAccount({ ...account, Fields: writeAfkPoint(account.Fields, null) });
    } catch {
      // O erro já virou toast no store.
    }
  }

  async function toggleAccount(userId: number) {
    const next = inAfk.includes(userId) ? inAfk.filter((id) => id !== userId) : [...inAfk, userId];
    if (!running) {
      setDraftUserIds(next);
      return;
    }
    setBusy(true);
    try {
      await store.setAfkAccounts(next);
      // Desmarcar a última conta encerra a sessão, e aí o espelho acima não
      // roda mais: o rascunho fica com o que o usuário pediu, e não com a
      // conta que ele acabou de tirar.
      setDraftUserIds(next);
      // E diz que desligou: sem isto a pílula virava "Off" calada, enquanto o
      // Parar avisa.
      if (next.length === 0) store.addToast(t("AFK mode off: no account is left in it"));
    } catch {
      // O erro já virou toast no store.
    } finally {
      setBusy(false);
    }
  }

  async function handleStart() {
    if (!canStart) return;
    setBusy(true);
    try {
      await store.startAfkMode({
        userIds: inAfk,
        intervalSeconds: effectiveInterval,
        key: effectiveKey,
        mode: effectiveMode,
        clickX: effectivePoint.x,
        clickY: effectivePoint.y,
      });
    } catch {
      // O erro já virou toast no store.
    } finally {
      setBusy(false);
    }
  }

  async function handleSendNow() {
    // Sem sessão não há conta no modo, e envio manual não pode alcançar cliente
    // de conta fora dele.
    if (!running || !sendReady || inAfk.length === 0) return;
    setSendingNow(true);
    try {
      const sent = await store.afkTriggerNow(inAfk);
      if (recordingMode) {
        store.addToast(
          sent === 1
            ? t("Played on 1 account")
            : sent > 1
              ? t("Played on {{count}} accounts", { count: sent })
              : t("The recording did not play on any window")
        );
        return;
      }
      if (clickMode) {
        store.addToast(
          sent === 1
            ? t("Clicked on 1 account")
            : sent > 1
              ? t("Clicked on {{count}} accounts", { count: sent })
              : t("No Roblox window received the click")
        );
        return;
      }
      store.addToast(
        sent === 1
          ? t("Sent {{key}} to 1 account", { key: effectiveKey })
          : sent > 1
            ? t("Sent {{key}} to {{count}} accounts", { key: effectiveKey, count: sent })
            : t("No Roblox window received the key")
      );
    } catch {
      // O erro já virou toast no store.
    } finally {
      setSendingNow(false);
    }
  }

  async function handleStop() {
    setBusy(true);
    try {
      await store.stopAfkMode();
    } catch {
      // O erro já virou toast no store.
    } finally {
      setBusy(false);
    }
  }

  /** Por que o Start não liga — a primeira coisa que falta. */
  const startBlocker: string | null = running
    ? null
    : candidates.length === 0
      ? t("No Roblox client opened by this app yet")
      : intervalTooShort
        ? t("At least 5 seconds.")
        : !sendReady
          ? recordingMode
            ? t("None of the ticked accounts has a recording to play")
            : t("Pick a key to send")
          : inAfk.length === 0
            ? t("Tick at least one account")
            : null;

  return {
    t,
    store,
    status,
    running,
    nowMs,
    busy,
    sendingNow,
    capture,
    /** Intervalo total em segundos (o da sessão, com o modo ligado). */
    intervalSeconds: effectiveInterval,
    /** Os dois campos da tela: com sessão, o intervalo dela quebrado. */
    intervalMinutesPart: Math.floor(effectiveInterval / 60),
    intervalSecondsPart: effectiveInterval % 60,
    intervalTooShort,
    effectiveKey,
    effectiveMode,
    effectivePoint,
    clickMode,
    recordingMode,
    recordingNameFor,
    beepOnCycle,
    inAfk,
    candidates,
    focusDenied,
    keyAllowed,
    sendReady,
    canStart,
    startBlocker,
    statusByUserId,
    totalSends,
    configDisabled: running || busy,
    setIntervalMinutes,
    setIntervalSecondsPart,
    setKey: (v: string) => {
      setKey(v);
      persist("Key", v);
    },
    setMode: (v: AfkMode) => {
      setMode(v);
      persist("Mode", v);
    },
    setBeepOnCycle: (v: boolean) => {
      setBeepOnCycle(v);
      persist("BeepOnCycle", v ? "true" : "false");
    },
    persist,
    sendErrorText,
    accountName,
    startMark,
    clearOwnPoint,
    toggleAccount,
    handleStart,
    handleSendNow,
    handleStop,
  };
}

export type ClicksController = ReturnType<typeof useClicksController>;
