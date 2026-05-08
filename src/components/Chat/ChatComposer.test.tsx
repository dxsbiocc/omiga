import { describe, expect, it } from "vitest";
import {
  COMPOSER_PROMPT_OVERLAY_BOTTOM,
  COMPOSER_PROMPT_OVERLAY_MAX_HEIGHT,
  COMPOSER_PROMPT_OVERLAY_POSITION,
  COMPOSER_PROMPT_OVERLAY_Z_INDEX,
} from "./ChatComposer";

describe("ChatComposer prompt overlay layout", () => {
  it("keeps ask-user and permission prompts out of composer document flow", () => {
    expect(COMPOSER_PROMPT_OVERLAY_POSITION).toBe("absolute");
    expect(COMPOSER_PROMPT_OVERLAY_BOTTOM).toBe("calc(100% + 8px)");
    expect(COMPOSER_PROMPT_OVERLAY_MAX_HEIGHT).toBe("min(42vh, 420px)");
    expect(COMPOSER_PROMPT_OVERLAY_Z_INDEX).toBeGreaterThan(0);
  });
});
