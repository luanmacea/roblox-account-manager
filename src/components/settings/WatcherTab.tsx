import { useStore } from "../../store";
import { isWindowsPlatform } from "../../utils/platform";
import type { UseSettingsReturn } from "../../hooks/useSettings";
import { Toggle } from "../ui/Toggle";
import { NumberField } from "../ui/NumberField";
import { TextField } from "../ui/TextField";
import { Divider } from "../ui/Divider";
import { SectionLabel } from "../ui/SectionLabel";
import { useTr } from "../../i18n/text";

export function WatcherTab({ s }: { s: UseSettingsReturn }) {
  const t = useTr();
  // `ReadInterval` so e lido dentro de `#[cfg(target_os = "macos")]`
  // (commands/watcher.rs). No Windows o campo so enfeitava a tela.
  const showReadInterval = !isWindowsPlatform(useStore().platformCapabilities);

  // Cada campo abaixo so e lido pelo watcher quando o interruptor acima dele
  // esta ligado (commands/watcher.rs); editavel desligado, era valor morto.
  const exitIfNoConnection = s.getBool("Watcher", "ExitIfNoConnection");
  const closeOnLowMemory = s.getBool("Watcher", "CloseRbxMemory");
  const closeOnTitleMismatch = s.getBool("Watcher", "CloseRbxWindowTitle");

  return (
    <div className="space-y-0">
      {/* O watcher e um loop que FECHA clientes (`kill_for_user`). Sem estas
          linhas o usuario liga um interruptor e descobre o efeito pelo cliente
          que sumiu. Tudo aqui sai de `start_watcher` (commands/watcher.rs). */}
      <div className="mx-1 mt-1 rounded-lg border border-zinc-800/70 bg-zinc-900/35 px-3 py-2.5 text-[12px] leading-relaxed text-zinc-400 space-y-1">
        <div>
          {t(
            "Every few seconds the watcher checks each Roblox client this app launched and closes the ones that match a rule below."
          )}
        </div>
        <div className="text-zinc-500">
          {t(
            "It never reopens them, ignores clients you started outside the app, and skips the window you are using right now."
          )}
        </div>
        <div className="text-zinc-500">
          {t("Memory and window title checks only start 30 seconds after a client opens.")}
        </div>
      </div>

      <SectionLabel>Scanner</SectionLabel>
      <Toggle
        checked={s.getBool("Watcher", "Enabled")}
        onChange={(v) => s.setBool("Watcher", "Enabled", v)}
        label="Enable Roblox Watcher"
        // `store.tsx` chama start_watcher/stop_watcher conforme esta chave.
        description="Starts the scan loop. With it off, nothing below runs."
      />
      <NumberField
        value={s.getNumber("Watcher", "ScanInterval", 6)}
        onChange={(v) => s.setNumber("Watcher", "ScanInterval", v)}
        label="Scan Interval"
        description="How long the watcher waits between checks of every tracked client."
        min={1}
        max={60}
        suffix="sec"
      />
      {showReadInterval && (
        <NumberField
          value={s.getNumber("Watcher", "ReadInterval", 250)}
          onChange={(v) => s.setNumber("Watcher", "ReadInterval", v)}
          label="Read Interval"
          description="How often the log file is re-read while the watcher is running."
          min={50}
          max={5000}
          suffix="ms"
        />
      )}

      <Divider />
      <SectionLabel>Connection</SectionLabel>
      <Toggle
        checked={s.getBool("Watcher", "ExitIfNoConnection")}
        onChange={(v) => s.setBool("Watcher", "ExitIfNoConnection", v)}
        label="Exit If No Connection"
        // Windows le o titulo da janela; macOS le o log do cliente.
        description="Closes a client that reports a lost connection and stays that way past the timeout below."
      />
      <NumberField
        value={s.getNumber("Watcher", "NoConnectionTimeout", 60)}
        onChange={(v) => s.setNumber("Watcher", "NoConnectionTimeout", v)}
        label="No Connection Timeout"
        description={
          !exitIfNoConnection
            ? "Requires Exit If No Connection"
            : "How long a client may stay disconnected before it is closed."
        }
        min={5}
        max={600}
        suffix="sec"
        disabled={!exitIfNoConnection}
      />

      <Divider />
      <SectionLabel>Process Behavior</SectionLabel>

      <Toggle
        checked={s.getBool("Watcher", "ExitOnBeta")}
        onChange={(v) => s.setBool("Watcher", "ExitOnBeta", v)}
        label="Exit on Beta"
        // Windows: titulo contem "roblox beta". macOS: linha de volta para a home.
        description="Closes a client that lands on the Roblox Beta app instead of staying in the game."
      />
      <Toggle
        checked={s.getBool("Watcher", "CloseIfNotResponding")}
        onChange={(v) => s.setBool("Watcher", "CloseIfNotResponding", v)}
        label="Close If Not Responding"
        // client_health.rs marca "Não respondendo" depois de 30 s de IsHungAppWindow.
        description="Closes a client whose window stays “Not responding” for 30 seconds."
      />

      <Divider />
      <SectionLabel>Memory & Window</SectionLabel>

      <Toggle
        checked={s.getBool("Watcher", "CloseRbxMemory")}
        onChange={(v) => s.setBool("Watcher", "CloseRbxMemory", v)}
        label="Close If Memory Low"
        // Working set ABAIXO do limite: a regra e de cliente travado, nao de consumo alto.
        description="Closes a client whose memory drops below the threshold, the usual sign of one that froze. With a memory limit set, it also closes a client that stays over it after MultiAlt freed its memory."
      />
      <NumberField
        value={s.getNumber("Watcher", "MemoryLowValue", 200)}
        onChange={(v) => s.setNumber("Watcher", "MemoryLowValue", v)}
        label="Memory Threshold"
        description={
          !closeOnLowMemory ? "Requires Close If Memory Low" : "A client using less than this is closed."
        }
        min={50}
        max={2048}
        suffix="MB"
        disabled={!closeOnLowMemory}
      />
      <Toggle
        checked={s.getBool("Watcher", "CloseRbxWindowTitle")}
        onChange={(v) => s.setBool("Watcher", "CloseRbxWindowTitle", v)}
        label="Close If Window Title Mismatch"
        // Comparacao exata, e so existe no config do Windows.
        description="Closes a client whose window title is not exactly the text below. Windows only."
      />
      <TextField
        value={s.get("Watcher", "ExpectedWindowTitle", "Roblox")}
        onChange={(v) => s.set("Watcher", "ExpectedWindowTitle", v)}
        label="Expected Title"
        placeholder="Roblox"
        disabled={!closeOnTitleMismatch}
      />
      <Toggle
        checked={s.getBool("Watcher", "SaveWindowPositions")}
        onChange={(v) => s.setBool("Watcher", "SaveWindowPositions", v)}
        label="Remember Window Positions"
        // Grava Window_Position_X/Y e Window_Width/Height na conta; o launch le de volta.
        description="Saves each client's window position and size, and the next launch of that account reopens it there."
      />
    </div>
  );
}
