import { describe, expect, it } from "vitest";
import {
  COMPOSER_CONTEXT_ITEM_MAX_WIDTH,
  COMPOSER_CONTEXT_TRAY_MAX_HEIGHT,
  COMPOSER_CONTEXT_TRAY_PLACEMENT,
  COMPOSER_INPUT_JOINED_BORDER_RADIUS,
  COMPOSER_INPUT_JOINED_Z_INDEX,
  COMPOSER_PROMPT_JOINED_BORDER_RADIUS,
  COMPOSER_PROMPT_OVERLAY_BOTTOM,
  COMPOSER_PROMPT_OVERLAY_MAX_HEIGHT,
  COMPOSER_PROMPT_OVERLAY_POSITION,
  COMPOSER_PROMPT_OVERLAY_WIDTH,
  COMPOSER_PROMPT_OVERLAY_Z_INDEX,
  COMPOSER_PERMISSION_MODE_MENU_WIDTH,
} from "./ChatComposer";

describe("ChatComposer prompt overlay layout", () => {
  it("keeps ask-user and permission prompts out of composer document flow", () => {
    expect(COMPOSER_PROMPT_OVERLAY_POSITION).toBe("absolute");
    expect(COMPOSER_PROMPT_OVERLAY_BOTTOM).toBe("calc(100% + 8px)");
    expect(COMPOSER_PROMPT_OVERLAY_MAX_HEIGHT).toBe("min(48vh, 360px)");
    expect(COMPOSER_PROMPT_OVERLAY_WIDTH).toBe(
      "min(520px, calc(100vw - 48px))",
    );
    expect(COMPOSER_PROMPT_OVERLAY_Z_INDEX).toBeGreaterThan(0);
    expect(COMPOSER_PROMPT_OVERLAY_Z_INDEX).toBeGreaterThan(
      COMPOSER_INPUT_JOINED_Z_INDEX,
    );
    expect(COMPOSER_PROMPT_JOINED_BORDER_RADIUS).toBe("12px");
    expect(COMPOSER_INPUT_JOINED_BORDER_RADIUS).toBe("24px");
    expect(COMPOSER_PERMISSION_MODE_MENU_WIDTH).toBe(180);
  });

  it("keeps composer references in a tray above the text input", () => {
    expect(COMPOSER_CONTEXT_TRAY_PLACEMENT).toBe("above-input");
    expect(COMPOSER_CONTEXT_TRAY_MAX_HEIGHT).toBe("min(28vh, 152px)");
    expect(COMPOSER_CONTEXT_ITEM_MAX_WIDTH).toBe("calc((100% - 16px) / 3)");
  });
});
