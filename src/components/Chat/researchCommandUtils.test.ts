import { describe, expect, it } from "vitest";

import { finalizeResearchCommandMessages } from "./researchCommandUtils";

describe("finalizeResearchCommandMessages", () => {
  it("replaces the optimistic user row while preserving messages added in flight", () => {
    const result = finalizeResearchCommandMessages(
      [
        { id: "earlier", role: "assistant", content: "before" },
        { id: "temp-user", role: "user", content: "/research run topic" },
        { id: "later", role: "assistant", content: "added while waiting" },
      ],
      "temp-user",
      { id: "persisted-user", role: "user", content: "/research run topic" },
      { id: "assistant-final", role: "assistant", content: "done" },
    );

    expect(result).toEqual([
      { id: "earlier", role: "assistant", content: "before" },
      { id: "persisted-user", role: "user", content: "/research run topic" },
      { id: "later", role: "assistant", content: "added while waiting" },
      { id: "assistant-final", role: "assistant", content: "done" },
    ]);
  });

  it("appends the persisted user row if the optimistic row was already removed", () => {
    const result = finalizeResearchCommandMessages(
      [{ id: "later", role: "assistant", content: "still here" }],
      "temp-user",
      { id: "persisted-user", role: "user", content: "/research help" },
      { id: "assistant-final", role: "assistant", content: "help output" },
    );

    expect(result).toEqual([
      { id: "later", role: "assistant", content: "still here" },
      { id: "persisted-user", role: "user", content: "/research help" },
      { id: "assistant-final", role: "assistant", content: "help output" },
    ]);
  });
});
