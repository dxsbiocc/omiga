import { describe, expect, it } from "vitest";
import {
  COMPOSER_INPUT_JOINED_BORDER_RADIUS,
  COMPOSER_INPUT_JOINED_Z_INDEX,
  COMPOSER_PROMPT_JOINED_BORDER_RADIUS,
  COMPOSER_PROMPT_OVERLAY_BOTTOM,
  COMPOSER_PROMPT_OVERLAY_MAX_HEIGHT,
  COMPOSER_PROMPT_OVERLAY_POSITION,
  COMPOSER_PROMPT_OVERLAY_Z_INDEX,
} from "./ChatComposer";

describe("ChatComposer prompt overlay layout", () => {
  it("keeps ask-user and permission prompts out of composer document flow", () => {
    expect(COMPOSER_PROMPT_OVERLAY_POSITION).toBe("absolute");
    expect(COMPOSER_PROMPT_OVERLAY_BOTTOM).toBe("100%");
    expect(COMPOSER_PROMPT_OVERLAY_MAX_HEIGHT).toBe("min(42vh, 420px)");
    expect(COMPOSER_PROMPT_OVERLAY_Z_INDEX).toBeGreaterThan(0);
    expect(COMPOSER_INPUT_JOINED_Z_INDEX).toBe(
      COMPOSER_PROMPT_OVERLAY_Z_INDEX,
    );
    expect(COMPOSER_PROMPT_JOINED_BORDER_RADIUS).toBe("24px 24px 0 0");
    expect(COMPOSER_INPUT_JOINED_BORDER_RADIUS).toBe("0 0 24px 24px");
  });
});
