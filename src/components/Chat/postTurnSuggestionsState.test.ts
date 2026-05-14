import { describe, expect, it } from "vitest";
import {
  shouldShowPostTurnSuggestionsGeneratingPlaceholder,
  shouldStartPostTurnSuggestionsIndicator,
} from "./postTurnSuggestionsState";

describe("postTurnSuggestionsState", () => {
  it("shows the generating placeholder only for an idle completed turn", () => {
    expect(
      shouldShowPostTurnSuggestionsGeneratingPlaceholder({
        suggestionsGenerating: true,
        showNextStepSuggestions: false,
        currentStreamId: null,
        isConnecting: false,
        isStreaming: false,
        waitingFirstChunk: false,
        activityIsStreaming: false,
      }),
    ).toBe(true);

    expect(
      shouldShowPostTurnSuggestionsGeneratingPlaceholder({
        suggestionsGenerating: true,
        showNextStepSuggestions: false,
        currentStreamId: "next-stream",
        isConnecting: true,
        isStreaming: false,
        waitingFirstChunk: true,
        activityIsStreaming: false,
      }),
    ).toBe(false);
  });

  it("suppresses stale generating events when a queued main message will continue", () => {
    expect(
      shouldStartPostTurnSuggestionsIndicator({
        activePostTurnStreamId: "old-stream",
        eventStreamId: "old-stream",
        currentStreamId: null,
        isConnecting: false,
        isStreaming: false,
        waitingFirstChunk: false,
        activityIsStreaming: false,
        queuedMainSendCount: 1,
        flushingQueuedMainSend: false,
      }),
    ).toBe(false);

    expect(
      shouldStartPostTurnSuggestionsIndicator({
        activePostTurnStreamId: "old-stream",
        eventStreamId: "old-stream",
        currentStreamId: null,
        isConnecting: true,
        isStreaming: false,
        waitingFirstChunk: true,
        activityIsStreaming: false,
        queuedMainSendCount: 0,
        flushingQueuedMainSend: true,
      }),
    ).toBe(false);
  });

  it("ignores post-turn events that no longer own the active suggestion slot", () => {
    expect(
      shouldStartPostTurnSuggestionsIndicator({
        activePostTurnStreamId: null,
        eventStreamId: "old-stream",
        currentStreamId: null,
        isConnecting: false,
        isStreaming: false,
        waitingFirstChunk: false,
        activityIsStreaming: false,
        queuedMainSendCount: 0,
        flushingQueuedMainSend: false,
      }),
    ).toBe(false);
  });
});
