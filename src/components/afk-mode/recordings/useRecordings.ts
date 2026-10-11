import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { RecordingsPayload } from "../../../recordings";

/** A resposta tem a forma de `get_recordings`? (Dublê sem resposta = `null`.) */
function isPayload(value: unknown): value is RecordingsPayload {
  const v = value as RecordingsPayload | null | undefined;
  return !!v && Array.isArray(v.recordings) && Array.isArray(v.keys);
}

/**
 * A biblioteca de gravações e quem toca o quê, lida do backend e relida a cada
 * `recordings-changed` (o backend avisa depois de cada gravação no arquivo).
 * Também diz se uma reprodução avulsa ("Tocar agora", depois da reconexão) está
 * rodando, pelo evento `recording-playback`.
 */
export function useRecordings() {
  const [payload, setPayload] = useState<RecordingsPayload | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);

  const reload = useCallback(async () => {
    try {
      const next = await invoke<RecordingsPayload>("get_recordings");
      if (isPayload(next)) {
        setPayload(next);
        setLoadError(null);
      }
    } catch (e) {
      setLoadError(String(e));
    }
  }, []);

  useEffect(() => {
    let alive = true;
    void reload();
    void invoke<{ active: boolean }>("get_recording_playback")
      .then((state) => {
        if (alive && state && typeof state.active === "boolean") setPlaying(state.active);
      })
      .catch(() => {});
    const offChanged = listen("recordings-changed", () => {
      if (alive) void reload();
    });
    const offPlayback = listen<{ active: boolean }>("recording-playback", (e) => {
      if (alive && e.payload && typeof e.payload.active === "boolean") setPlaying(e.payload.active);
    });
    return () => {
      alive = false;
      void offChanged.then((fn) => fn()).catch(() => {});
      void offPlayback.then((fn) => fn()).catch(() => {});
    };
  }, [reload]);

  return { payload, loadError, playing, reload };
}
