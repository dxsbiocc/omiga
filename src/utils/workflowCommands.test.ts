import { describe, expect, it } from "vitest";
import { parseWorkflowCommand } from "./workflowCommands";

describe("parseWorkflowCommand", () => {
  it("parses /schedule with task body", () => {
    expect(parseWorkflowCommand("/schedule refactor login flow")).toEqual({
      command: "schedule",
      body: "refactor login flow",
    });
  });

  it("parses /plan with task body", () => {
    expect(parseWorkflowCommand("/plan design an agent avatar system")).toEqual({
      command: "plan",
      body: "design an agent avatar system",
    });
  });

  it("parses /team with task body", () => {
    expect(parseWorkflowCommand("/team fix export race condition")).toEqual({
      command: "team",
      body: "fix export race condition",
    });
  });

  it("parses /autopilot with task body", () => {
    expect(parseWorkflowCommand("/autopilot build a verified settings sync")).toEqual({
      command: "autopilot",
      body: "build a verified settings sync",
    });
  });

  it("returns null for non-workflow slash commands", () => {
    expect(parseWorkflowCommand("/Explore")).toBeNull();
    expect(parseWorkflowCommand("/planthis")).toBeNull();
  });
});
