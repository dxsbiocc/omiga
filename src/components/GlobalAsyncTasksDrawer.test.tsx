import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  HookRuntimeApi,
  RenderedNode,
} from "../test/__tests__/componentHarness";
import {
  ComponentHarness,
  createHookRuntime,
  findAllNodes,
  installComponentTestWindow,
  textContent,
} from "../test/__tests__/componentHarness";
import { GlobalAsyncTasksDrawer } from "./GlobalAsyncTasksDrawer";

const hookRuntimeRef = vi.hoisted(() => ({
  current: null as HookRuntimeApi | null,
}));

const pluginStoreMock = vi.hoisted(() => ({
  state: {
    activeOperatorTasks: {} as Record<string, string>,
    activeOperatorTaskStartedAt: {} as Record<string, number>,
    activeOperatorTaskStatus: {} as Record<
      string,
      { scheduler: string; state: string; jobId?: string | null }
    >,
    cancelOperatorTask: vi.fn(),
  },
}));

vi.mock("react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react")>();
  return {
    ...actual,
    useEffect: (effect: () => void | (() => void), deps?: readonly unknown[]) =>
      hookRuntimeRef.current?.useEffect(effect, deps),
    useMemo: <T,>(factory: () => T, deps?: readonly unknown[]) =>
      hookRuntimeRef.current?.useMemo(factory, deps),
    useState: <T,>(initial: T | (() => T)) =>
      hookRuntimeRef.current?.useState(initial),
  };
});

vi.mock("@mui/material", async () => {
  const { createMuiMaterialMock } = await import("../test/__tests__/muiMocks");
  return createMuiMaterialMock();
});

vi.mock("lucide-react", async () => {
  const { createIconMock } = await import("../test/__tests__/muiMocks");
  return createIconMock(["CircleStop", "Clock3", "ListTodo", "X"]);
});

vi.mock("../state/pluginStore", () => ({
  __esModule: true,
  usePluginStore: <T,>(selector: (state: typeof pluginStoreMock.state) => T): T =>
    selector(pluginStoreMock.state),
}));

const baseTime = new Date("2026-05-23T12:00:00Z");

const setStoreTasks = (
  overrides: Partial<typeof pluginStoreMock.state> = {},
) => {
  pluginStoreMock.state.activeOperatorTasks = {
    align_reads: "task-align-1",
  };
  pluginStoreMock.state.activeOperatorTaskStartedAt = {
    align_reads: baseTime.getTime() - 61_000,
  };
  pluginStoreMock.state.activeOperatorTaskStatus = {
    align_reads: {
      scheduler: "slurm",
      state: "R",
      jobId: "42",
    },
  };
  pluginStoreMock.state.cancelOperatorTask = vi.fn().mockResolvedValue(undefined);
  Object.assign(pluginStoreMock.state, overrides);
};

const createDrawerHarness = () => {
  installComponentTestWindow();
  const runtime = createHookRuntime();
  hookRuntimeRef.current = runtime;
  const harness = new ComponentHarness(
    runtime,
    (): ReactElement => <GlobalAsyncTasksDrawer />,
  );
  harness.render();
  return harness;
};

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

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(baseTime);
  setStoreTasks();
});

afterEach(() => {
  hookRuntimeRef.current?.cleanup();
  hookRuntimeRef.current = null;
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe("GlobalAsyncTasksDrawer", () => {
  it("does not start an elapsed timer when there are no active tasks", () => {
    setStoreTasks({
      activeOperatorTasks: {},
      activeOperatorTaskStartedAt: {},
      activeOperatorTaskStatus: {},
    });

    const harness = createDrawerHarness();

    expect(textContent(harness.tree)).toBe("");
    expect(vi.getTimerCount()).toBe(0);
  });

  it("shows the active task count on the floating toggle and opens the drawer", () => {
    setStoreTasks({
      activeOperatorTasks: {
        align_reads: "task-align-1",
        trim_reads: "task-trim-1",
      },
      activeOperatorTaskStartedAt: {
        align_reads: baseTime.getTime() - 61_000,
        trim_reads: baseTime.getTime() - 5_000,
      },
    });
    const harness = createDrawerHarness();

    const badge = findAllNodes(harness.tree, (node) => node.type === "badge")[0];
    expect(badge.props.badgeContent).toBe(2);
    expect(textContent(harness.tree)).toContain("2");

    harness.click(getButtonByAriaLabel(harness, "Open async operator tasks"));

    expect(textContent(harness.tree)).toContain("Async operator tasks");
    expect(textContent(harness.tree)).toContain("2 active");
  });

  it("renders task aliases and updates elapsed time while the drawer is open", () => {
    const harness = createDrawerHarness();

    harness.click(getButtonByAriaLabel(harness, "Open async operator tasks"));

    expect(textContent(harness.tree)).toContain("align_reads");
    expect(textContent(harness.tree)).toContain("01:01");

    vi.advanceTimersByTime(2_000);
    harness.flush();

    expect(textContent(harness.tree)).toContain("01:03");
  });

  it("cancels the selected active task", () => {
    const harness = createDrawerHarness();

    harness.click(getButtonByAriaLabel(harness, "Open async operator tasks"));
    harness.click(getButtonByText(harness, "Cancel"));

    expect(pluginStoreMock.state.cancelOperatorTask).toHaveBeenCalledWith(
      "task-align-1",
    );
  });
});
