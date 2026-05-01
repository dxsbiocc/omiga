import { describe, expect, it } from "vitest";
import {
  isNearScrollBottom,
  shouldShowJumpToLatestButton,
} from "./chatScrollState";

describe("chatScrollState", () => {
  it("treats positions within the threshold as near the latest message", () => {
    expect(
      isNearScrollBottom({
        scrollTop: 805,
        clientHeight: 200,
        scrollHeight: 1100,
      }),
    ).toBe(true);
  });

  it("shows the jump button only after the user scrolls away from latest messages", () => {
    expect(
      shouldShowJumpToLatestButton({
        scrollTop: 600,
        clientHeight: 200,
        scrollHeight: 1100,
      }),
    ).toBe(true);

    expect(
      shouldShowJumpToLatestButton({
        scrollTop: 805,
        clientHeight: 200,
        scrollHeight: 1100,
      }),
    ).toBe(false);
  });

  it("does not show the jump button when the transcript cannot scroll", () => {
    expect(
      shouldShowJumpToLatestButton({
        scrollTop: 0,
        clientHeight: 600,
        scrollHeight: 600,
      }),
    ).toBe(false);
  });
});

