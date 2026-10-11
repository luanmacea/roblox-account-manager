import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useEffect } from "react";

vi.mock("../../store", async () => (await import("../../test-utils/renderWithStore")).storeModuleMock());
vi.mock("@tauri-apps/api/core", async () => (await import("../../test-utils/tauriMocks")).tauriCoreMock());

import { MiscellaneousTab } from "./MiscellaneousTab";
import { useSettings } from "../../hooks/useSettings";
import { setStore } from "../../test-utils/renderWithStore";
import { invokeMock, resetTauriMocks, setInvokeHandler } from "../../test-utils/tauriMocks";

let stored: Record<string, Record<string, string>> = {};

function Harness() {
  const s = useSettings();
  useEffect(() => {
    void s.load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  if (!s.loaded) return <div>Loading settings...</div>;
  return <MiscellaneousTab s={s} />;
}

beforeEach(() => {
  resetTauriMocks();
  stored = {};
  setInvokeHandler((cmd) => {
    if (cmd === "get_all_settings") return stored;
    if (cmd === "remembered_unlock_state") return { supported: false, active: false, defaultHours: 24 };
    return undefined;
  });
});
afterEach(cleanup);

describe("Settings › Misc › Lock after inactivity (idea 27)", () => {
  it("is off by default and can't be turned on without an app password", async () => {
    setStore({ accountsEncrypted: false });
    render(<Harness />);
    const toggle = await screen.findByRole("switch", { name: "Lock after inactivity" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(/Needs an app password/)).toBeInTheDocument();
    await userEvent.click(toggle);
    expect(invokeMock).not.toHaveBeenCalledWith("update_setting", expect.objectContaining({ key: "LockOnInactivity" }));
  });

  it("with an app password it saves the switch", async () => {
    setStore({ accountsEncrypted: true });
    render(<Harness />);
    const toggle = await screen.findByRole("switch", { name: "Lock after inactivity" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await userEvent.click(toggle);
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("update_setting", {
        section: "General",
        key: "LockOnInactivity",
        value: "true",
      })
    );
    expect(screen.getByText(/keep running/)).toBeInTheDocument();
  });
});
