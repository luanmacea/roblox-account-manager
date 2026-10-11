import { describe, expect, it } from "vitest";
import { formatAfkIntervalSeconds, recordingTriggers } from "./triggers";
import type { RecordingsPayload } from "../../../recordings";

function payload(overrides: Partial<RecordingsPayload> = {}): RecordingsPayload {
  return {
    recordings: [
      { id: "a", name: "Farm loop", steps: [], createdAt: 0, updatedAt: 0 },
      { id: "b", name: "Jump", steps: [], createdAt: 0, updatedAt: 0 },
    ],
    defaultId: null,
    accountIds: {},
    keys: [],
    ...overrides,
  };
}

/**
 * Os dois gatilhos das gravações (pedido do dono): o Modo AFK repetindo a
 * gravação de minutos em minutos e a gravação depois da reconexão. O resumo é
 * o mesmo na aba Recordings, na página Session e no cartão da aba.
 */
describe("gatilhos das gravações", () => {
  it("lê o modo, o intervalo e a reconexão do INI", () => {
    const out = recordingTriggers(payload({ defaultId: "a", accountIds: { "11": "b", "22": "a" } }), {
      Afk: { Mode: "recording", IntervalMinutes: "2", IntervalSeconds: "30" },
      Recordings: { AfterReconnect: "true", AfterReconnectDelaySeconds: "45" },
    });
    expect(out).toEqual({
      afkRepeats: true,
      intervalSeconds: 150,
      afterReconnect: true,
      delaySeconds: 45,
      allAccountsName: "Farm loop",
      ownCount: 2,
    });
  });

  it("sem nada salvo: o Modo AFK manda tecla, a reconexão não toca, 10 min e 30 s", () => {
    expect(recordingTriggers(null, undefined)).toEqual({
      afkRepeats: false,
      intervalSeconds: 600,
      afterReconnect: false,
      delaySeconds: 30,
      allAccountsName: null,
      ownCount: 0,
    });
  });

  it("escolha que aponta para gravação apagada não conta", () => {
    const out = recordingTriggers(payload({ defaultId: "gone", accountIds: { "11": "gone" } }), {});
    expect(out.allAccountsName).toBeNull();
    expect(out.ownCount).toBe(0);
  });

  it("o tempo depois da reconexão fica entre 5 s e 1 h, como no backend", () => {
    expect(recordingTriggers(null, { Recordings: { AfterReconnectDelaySeconds: "1" } }).delaySeconds).toBe(5);
    expect(recordingTriggers(null, { Recordings: { AfterReconnectDelaySeconds: "99999" } }).delaySeconds).toBe(3600);
  });

  it("o intervalo aparece em minutos e segundos", () => {
    expect(formatAfkIntervalSeconds(600)).toBe("10 min");
    expect(formatAfkIntervalSeconds(150)).toBe("2 min 30 s");
    expect(formatAfkIntervalSeconds(45)).toBe("45 s");
  });
});
