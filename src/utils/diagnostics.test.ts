import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { diagnosticText, diagnosticsSummaryLines, overallStatus, type DiagnosticCheck } from "./diagnostics";

const t = (text: string, options?: Record<string, unknown>) =>
  text.replace(/\{\{(\w+)\}\}/g, (_, key) => String(options?.[key] ?? ""));

/** Os pares `id`/`reason` que `commands/diagnostics.rs` pode devolver. */
function backendPairs(): string[] {
  const source = readFileSync(path.resolve(__dirname, "../../src-tauri/src/commands/diagnostics.rs"), "utf8");
  const body = source.split("#[cfg(test)]")[0];
  const pairs = new Set<string>();
  // DiagnosticCheck::new("id", CheckStatus::X, "reason") com id literal...
  for (const m of body.matchAll(/DiagnosticCheck::new\("(\w+)",\s*CheckStatus::\w+,\s*"(\w+)"\)/g)) {
    pairs.add(`${m[1]}.${m[2]}`);
  }
  // ...e as pastas, que recebem o id por parâmetro.
  for (const m of body.matchAll(/DiagnosticCheck::new\(id,\s*CheckStatus::\w+,\s*"(\w+)"\)/g)) {
    pairs.add(`dataFolder.${m[1]}`);
    pairs.add(`versionsFolder.${m[1]}`);
  }
  return [...pairs];
}

describe("diagnostics text", () => {
  it("has a sentence for every result the backend can send", () => {
    const pairs = backendPairs();
    expect(pairs.length).toBeGreaterThan(10);
    for (const pair of pairs) {
      const [id, reason] = pair.split(".");
      const text = diagnosticText({ id, reason, status: "ok" }, t);
      expect(text.title, pair).not.toBe(`${id}: ${reason}`);
    }
  });

  it("every warning or problem says what to do", () => {
    for (const pair of backendPairs()) {
      const [id, reason] = pair.split(".");
      const text = diagnosticText({ id, reason, status: "warn" }, t);
      const okReasons = ["found", "writable", "reachable", "none", "held", "free", "clientOpen"];
      if (!okReasons.includes(reason)) {
        expect(text.detail, pair).not.toBe("");
      }
    }
  });

  it("puts the count of stuck processes in the title", () => {
    const text = diagnosticText({ id: "stuckProcesses", reason: "stuck", status: "warn", count: 3 }, t);
    expect(text.title).toContain("3");
    expect(text.detail).toMatch(/doesn't close them/);
  });

  it("never tells the user that MultiAlt will close something", () => {
    for (const pair of backendPairs()) {
      const [id, reason] = pair.split(".");
      const { title, detail } = diagnosticText({ id, reason, status: "warn", count: 1 }, t);
      expect(`${title} ${detail}`).not.toMatch(/MultiAlt (will )?close[sd]? /i);
    }
  });

  it("falls back to the raw id for something unknown instead of crashing", () => {
    expect(diagnosticText({ id: "new", reason: "thing", status: "ok" }, t).title).toBe("new: thing");
  });

  it("the overall status is the worst one", () => {
    const ok: DiagnosticCheck = { id: "a", reason: "x", status: "ok" };
    const warn: DiagnosticCheck = { id: "b", reason: "x", status: "warn" };
    const problem: DiagnosticCheck = { id: "c", reason: "x", status: "problem" };
    expect(overallStatus([])).toBe("ok");
    expect(overallStatus([ok, warn])).toBe("warn");
    expect(overallStatus([warn, problem, ok])).toBe("problem");
  });

  it("summarises each check in one plain line for the problem report", () => {
    expect(
      diagnosticsSummaryLines([
        { id: "internet", reason: "reachable", status: "ok" },
        { id: "stuckProcesses", reason: "stuck", status: "warn", count: 2 },
      ])
    ).toEqual(["- internet: ok (reachable)", "- stuckProcesses: warn (stuck, 2)"]);
  });
});
