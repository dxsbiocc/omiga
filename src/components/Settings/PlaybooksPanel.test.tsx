import type { ReactElement, ReactNode } from "react";
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
import type { Playbook, ReplayPlaybookResponse } from "../../state/playbookTypes";

type ReplayPlaybookArgs = {
  playbookId: string;
  projectRoot?: string;
  sessionId?: string;
  executionEnvironment?: string;
  sshServer?: string;
  sandboxBackend?: string;
};

const PROJECT_PATH = "/tmp/omiga-project";

const hookRuntimeRef = vi.hoisted(() => ({
  current: null as HookRuntimeApi | null,
}));

const playbookStoreMock = vi.hoisted(() => ({
  state: {
    playbooks: [] as Playbook[],
    isLoading: false,
    error: null as string | null,
  },
  listPlaybooks: vi.fn<(projectRoot?: string) => Promise<Playbook[]>>(),
  replayPlaybook: vi.fn<(args: ReplayPlaybookArgs) => Promise<ReplayPlaybookResponse>>(),
}));

vi.mock("react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react")>();
  return {
    ...actual,
    useEffect: (effect: () => void | (() => void), deps?: readonly unknown[]) =>
      hookRuntimeRef.current?.useEffect(effect, deps),
    useState: <T,>(initial: T | (() => T)) =>
      hookRuntimeRef.current?.useState(initial),
    useMemo: <T,>(factory: () => T) => factory(),
  };
});

vi.mock("@mui/material", async () => {
  const React = await import("react");
  const { createMuiMaterialMock } = await import("../../test/__tests__/muiMocks");

  return {
    ...createMuiMaterialMock(),
    Snackbar: ({
      children,
      open,
      ...props
    }: {
      children?: ReactNode;
      open?: boolean;
      [key: string]: unknown;
    }) => (open ? React.createElement("snackbar", props, children) : null),
  };
});

vi.mock("@mui/material/styles", async () => {
  const { createMuiStylesMock } = await import("../../test/__tests__/muiMocks");
  return createMuiStylesMock();
});

vi.mock("@mui/icons-material", async () => {
  const { createIconMock } = await import("../../test/__tests__/muiMocks");
  return createIconMock(["AddRounded", "PlayArrowRounded", "RefreshRounded"]);
});

vi.mock("../../state/playbookStore", () => ({
  usePlaybookStore: () => ({
    playbooks: playbookStoreMock.state.playbooks,
    isLoading: playbookStoreMock.state.isLoading,
    error: playbookStoreMock.state.error,
    listPlaybooks: playbookStoreMock.listPlaybooks,
    replayPlaybook: playbookStoreMock.replayPlaybook,
  }),
}));

const pluginStoreMock = vi.hoisted(() => ({
  operators: [] as unknown[],
  loadOperators: vi.fn<(projectRoot?: string) => Promise<void>>(),
  runOperatorChain: vi.fn<(...args: unknown[]) => Promise<unknown>>(),
}));

vi.mock("../../state/pluginStore", () => ({
  usePluginStore: <T,>(
    selector: (state: {
      operators: unknown[];
      loadOperators: typeof pluginStoreMock.loadOperators;
      runOperatorChain: typeof pluginStoreMock.runOperatorChain;
    }) => T,
  ): T =>
    selector({
      operators: pluginStoreMock.operators,
      loadOperators: pluginStoreMock.loadOperators,
      runOperatorChain: pluginStoreMock.runOperatorChain,
    }),
}));

vi.mock("../../state/chatComposerStore", () => ({
  useChatComposerStore: <T,>(
    selector: (state: {
      environment: string | null;
      sshServer: string | null;
      sandboxBackend: string | null;
    }) => T,
  ): T => selector({ environment: "local", sshServer: null, sandboxBackend: null }),
}));

vi.mock("../../state/sessionStore", () => ({
  useSessionStore: <T,>(
    selector: (state: { currentSession: { id: string } | null }) => T,
  ): T => selector({ currentSession: null }),
}));

vi.mock("./OperatorChainEditorDialog", () => ({
  OperatorChainEditorDialog: () => null,
}));

import { PlaybooksPanel } from "./PlaybooksPanel";

function playbook(overrides: Partial<Playbook> = {}): Playbook {
  return {
    playbookId: "pb-fastqc",
    title: "FastQC report",
    fingerprint: {
      canonicalId: "operator.fastqc",
      operatorVersion: "1.2.3",
      paramSchemaHash: "sha256:params",
      envSignature: null,
    },
    kind: "chain",
    canonicalId: "operator.fastqc",
    operatorVersion: "1.2.3",
    params: [],
    inputs: {},
    verification: {
      expectedStatus: "succeeded",
      expectedOutputKeys: ["report"],
    },
    provenance: {
      distilledFrom: ["proposal-1"],
      proposalId: "proposal-1",
      createdAt: "2026-05-01T00:00:00Z",
    },
    health: {
      hitCount: 5,
      successCount: 4,
      lastVerifiedAt: null,
      status: "active",
    },
    ...overrides,
  };
}

function resetPlaybookStoreMock() {
  playbookStoreMock.state.playbooks = [];
  playbookStoreMock.state.isLoading = false;
  playbookStoreMock.state.error = null;
  playbookStoreMock.listPlaybooks.mockReset().mockResolvedValue([]);
  playbookStoreMock.replayPlaybook.mockReset().mockResolvedValue({
    outcome: "replayed",
    verified: true,
  });
  pluginStoreMock.operators = [];
  pluginStoreMock.loadOperators.mockReset().mockResolvedValue(undefined);
  pluginStoreMock.runOperatorChain.mockReset().mockResolvedValue({ steps: [], ok: true });
}

function createPanelHarness() {
  installComponentTestWindow();
  const runtime = createHookRuntime();
  hookRuntimeRef.current = runtime;
  const harness = new ComponentHarness(
    runtime,
    (): ReactElement => <PlaybooksPanel projectPath={PROJECT_PATH} />,
  );
  harness.render();
  return harness;
}

function chips(harness: ComponentHarness): RenderedNode[] {
  return findAllNodes(harness.tree, (node) => node.type === "chip");
}

function buttons(harness: ComponentHarness): RenderedNode[] {
  return findAllNodes(harness.tree, (node) => node.type === "button");
}

function getButtonByText(harness: ComponentHarness, label: string): RenderedNode {
  const button = buttons(harness).find((node) => textContent(node).includes(label));
  if (!button) throw new Error(`Unable to find button containing "${label}".`);
  return button;
}

beforeEach(() => {
  resetPlaybookStoreMock();
});

afterEach(() => {
  hookRuntimeRef.current?.cleanup();
  hookRuntimeRef.current = null;
  vi.clearAllMocks();
});

describe("PlaybooksPanel", () => {
  it("renders a playbook title and status chip", () => {
    playbookStoreMock.state.playbooks = [playbook()];

    const harness = createPanelHarness();
    const statusChip = chips(harness).find((node) => node.props.label === "active");

    expect(textContent(harness.tree)).toContain("FastQC report");
    expect(statusChip?.props.color).toBe("success");
  });

  it("renders an empty-state message when no playbooks exist", () => {
    const harness = createPanelHarness();

    expect(textContent(harness.tree)).toContain("No playbooks yet");
  });

  it("renders a Compose Chain button and loads the operator catalog", () => {
    const harness = createPanelHarness();

    expect(() => getButtonByText(harness, "Compose Chain")).not.toThrow();
    expect(pluginStoreMock.loadOperators).toHaveBeenCalledWith(PROJECT_PATH);
  });

  it("calls replayPlaybook with the selected playbook id", () => {
    playbookStoreMock.state.playbooks = [
      playbook({ playbookId: "pb-selected", title: "Selected playbook" }),
    ];
    const harness = createPanelHarness();
    const replayButton = getButtonByText(harness, "Replay");

    harness.click(replayButton);

    expect(playbookStoreMock.replayPlaybook).toHaveBeenCalledTimes(1);
    expect(playbookStoreMock.replayPlaybook).toHaveBeenCalledWith({
      playbookId: "pb-selected",
      projectRoot: PROJECT_PATH,
      sessionId: undefined,
      executionEnvironment: "local",
      sshServer: undefined,
      sandboxBackend: undefined,
    });
  });

  it("renders an error alert when the store has an error", () => {
    playbookStoreMock.state.error = "Unable to load playbooks";

    const harness = createPanelHarness();
    const alerts = findAllNodes(harness.tree, (node) => node.type === "alert");

    expect(textContent(harness.tree)).toContain("Unable to load playbooks");
    expect(alerts.some((node) => node.props.severity === "error")).toBe(true);
  });
});
