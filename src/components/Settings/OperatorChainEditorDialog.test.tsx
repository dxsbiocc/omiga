import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  HookRuntimeApi,
  RenderedNode,
} from "../../test/__tests__/componentHarness";
import {
  ComponentHarness,
  createHookRuntime,
  findAllNodes,
  installComponentTestWindow,
  textContent,
} from "../../test/__tests__/componentHarness";
import type { OperatorSummary } from "../../state/pluginStore";
import { OperatorChainEditorDialog } from "./OperatorChainEditorDialog";

const hookRuntimeRef = vi.hoisted(() => ({
  current: null as HookRuntimeApi | null,
}));

vi.mock("react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react")>();
  return {
    ...actual,
    useEffect: (effect: () => void | (() => void), deps?: readonly unknown[]) =>
      hookRuntimeRef.current?.useEffect(effect, deps),
    useMemo: <T,>(factory: () => T, deps?: readonly unknown[]) =>
      hookRuntimeRef.current?.useMemo(factory, deps),
    useRef: <T,>(initial: T) => hookRuntimeRef.current?.useRef(initial),
    useState: <T,>(initial: T | (() => T)) =>
      hookRuntimeRef.current?.useState(initial),
  };
});

vi.mock("@mui/material", async () => {
  const { createMuiMaterialMock } = await import("../../test/__tests__/muiMocks");
  return createMuiMaterialMock();
});

vi.mock("@mui/material/styles", async () => {
  const { createMuiStylesMock } = await import("../../test/__tests__/muiMocks");
  return createMuiStylesMock();
});

vi.mock("@mui/icons-material", async () => {
  const { createIconMock } = await import("../../test/__tests__/muiMocks");
  return createIconMock([
    "AccountTreeRounded",
    "AddRounded",
    "ArrowDownwardRounded",
    "ArrowUpwardRounded",
    "CloseRounded",
    "DeleteOutlineRounded",
  ]);
});

const operators: OperatorSummary[] = [
  {
    id: "align_reads",
    version: "1.0.0",
    name: "Align reads",
    description: "Align reads to a reference.",
    sourcePlugin: "analysis@local",
    manifestPath: "/plugins/analysis/operators/align_reads/operator.yaml",
    enabledAliases: ["align_reads"],
    exposed: true,
    unavailableReason: null,
    smokeTests: [],
    interface: {
      inputs: {
        reads: {
          kind: "string",
          required: true,
          description: "Input FASTQ path.",
        },
      },
      params: {
        threads: {
          kind: "integer",
          default: 4,
        },
      },
    },
  },
  {
    id: "trim_reads",
    version: "1.0.0",
    name: "Trim reads",
    description: "Trim adapters.",
    sourcePlugin: "analysis@local",
    manifestPath: "/plugins/analysis/operators/trim_reads/operator.yaml",
    enabledAliases: ["trim_reads"],
    exposed: true,
    unavailableReason: null,
    smokeTests: [],
    interface: {
      inputs: {
        reads: {
          kind: "string",
          required: true,
        },
      },
    },
  },
];

const createDialogHarness = () => {
  installComponentTestWindow();
  const runtime = createHookRuntime();
  hookRuntimeRef.current = runtime;
  const onClose = vi.fn();
  const onRun = vi.fn().mockResolvedValue(undefined);
  const harness = new ComponentHarness(
    runtime,
    (): ReactElement => (
      <OperatorChainEditorDialog
        open
        onClose={onClose}
        operators={operators}
        onRun={onRun}
      />
    ),
  );
  harness.render();
  return { harness, onClose, onRun };
};

const nodeLabel = (node: RenderedNode): string | undefined =>
  typeof node.props.label === "string" ? node.props.label : undefined;

const buttons = (harness: ComponentHarness): RenderedNode[] =>
  findAllNodes(harness.tree, (node) => node.type === "button");

const getButtonByText = (harness: ComponentHarness, label: string): RenderedNode => {
  const button = buttons(harness).find((node) => textContent(node).includes(label));
  if (!button) throw new Error(`Unable to find button containing "${label}".`);
  return button;
};

const getButtonByAriaLabel = (harness: ComponentHarness, label: string): RenderedNode => {
  const button = buttons(harness).find((node) => node.props["aria-label"] === label);
  if (!button) throw new Error(`Unable to find button labelled "${label}".`);
  return button;
};

const controlsByLabel = (harness: ComponentHarness, label: string): RenderedNode[] =>
  findAllNodes(
    harness.tree,
    (node) =>
      (node.type === "input" || node.type === "select")
      && nodeLabel(node) === label,
  );

const getControlByLabel = (harness: ComponentHarness, label: string): RenderedNode => {
  const [control] = controlsByLabel(harness, label);
  if (!control) throw new Error(`Unable to find control labelled "${label}".`);
  return control;
};

const getLastControlByLabel = (harness: ComponentHarness, label: string): RenderedNode => {
  const controls = controlsByLabel(harness, label);
  const control = controls.at(-1);
  if (!control) throw new Error(`Unable to find control labelled "${label}".`);
  return control;
};

const stepChips = (harness: ComponentHarness): RenderedNode[] =>
  findAllNodes(
    harness.tree,
    (node) =>
      node.type === "chip"
      && typeof node.props.label === "string"
      && /^Step \d+$/.test(node.props.label),
  );

let consoleErrorSpy: ReturnType<typeof vi.spyOn> | null = null;

beforeEach(() => {
  const originalError = console.error;
  consoleErrorSpy = vi.spyOn(console, "error").mockImplementation((message, ...args) => {
    if (
      typeof message === "string"
      && message.includes('A props object containing a "key" prop')
    ) {
      return;
    }
    originalError(message, ...args);
  });
});

afterEach(() => {
  hookRuntimeRef.current?.cleanup();
  hookRuntimeRef.current = null;
  consoleErrorSpy?.mockRestore();
  consoleErrorSpy = null;
  vi.clearAllMocks();
});

describe("OperatorChainEditorDialog", () => {
  it("renders the empty step state and add affordance when opened", () => {
    const { harness } = createDialogHarness();

    const emptyRows = findAllNodes(
      harness.tree,
      (node) => node.type === "paper" && textContent(node).includes("No steps yet."),
    );
    expect(textContent(harness.tree)).toContain("No steps yet.");
    expect(textContent(harness.tree)).toContain("0 steps");
    expect(emptyRows).toHaveLength(1);
    expect(stepChips(harness)).toHaveLength(0);
    expect(getButtonByText(harness, "Add step").props.disabled).toBeFalsy();
    expect(getButtonByText(harness, "Run chain").props.disabled).toBe(true);
  });

  it("adds and removes visible chain steps", () => {
    const { harness } = createDialogHarness();

    harness.click(getButtonByText(harness, "Add step"));
    expect(stepChips(harness)).toHaveLength(1);

    harness.click(getButtonByText(harness, "Add step"));
    expect(stepChips(harness)).toHaveLength(2);

    harness.click(getButtonByAriaLabel(harness, "Remove step 2"));
    expect(stepChips(harness)).toHaveLength(1);
    expect(textContent(harness.tree)).toContain("Step 1");
  });

  it("keeps Run chain disabled until required inputs are complete and sends the payload", () => {
    const { harness, onRun } = createDialogHarness();

    expect(getButtonByText(harness, "Run chain").props.disabled).toBe(true);

    harness.click(getButtonByText(harness, "Add step"));
    expect(getButtonByText(harness, "Run chain").props.disabled).toBe(true);

    harness.change(getControlByLabel(harness, "reads"), "/data/sample.fastq");
    expect(getButtonByText(harness, "Run chain").props.disabled).toBe(false);

    harness.click(getButtonByText(harness, "Run chain"));

    expect(onRun).toHaveBeenCalledWith([
      {
        alias: "align_reads",
        arguments: {
          inputs: { reads: "/data/sample.fastq" },
          params: { threads: 4 },
          resources: {},
        },
      },
    ]);
  });

  it("reorders steps with the move buttons", () => {
    const { harness, onRun } = createDialogHarness();

    harness.click(getButtonByText(harness, "Add step"));
    harness.change(getLastControlByLabel(harness, "reads"), "first.fastq");
    harness.click(getButtonByText(harness, "Add step"));
    harness.change(getLastControlByLabel(harness, "reads"), "second.fastq");

    harness.click(getButtonByAriaLabel(harness, "Move step 1 down"));
    harness.click(getButtonByText(harness, "Run chain"));

    expect(onRun).toHaveBeenCalledWith([
      expect.objectContaining({
        arguments: expect.objectContaining({
          inputs: { reads: "second.fastq" },
        }),
      }),
      expect.objectContaining({
        arguments: expect.objectContaining({
          inputs: { reads: "first.fastq" },
        }),
      }),
    ]);
  });

  it("inserts a prior step output placeholder into the focused input", () => {
    const { harness } = createDialogHarness();

    harness.click(getButtonByText(harness, "Add step"));
    harness.change(getLastControlByLabel(harness, "reads"), "first.fastq");
    harness.click(getButtonByText(harness, "Add step"));
    harness.focus(getLastControlByLabel(harness, "reads"));

    const outputSelector = getLastControlByLabel(harness, "Use output from");
    expect(outputSelector.props.disabled).toBeFalsy();

    harness.change(outputSelector, 0);

    expect(getLastControlByLabel(harness, "reads").props.value).toBe(
      "{{step1.outputDir}}",
    );
  });
});
