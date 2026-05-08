import { describe, expect, it } from "vitest";
import {
  EDITABLE_EMPTY_SENTINEL,
  getEditableInputUpdate,
  normalizeEditableText,
} from "./editableText";

describe("editableText", () => {
  it("does not commit or rewrite DOM while IME composition is active", () => {
    expect(getEditableInputUpdate("m", true)).toEqual({
      nextValue: "m",
      shouldCommit: false,
      shouldNormalizeDom: false,
    });
    expect(getEditableInputUpdate("me", true)).toEqual({
      nextValue: "me",
      shouldCommit: false,
      shouldNormalizeDom: false,
    });
  });

  it("commits the final composition text after composition ends", () => {
    expect(getEditableInputUpdate("么", false)).toEqual({
      nextValue: "么",
      shouldCommit: true,
      shouldNormalizeDom: false,
    });
  });

  it("normalizes the empty sentinel only outside composition", () => {
    expect(normalizeEditableText(`${EDITABLE_EMPTY_SENTINEL}abc`)).toBe("abc");
    expect(getEditableInputUpdate(EDITABLE_EMPTY_SENTINEL, false)).toEqual({
      nextValue: "",
      shouldCommit: true,
      shouldNormalizeDom: true,
    });
  });
});
