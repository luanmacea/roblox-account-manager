import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";

vi.mock("../../store", async () => (await import("../../test-utils/renderWithStore")).storeModuleMock());
vi.mock("@tauri-apps/api/core", async () => (await import("../../test-utils/tauriMocks")).tauriCoreMock());

import { QUICK_LOGIN_POLL_MS, QuickLoginDialog } from "./QuickLoginDialog";
import { setStore } from "../../test-utils/renderWithStore";
import { invokeMock, resetTauriMocks, setInvokeHandler } from "../../test-utils/tauriMocks";

let polls: unknown[] = [];

function backend(startError?: string) {
  setInvokeHandler((cmd) => {
    switch (cmd) {
      case "add_by_quick_login_start":
        if (startError) throw startError;
        return { code: "ABC123", expiresAt: null };
      case "add_by_quick_login_poll":
        return polls.length > 1 ? polls.shift() : polls[0];
      case "add_by_quick_login_cancel":
        return null;
      default:
        return undefined;
    }
  });
}

async function tick(ms = QUICK_LOGIN_POLL_MS) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

beforeEach(() => {
  resetTauriMocks();
  vi.useFakeTimers();
  polls = [{ kind: "pending" }];
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("QuickLoginDialog (idea 12)", () => {
  it("shows the code and how to approve it, then waits", async () => {
    setStore({});
    backend();
    render(<QuickLoginDialog open onClose={() => {}} />);
    await tick(0);
    expect(screen.getByTestId("quick-login-code")).toHaveTextContent("ABC123");
    expect(screen.getByText(/roblox\.com\/crossdevicelogin/)).toBeInTheDocument();
    expect(screen.getByText("Waiting for the code...")).toBeInTheDocument();
    await tick();
    expect(invokeMock.mock.calls.some((c) => c[0] === "add_by_quick_login_poll")).toBe(true);
  });

  it("says when the code was entered and for which account", async () => {
    setStore({});
    polls = [{ kind: "linked", accountName: "SomeUser" }];
    backend();
    render(<QuickLoginDialog open onClose={() => {}} />);
    await tick(0);
    await tick();
    expect(screen.getByText(/Code entered for SomeUser/)).toBeInTheDocument();
  });

  it("adds the account through the store and closes once approved", async () => {
    const store = setStore({});
    const onClose = vi.fn();
    polls = [{ kind: "pending" }, { kind: "added", userId: 7, username: "NewAlt", alreadySaved: false }];
    backend();
    render(<QuickLoginDialog open onClose={onClose} />);
    await tick(0);
    await tick();
    await tick();
    expect(store.loadAccounts).toHaveBeenCalled();
    expect(store.addToast).toHaveBeenCalledWith("Added NewAlt", "success");
    expect(onClose).toHaveBeenCalled();
  });

  it("offers a new code when the sign-in was cancelled or expired", async () => {
    setStore({});
    polls = [{ kind: "cancelled" }];
    backend();
    render(<QuickLoginDialog open onClose={() => {}} />);
    await tick(0);
    await tick();
    expect(screen.getByText("The sign-in was cancelled on the other device.")).toBeInTheDocument();
    polls = [{ kind: "pending" }];
    await act(async () => {
      screen.getByRole("button", { name: /Get a new code/ }).click();
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(invokeMock.mock.calls.filter((c) => c[0] === "add_by_quick_login_start")).toHaveLength(2);
  });

  it("shows Roblox's error (a captcha it won't solve, for example) instead of retrying", async () => {
    setStore({});
    polls = [];
    backend();
    setInvokeHandler((cmd) => {
      if (cmd === "add_by_quick_login_start") return { code: "ABC123" };
      if (cmd === "add_by_quick_login_poll") throw "Roblox asked for an extra check (like a CAPTCHA) to finish this sign-in";
      return null;
    });
    render(<QuickLoginDialog open onClose={() => {}} />);
    await tick(0);
    await tick();
    expect(screen.getByRole("alert")).toHaveTextContent(/CAPTCHA/);
    const pollsBefore = invokeMock.mock.calls.filter((c) => c[0] === "add_by_quick_login_poll").length;
    await tick();
    await tick();
    expect(invokeMock.mock.calls.filter((c) => c[0] === "add_by_quick_login_poll")).toHaveLength(pollsBefore);
  });

  it("cancels the login on the backend when closed", async () => {
    setStore({});
    backend();
    const { rerender } = render(<QuickLoginDialog open onClose={() => {}} />);
    await tick(0);
    rerender(<QuickLoginDialog open={false} onClose={() => {}} />);
    expect(invokeMock.mock.calls.some((c) => c[0] === "add_by_quick_login_cancel")).toBe(true);
  });
});
