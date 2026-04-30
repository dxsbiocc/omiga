import { describe, expect, it } from "vitest";
import { extractErrorMessage } from "./errorMessage";

describe("extractErrorMessage", () => {
  it("extracts nested Tauri AppError messages instead of rendering object placeholders", () => {
    const message =
      "Workspace root '/Users/example/.omiga/skills/demo' requires a valid session id";

    expect(
      extractErrorMessage({
        type: "Fs",
        details: {
          kind: "IoError",
          message,
        },
      }),
    ).toBe(message);
  });

  it("falls back to structured JSON for unknown object errors", () => {
    expect(extractErrorMessage({ code: "E_UNKNOWN" })).toBe(
      '{"code":"E_UNKNOWN"}',
    );
  });
});
