import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@tauri-apps/api/core", async () => (await import("../../test-utils/tauriMocks")).tauriCoreMock());

import { DiagnosticsDialog } from "./DiagnosticsDialog";
import { invokeMock, resetTauriMocks, setInvokeHandler, setInvokeMap } from "../../test-utils/tauriMocks";

const RESULTS = [
  { id: "robloxInstall", status: "ok", reason: "found" },
  { id: "dataFolder", status: "ok", reason: "writable" },
  { id: "internet", status: "problem", reason: "unreachable" },
  { id: "stuckProcesses", status: "warn", reason: "stuck", count: 2 },
  { id: "multiRoblox", status: "ok", reason: "held" },
];

describe("DiagnosticsDialog", () => {
  beforeEach(() => resetTauriMocks());
  afterEach(() => cleanup());

  it("runs the check when it opens and shows one line per result with what to do", async () => {
    setInvokeMap({ run_launch_diagnostics: RESULTS });
    render(<DiagnosticsDialog open onClose={() => {}} />);

    const list = await screen.findByRole("list", { name: "Check results" });
    expect(list.querySelectorAll("li")).toHaveLength(RESULTS.length);
    expect(screen.getByText("Roblox can't be reached")).toBeInTheDocument();
    expect(screen.getByText(/Check your internet connection/)).toBeInTheDocument();
    expect(screen.getByText("2 Roblox process(es) running with no window")).toBeInTheDocument();
    expect(screen.getByText("Found a problem that stops launches.")).toBeInTheDocument();
  });

  it("only ever calls the read-only check, never a command that closes something", async () => {
    setInvokeMap({ run_launch_diagnostics: RESULTS });
    render(<DiagnosticsDialog open onClose={() => {}} />);
    await screen.findByRole("list", { name: "Check results" });
    await userEvent.click(screen.getByRole("button", { name: /Check again/ }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(2));
    for (const call of invokeMock.mock.calls) {
      expect(call[0]).toBe("run_launch_diagnostics");
    }
  });

  it("says when nothing is wrong", async () => {
    setInvokeMap({ run_launch_diagnostics: [{ id: "internet", status: "ok", reason: "reachable" }] });
    render(<DiagnosticsDialog open onClose={() => {}} />);
    expect(await screen.findByText("Nothing wrong found.")).toBeInTheDocument();
  });

  it("shows the error when the check itself fails", async () => {
    setInvokeHandler(() => {
      throw new Error("boom");
    });
    render(<DiagnosticsDialog open onClose={() => {}} />);
    expect(await screen.findByText(/The check could not run/)).toBeInTheDocument();
  });

  it("closes with Escape and renders nothing when closed", async () => {
    setInvokeMap({ run_launch_diagnostics: [] });
    const onClose = vi.fn();
    const { rerender } = render(<DiagnosticsDialog open onClose={onClose} />);
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
    rerender(<DiagnosticsDialog open={false} onClose={onClose} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
