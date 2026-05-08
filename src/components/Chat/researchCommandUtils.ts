export interface ResearchCommandMessageLike {
  id: string;
  role: string;
}

export function finalizeResearchCommandMessages<T extends ResearchCommandMessageLike>(
  messages: T[],
  optimisticUserMessageId: string,
  persistedUserMessage: T,
  assistantMessage: T,
): T[] {
  const replaced = messages.map((message) =>
    message.id === optimisticUserMessageId ? persistedUserMessage : message,
  );

  const hasOptimisticUser = replaced.some(
    (message) => message.id === persistedUserMessage.id,
  );
  const next = hasOptimisticUser
    ? replaced
    : [...replaced, persistedUserMessage];

  return [...next, assistantMessage];
}
