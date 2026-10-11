import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook } from "@testing-library/react";
import { INACTIVITY_CHECK_MS, useInactivityLock } from "./useInactivityLock";

describe("useInactivityLock", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("locks after the minutes pass without a click or key in the window", () => {
    const onLock = vi.fn();
    renderHook(() => useInactivityLock(true, 2, false, onLock));
    vi.advanceTimersByTime(2 * 60_000 - INACTIVITY_CHECK_MS);
    expect(onLock).not.toHaveBeenCalled();
    vi.advanceTimersByTime(INACTIVITY_CHECK_MS);
    expect(onLock).toHaveBeenCalled();
  });

  it("any interaction restarts the count", () => {
    const onLock = vi.fn();
    renderHook(() => useInactivityLock(true, 2, false, onLock));
    vi.advanceTimersByTime(90_000);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "a" }));
    vi.advanceTimersByTime(90_000);
    expect(onLock).not.toHaveBeenCalled();
    window.dispatchEvent(new Event("pointerdown"));
    vi.advanceTimersByTime(90_000);
    expect(onLock).not.toHaveBeenCalled();
    vi.advanceTimersByTime(40_000);
    expect(onLock).toHaveBeenCalled();
  });

  it("does nothing while off or already locked", () => {
    const onLock = vi.fn();
    renderHook(() => useInactivityLock(false, 1, false, onLock));
    renderHook(() => useInactivityLock(true, 1, true, onLock));
    vi.advanceTimersByTime(10 * 60_000);
    expect(onLock).not.toHaveBeenCalled();
  });

  it("stops watching when unmounted", () => {
    const onLock = vi.fn();
    const { unmount } = renderHook(() => useInactivityLock(true, 1, false, onLock));
    unmount();
    vi.advanceTimersByTime(10 * 60_000);
    expect(onLock).not.toHaveBeenCalled();
  });
});
