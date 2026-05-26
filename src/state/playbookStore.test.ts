import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

import {
  usePlaybookStore,
  type ReplayPlaybookArgs,
  type SavePlaybookFromChainArgs,
} from "./playbookStore";
import type {
  OperatorChainStep,
  Playbook,
  ReplayPlaybookResponse,
} from "./playbookTypes";

function playbook(overrides: Partial<Playbook> = {}): Playbook {
  return {
    playbookId: "pb-chain-1",
    title: "Reusable chain",
    fingerprint: {
      canonicalId: "chain:qc",
      operatorVersion: "0.1.0",
      paramSchemaHash: "sha256:param",
      envSignature: null,
    },
    kind: "chain",
    canonicalId: "chain:qc",
    operatorVersion: "0.1.0",
    params: [],
    inputs: {},
    verification: {
      expectedStatus: "ok",
      expectedOutputKeys: ["report"],
    },
    provenance: {
      distilledFrom: ["run-1"],
      proposalId: null,
      createdAt: "2026-05-26T00:00:00Z",
    },
    health: {
      hitCount: 0,
      successCount: 0,
      lastVerifiedAt: null,
      status: "active",
    },
    ...overrides,
  };
}

describe("usePlaybookStore", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    usePlaybookStore.setState({
      playbooks: [],
      isLoading: false,
      error: null,
    });
  });

  it("lists playbooks, stores them, and returns the loaded array", async () => {
    const playbooks = [
      playbook({ playbookId: "pb-1" }),
      playbook({ playbookId: "pb-2", title: "Second chain" }),
    ];
    invokeMock.mockResolvedValueOnce(playbooks);

    const result = await usePlaybookStore.getState().listPlaybooks("/project");

    expect(result).toBe(playbooks);
    expect(invokeMock).toHaveBeenCalledWith("list_playbooks", {
      projectRoot: "/project",
    });
    expect(usePlaybookStore.getState()).toMatchObject({
      playbooks,
      isLoading: false,
      error: null,
    });
  });

  it("stores an error and throws when listing playbooks fails", async () => {
    invokeMock.mockRejectedValueOnce({
      details: { kind: "IoError", message: "catalog unavailable" },
    });

    await expect(
      usePlaybookStore.getState().listPlaybooks("/project"),
    ).rejects.toThrow("catalog unavailable");

    expect(invokeMock).toHaveBeenCalledWith("list_playbooks", {
      projectRoot: "/project",
    });
    expect(usePlaybookStore.getState()).toMatchObject({
      playbooks: [],
      isLoading: false,
      error: "catalog unavailable",
    });
  });

  it("replays a playbook and refreshes the list once", async () => {
    const response: ReplayPlaybookResponse = {
      outcome: "replayed",
      verified: true,
      status: "active",
      result: {
        ok: true,
        steps: [
          {
            alias: "summarize",
            ok: true,
            runDir: "/runs/1",
            result: { report: "done" },
            error: null,
          },
        ],
        error: null,
      },
    };
    const refreshed = [playbook({ playbookId: "pb-1" })];
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "replay_playbook") return response;
      if (command === "list_playbooks") return refreshed;
      throw new Error(`unexpected command ${command}`);
    });
    const args: ReplayPlaybookArgs = {
      playbookId: "pb-1",
      projectRoot: "/project",
      sessionId: "session-1",
      executionEnvironment: "local",
      sshServer: "analysis-box",
      sandboxBackend: "native",
    };

    const result = await usePlaybookStore.getState().replayPlaybook(args);

    expect(result).toBe(response);
    expect(invokeMock).toHaveBeenCalledTimes(2);
    expect(invokeMock.mock.calls).toEqual([
      ["replay_playbook", args],
      ["list_playbooks", { projectRoot: "/project" }],
    ]);
    expect(usePlaybookStore.getState().playbooks).toBe(refreshed);
  });

  it("saves a playbook from a chain and refreshes the list once", async () => {
    const steps: OperatorChainStep[] = [
      {
        alias: "summarize",
        label: "Summarize reads",
        arguments: { input: "reads.fastq" },
        inheritPrevOutputAs: null,
        dependsOn: [],
      },
    ];
    const saved = playbook({ playbookId: "pb-saved", title: "Saved chain" });
    const refreshed = [saved];
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "save_playbook_from_chain") return saved;
      if (command === "list_playbooks") return refreshed;
      throw new Error(`unexpected command ${command}`);
    });
    const args: SavePlaybookFromChainArgs = {
      playbookId: "pb-saved",
      title: "Saved chain",
      steps,
      expectedOutputKeys: ["report"],
      chainOk: true,
      projectRoot: "/project",
      executionEnvironment: "local",
    };

    const result = await usePlaybookStore.getState().savePlaybookFromChain(args);

    expect(result).toBe(saved);
    expect(invokeMock).toHaveBeenCalledTimes(2);
    expect(invokeMock.mock.calls).toEqual([
      ["save_playbook_from_chain", args],
      ["list_playbooks", { projectRoot: "/project" }],
    ]);
    expect(usePlaybookStore.getState().playbooks).toBe(refreshed);
  });
});
