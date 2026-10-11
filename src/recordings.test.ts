import { describe, expect, it } from "vitest";
import {
  changeStepType,
  CLICK_ESTIMATE_MS,
  formatDurationMs,
  moveStep,
  newStep,
  recordingDurationMs,
  recordingForAccount,
  recordingProblem,
  type RecordingsPayload,
  type RecordingStep,
  aspectDiffers,
  recordingNameFromFile,
} from "./recordings";

describe("gravações — importar do TinyTask", () => {
  it("o nome vem do arquivo, sem a extensão e dentro do limite", () => {
    expect(recordingNameFromFile("farm loop.rec")).toBe("farm loop");
    expect(recordingNameFromFile("C:\\macros\\Boss.REC")).toBe("Boss");
    expect(recordingNameFromFile(".rec")).toBe("");
    expect([...recordingNameFromFile(`${"x".repeat(80)}.rec`)]).toHaveLength(60);
  });

  /**
   * Os cliques são porcentagens da janela: tocar numa janela de outro formato
   * desloca o ponto. Mais de 5% de diferença na proporção = aviso.
   */
  it("avisa quando a janela da conta tem outro formato (mais de 5%)", () => {
    const wide = 1920 / 1080; // 1.7778
    expect(aspectDiffers(wide, 1280, 720)).toBe(false); // mesmo formato, outro tamanho
    expect(aspectDiffers(wide, 1296, 759)).toBe(false); // 1.7075: 4% — passa
    expect(aspectDiffers(wide, 800, 600)).toBe(true); // 4:3
    expect(aspectDiffers(wide, 1080, 1920)).toBe(true); // em pé
    // Sem a proporção (gravação escrita à mão) ou sem janela: nada a dizer.
    expect(aspectDiffers(undefined, 800, 600)).toBe(false);
    expect(aspectDiffers(wide, 0, 600)).toBe(false);
  });
});

const KEYS = ["Space", "W", "E"];

describe("gravações — passos", () => {
  it("passo novo já pode ser tocado", () => {
    expect(newStep("key")).toEqual({ type: "key", key: "Space", holdMs: 40 });
    expect(newStep("wait")).toEqual({ type: "wait", ms: 500 });
    expect(newStep("click")).toEqual({ type: "click", xPct: 50, yPct: 50 });
    expect(newStep("keyDown", "W")).toEqual({ type: "keyDown", key: "W" });
  });

  it("trocar o tipo mantém a tecla quando dá", () => {
    expect(changeStepType({ type: "key", key: "W", holdMs: 900 }, "keyDown")).toEqual({ type: "keyDown", key: "W" });
    expect(changeStepType({ type: "wait", ms: 10 }, "key", "E")).toEqual({ type: "key", key: "E", holdMs: 40 });
  });

  it("a duração conta esperas, teclas seguradas e cliques, como o backend", () => {
    const steps: RecordingStep[] = [
      { type: "key", key: "W", holdMs: 300 },
      { type: "wait", ms: 200 },
      { type: "click", xPct: 1, yPct: 1 },
      { type: "keyDown", key: "E" },
      { type: "keyUp", key: "E" },
    ];
    expect(recordingDurationMs(steps)).toBe(300 + 200 + CLICK_ESTIMATE_MS);
  });

  it("formata a duração de forma curta", () => {
    expect(formatDurationMs(1_234)).toBe("1.2 s");
    expect(formatDurationMs(45_000)).toBe("45 s");
    expect(formatDurationMs(185_000)).toBe("3 min 5 s");
    expect(formatDurationMs(120_000)).toBe("2 min");
  });

  it("move passos sem sair da lista", () => {
    const steps: RecordingStep[] = [newStep("wait"), newStep("click")];
    expect(moveStep(steps, 1, -1).map((s) => s.type)).toEqual(["click", "wait"]);
    expect(moveStep(steps, 0, -1)).toBe(steps);
    expect(moveStep(steps, 1, 1)).toBe(steps);
  });
});

describe("gravações — o que impede salvar", () => {
  it("nome, tecla da lista, passos e tempo", () => {
    expect(recordingProblem("  ", [], KEYS)).toBe("noName");
    expect(recordingProblem("x".repeat(61), [], KEYS)).toBe("nameTooLong");
    expect(recordingProblem("A", [{ type: "key", key: "Enter", holdMs: 40 }], KEYS)).toBe("badKey");
    expect(recordingProblem("A", Array(501).fill(newStep("wait")), KEYS)).toBe("tooManySteps");
    expect(
      recordingProblem(
        "A",
        [
          { type: "wait", ms: 600_000 },
          { type: "wait", ms: 1 },
        ],
        KEYS
      )
    ).toBe("tooLong");
    expect(recordingProblem("A", [newStep("key")], KEYS)).toBeNull();
  });
});

describe("gravações — qual vale para a conta", () => {
  const payload: RecordingsPayload = {
    recordings: [
      { id: "all", name: "All", steps: [], createdAt: 0, updatedAt: 0 },
      { id: "mine", name: "Mine", steps: [], createdAt: 0, updatedAt: 0 },
    ],
    defaultId: "all",
    accountIds: { "11": "mine", "33": "gone" },
    keys: KEYS,
  };

  it("a da conta vence a de todas", () => {
    expect(recordingForAccount(payload, 11)?.id).toBe("mine");
    expect(recordingForAccount(payload, 22)?.id).toBe("all");
    expect(recordingForAccount(payload, 33)?.id).toBe("all");
  });

  it("sem escolha, nenhuma", () => {
    expect(recordingForAccount({ ...payload, defaultId: null }, 22)).toBeNull();
    expect(recordingForAccount(null, 22)).toBeNull();
  });
});
