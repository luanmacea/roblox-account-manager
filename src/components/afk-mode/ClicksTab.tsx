import { Check, Crosshair, Send } from "lucide-react";
import { formatAfkPoint, readAfkPoint, type AfkMode } from "../../afkClickPoint";
import { Select } from "../ui/Select";
import { NumericInput } from "../ui/NumericInput";
import { ToggleRow } from "../ui/ToggleRow";
import { DANGER_ACTION, ModeStatusBar, NEUTRAL_ACTION, PRIMARY_ACTION } from "./ModeStatusBar";
import {
  formatAfkCountdown,
  formatAfkElapsed,
  parseAfkMode,
  useClicksController,
  type ClicksController,
  type ClicksTabOptions,
} from "./clicks/useClicksController";

export { formatAfkElapsed };

const CARD = "theme-surface rounded-xl border theme-border p-3";

/**
 * Aba de cliques AFK do Modo AFK (antes o diálogo de AFK mode): o app manda uma
 * tecla, ou um clique, de tempo em tempo, para a janela de cada conta que está
 * no modo — para o jogo não contar a conta como parada e não precisar de rejoin.
 *
 * A tela tem duas obrigações que não são enfeite:
 * 1. dizer que **cada ciclo tira o foco da janela do usuário** e só o devolve
 *    depois da última conta — cerca de meio segundo por conta (é o preço do
 *    `SendInput`, que só alcança a janela em primeiro plano);
 * 2. só oferecer tecla da lista fechada que o backend entrega (`afkKeys`) — sem
 *    campo livre e sem tecla padrão, porque ligar o modo não pode mexer no
 *    personagem com uma tecla que o usuário não escolheu.
 *
 * No modo **clique** não há tecla: cada conta leva um clique esquerdo num ponto
 * relativo (%) da janela dela — o padrão, ou o próprio da conta. O Marcar dá
 * 3 s para o usuário parar o mouse em cima do ponto e o backend lê a posição.
 */
export function ClicksTab(props: ClicksTabOptions) {
  const ctl = useClicksController(props);
  const { t, running } = ctl;

  const sendNowLabel = ctl.sendingNow
    ? t("Sending...")
    : ctl.recordingMode
      ? t("Play now")
      : ctl.clickMode
      ? t("Click now")
      : t("Send the key now");

  const sendNow = (
    <button
      onClick={() => void ctl.handleSendNow()}
      disabled={ctl.sendingNow || !running || !ctl.sendReady || ctl.inAfk.length === 0}
      className={`${NEUTRAL_ACTION} flex items-center gap-1.5`}
    >
      <Send size={13} strokeWidth={1.75} aria-hidden />
      {sendNowLabel}
    </button>
  );

  return (
    <div className="@container/clicks flex h-full min-h-0 flex-col gap-3">
      <ModeStatusBar
        testId="clicks-status"
        dataTour="afk-clicks-start"
        running={running}
        title={running ? t("AFK clicks are on") : t("AFK clicks are off")}
        facts={[
          // Estado, não ação: entre um ciclo e outro — ou com o foco negado em
          // todas as contas — nada está sendo enviado.
          <span
            key="pill"
            className={`px-2 py-0.5 rounded-full text-[11px] border ${
              running
                ? "border-emerald-500/30 bg-emerald-500/15 text-emerald-300"
                : "theme-border theme-soft theme-muted"
            }`}
          >
            {running ? t("On") : t("Off")}
          </span>,
          running
            ? t("Running for {{elapsed}}", {
                elapsed: formatAfkElapsed(ctl.status?.startedAtMs ?? null, ctl.nowMs),
              })
            : null,
          running && ctl.statusByUserId.size > 0
            ? t("Sends so far: {{count}}", { count: ctl.totalSends })
            : null,
        ]}
        actions={
          running ? (
            <>
              {sendNow}
              <button onClick={() => void ctl.handleStop()} disabled={ctl.busy} className={DANGER_ACTION}>
                {t("Stop AFK Mode")}
              </button>
            </>
          ) : (
            <>
              <button onClick={() => void ctl.handleStart()} disabled={!ctl.canStart} className={PRIMARY_ACTION}>
                {t("Start AFK Mode")}
              </button>
              {sendNow}
            </>
          )
        }
        message={ctl.startBlocker ? { text: ctl.startBlocker, tone: "muted" } : null}
      />

      <div className="flex-1 min-h-0 overflow-y-auto pr-0.5">
        <div className="grid grid-cols-1 gap-3 @3xl/clicks:grid-cols-[minmax(300px,380px)_minmax(0,1fr)] @7xl/clicks:grid-cols-[440px_minmax(0,1fr)] items-start">
          <SettingsCard ctl={ctl} />
          <AccountsCard ctl={ctl} />
        </div>
      </div>
    </div>
  );
}

function SettingsCard({ ctl }: { ctl: ClicksController }) {
  const { t, clickMode, configDisabled, capture } = ctl;
  return (
    <section data-tour="afk-clicks-settings" className={`${CARD} space-y-2.5`}>
      <div className="text-[13px] font-semibold text-[var(--panel-fg)]">{t("Settings")}</div>
      {/* Minutos + segundos, contados do fim de cada ciclo. Piso de 5 s
          (`clamp_afk_interval_seconds`): abaixo disso o Start não liga. */}
      <div className="flex items-center gap-2">
        <span className="text-[12px] theme-muted w-32 shrink-0">{t("Send every")}</span>
        <NumericInput
          value={ctl.intervalMinutesPart}
          min={0}
          max={120}
          integer
          disabled={configDisabled}
          ariaLabel={t("Send every: minutes")}
          onChange={ctl.setIntervalMinutes}
          onCommit={(v) => ctl.persist("IntervalMinutes", String(v))}
          containerClassName="relative flex-1 min-w-0"
          className="sidebar-input text-xs w-full disabled:opacity-60"
        />
        <span className="text-[12px] theme-muted">{t("min")}</span>
        <NumericInput
          value={ctl.intervalSecondsPart}
          min={0}
          max={59}
          integer
          disabled={configDisabled}
          ariaLabel={t("Send every: seconds")}
          onChange={ctl.setIntervalSecondsPart}
          onCommit={(v) => ctl.persist("IntervalSeconds", String(v))}
          containerClassName="relative flex-1 min-w-0"
          className="sidebar-input text-xs w-full disabled:opacity-60"
        />
        <span className="text-[12px] theme-muted">s</span>
      </div>
      {ctl.intervalTooShort ? (
        <div className="text-[11px] theme-muted leading-4">{t("At least 5 seconds.")}</div>
      ) : null}
      <div className="flex items-center gap-2">
        <span className="text-[12px] theme-muted w-32 shrink-0">{t("What to send")}</span>
        <Select
          value={ctl.effectiveMode}
          options={[
            { value: "key", label: "Key press" },
            { value: "click", label: "Mouse click" },
            { value: "recording", label: "Play the recording" },
          ]}
          disabled={configDisabled}
          ariaLabel="What to send"
          onChange={(v) => ctl.setMode(parseAfkMode(v) as AfkMode)}
          className="flex-1"
        />
      </div>
      {ctl.recordingMode ? (
        <div className="text-[11px] theme-muted leading-4">
          {t(
            "Each account plays its own recording, or the one for all accounts, from start to end on every turn. Pick which one in the Recordings tab; the list below shows it next to each account."
          )}
        </div>
      ) : clickMode ? (
        <>
          <div className="flex items-center gap-2">
            <span className="text-[12px] theme-muted w-32 shrink-0">{t("Click point")}</span>
            <span className="flex-1 text-[12px] font-mono text-[var(--panel-fg)]">
              {formatAfkPoint(ctl.effectivePoint)}
            </span>
            <button
              onClick={() => void ctl.startMark("default")}
              disabled={configDisabled || capture !== null}
              aria-label={t("Mark the click point for all accounts")}
              className="sidebar-btn-sm flex items-center gap-1.5 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Crosshair size={13} strokeWidth={1.75} />
              {t("Mark")}
            </button>
          </div>
          {capture ? (
            <div className="text-[11px] text-sky-300 leading-4" role="status">
              {t("Put the mouse over the spot in a game window: {{seconds}}", {
                seconds: capture.secondsLeft,
              })}
            </div>
          ) : null}
          <div className="text-[11px] theme-muted leading-4">
            {t(
              "Mark gives you 3 seconds to rest the mouse on the spot inside the Roblox window of any account in the list. The point is a percentage of the window, so it lands in the same place in windows of any size. An account can have its own point below."
            )}
          </div>
        </>
      ) : (
        <>
          <div className="flex items-center gap-2">
            {/* "Key" sozinho é a chave de campo da conta ("Chave" em pt), da
                tela de campos; aqui é tecla, e precisa de texto próprio. */}
            <span className="text-[12px] theme-muted w-32 shrink-0">{t("Key to send")}</span>
            <Select
              value={ctl.effectiveKey}
              options={ctl.store.afkKeys.map((k) => ({ value: k, label: k }))}
              disabled={configDisabled}
              ariaLabel="Key to send"
              onChange={ctl.setKey}
              className="flex-1"
            />
          </div>
          {!ctl.keyAllowed && !ctl.running ? (
            <div className="text-[11px] text-amber-300/90 leading-4">
              {t("Choose one of these keys — AFK mode does not start without a key you picked")}
            </div>
          ) : null}
        </>
      )}
      <ToggleRow label="Beep when a cycle finishes" checked={ctl.beepOnCycle} onChange={ctl.setBeepOnCycle} />
      <div className="rounded-lg border theme-border bg-[var(--panel-soft)] px-3 py-2 text-[11px] theme-muted leading-4">
        {t(
          "Each cycle takes the focus away from the window you are using: it brings the Roblox window of each account whose turn it is to the front, one after another, for about half a second each, and gives the focus back only after the last one — about 4 seconds with 10 accounts."
        )}{" "}
        {t("Meanwhile, what you type goes to the Roblox window, not to the program you were using.")}{" "}
        {clickMode ? (
          <>
            {t(
              "In click mode the cursor also jumps to the point and comes back, and the cycle takes about 1.2 seconds per account: the game has to see the mouse move and get a focus click before the real one."
            )}{" "}
          </>
        ) : null}
        {ctl.recordingMode ? (
          <>
            {t(
              "With a recording, each window stays in front for the whole recording, so the cycle takes as long as the recordings add up to."
            )}{" "}
          </>
        ) : null}
        {t(
          "And when Windows keeps the window in the background — which is what it usually does while this app is not the one you are using — nothing is sent at all, and the account below says so."
        )}
      </div>
    </section>
  );
}

function AccountsCard({ ctl }: { ctl: ClicksController }) {
  const { t, store, running, inAfk, candidates, statusByUserId, clickMode, capture } = ctl;
  return (
    <section data-tour="afk-clicks-accounts" className={`@container ${CARD}`}>
      <div className="mb-2 flex items-baseline justify-between gap-2">
        <div className="text-[13px] font-semibold text-[var(--panel-fg)]">{t("Accounts in AFK mode")}</div>
        {candidates.length > 0 ? (
          <span className="text-[12px] theme-muted">
            {inAfk.length} / {candidates.length}
          </span>
        ) : null}
      </div>
      {candidates.length === 0 ? (
        <div className="text-[11px] theme-muted leading-4">
          {t("Open an account first: AFK mode only reaches a Roblox client this app opened.")}
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-1.5 @xl:grid-cols-2 @4xl:grid-cols-3">
          {candidates.map((account) => {
            const picked = inAfk.includes(account.UserID);
            const row = statusByUserId.get(account.UserID);
            const name = ctl.accountName(account.UserID);
            const ownPoint = readAfkPoint(account.Fields);
            return (
              <div
                key={account.UserID}
                className={`rounded-lg border transition-colors ${
                  picked ? "border-emerald-500/30 bg-emerald-500/10" : "theme-border"
                }`}
              >
                <button
                  onClick={() => void ctl.toggleAccount(account.UserID)}
                  disabled={ctl.busy}
                  aria-pressed={picked}
                  // O nome acessível é só o da conta: o relógio e o aviso ao
                  // lado mudam a cada segundo e tornariam o botão impossível de
                  // achar por nome.
                  aria-label={name}
                  className={`w-full flex items-center gap-2 px-2 py-1.5 rounded-lg text-left ${
                    picked ? "" : "hover:bg-[var(--panel-soft)]"
                  } disabled:opacity-60`}
                >
                  <span
                    className={`w-4 h-4 shrink-0 rounded border flex items-center justify-center ${
                      picked ? "border-emerald-400/60 text-emerald-300" : "theme-border"
                    }`}
                  >
                    {picked ? <Check size={11} strokeWidth={3} /> : null}
                  </span>
                  <span className="flex-1 truncate text-[12px] text-[var(--panel-fg)]" title={name}>
                    {name}
                  </span>
                  {running && picked ? (
                    <span className="text-[11px] font-mono theme-muted shrink-0">
                      {formatAfkCountdown(row?.nextSendAtMs ?? null, ctl.nowMs, ctl.intervalSeconds * 1000)}
                    </span>
                  ) : null}
                  {ctl.recordingMode && picked ? (
                    <span
                      className={`text-[11px] shrink-0 max-w-[8rem] truncate ${
                        ctl.recordingNameFor(account.UserID) ? "theme-muted" : "text-amber-300/90"
                      }`}
                      title={ctl.recordingNameFor(account.UserID) ?? undefined}
                    >
                      {ctl.recordingNameFor(account.UserID) ?? t("no recording")}
                    </span>
                  ) : null}
                  {row?.lastErrorCode === "focusDenied" ? (
                    <span className="text-[11px] text-amber-300/90 shrink-0">{t("not sent")}</span>
                  ) : null}
                  {!store.launchedByProgram.has(account.UserID) ? (
                    <span className="text-[11px] text-amber-300/90 shrink-0">{t("no client")}</span>
                  ) : null}
                </button>
                {clickMode && picked ? (
                  // Fora do botão da linha: botão dentro de botão não existe. O
                  // ponto próprio é lido a cada ciclo, então muda com a sessão
                  // ligada.
                  <div className="flex items-center gap-2 pl-8 pr-2 pb-1.5 text-[11px] theme-muted">
                    <span className="flex-1 font-mono">
                      {ownPoint ? formatAfkPoint(ownPoint) : t("Default point")}
                    </span>
                    <button
                      onClick={() => void ctl.startMark(account.UserID)}
                      disabled={capture !== null}
                      aria-label={t("Mark the click point for {{name}}", { name })}
                      className="px-1.5 py-0.5 rounded-md border theme-border hover:bg-[var(--panel-soft)] disabled:opacity-50"
                    >
                      {t("Mark")}
                    </button>
                    {ownPoint ? (
                      <button
                        onClick={() => void ctl.clearOwnPoint(account.UserID)}
                        aria-label={t("Use the default point for {{name}}", { name })}
                        className="px-1.5 py-0.5 rounded-md border theme-border hover:bg-[var(--panel-soft)]"
                      >
                        {t("Use default")}
                      </button>
                    ) : null}
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>
      )}
      {running
        ? [...statusByUserId.values()]
            .filter((a) => a.lastError || a.lastErrorCode)
            .slice(0, 4)
            .map((a) => (
              <div
                key={a.userId}
                className="mt-2 rounded-lg bg-amber-500/10 border border-amber-500/20 px-3 py-2 text-[11px] text-amber-200 break-words"
              >
                {ctl.sendErrorText(a.lastErrorCode, a.lastError, ctl.accountName(a.userId))}
              </div>
            ))
        : null}
      {running && ctl.focusDenied ? (
        <div className="mt-2 text-[11px] theme-muted leading-4">
          {t(
            "Windows only lets an app change which window is in front in some situations, so the automatic send can be skipped for a while."
          )}{" "}
          {clickMode
            ? t(
                "\"Click now\" works because you just clicked this window, and the manager being the window you are using makes the next cycle go through."
              )
            : t(
                "\"Send the key now\" works because you just clicked this window, and the manager being the window you are using makes the next cycle go through."
              )}
        </div>
      ) : null}
      <div className="mt-3 text-[11px] theme-muted leading-4">
        {t("Stopping interrupts a cycle already under way and never closes a client.")}
      </div>
    </section>
  );
}
