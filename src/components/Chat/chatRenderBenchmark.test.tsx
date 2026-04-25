import { renderToStaticMarkup } from "react-dom/server";
import { createTheme } from "@mui/material/styles";
import { describe, expect, it } from "vitest";
import { AssistantMessageBubble } from "./AssistantMessageBubble";
import { ToolCallCard } from "./ToolCallCard";
import { ToolFoldHeader } from "./ToolFoldSummary";
import { UserMessageBubble } from "./UserMessageBubble";
import { getChatTokens } from "./chatTokens";

const chat = getChatTokens(createTheme());
const noop = () => undefined;

describe("chat large transcript render benchmark", () => {
  it("server-renders a synthetic long transcript within a broad budget", () => {
    const itemCount = 180;
    const startedAt = performance.now();
    const html = renderToStaticMarkup(
      <>
        {Array.from({ length: itemCount }, (_, index) => (
          <section key={index}>
            <UserMessageBubble
              content={`User request ${index}`}
              displayText={`User request ${index}`}
              timestamp={1_700_000_000_000 + index}
              attachedPaths={index % 5 === 0 ? ["src/components/Chat/index.tsx"] : []}
              isEditing={false}
              editDraft=""
              chat={chat}
              bubbleRadiusPx={10}
              maxWidth="min(960px, 100%)"
              onRetry={noop}
              onEdit={noop}
              onCopy={noop}
              onEditDraftChange={noop}
              onCancelEdit={noop}
              onSaveEdit={noop}
            />
            <AssistantMessageBubble
              content={`Assistant response ${index}\n\n- checked render item\n- kept memo boundaries`}
              components={{}}
              chat={chat}
              bubbleRadiusPx={10}
            />
            {index % 3 === 0 && (
              <ToolFoldHeader
                foldId={`rf-${index}`}
                expanded={index % 2 === 0}
                summary="Reasoning · Ran 1 command"
                anyRunning={false}
                runningToolName={null}
                runningToolCount={0}
                showGroupDone
                isLastFold={false}
                activityIsStreaming={false}
                waitingFirstChunk={false}
                chat={chat}
                onToggle={noop}
              />
            )}
            {index % 4 === 0 && (
              <ToolCallCard
                foldId={`rf-${index}`}
                messageId={`tool-${index}`}
                content="ok"
                timestamp={1_700_000_000_000 + index}
                toolCall={{
                  name: "bash",
                  status: "completed",
                  input: JSON.stringify({ description: "Run check", command: "npm test" }),
                  output: "passed",
                  completedAt: 1_700_000_000_050 + index,
                }}
                previousAssistantHasText
                generatedThoughtSummary=""
                nestedOpen={index % 8 === 0}
                showAskUserPanel={false}
                chat={chat}
                components={{}}
                onToggle={noop}
              />
            )}
          </section>
        ))}
      </>,
    );
    const durationMs = performance.now() - startedAt;

    console.info(
      `[RenderPerfTest] SSR synthetic transcript | items=${itemCount} | duration=${Math.round(durationMs)}ms | html=${html.length}`,
    );

    expect(html).toContain("Assistant response 179");
    expect(durationMs).toBeLessThan(6_000);
  });
});
