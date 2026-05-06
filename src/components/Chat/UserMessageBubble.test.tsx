import { renderToStaticMarkup } from "react-dom/server";
import { createTheme, ThemeProvider } from "@mui/material/styles";
import { describe, expect, it, vi } from "vitest";
import {
  UserMessageBubble,
  formatUserMessageTimestamp,
  splitUserMessageInlineCommand,
} from "./UserMessageBubble";
import { getChatTokens } from "./chatTokens";

const theme = createTheme();
const chat = getChatTokens(theme);

function renderBubble(overrides: Partial<Parameters<typeof UserMessageBubble>[0]> = {}) {
  return renderToStaticMarkup(
    <ThemeProvider theme={theme}>
      <UserMessageBubble
        content="@src/App.tsx\n\nhello"
        displayText="hello"
        timestamp={new Date("2026-04-25T12:00:00Z").getTime()}
        composerAgentType="executor"
        attachedPaths={["src/App.tsx"]}
        isEditing={false}
        editDraft=""
        chat={chat}
        bubbleRadiusPx={10}
        maxWidth="min(960px, 100%)"
        onRetry={vi.fn()}
        onEdit={vi.fn()}
        onCopy={vi.fn()}
        onEditDraftChange={vi.fn()}
        onCancelEdit={vi.fn()}
        onSaveEdit={vi.fn()}
        {...overrides}
      />
    </ThemeProvider>,
  );
}

describe("UserMessageBubble", () => {
  it("formats timestamps defensively", () => {
    expect(formatUserMessageTimestamp(Number.NaN)).toBeTruthy();
  });

  it("server-renders user text, agent chip, attached file chip, and actions", () => {
    const html = renderBubble();

    expect(html).toContain("hello");
    expect(html).toContain("/executor");
    expect(html).toContain("@src/App.tsx");
    expect(html).toContain("user-msg-inline-flow");
    expect(html).toContain("user-msg-inline-chip");
    expect(html).toContain("user-msg-agent-chip");
    expect(html).toContain("user-msg-file-chip");
    expect(html).toContain("user-msg-body-text");
    expect(html).toContain("aria-label=\"重试\"");
    expect(html).toContain("aria-label=\"编辑\"");
    expect(html).toContain("aria-label=\"复制\"");
  });

  it("renders selected plugin chips with # to avoid @ file conflicts", () => {
    const html = renderBubble({ selectedPluginIds: ["sample@market"] });

    expect(html).toContain("#sample@market");
    expect(html).toContain("user-msg-plugin-chip");
  });

  it("splits workflow slash commands so command chips flow inline with body text", () => {
    const command = splitUserMessageInlineCommand(
      "/plan 提取文件中与 QS 核心相关的分组、基因",
    );

    expect(command?.command.label).toBe("/plan");
    expect(command?.body).toBe("提取文件中与 QS 核心相关的分组、基因");

    const html = renderBubble({
      displayText: "/plan 提取文件中与 QS 核心相关的分组、基因",
      composerAgentType: undefined,
    });

    expect(html).toContain("/plan");
    expect(html).toContain("提取文件中与 QS 核心相关的分组、基因");
    expect(html).toContain("user-msg-inline-flow");
    expect(html).toContain("user-msg-inline-chip");
    expect(html).toContain("user-msg-command-chip");
  });

  it("renders /goal as a workflow command chip", () => {
    const command = splitUserMessageInlineCommand(
      "/goal 解析 QS 核心基因并形成机制假设",
    );

    expect(command?.command.label).toBe("/goal");
    expect(command?.body).toBe("解析 QS 核心基因并形成机制假设");
  });

  it("server-renders edit controls while editing", () => {
    const html = renderBubble({ isEditing: true, editDraft: "draft body" });

    expect(html).toContain("draft body");
    expect(html).toContain("保存后将截断后续消息");
    expect(html).toContain("取消");
    expect(html).toContain("保存");
    expect(html).not.toContain("aria-label=\"重试\"");
  });
});
