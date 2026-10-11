import { describe, expect, it } from "vitest";
import {
  idleLongEnough,
  inactivityLockActive,
  LOCK_MINUTES_DEFAULT,
  LOCK_MINUTES_MAX,
  LOCK_MINUTES_MIN,
  lockErrorText,
  normalizeLockMinutes,
} from "./inactivityLock";

describe("inactivity lock", () => {
  it("is off by default and needs both the setting and an app password", () => {
    expect(inactivityLockActive(undefined, true)).toBe(false);
    expect(inactivityLockActive("false", true)).toBe(false);
    expect(inactivityLockActive("true", false)).toBe(false);
    expect(inactivityLockActive("true", null)).toBe(false);
    expect(inactivityLockActive("true", true)).toBe(true);
  });

  it("reads the minutes from the INI within limits", () => {
    expect(normalizeLockMinutes(undefined)).toBe(LOCK_MINUTES_DEFAULT);
    expect(normalizeLockMinutes("")).toBe(LOCK_MINUTES_DEFAULT);
    expect(normalizeLockMinutes("abc")).toBe(LOCK_MINUTES_DEFAULT);
    expect(normalizeLockMinutes("15")).toBe(15);
    expect(normalizeLockMinutes("0")).toBe(LOCK_MINUTES_MIN);
    expect(normalizeLockMinutes(-5)).toBe(LOCK_MINUTES_MIN);
    expect(normalizeLockMinutes("99999")).toBe(LOCK_MINUTES_MAX);
  });

  it("locks only once the full time passed without activity", () => {
    const start = 1_000_000;
    expect(idleLongEnough(start, start + 4 * 60_000, 5)).toBe(false);
    expect(idleLongEnough(start, start + 5 * 60_000 - 1, 5)).toBe(false);
    expect(idleLongEnough(start, start + 5 * 60_000, 5)).toBe(true);
  });

  it("translates the backend errors and keeps anything else as is", () => {
    const t = (text: string, o?: Record<string, unknown>) => `T:${text.replace("{{seconds}}", String(o?.seconds ?? ""))}`;
    expect(lockErrorText("Wrong password.", t)).toBe("T:Wrong password.");
    expect(lockErrorText("Too many wrong passwords. Wait 20 seconds and try again.", t)).toBe(
      "T:Too many wrong passwords. Wait 20 seconds and try again."
    );
    expect(lockErrorText("Failed to read account file: x", t)).toBe("Failed to read account file: x");
  });
});
