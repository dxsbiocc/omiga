import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

describe("ProviderSwitcher hooks", () => {
  it("does not call hooks after the empty-provider early return", () => {
    const source = readFileSync(
      new URL("./ProviderSwitcher.tsx", import.meta.url),
      "utf8",
    );

    const earlyReturnIndex = source.indexOf("if (providers.length === 0)");

    expect(earlyReturnIndex).toBeGreaterThan(-1);
    expect(source.slice(earlyReturnIndex)).not.toMatch(/\buse[A-Z]\w*\s*\(/);
  });
});
