export interface PostTurnSuggestionsBusyState {
  currentStreamId: string | null;
  isConnecting: boolean;
  isStreaming: boolean;
  waitingFirstChunk?: boolean;
  activityIsStreaming: boolean;
  activityIsConnecting?: boolean;
}

export interface PostTurnSuggestionsQueueState {
  queuedMainSendCount: number;
  flushingQueuedMainSend: boolean;
}

export interface PostTurnSuggestionsIndicatorState
  extends PostTurnSuggestionsBusyState,
    PostTurnSuggestionsQueueState {
  activePostTurnStreamId: string | null;
  eventStreamId: string;
}

export interface PostTurnSuggestionsPlaceholderState
  extends PostTurnSuggestionsBusyState {
  suggestionsGenerating: boolean;
  showNextStepSuggestions: boolean;
}

export function isMainTurnBusyForPostTurnSuggestions(
  state: PostTurnSuggestionsBusyState,
): boolean {
  return (
    state.currentStreamId !== null ||
    state.isConnecting ||
    state.isStreaming ||
    Boolean(state.waitingFirstChunk) ||
    state.activityIsStreaming ||
    Boolean(state.activityIsConnecting)
  );
}

export function hasQueuedMainContinuation(
  state: PostTurnSuggestionsQueueState,
): boolean {
  return state.queuedMainSendCount > 0 || state.flushingQueuedMainSend;
}

export function shouldStartPostTurnSuggestionsIndicator(
  state: PostTurnSuggestionsIndicatorState,
): boolean {
  return (
    state.activePostTurnStreamId === state.eventStreamId &&
    !isMainTurnBusyForPostTurnSuggestions(state) &&
    !hasQueuedMainContinuation(state)
  );
}

export function shouldShowPostTurnSuggestionsGeneratingPlaceholder(
  state: PostTurnSuggestionsPlaceholderState,
): boolean {
  return (
    state.suggestionsGenerating &&
    !state.showNextStepSuggestions &&
    !isMainTurnBusyForPostTurnSuggestions(state)
  );
}
