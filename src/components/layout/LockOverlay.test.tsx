import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("../../store", async () => (await import("../../test-utils/renderWithStore")).storeModuleMock());
vi.mock("@tauri-apps/api/core", async () => (await import("../../test-utils/tauriMocks")).tauriCoreMock());
vi.mock("@tauri-apps/api/window", async () => (await import("../../test-utils/tauriMocks")).tauriWindowMock());

import { LockOverlay } from "./LockOverlay";
import { defaultSettings, setStore } from "../../test-utils/renderWithStore";
import { invokeMock, resetTauriMocks } from "../../test-utils/tauriMocks";
import type { StoreValue } from "../../store";

function renderLock(overrides: Partial<StoreValue> = {}) {
  const settings = defaultSettings();
  settings.General.RestrictedBackgroundStyle = "waves";
  const store = setStore({ settings, appLocked: true, ...overrides });
  render(<LockOverlay />);
  return store;
}

const passwordBox = () => screen.getByPlaceholderText("Password") as HTMLInputElement;

beforeEach(resetTauriMocks);
afterEach(cleanup);

describe("LockOverlay (locked after inactivity)", () => {
  it("says the app is locked and that what was started keeps running", () => {
    renderLock();
    expect(screen.getByRole("dialog", { name: "MultiAlt is locked" })).toBeInTheDocument();
    expect(screen.getByText(/everything you started keeps running/)).toBeInTheDocument();
  });

  it("only verifies the password: no vault reload and no 'keep me signed in'", async () => {
    const store = renderLock();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    await userEvent.type(passwordBox(), "hunter2{Enter}");
    expect(store.unlockApp).toHaveBeenCalledWith("hunter2");
    expect(store.unlock).not.toHaveBeenCalled();
    expect(invokeMock).not.toHaveBeenCalledWith("unlock_accounts", expect.anything());
  });

  it("shows the error of a wrong password and keeps the lock", async () => {
    renderLock({ unlockApp: vi.fn(async () => "Wrong password.") });
    await userEvent.type(passwordBox(), "nope{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("Wrong password.");
  });

  it("keys typed while locked never reach the window shortcuts behind", async () => {
    const behind = vi.fn();
    window.addEventListener("keydown", behind);
    try {
      renderLock();
      await userEvent.type(passwordBox(), "a{Escape}");
      // Tecla vinda de fora da tela de senha (foco perdido) também não passa.
      document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      await waitFor(() => expect(behind).not.toHaveBeenCalled());
    } finally {
      window.removeEventListener("keydown", behind);
    }
  });
});
