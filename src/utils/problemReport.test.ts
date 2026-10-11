import { describe, expect, it } from "vitest";
import type { Account } from "../types";
import type { LaunchLogEntry } from "../store";
import { anonymizeContextFor, buildProblemReport, REPORT_LOG_LINES } from "./problemReport";

function account(id: number, name: string, extra: Partial<Account> = {}): Account {
  return {
    Valid: true,
    SecurityToken: `_|WARNING:-DO-NOT-SHARE-THIS.|_COOKIE${id}xxxxxxxxxxxxxxxx`,
    Username: name,
    LastUse: "",
    Alias: "",
    Description: "",
    Password: "",
    Group: "",
    UserID: id,
    Fields: {},
    LastAttemptedRefresh: "",
    BrowserTrackerID: "",
    ...extra,
  };
}

function log(id: number, userId: number | null, message: string, level: LaunchLogEntry["level"] = "info"): LaunchLogEntry {
  return { id, userId, level, step: "launch", message, ts: new Date(2026, 9, 11, 10, 0, id).getTime() };
}

const ENV = { version: "1.7.0", edition: "standard", os: "Windows 11 Home (24H2, build 26200)" };

describe("problem report", () => {
  it("lists app version, edition, OS, account count, the check and the console", () => {
    const report = buildProblemReport({
      env: ENV,
      accounts: [account(111111, "MainGuy"), account(222222, "AltGirl")],
      diagnostics: [{ id: "internet", status: "problem", reason: "unreachable" }],
      logs: [log(1, 222222, "Opening the game", "info"), log(2, null, "Queue finished", "success")],
    });
    expect(report).toContain("MultiAlt 1.7.0 (standard edition)");
    expect(report).toContain("Windows 11 Home (24H2, build 26200)");
    expect(report).toContain("Accounts saved: 2");
    expect(report).toContain("- internet: problem (unreachable)");
    // A conta vira um número estável, pela ordem da lista.
    expect(report).toContain("10:00:01 [info] [launch] account 2: Opening the game");
    expect(report).toContain("10:00:02 [success] [launch] Queue finished");
  });

  it("never carries a name, ID, cookie or password of a saved account", () => {
    const accounts = [
      account(123456789, "SecretMain", { Alias: "My Main", Password: "Pa55word!" }),
      account(987654321, "SecretAlt"),
    ];
    const report = buildProblemReport({
      env: ENV,
      accounts,
      diagnostics: null,
      logs: [
        log(1, 123456789, "SecretMain (123456789) failed: Invalid cookie for My Main"),
        log(2, 987654321, `Cookie was ${accounts[1].SecurityToken}`),
        log(3, null, "password Pa55word! rejected"),
      ],
    });
    for (const leak of ["SecretMain", "SecretAlt", "My Main", "123456789", "987654321", "COOKIE1", "COOKIE9", "Pa55word!"]) {
      expect(report, leak).not.toContain(leak);
    }
  });

  it("keeps only the last lines of the console", () => {
    const logs = Array.from({ length: REPORT_LOG_LINES + 25 }, (_, i) => log(i, null, `line ${i}`));
    const report = buildProblemReport({ env: ENV, accounts: [], diagnostics: null, logs });
    expect(report).toContain(`Last ${REPORT_LOG_LINES} console line(s):`);
    expect(report).not.toContain("line 0\n");
    expect(report).toContain(`line ${REPORT_LOG_LINES + 24}`);
  });

  it("says when the check did not run and the console is empty", () => {
    const report = buildProblemReport({ env: null, accounts: [], diagnostics: null, logs: [] });
    expect(report).toContain("- (not run)");
    expect(report).toContain("(empty)");
    expect(report).not.toContain("MultiAlt undefined");
  });

  it("collects names, IDs and secrets of every account for the anonymiser", () => {
    const ctx = anonymizeContextFor([account(5, "Abc", { Alias: "Xyz", Password: "pw1234" })]);
    expect(ctx.accountNames).toEqual(["Abc", "Xyz"]);
    expect(ctx.userIds).toEqual([5]);
    expect(ctx.secrets).toContain("pw1234");
    expect(ctx.secrets?.some((s) => s.includes("COOKIE5"))).toBe(true);
  });
});
