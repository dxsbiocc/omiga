import { renderToStaticMarkup } from "react-dom/server";
import { createTheme, ThemeProvider } from "@mui/material/styles";
import { describe, expect, it, vi } from "vitest";
import {
  UserMessageBubble,
  formatUserMessageTimestamp,
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
    expect(html).toContain("aria-label=\"重试\"");
    expect(html).toContain("aria-label=\"编辑\"");
    expect(html).toContain("aria-label=\"复制\"");
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
