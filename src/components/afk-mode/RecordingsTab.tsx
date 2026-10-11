import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowDown, ArrowUp, Check, Copy, Crosshair, FileUp, FlaskConical, Pencil, Play, Plus, Square, Trash2 } from "lucide-react";
import { useStore } from "../../store";
import { useTr } from "../../i18n/text";
import { useAccountLabel } from "../../hooks/useAccountLabel";
import { useConfirm, usePrompt } from "../../hooks/usePrompt";
import { Select } from "../ui/Select";
import { NumericInput } from "../ui/NumericInput";
import { ToggleRow } from "../ui/ToggleRow";
import { Toggle } from "../ui/Toggle";
import { recordingTriggers } from "./recordings/triggers";
import { RecordingTriggerLines } from "./recordings/RecordingTriggerLines";
import { DANGER_ACTION, ModeStatusBar, NEUTRAL_ACTION, PRIMARY_ACTION } from "./ModeStatusBar";
import { useRecordings } from "./recordings/useRecordings";
import { TinyTaskImportPanel, tinyTaskSummaryLines, type ImportedDraft } from "./recordings/TinyTaskImport";
import {
  changeStepType,
  clampInt,
  formatDurationMs,
  MAX_HOLD_MS,
  MAX_RECORDING_NAME_CHARS,
  MAX_RECORDING_STEPS,
  MAX_WAIT_MS,
  MIN_HOLD_MS,
  aspectDiffers,
  moveStep,
  newStep,
  recordingDurationMs,
  recordingProblem,
  type Recording,
  type RecordingPlayResult,
  type RecordingProblem,
  type RecordingStep,
  type RecordingStepType,
  type TinyTaskArea,
  type TinyTaskSummary,
} from "../../recordings";

const CARD = "theme-surface rounded-xl border theme-border p-3";
const SMALL_BTN =
  "px-1.5 py-0.5 rounded-md border theme-border hover:bg-[var(--panel-soft)] disabled:opacity-50 disabled:cursor-not-allowed";

interface Draft {
  id: string;
  name: string;
  steps: RecordingStep[];
  /** Proporção da janela de origem (gravação importada do TinyTask). */
  sourceAspect?: number;
}

/** Tempo no jogo antes de tocar depois da reconexão (o clamp do backend). */
const MIN_AFTER_RECONNECT_S = 5;
const MAX_AFTER_RECONNECT_S = 3_600;

function sameDraft(a: Draft | null, b: Recording | null): boolean {
  if (!a || !b) return false;
  return (
    a.id === b.id &&
    a.name === b.name &&
    (a.sourceAspect ?? null) === (b.sourceAspect ?? null) &&
    JSON.stringify(a.steps) === JSON.stringify(b.steps)
  );
}

/**
 * Aba **Recordings** do Modo AFK: a biblioteca de gravações (criar, renomear,
 * duplicar, apagar), o editor de passos (tecla, apertar/soltar, clique num
 * ponto da janela, espera), quem toca qual gravação (uma para todas, ou uma por
 * conta, que vence) e quando ela toca (no Modo AFK e depois da reconexão).
 *
 * Tocar é o ciclo do Modo AFK: uma janela por vez, trazida para frente, com a
 * gravação inteira tocada antes de passar para a próxima — e o foco só volta no
 * fim. Ver docs/features/recordings.md.
 */
export function RecordingsTab() {
  const t = useTr();
  const store = useStore();
  const accountLabel = useAccountLabel();
  const confirm = useConfirm();
  const prompt = usePrompt();
  const { payload, loadError, playing } = useRecordings();

  const recordings = useMemo(() => payload?.recordings ?? [], [payload]);
  const keys = useMemo(() => payload?.keys ?? [], [payload]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);
  const [playOn, setPlayOn] = useState<number[]>([]);
  const [starting, setStarting] = useState(false);
  const [lastResults, setLastResults] = useState<RecordingPlayResult[]>([]);
  const [capture, setCapture] = useState<{ index: number; secondsLeft: number } | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [imported, setImported] = useState<{ steps: number; summary: TinyTaskSummary } | null>(null);
  const [testing, setTesting] = useState(false);
  /** Área interna da janela de cada conta aberta, para o aviso de formato. */
  const [windowAreas, setWindowAreas] = useState<Map<number, TinyTaskArea>>(new Map());
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const saved = recordings.find((r) => r.id === selectedId) ?? null;
  // Sem nada aberto, abre a primeira gravação da biblioteca.
  useEffect(() => {
    if (draft || recordings.length === 0) return;
    const first = recordings[0];
    setSelectedId(first.id);
    setDraft({ id: first.id, name: first.name, steps: first.steps, sourceAspect: first.sourceAspect });
  }, [recordings, draft]);

  const dirty = !!draft && !sameDraft(draft, saved);
  /** Contas que têm a gravação aberta como própria. */
  const ownUses = saved ? Object.values(payload?.accountIds ?? {}).filter((id) => id === saved.id).length : 0;
  const problem: RecordingProblem | null = draft ? recordingProblem(draft.name, draft.steps, keys) : null;
  const firstKey = keys[0] ?? "Space";

  /** Só conta com cliente aberto **por este app** recebe a gravação. */
  const openAccounts = useMemo(
    () => store.accounts.filter((a) => store.launchedByProgram.has(a.UserID)),
    [store.accounts, store.launchedByProgram]
  );
  /** Na lista de "quem toca o quê": quem tem cliente aberto e quem já tem gravação própria. */
  const assignAccounts = useMemo(() => {
    const own = new Set(Object.keys(payload?.accountIds ?? {}).map(Number));
    return store.accounts.filter((a) => store.launchedByProgram.has(a.UserID) || own.has(a.UserID));
  }, [store.accounts, store.launchedByProgram, payload]);

  // Os gatilhos (Modo AFK e depois da reconexão), lidos do INI do store: a aba
  // AFK clicks grava pelo store também, então os dois lados se veem.
  const triggers = recordingTriggers(payload, store.settings);
  const afkRunning = store.afkStatus?.active === true;
  const iniAfk = store.settings?.Afk;
  const [afkMinutes, setAfkMinutes] = useState(() => Math.floor(triggers.intervalSeconds / 60));
  const [afkSeconds, setAfkSeconds] = useState(() => triggers.intervalSeconds % 60);
  useEffect(() => {
    setAfkMinutes(Math.floor(triggers.intervalSeconds / 60));
    setAfkSeconds(triggers.intervalSeconds % 60);
  }, [triggers.intervalSeconds, iniAfk?.IntervalMinutes, iniAfk?.IntervalSeconds]);

  const settingsRec = store.settings?.Recordings ?? {};
  const afterReconnect = settingsRec.AfterReconnect === "true";
  const parsedDelay = Number.parseInt(settingsRec.AfterReconnectDelaySeconds ?? "", 10);
  const [delayS, setDelayS] = useState(() =>
    clampInt(parsedDelay, MIN_AFTER_RECONNECT_S, MAX_AFTER_RECONNECT_S, 30)
  );
  // As settings chegam depois da primeira pintura: o campo acompanha o INI.
  useEffect(() => {
    if (Number.isFinite(parsedDelay)) {
      setDelayS(clampInt(parsedDelay, MIN_AFTER_RECONNECT_S, MAX_AFTER_RECONNECT_S, 30));
    }
  }, [parsedDelay]);

  function accountName(userId: number): string {
    const account = store.accounts.find((a) => a.UserID === userId);
    return accountLabel(account, `${t("User ID")}: ${userId}`);
  }

  function problemText(p: RecordingProblem): string {
    switch (p) {
      case "noName":
        return t("Give the recording a name.");
      case "nameTooLong":
        return t("The name is too long (max {{count}} characters).", { count: MAX_RECORDING_NAME_CHARS });
      case "tooManySteps":
        return t("A recording holds up to {{count}} steps.", { count: MAX_RECORDING_STEPS });
      case "tooLong":
        return t("A recording can take up to 10 minutes.");
      case "badKey":
        return t("Pick a key from the list for every key step.");
    }
  }

  function resultText(r: RecordingPlayResult): string {
    const name = accountName(r.userId);
    return recordingErrorText(t, r.errorCode, r.error, name);
  }

  async function guardDirty(): Promise<boolean> {
    if (!dirty) return true;
    return confirm(t("Discard the changes to this recording?"), true);
  }

  async function open(recording: Recording) {
    if (recording.id === draft?.id) return;
    if (!(await guardDirty())) return;
    setSelectedId(recording.id);
    setImported(null);
    setDraft({ id: recording.id, name: recording.name, steps: recording.steps, sourceAspect: recording.sourceAspect });
  }

  async function createNew() {
    if (!(await guardDirty())) return;
    setSelectedId(null);
    setImported(null);
    setDraft({ id: "", name: t("New recording"), steps: [] });
  }

  /** O rascunho do TinyTask vira uma gravação nova, ainda não salva. */
  async function takeImport(out: ImportedDraft) {
    if (!(await guardDirty())) return;
    setSelectedId(null);
    setImportOpen(false);
    setImported({ steps: out.steps.length, summary: out.summary });
    setDraft({
      id: "",
      name: out.name.slice(0, MAX_RECORDING_NAME_CHARS),
      steps: out.steps.slice(0, MAX_RECORDING_STEPS),
      sourceAspect: out.sourceAspect > 0 ? out.sourceAspect : undefined,
    });
  }

  /** "Test on one account": toca o rascunho sem salvar, numa conta só. */
  async function testDraft() {
    if (!draft || problem || playOn.length !== 1 || draft.steps.length === 0) return;
    setTesting(true);
    setLastResults([]);
    try {
      const results = await invoke<RecordingPlayResult[]>("play_recording_draft", {
        userId: playOn[0],
        recording: {
          id: draft.id,
          name: draft.name.trim(),
          steps: draft.steps,
          createdAt: 0,
          updatedAt: 0,
          sourceAspect: draft.sourceAspect,
        },
      });
      const list = Array.isArray(results) ? results : [];
      setLastResults(list.filter((r) => r.errorCode));
      store.addToast(
        list.some((r) => !r.errorCode) ? t("Played on 1 account") : t("The recording did not play on any window")
      );
    } catch (e) {
      store.addToast(String(e), "error");
    } finally {
      setTesting(false);
    }
  }

  async function save() {
    if (!draft || problem) return;
    setSaving(true);
    try {
      const out = await invoke<Recording>("save_recording", {
        recording: {
          id: draft.id,
          name: draft.name.trim(),
          steps: draft.steps,
          createdAt: 0,
          updatedAt: 0,
          sourceAspect: draft.sourceAspect,
        },
      });
      setSelectedId(out.id);
      setImported(null);
      setDraft({ id: out.id, name: out.name, steps: out.steps, sourceAspect: out.sourceAspect });
      store.addToast(t("Recording saved"));
    } catch (e) {
      store.addToast(String(e), "error");
    } finally {
      setSaving(false);
    }
  }

  function discard() {
    setImported(null);
    if (saved) setDraft({ id: saved.id, name: saved.name, steps: saved.steps, sourceAspect: saved.sourceAspect });
    else setDraft(null);
  }

  async function rename(recording: Recording) {
    const next = await prompt(t("New name for the recording"), recording.name, {
      maxLength: MAX_RECORDING_NAME_CHARS,
    });
    if (next === null || !next.trim() || next.trim() === recording.name) return;
    try {
      const out = await invoke<Recording>("save_recording", { recording: { ...recording, name: next.trim() } });
      if (draft?.id === out.id) setDraft((d) => (d ? { ...d, name: out.name } : d));
    } catch (e) {
      store.addToast(String(e), "error");
    }
  }

  async function duplicate(recording: Recording) {
    try {
      await invoke<Recording>("duplicate_recording", {
        id: recording.id,
        name: t("{{name}} (copy)", { name: recording.name }).slice(0, MAX_RECORDING_NAME_CHARS),
      });
    } catch (e) {
      store.addToast(String(e), "error");
    }
  }

  async function remove(recording: Recording) {
    if (!(await confirm(t("Delete the recording {{name}}?", { name: recording.name }), true))) return;
    try {
      await invoke("delete_recording", { id: recording.id });
      if (draft?.id === recording.id) {
        setDraft(null);
        setSelectedId(null);
      }
    } catch (e) {
      store.addToast(String(e), "error");
    }
  }

  async function setDefault(id: string) {
    try {
      await invoke("set_default_recording", { id: id || null });
    } catch (e) {
      store.addToast(String(e), "error");
    }
  }

  async function setForAccount(userId: number, id: string) {
    try {
      await invoke("set_account_recording", { userId, id: id || null });
    } catch (e) {
      store.addToast(String(e), "error");
    }
  }

  function updateStep(index: number, next: RecordingStep) {
    setDraft((d) => (d ? { ...d, steps: d.steps.map((s, i) => (i === index ? next : s)) } : d));
  }

  function addStep(type: RecordingStepType) {
    setDraft((d) => (d && d.steps.length < MAX_RECORDING_STEPS ? { ...d, steps: [...d.steps, newStep(type, firstKey)] } : d));
  }

  /** Marcar: os mesmos 3 s do Modo AFK; o backend lê a posição do cursor uma vez. */
  async function markPoint(index: number) {
    if (capture) return;
    for (let seconds = 3; seconds > 0; seconds--) {
      if (!mountedRef.current) return;
      setCapture({ index, secondsLeft: seconds });
      await new Promise((resolve) => window.setTimeout(resolve, 1000));
    }
    if (!mountedRef.current) return;
    setCapture(null);
    try {
      const got = await store.captureAfkPoint();
      setDraft((d) =>
        d
          ? {
              ...d,
              steps: d.steps.map((s, i) => (i === index && s.type === "click" ? { ...s, xPct: got.xPct, yPct: got.yPct } : s)),
            }
          : d
      );
      store.addToast(t("Point marked on {{name}}'s window", { name: accountName(got.userId) }));
    } catch (e) {
      store.addToast(markErrorText(t, String(e)), "error");
    }
  }

  async function playNow() {
    if (!saved || dirty || playOn.length === 0) return;
    setStarting(true);
    setLastResults([]);
    try {
      const results = await invoke<RecordingPlayResult[]>("play_recording_now", {
        userIds: playOn,
        recordingId: saved.id,
      });
      const list = Array.isArray(results) ? results : [];
      setLastResults(list.filter((r) => r.errorCode));
      const ok = list.filter((r) => !r.errorCode).length;
      store.addToast(
        ok === 1
          ? t("Played on 1 account")
          : ok > 1
            ? t("Played on {{count}} accounts", { count: ok })
            : t("The recording did not play on any window")
      );
    } catch (e) {
      store.addToast(String(e), "error");
    } finally {
      setStarting(false);
    }
  }

  async function stop() {
    try {
      await invoke("stop_recording_playback");
    } catch {
      // Parar não falha de um jeito que a tela possa consertar.
    }
  }

  function persist(key: string, value: string) {
    void store.updateSetting("Recordings", key, value).catch(() => {});
  }

  function persistAfk(key: string, value: string) {
    void store.updateSetting("Afk", key, value).catch(() => {});
  }

  const duration = draft ? recordingDurationMs(draft.steps) : 0;
  const busy = playing || starting || testing;

  // Gravação com a proporção da janela de origem: lê a janela de cada conta
  // aberta (só o retângulo) para avisar quem tem outro formato.
  const sourceAspect = draft?.sourceAspect;
  const openIdsKey = openAccounts.map((a) => a.UserID).join(",");
  useEffect(() => {
    if (!sourceAspect || !openIdsKey) {
      setWindowAreas(new Map());
      return;
    }
    let alive = true;
    const ids = openIdsKey.split(",").map(Number);
    void Promise.all(
      ids.map((userId) =>
        invoke<TinyTaskArea>("recording_window_area", { userId })
          .then((area) => [userId, area] as const)
          .catch(() => null)
      )
    ).then((pairs) => {
      if (!alive) return;
      const next = new Map<number, TinyTaskArea>();
      for (const pair of pairs) if (pair && pair[1]) next.set(pair[0], pair[1]);
      setWindowAreas(next);
    });
    return () => {
      alive = false;
    };
  }, [sourceAspect, openIdsKey]);
  const shapeWarnings = openAccounts.filter((a) => {
    const area = windowAreas.get(a.UserID);
    return !!area && aspectDiffers(sourceAspect, area.width, area.height);
  });

  return (
    <div className="@container/recs flex h-full min-h-0 flex-col gap-3">
      <ModeStatusBar
        testId="recordings-status"
        running={busy}
        title={busy ? t("Playing a recording") : t("Recordings")}
        facts={[
          t("Saved: {{count}}", { count: recordings.length }),
          busy ? t("One window at a time; the focus comes back at the end.") : null,
        ]}
        actions={
          busy ? (
            <button onClick={() => void stop()} className={DANGER_ACTION}>
              <span className="flex items-center gap-1.5">
                <Square size={12} strokeWidth={2} aria-hidden />
                {t("Stop playing")}
              </span>
            </button>
          ) : (
            <button onClick={() => void createNew()} className={PRIMARY_ACTION}>
              <span className="flex items-center gap-1.5">
                <Plus size={13} strokeWidth={2} aria-hidden />
                {t("New recording")}
              </span>
            </button>
          )
        }
        message={loadError ? { text: loadError, tone: "error" } : null}
      />

      <div className="flex-1 min-h-0 overflow-y-auto pr-0.5">
        <div className="grid grid-cols-1 gap-3 @3xl/recs:grid-cols-[minmax(280px,360px)_minmax(0,1fr)] items-start">
          <div className="space-y-3">
            <section className={CARD} aria-label={t("Library")}>
              <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                <span className="text-[13px] font-semibold text-[var(--panel-fg)]">{t("Library")}</span>
                <button
                  onClick={() => setImportOpen((v) => !v)}
                  aria-expanded={importOpen}
                  className="sidebar-btn-sm flex items-center gap-1"
                >
                  <FileUp size={12} strokeWidth={1.75} aria-hidden />
                  {t("Import from TinyTask (.rec)")}
                </button>
              </div>
              {importOpen ? (
                <TinyTaskImportPanel
                  windows={openAccounts.map((a) => ({ userId: a.UserID, name: accountName(a.UserID) }))}
                  onImported={(out) => void takeImport(out)}
                  onCancel={() => setImportOpen(false)}
                />
              ) : null}
              {recordings.length === 0 ? (
                <div className="text-[11px] theme-muted leading-4">
                  {t("No recordings yet. Create one and add its steps: keys, clicks on a point of the window and waits.")}
                </div>
              ) : (
                <ul className="space-y-1">
                  {recordings.map((r) => {
                    const current = r.id === draft?.id;
                    return (
                      <li
                        key={r.id}
                        className={`flex items-center gap-1.5 rounded-lg border px-2 py-1 ${
                          current ? "theme-accent-border bg-[var(--accent-soft)]" : "theme-border"
                        }`}
                      >
                        <button
                          onClick={() => void open(r)}
                          aria-pressed={current}
                          className="flex-1 min-w-0 text-left"
                        >
                          <span className="block truncate text-[12px] text-[var(--panel-fg)]" title={r.name}>
                            {r.name}
                          </span>
                          <span className="block text-[11px] theme-muted">
                            {t("{{count}} steps · {{duration}}", {
                              count: r.steps.length,
                              duration: formatDurationMs(recordingDurationMs(r.steps)),
                            })}
                          </span>
                        </button>
                        <button
                          onClick={() => void rename(r)}
                          aria-label={t("Rename {{name}}", { name: r.name })}
                          title={t("Rename")}
                          className={SMALL_BTN}
                        >
                          <Pencil size={12} strokeWidth={1.75} aria-hidden />
                        </button>
                        <button
                          onClick={() => void duplicate(r)}
                          aria-label={t("Duplicate {{name}}", { name: r.name })}
                          title={t("Duplicate")}
                          className={SMALL_BTN}
                        >
                          <Copy size={12} strokeWidth={1.75} aria-hidden />
                        </button>
                        <button
                          onClick={() => void remove(r)}
                          aria-label={t("Delete {{name}}", { name: r.name })}
                          title={t("Delete")}
                          className={SMALL_BTN}
                        >
                          <Trash2 size={12} strokeWidth={1.75} aria-hidden />
                        </button>
                      </li>
                    );
                  })}
                </ul>
              )}
            </section>

            <section className={`${CARD} space-y-2`} aria-label={t("Which recording plays")}>
              <div className="text-[13px] font-semibold text-[var(--panel-fg)]">{t("Which recording plays")}</div>
              <div className="flex items-center gap-2">
                <span className="text-[12px] theme-muted w-28 shrink-0">{t("All accounts")}</span>
                <Select
                  value={payload?.defaultId ?? ""}
                  options={[
                    { value: "", label: t("None") },
                    ...recordings.map((r) => ({ value: r.id, label: r.name })),
                  ]}
                  ariaLabel={t("Recording for all accounts")}
                  onChange={(v) => void setDefault(v)}
                  className="flex-1 min-w-0"
                />
              </div>
              {assignAccounts.length > 0 ? (
                <ul className="space-y-1">
                  {assignAccounts.map((a) => {
                    const own = payload?.accountIds[String(a.UserID)] ?? "";
                    const name = accountName(a.UserID);
                    return (
                      <li key={a.UserID} className="flex items-center gap-2">
                        <span className="w-28 shrink-0 truncate text-[12px] text-[var(--panel-fg)]" title={name}>
                          {name}
                        </span>
                        <Select
                          value={own}
                          options={[
                            { value: "", label: t("Same as all accounts") },
                            ...recordings.map((r) => ({ value: r.id, label: r.name })),
                          ]}
                          ariaLabel={t("Recording for {{name}}", { name })}
                          onChange={(v) => void setForAccount(a.UserID, v)}
                          className="flex-1 min-w-0"
                        />
                      </li>
                    );
                  })}
                </ul>
              ) : null}
              <div className="text-[11px] theme-muted leading-4">
                {t(
                  "An account's own recording wins over the one for all accounts. The list shows the accounts with a client open by this app and the ones that already have their own."
                )}
              </div>
            </section>

            <section className={`${CARD} space-y-2`} aria-label={t("When it plays")}>
              <div className="text-[13px] font-semibold text-[var(--panel-fg)]">{t("When it plays")}</div>
              <Toggle
                checked={triggers.afkRepeats}
                disabled={afkRunning}
                onChange={(v) => persistAfk("Mode", v ? "recording" : "key")}
                label="Repeat in AFK mode"
                description={
                  afkRunning
                    ? "AFK mode is on: stop it in AFK clicks to change what it sends."
                    : "While AFK mode is on, each account plays its recording again at this interval, counted from the end of the last one. Pick the accounts and start it in AFK clicks."
                }
              />
              {triggers.afkRepeats ? (
                <div className="flex items-center gap-2">
                  <span className="text-[12px] theme-muted flex-1">{t("Every")}</span>
                  <NumericInput
                    value={afkMinutes}
                    min={0}
                    max={120}
                    integer
                    disabled={afkRunning}
                    ariaLabel={t("Repeat every: minutes")}
                    onChange={setAfkMinutes}
                    onCommit={(v) => persistAfk("IntervalMinutes", String(v))}
                    containerClassName="relative w-16"
                    className="sidebar-input text-xs w-full"
                  />
                  <span className="text-[12px] theme-muted">{t("min")}</span>
                  <NumericInput
                    value={afkSeconds}
                    min={0}
                    max={59}
                    integer
                    disabled={afkRunning}
                    ariaLabel={t("Repeat every: seconds")}
                    onChange={setAfkSeconds}
                    onCommit={(v) => persistAfk("IntervalSeconds", String(v))}
                    containerClassName="relative w-16"
                    className="sidebar-input text-xs w-full"
                  />
                  <span className="text-[12px] theme-muted">s</span>
                </div>
              ) : null}
              <ToggleRow
                label="Play after an automatic reconnect"
                checked={afterReconnect}
                onChange={(v) => persist("AfterReconnect", v ? "true" : "false")}
              />
              <div className="flex items-center gap-2">
                <span className="text-[12px] theme-muted flex-1">{t("Once the account has been in the game for")}</span>
                <NumericInput
                  value={delayS}
                  min={MIN_AFTER_RECONNECT_S}
                  max={MAX_AFTER_RECONNECT_S}
                  integer
                  ariaLabel={t("Seconds in the game before playing")}
                  onChange={setDelayS}
                  onCommit={(v) => persist("AfterReconnectDelaySeconds", String(v))}
                  containerClassName="relative w-20"
                  className="sidebar-input text-xs w-full"
                />
                <span className="text-[12px] theme-muted">s</span>
              </div>
              <div className="text-[11px] theme-muted leading-4">
                {t("After a reconnect, only the account that came back plays, and only once.")}
              </div>
            </section>
          </div>

          <section className={`${CARD} space-y-2.5`} aria-label={t("Editor")}>
            {!draft ? (
              <div className="text-[12px] theme-muted">{t("Pick a recording in the library or create a new one.")}</div>
            ) : (
              <>
                <div className="flex items-center gap-2">
                  <span className="text-[12px] theme-muted w-16 shrink-0">{t("Name")}</span>
                  <input
                    value={draft.name}
                    maxLength={MAX_RECORDING_NAME_CHARS}
                    onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                    aria-label={t("Recording name")}
                    className="sidebar-input text-xs flex-1 min-w-0"
                  />
                </div>
                {imported ? (
                  <div
                    className="rounded-lg border border-sky-500/25 bg-sky-500/10 px-3 py-2 text-[11px] leading-4 text-sky-100 space-y-0.5"
                    role="status"
                    data-testid="tinytask-summary"
                  >
                    {tinyTaskSummaryLines(t, imported.steps, imported.summary).map((line) => (
                      <div key={line}>{line}</div>
                    ))}
                    <div className="theme-muted">
                      {t("Review the steps, test them on one account below, then save.")}
                    </div>
                  </div>
                ) : null}
                {saved && !dirty ? (
                  <div className="rounded-lg border theme-border px-3 py-2 space-y-1" data-testid="recording-use">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <span className="text-[11.5px] text-[var(--panel-fg)]">
                        {payload?.defaultId === saved.id
                          ? t("Plays for every account that has no recording of its own.")
                          : ownUses > 0
                            ? t("Plays for {{count}} account(s) as their own recording.", { count: ownUses })
                            : t("No account plays this recording yet.")}
                      </span>
                      {payload?.defaultId !== saved.id ? (
                        <button onClick={() => void setDefault(saved.id)} className="sidebar-btn-sm">
                          {t("Use for all accounts")}
                        </button>
                      ) : null}
                    </div>
                    <RecordingTriggerLines triggers={triggers} />
                  </div>
                ) : null}
                <div className="text-[11px] theme-muted">
                  {t("{{count}} steps · about {{duration}} per window", {
                    count: draft.steps.length,
                    duration: formatDurationMs(duration),
                  })}
                </div>

                {draft.steps.length === 0 ? (
                  <div className="rounded-lg border border-dashed theme-border px-3 py-3 text-[11px] theme-muted">
                    {t("No steps yet. Add them below, in the order they should play.")}
                  </div>
                ) : (
                  <ol className="space-y-1" aria-label={t("Steps")}>
                    {draft.steps.map((step, index) => (
                      <StepRow
                        key={index}
                        index={index}
                        step={step}
                        last={index === draft.steps.length - 1}
                        keys={keys}
                        marking={capture?.index === index ? capture.secondsLeft : null}
                        markDisabled={capture !== null}
                        onChange={(next) => updateStep(index, next)}
                        onMove={(delta) => setDraft({ ...draft, steps: moveStep(draft.steps, index, delta) })}
                        onRemove={() => setDraft({ ...draft, steps: draft.steps.filter((_, i) => i !== index) })}
                        onMark={() => void markPoint(index)}
                      />
                    ))}
                  </ol>
                )}

                <div className="flex flex-wrap items-center gap-1.5">
                  <span className="text-[11px] theme-muted mr-1">{t("Add")}</span>
                  {(
                    [
                      ["key", t("Key press")],
                      ["keyDown", t("Hold a key down")],
                      ["keyUp", t("Release a key")],
                      ["click", t("Mouse click")],
                      ["wait", t("Wait")],
                    ] as [RecordingStepType, string][]
                  ).map(([type, label]) => (
                    <button
                      key={type}
                      onClick={() => addStep(type)}
                      disabled={draft.steps.length >= MAX_RECORDING_STEPS}
                      className="sidebar-btn-sm flex items-center gap-1 disabled:opacity-50"
                    >
                      <Plus size={11} strokeWidth={2} aria-hidden />
                      {label}
                    </button>
                  ))}
                </div>

                {problem ? (
                  <div className="text-[11px] text-amber-300/90 leading-4" role="status">
                    {problemText(problem)}
                  </div>
                ) : null}

                <div className="flex flex-wrap items-center gap-2 pt-1">
                  <button
                    onClick={() => void save()}
                    disabled={!dirty || !!problem || saving}
                    className={PRIMARY_ACTION}
                  >
                    {t("Save recording")}
                  </button>
                  <button onClick={discard} disabled={!dirty} className={NEUTRAL_ACTION}>
                    {t("Discard changes")}
                  </button>
                </div>

                <div className="mt-2 rounded-lg border theme-border bg-[var(--panel-soft)] p-2.5 space-y-2">
                  <div className="text-[12px] font-semibold text-[var(--panel-fg)]">{t("Try it now")}</div>
                  {openAccounts.length === 0 ? (
                    <div className="text-[11px] theme-muted leading-4">
                      {t("Open an account first: a recording only reaches a Roblox client this app opened.")}
                    </div>
                  ) : (
                    <div className="flex flex-wrap gap-1.5">
                      {openAccounts.map((a) => {
                        const picked = playOn.includes(a.UserID);
                        const name = accountName(a.UserID);
                        return (
                          <button
                            key={a.UserID}
                            onClick={() =>
                              setPlayOn((ids) => (picked ? ids.filter((id) => id !== a.UserID) : [...ids, a.UserID]))
                            }
                            aria-pressed={picked}
                            aria-label={name}
                            className={`flex items-center gap-1 rounded-md border px-2 py-0.5 text-[12px] ${
                              picked ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-200" : "theme-border"
                            }`}
                          >
                            {picked ? <Check size={11} strokeWidth={3} aria-hidden /> : null}
                            <span className="max-w-[10rem] truncate">{name}</span>
                          </button>
                        );
                      })}
                    </div>
                  )}
                  <div className="flex items-center gap-2">
                    <button
                      onClick={() => void playNow()}
                      disabled={!saved || dirty || playOn.length === 0 || busy || draft.steps.length === 0}
                      className={`${NEUTRAL_ACTION} flex items-center gap-1.5`}
                    >
                      <Play size={12} strokeWidth={2} aria-hidden />
                      {t("Play now")}
                    </button>
                    {dirty ? (
                      <button
                        onClick={() => void testDraft()}
                        disabled={!!problem || playOn.length !== 1 || busy || draft.steps.length === 0}
                        className={`${NEUTRAL_ACTION} flex items-center gap-1.5`}
                      >
                        <FlaskConical size={12} strokeWidth={2} aria-hidden />
                        {t("Test on one account")}
                      </button>
                    ) : null}
                  </div>
                  {dirty ? (
                    <div className="text-[11px] theme-muted">
                      {t("Not saved yet: pick exactly one account to test these steps, or save to play them on several.")}
                    </div>
                  ) : null}
                  {shapeWarnings.map((a) => (
                    <div
                      key={a.UserID}
                      className="rounded-lg bg-amber-500/10 border border-amber-500/20 px-3 py-2 text-[11px] text-amber-200 break-words"
                      data-testid={`recording-shape-warning-${a.UserID}`}
                    >
                      {t(
                        "{{name}}: this window has a different shape from the one the recording was made in, so the clicks may land in other spots. It still plays.",
                        { name: accountName(a.UserID) }
                      )}
                    </div>
                  ))}
                  {lastResults.map((r) => (
                    <div
                      key={r.userId}
                      className="rounded-lg bg-amber-500/10 border border-amber-500/20 px-3 py-2 text-[11px] text-amber-200 break-words"
                    >
                      {resultText(r)}
                    </div>
                  ))}
                  <div className="text-[11px] theme-muted leading-4">
                    {t(
                      "Playing brings each account's Roblox window to the front, one after another, plays the whole recording there and gives the focus back only after the last one. Meanwhile, what you type goes to the Roblox window. If another window comes to the front, the rest of the recording is not played on that account."
                    )}{" "}
                    {t("Stopping interrupts at once, releases any key the recording was holding and never closes a client.")}
                  </div>
                </div>
              </>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}

type Translate = ReturnType<typeof useTr>;

/** A frase de um código de erro de reprodução (os mesmos do Modo AFK). */
export function recordingErrorText(t: Translate, code: string | null, raw: string | null, name: string): string {
  switch (code) {
    case "focusDenied":
      return t("{{name}}: Windows did not let this account's window come to the front, so nothing was sent.", { name });
    case "noWindow":
      return t("{{name}}: has no Roblox client open right now.", { name });
    case "keyRefused":
      return t("{{name}}: Windows refused the key.", { name });
    case "clickRefused":
      return t("{{name}}: Windows refused the click.", { name });
    case "noRecording":
      return t("{{name}}: has no recording to play.", { name });
    case "focusLost":
      return t("{{name}}: another window came to the front, so the rest of the recording was not played.", { name });
    case "stopped":
      return t("{{name}}: stopped in the middle of the recording.", { name });
    default:
      return `${name}: ${raw ?? ""}`.trim();
  }
}

function markErrorText(t: Translate, code: string): string {
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

function StepRow({
  index,
  step,
  last,
  keys,
  marking,
  markDisabled,
  onChange,
  onMove,
  onRemove,
  onMark,
}: {
  index: number;
  step: RecordingStep;
  last: boolean;
  keys: string[];
  marking: number | null;
  markDisabled: boolean;
  onChange: (next: RecordingStep) => void;
  onMove: (delta: -1 | 1) => void;
  onRemove: () => void;
  onMark: () => void;
}) {
  const t = useTr();
  const n = index + 1;
  const typeOptions = [
    { value: "key", label: t("Key press") },
    { value: "keyDown", label: t("Hold a key down") },
    { value: "keyUp", label: t("Release a key") },
    { value: "click", label: t("Mouse click") },
    { value: "wait", label: t("Wait") },
  ];
  const keySelect = (key: string, update: (key: string) => void) => (
    <Select
      value={key}
      options={keys.map((k) => ({ value: k, label: k }))}
      ariaLabel={t("Key of step {{n}}", { n })}
      onChange={update}
      className="w-24"
    />
  );

  return (
    <li className="rounded-lg border theme-border px-2 py-1.5" data-testid={`recording-step-${n}`}>
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="w-5 text-right text-[11px] font-mono theme-muted">{n}</span>
        <Select
          value={step.type}
          options={typeOptions}
          ariaLabel={t("Type of step {{n}}", { n })}
          onChange={(v) => onChange(changeStepType(step, v as RecordingStepType, keys[0] ?? "Space"))}
          className="w-36"
        />
        {step.type === "key" ? (
          <>
            {keySelect(step.key, (key) => onChange({ ...step, key }))}
            <NumericInput
              value={step.holdMs}
              min={MIN_HOLD_MS}
              max={MAX_HOLD_MS}
              integer
              ariaLabel={t("Hold time of step {{n}} in milliseconds", { n })}
              onChange={(v) => onChange({ ...step, holdMs: clampInt(v, MIN_HOLD_MS, MAX_HOLD_MS, 40) })}
              containerClassName="relative w-20"
              className="sidebar-input text-xs w-full"
            />
            <span className="text-[11px] theme-muted">{t("ms held")}</span>
          </>
        ) : null}
        {step.type === "keyDown" || step.type === "keyUp"
          ? keySelect(step.key, (key) => onChange({ ...step, key }))
          : null}
        {step.type === "wait" ? (
          <>
            <NumericInput
              value={step.ms}
              min={0}
              max={MAX_WAIT_MS}
              integer
              ariaLabel={t("Wait of step {{n}} in milliseconds", { n })}
              onChange={(v) => onChange({ ...step, ms: clampInt(v, 0, MAX_WAIT_MS, 0) })}
              containerClassName="relative w-24"
              className="sidebar-input text-xs w-full"
            />
            <span className="text-[11px] theme-muted">ms</span>
          </>
        ) : null}
        {step.type === "click" ? (
          <>
            <span className="text-[12px] font-mono text-[var(--panel-fg)]">
              {step.xPct}% × {step.yPct}%
            </span>
            <button
              onClick={onMark}
              disabled={markDisabled}
              aria-label={t("Mark the point of step {{n}}", { n })}
              className="sidebar-btn-sm flex items-center gap-1 disabled:opacity-50"
            >
              <Crosshair size={12} strokeWidth={1.75} aria-hidden />
              {t("Mark")}
            </button>
          </>
        ) : null}
        <span className="flex-1" />
        <button onClick={() => onMove(-1)} disabled={index === 0} aria-label={t("Move step {{n}} up", { n })} className={SMALL_BTN}>
          <ArrowUp size={12} strokeWidth={1.75} aria-hidden />
        </button>
        <button onClick={() => onMove(1)} disabled={last} aria-label={t("Move step {{n}} down", { n })} className={SMALL_BTN}>
          <ArrowDown size={12} strokeWidth={1.75} aria-hidden />
        </button>
        <button onClick={onRemove} aria-label={t("Remove step {{n}}", { n })} className={SMALL_BTN}>
          <Trash2 size={12} strokeWidth={1.75} aria-hidden />
        </button>
      </div>
      {marking !== null ? (
        <div className="mt-1 pl-6 text-[11px] text-sky-300 leading-4" role="status">
          {t("Put the mouse over the spot in a game window: {{seconds}}", { seconds: marking })}
        </div>
      ) : null}
    </li>
  );
}
