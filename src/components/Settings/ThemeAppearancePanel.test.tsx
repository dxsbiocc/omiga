import { renderToStaticMarkup } from "react-dom/server";
import { createTheme, ThemeProvider } from "@mui/material/styles";
import { describe, expect, it, vi } from "vitest";
import { ThemeAppearancePanel } from "./ThemeAppearancePanel";

function renderPanel() {
  return renderToStaticMarkup(
    <ThemeProvider theme={createTheme()}>
      <ThemeAppearancePanel
        colorMode="dark"
        onColorModeChange={vi.fn()}
        accentPreset="asana"
        onAccentPresetChange={vi.fn()}
        appSkin="warm-capybara"
        onAppSkinChange={vi.fn()}
      />
    </ThemeProvider>,
  );
}

describe("ThemeAppearancePanel", () => {
  it("renders the app icon skin choices and marks the selected one", () => {
    const html = renderPanel();

    expect(html).toContain("App icon");
    expect(html).toContain("Classic Capybara");
    expect(html).toContain("Warm Capybara");
    expect(html).toContain('aria-pressed="true" aria-label="Warm Capybara app icon"');
    expect(html).toContain(
      'aria-pressed="false" aria-label="Classic Capybara app icon"',
    );
  });
});
