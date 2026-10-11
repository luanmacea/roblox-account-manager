import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@tauri-apps/api/core", async () => (await import("../../test-utils/tauriMocks")).tauriCoreMock());

import { FeedbackDialog, type ReportSource } from "./FeedbackDialog";
import { useEscapeStack } from "../../hooks/useEscapeStack";
import { invokeMock, resetTauriMocks, setInvokeMap } from "../../test-utils/tauriMocks";
import type { Account } from "../../types";

const COOKIE = "_|WARNING:-DO-NOT-SHARE-THIS.|_SECRETCOOKIEVALUE0123456789abcdefABCDEF";

function account(id: number, name: string): Account {
  return {
    Valid: true,
    SecurityToken: COOKIE + id,
    Username: name,
    LastUse: "",
    Alias: "",
    Description: "",
    Password: "hunter22",
    Group: "",
    UserID: id,
    Fields: {},
    LastAttemptedRefresh: "",
    BrowserTrackerID: "",
  };
}

const SOURCE: ReportSource = {
  accounts: [account(424242, "VerySecretName")],
  logs: [
    {
      id: 1,
      userId: 424242,
      level: "error",
      step: "launch",
      message: "VerySecretName failed at C:\\Users\\winuser\\AppData with job 0f8fad5b-d9cb-469f-a165-70867728950e",
      ts: Date.now(),
    },
  ],
};

async function openReportStep() {
  await userEvent.click(screen.getByRole("button", { name: /Report a problem/ }));
}

describe("FeedbackDialog", () => {
  beforeEach(() => {
    resetTauriMocks();
    setInvokeMap({
      get_report_environment: { version: "1.7.0", edition: "standard", os: "Windows 11" },
      run_launch_diagnostics: [{ id: "internet", status: "ok", reason: "reachable" }],
    });
  });
  afterEach(() => cleanup());

  it("opens the bug form by kind only, never by URL, and closes", async () => {
    const onClose = vi.fn();
    render(<FeedbackDialog open onClose={onClose} />);

    await openReportStep();
    await userEvent.click(screen.getByRole("button", { name: /Open the form/ }));

    expect(invokeMock).toHaveBeenCalledWith("open_feedback_form", { kind: "bug" });
    expect(onClose).toHaveBeenCalled();
  });

  it("opens the idea form", async () => {
    render(<FeedbackDialog open onClose={() => {}} />);

    await userEvent.click(screen.getByRole("button", { name: /Suggest an idea/ }));

    expect(invokeMock).toHaveBeenCalledWith("open_feedback_form", { kind: "idea" });
  });

  it("says the app sends nothing on its own and a GitHub account is needed", () => {
    render(<FeedbackDialog open onClose={() => {}} />);

    const dialog = screen.getByRole("dialog", { name: "Send feedback" });
    expect(dialog).toHaveTextContent("The app sends nothing on its own");
    expect(dialog).toHaveTextContent("free GitHub account");
  });

  it("builds nothing until the summary box is ticked", async () => {
    render(<FeedbackDialog open onClose={() => {}} reportSource={() => SOURCE} />);
    await openReportStep();

    expect(screen.getByRole("checkbox", { name: /Add a diagnostic summary/ })).not.toBeChecked();
    expect(screen.queryByRole("textbox", { name: "Summary preview" })).not.toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("shows the anonymised summary before anything is copied", async () => {
    render(<FeedbackDialog open onClose={() => {}} reportSource={() => SOURCE} />);
    await openReportStep();
    await userEvent.click(screen.getByRole("checkbox", { name: /Add a diagnostic summary/ }));

    const preview = screen.getByRole("textbox", { name: "Summary preview" }) as HTMLTextAreaElement;
    await waitFor(() => expect(preview.value).toContain("MultiAlt 1.7.0 (standard edition)"));
    expect(preview.value).toContain("- internet: ok (reachable)");
    for (const leak of ["VerySecretName", "424242", "winuser", "0f8fad5b", "SECRETCOOKIEVALUE", "hunter22"]) {
      expect(preview.value, leak).not.toContain(leak);
    }
    // Montar o resumo só lê: ambiente e checagem do launch, nada mais.
    const commands = invokeMock.mock.calls.map((c) => c[0]);
    expect(new Set(commands)).toEqual(new Set(["get_report_environment", "run_launch_diagnostics"]));
  });

  it("copies exactly the text in the preview, only on the click", async () => {
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<FeedbackDialog open onClose={() => {}} reportSource={() => SOURCE} />);
    await openReportStep();
    await userEvent.click(screen.getByRole("checkbox", { name: /Add a diagnostic summary/ }));
    const preview = screen.getByRole("textbox", { name: "Summary preview" }) as HTMLTextAreaElement;
    await waitFor(() => expect(preview.value).toContain("MultiAlt"));

    expect(writeText).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: /Copy summary/ }));

    expect(writeText).toHaveBeenCalledWith(preview.value);
    expect(await screen.findByRole("button", { name: /Copied/ })).toBeInTheDocument();
    // Copiar não abre nada nem manda nada.
    expect(invokeMock).not.toHaveBeenCalledWith("open_feedback_form", expect.anything());
  });

  it("does not rebuild the summary in a loop when the source function changes every render", async () => {
    const { rerender } = render(<FeedbackDialog open onClose={() => {}} reportSource={() => SOURCE} />);
    await openReportStep();
    await userEvent.click(screen.getByRole("checkbox", { name: /Add a diagnostic summary/ }));
    await waitFor(() =>
      expect((screen.getByRole("textbox", { name: "Summary preview" }) as HTMLTextAreaElement).value).toContain("MultiAlt")
    );
    rerender(<FeedbackDialog open onClose={() => {}} reportSource={() => SOURCE} />);
    rerender(<FeedbackDialog open onClose={() => {}} reportSource={() => SOURCE} />);
    expect(invokeMock.mock.calls.filter((c) => c[0] === "run_launch_diagnostics")).toHaveLength(1);
  });

  it("goes back to the two choices", async () => {
    render(<FeedbackDialog open onClose={() => {}} reportSource={() => SOURCE} />);
    await openReportStep();
    await userEvent.click(screen.getByRole("button", { name: /Back/ }));
    expect(screen.getByRole("button", { name: /Suggest an idea/ })).toBeInTheDocument();
  });

  it("closes with Escape", async () => {
    const onClose = vi.fn();
    render(<FeedbackDialog open onClose={onClose} />);

    await userEvent.keyboard("{Escape}");

    expect(onClose).toHaveBeenCalled();
  });

  it("Escape closes only the dialog, not the page behind it", async () => {
    // A página atrás (ex.: Grupos) também volta para a lista de contas com Esc;
    // com o diálogo aberto, o Esc é só dele (achado no teste real de 08/10/2026).
    const pageEscape = vi.fn();
    const onClose = vi.fn();
    function PageWithDialog({ open }: { open: boolean }) {
      useEscapeStack(true, pageEscape);
      return <FeedbackDialog open={open} onClose={onClose} />;
    }
    // A página já estava montada; o diálogo abre depois, como no app.
    const { rerender } = render(<PageWithDialog open={false} />);
    rerender(<PageWithDialog open />);

    await userEvent.keyboard("{Escape}");

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(pageEscape).not.toHaveBeenCalled();
  });

  it("renders nothing when closed", () => {
    render(<FeedbackDialog open={false} onClose={() => {}} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
