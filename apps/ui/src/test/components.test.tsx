import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import {
  DataTable,
  EmptyState,
  ErrorBoundary,
  GlassPanel,
  KeyValueList,
  LogStream,
  Modal,
  NeonButton,
  SectionHeader,
  Spinner,
  StatusPill,
  Tooltip,
} from "../design-system/index.js";

describe("design system", () => {
  it("renders status pills and buttons with states", async () => {
    const user = userEvent.setup();
    const onClick = () => {};
    render(
      <GlassPanel label="panel">
        <SectionHeader title="Header" subtitle="sub" />
        <StatusPill tone="ok">fine</StatusPill>
        <NeonButton onClick={onClick}>Launch</NeonButton>
        <NeonButton onClick={onClick} loading>
          Busy
        </NeonButton>
        <Spinner label="Waiting" />
      </GlassPanel>,
    );
    expect(screen.getByText("fine")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Launch" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Busy" })).toBeDisabled();
    expect(screen.getByText("Waiting…")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Launch" }));
  });

  it("renders empty state, key-values, and activity log", () => {
    render(
      <div>
        <EmptyState title="Nothing here" message="All clear" />
        <KeyValueList entries={[{ key: "version", value: "0.1.0" }]} />
        <LogStream
          events={[
            {
              id: "e1",
              at: "2026-10-31T12:00:00.000Z",
              actor: "broker",
              message: "ok",
              tone: "success",
            },
          ]}
        />
      </div>,
    );
    expect(screen.getByText("Nothing here")).toBeInTheDocument();
    expect(screen.getByText("0.1.0")).toBeInTheDocument();
    expect(screen.getByRole("log")).toBeInTheDocument();
  });

  it("renders a data table with rows", () => {
    render(
      <DataTable<{ id: string }>
        caption="rows"
        rows={[{ id: "a" }]}
        keyOf={(row) => row.id}
        columns={[{ header: "ID", render: (row) => row.id }]}
      />,
    );
    expect(screen.getByText("a")).toBeInTheDocument();
  });

  it("opens and closes a modal with escape", async () => {
    const user = userEvent.setup();
    function Harness() {
      return (
        <Modal title="Dialog" onClose={() => {}}>
          <p>content</p>
        </Modal>
      );
    }
    render(<Harness />);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await user.keyboard("{Escape}");
  });

  it("reveals tooltip label accessibly", () => {
    render(
      <Tooltip label="hint">
        <button type="button">hover me</button>
      </Tooltip>,
    );
    expect(screen.getByRole("tooltip", { hidden: true })).toBeInTheDocument();
  });

  it("error boundary recovers from panel crashes", () => {
    function Boom(): never {
      throw new Error("boom");
    }
    const spy = console.error;
    console.error = () => {};
    try {
      render(
        <ErrorBoundary>
          <Boom />
        </ErrorBoundary>,
      );
      expect(screen.getByRole("alert")).toBeInTheDocument();
    } finally {
      console.error = spy;
    }
  });
});
