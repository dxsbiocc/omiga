import { describe, expect, it } from "vitest";
import { TerminalScreen } from "./terminalScreen";

function text(screen: TerminalScreen) {
  return screen.toPlainText();
}

describe("TerminalScreen", () => {
  it("handles carriage return redraws", () => {
    const screen = new TerminalScreen();
    screen.write("hello\rhey");
    expect(text(screen)).toBe("heylo");
  });

  it("handles ansi colors", () => {
    const screen = new TerminalScreen();
    screen.write("\x1b[31mred\x1b[0m normal");
    const first = screen.snapshot()[0].segments[0];
    expect(first.text).toBe("red");
    expect(first.style.fg).toBeTruthy();
  });

  it("handles erase to end of line", () => {
    const screen = new TerminalScreen();
    screen.write("abcdef\rabc\x1b[Kxy");
    expect(text(screen)).toBe("abcxy");
  });
});
