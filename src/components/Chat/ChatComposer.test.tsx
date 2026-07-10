import { describe, expect, it } from "vitest";
import {
  buildComposerWorkspaceUploadPath,
  composerPointInRect,
  COMPOSER_CONTEXT_ITEM_MAX_WIDTH,
  COMPOSER_CONTEXT_TRAY_MAX_HEIGHT,
  COMPOSER_CONTEXT_TRAY_PLACEMENT,
  COMPOSER_DROP_UPLOAD_SNACKBAR_AUTO_HIDE_MS,
  COMPOSER_DROP_UPLOAD_SNACKBAR_ERROR_AUTO_HIDE_MS,
  COMPOSER_FILE_CONTEXT_CHIP_MAX_WIDTH,
  COMPOSER_INPUT_JOINED_BORDER_RADIUS,
  COMPOSER_INPUT_JOINED_Z_INDEX,
  COMPOSER_PROMPT_JOINED_BORDER_RADIUS,
  COMPOSER_PROMPT_OVERLAY_BOTTOM,
  COMPOSER_PROMPT_OVERLAY_MAX_HEIGHT,
  COMPOSER_PROMPT_OVERLAY_POSITION,
  COMPOSER_PROMPT_OVERLAY_WIDTH,
  COMPOSER_PROMPT_OVERLAY_Z_INDEX,
  COMPOSER_PERMISSION_MODE_MENU_WIDTH,
  COMPOSER_SSH_DROP_UPLOAD_MAX_BYTES,
  COMPOSER_SSH_DROP_UPLOAD_MAX_FILES,
  normalizeComposerDroppedLocalPath,
  sanitizeComposerDroppedFileName,
} from "./ChatComposer";

describe("ChatComposer prompt overlay layout", () => {
  it("keeps ask-user and permission prompts out of composer document flow", () => {
    expect(COMPOSER_PROMPT_OVERLAY_POSITION).toBe("absolute");
    expect(COMPOSER_PROMPT_OVERLAY_BOTTOM).toBe("calc(100% + 8px)");
    expect(COMPOSER_PROMPT_OVERLAY_MAX_HEIGHT).toBe("min(48vh, 360px)");
    expect(COMPOSER_PROMPT_OVERLAY_WIDTH).toBe("100%");
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

describe("ChatComposer SSH drop upload helpers", () => {
  it("sanitizes dropped file names before writing remote paths", () => {
    expect(sanitizeComposerDroppedFileName(" data.tsv ")).toBe("data.tsv");
    expect(sanitizeComposerDroppedFileName("../secret.txt")).toBe("secret.txt");
    expect(sanitizeComposerDroppedFileName("bad/name.txt")).toBe("name.txt");
    expect(sanitizeComposerDroppedFileName("...")).toBe("upload.bin");
  });

  it("uploads dropped files into the current workspace root", () => {
    expect(buildComposerWorkspaceUploadPath("/home/me/work", "data.tsv")).toBe(
      "/home/me/work/data.tsv",
    );
    expect(buildComposerWorkspaceUploadPath("/home/me/work/", "data.tsv")).toBe(
      "/home/me/work/data.tsv",
    );
    expect(buildComposerWorkspaceUploadPath("~/work", "data.tsv")).toBe(
      "~/work/data.tsv",
    );
    expect(buildComposerWorkspaceUploadPath("/", "data.tsv")).toBe("/data.tsv");
  });

  it("keeps drag upload limits explicit", () => {
    expect(COMPOSER_SSH_DROP_UPLOAD_MAX_FILES).toBeGreaterThan(0);
    expect(COMPOSER_SSH_DROP_UPLOAD_MAX_BYTES).toBe(50 * 1024 * 1024);
  });

  it("keeps completed upload notifications temporary", () => {
    expect(COMPOSER_DROP_UPLOAD_SNACKBAR_AUTO_HIDE_MS).toBeGreaterThan(0);
    expect(COMPOSER_DROP_UPLOAD_SNACKBAR_ERROR_AUTO_HIDE_MS).toBeGreaterThan(
      COMPOSER_DROP_UPLOAD_SNACKBAR_AUTO_HIDE_MS,
    );
  });

  it("keeps file attachment chips compact inside the composer reference tray", () => {
    expect(COMPOSER_FILE_CONTEXT_CHIP_MAX_WIDTH).toBe("min(46vw, 240px)");
  });

  it("accepts both CSS and physical pixel drag positions", () => {
    const rect = { left: 100, right: 300, top: 200, bottom: 260 };

    expect(composerPointInRect({ x: 150, y: 220 }, rect, 2)).toBe(true);
    expect(composerPointInRect({ x: 300, y: 440 }, rect, 2)).toBe(true);
    expect(composerPointInRect({ x: 40, y: 40 }, rect, 2)).toBe(false);
  });

  it("turns local desktop drops inside the workspace into @-relative paths", () => {
    expect(
      normalizeComposerDroppedLocalPath(
        "/Users/me/project",
        "/Users/me/project/src/App.tsx",
      ),
    ).toBe("src/App.tsx");
    expect(
      normalizeComposerDroppedLocalPath(
        "/Users/me/project/",
        "/Users/me/project/data",
      ),
    ).toBe("data");
    expect(
      normalizeComposerDroppedLocalPath(
        "/Users/me/project",
        "/Users/me/project",
      ),
    ).toBe(".");
    expect(normalizeComposerDroppedLocalPath("/", "/data.tsv")).toBe(
      "data.tsv",
    );
  });

  it("preserves exact local desktop drop paths outside the workspace", () => {
    expect(
      normalizeComposerDroppedLocalPath(
        "/Users/me/project",
        "/Users/me/Downloads/data.csv",
      ),
    ).toBe("/Users/me/Downloads/data.csv");
    expect(
      normalizeComposerDroppedLocalPath(
        "C:\\Users\\me\\project",
        "C:\\Users\\me\\project\\data\\raw.csv",
      ),
    ).toBe("data/raw.csv");
  });
});
