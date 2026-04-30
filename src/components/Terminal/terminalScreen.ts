export interface TerminalCellStyle {
  fg?: string;
  bg?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
  inverse?: boolean;
  cursor?: boolean;
}

interface TerminalCell extends TerminalCellStyle {
  ch: string;
}

export interface TerminalRenderSegment {
  key: string;
  text: string;
  style: TerminalCellStyle;
}

export interface TerminalRenderLine {
  key: string;
  segments: TerminalRenderSegment[];
}

const BASIC_COLORS = [
  "#111827",
  "#ef4444",
  "#22c55e",
  "#eab308",
  "#3b82f6",
  "#a855f7",
  "#06b6d4",
  "#e5e7eb",
];

const BRIGHT_COLORS = [
  "#6b7280",
  "#f87171",
  "#86efac",
  "#fde047",
  "#60a5fa",
  "#c084fc",
  "#67e8f9",
  "#ffffff",
];

function cloneStyle(style: TerminalCellStyle): TerminalCellStyle {
  return { ...style };
}

function styleKey(style: TerminalCellStyle): string {
  return [
    style.fg ?? "",
    style.bg ?? "",
    style.bold ? "b" : "",
    style.dim ? "d" : "",
    style.italic ? "i" : "",
    style.underline ? "u" : "",
    style.inverse ? "r" : "",
    style.cursor ? "c" : "",
  ].join("|");
}

function color256(code: number): string | undefined {
  if (code >= 0 && code < 8) return BASIC_COLORS[code];
  if (code >= 8 && code < 16) return BRIGHT_COLORS[code - 8];
  if (code >= 16 && code <= 231) {
    const n = code - 16;
    const r = Math.floor(n / 36);
    const g = Math.floor((n % 36) / 6);
    const b = n % 6;
    const scale = [0, 95, 135, 175, 215, 255];
    return `rgb(${scale[r]}, ${scale[g]}, ${scale[b]})`;
  }
  if (code >= 232 && code <= 255) {
    const value = 8 + (code - 232) * 10;
    return `rgb(${value}, ${value}, ${value})`;
  }
  return undefined;
}

function parseNumbers(params: string): number[] {
  if (!params) return [0];
  const clean = params.replace(/[?>]/gu, "");
  return clean.split(";").map((part) => {
    if (!part) return 0;
    const parsed = Number(part);
    return Number.isFinite(parsed) ? parsed : 0;
  });
}

export class TerminalScreen {
  private lines: TerminalCell[][] = [[]];
  private row = 0;
  private col = 0;
  private style: TerminalCellStyle = {};

  constructor(private readonly maxLines = 1000) {}

  clear() {
    this.lines = [[]];
    this.row = 0;
    this.col = 0;
    this.style = {};
  }

  write(data: string) {
    let i = 0;
    while (i < data.length) {
      const ch = data[i];
      if (ch === "\x1b") {
        i = this.consumeEscape(data, i);
        continue;
      }
      if (ch === "\r") {
        this.col = 0;
        i += 1;
        continue;
      }
      if (ch === "\n") {
        this.newLine();
        i += 1;
        continue;
      }
      if (ch === "\b") {
        this.col = Math.max(0, this.col - 1);
        i += 1;
        continue;
      }
      if (ch === "\t") {
        const nextTab = this.col + (8 - (this.col % 8));
        while (this.col < nextTab) this.putChar(" ");
        i += 1;
        continue;
      }
      if (ch < " " || ch === "\x7f") {
        i += 1;
        continue;
      }
      this.putChar(ch);
      i += 1;
    }
  }

  snapshot(): TerminalRenderLine[] {
    return this.lines.map((line, rowIndex) => {
      const cells = rowIndex === this.row ? this.withCursor(line) : line;
      const segments: TerminalRenderSegment[] = [];
      let currentKey = "";
      let currentText = "";
      let currentStyle: TerminalCellStyle = {};

      cells.forEach((cell, colIndex) => {
        const key = styleKey(cell);
        if (key !== currentKey && currentText) {
          segments.push({
            key: `${rowIndex}-${segments.length}`,
            text: currentText,
            style: currentStyle,
          });
          currentText = "";
        }
        currentKey = key;
        const { ch: _ch, ...style } = cell;
        currentStyle = style;
        currentText += cell.ch;
        if (colIndex === cells.length - 1 && currentText) {
          segments.push({
            key: `${rowIndex}-${segments.length}`,
            text: currentText,
            style: currentStyle,
          });
        }
      });

      if (segments.length === 0) {
        const cursor = rowIndex === this.row;
        segments.push({
          key: `${rowIndex}-empty`,
          text: cursor ? " " : "",
          style: cursor ? { cursor: true } : {},
        });
      }

      return { key: `line-${rowIndex}`, segments };
    });
  }

  toPlainText(): string {
    return this.lines.map((line) => line.map((cell) => cell.ch).join("")).join("\n");
  }

  private ensureLine(row = this.row) {
    while (this.lines.length <= row) this.lines.push([]);
  }

  private putChar(ch: string) {
    this.ensureLine();
    this.lines[this.row][this.col] = { ch, ...cloneStyle(this.style) };
    this.col += 1;
  }

  private newLine() {
    this.row += 1;
    this.col = 0;
    this.ensureLine();
    while (this.lines.length > this.maxLines) {
      this.lines.shift();
      this.row = Math.max(0, this.row - 1);
    }
  }

  private withCursor(line: TerminalCell[]): TerminalCell[] {
    const cells = line.slice();
    const current = cells[this.col] ?? { ch: " " };
    cells[this.col] = { ...current, cursor: true };
    return cells;
  }

  private consumeEscape(data: string, start: number): number {
    const next = data[start + 1];
    if (!next) return start + 1;

    if (next === "[") {
      let end = start + 2;
      while (end < data.length && !/[A-Za-z~]/u.test(data[end])) end += 1;
      if (end >= data.length) return data.length;
      this.handleCsi(data.slice(start + 2, end), data[end]);
      return end + 1;
    }

    if (next === "]") {
      let end = start + 2;
      while (end < data.length) {
        if (data[end] === "\x07") return end + 1;
        if (data[end] === "\x1b" && data[end + 1] === "\\") return end + 2;
        end += 1;
      }
      return data.length;
    }

    return start + 2;
  }

  private handleCsi(params: string, final: string) {
    const nums = parseNumbers(params);
    switch (final) {
      case "m":
        this.handleSgr(nums);
        return;
      case "K":
        this.eraseLine(nums[0] ?? 0);
        return;
      case "J":
        this.eraseDisplay(nums[0] ?? 0);
        return;
      case "G":
        this.col = Math.max(0, (nums[0] || 1) - 1);
        return;
      case "C":
        this.col += nums[0] || 1;
        return;
      case "D":
        this.col = Math.max(0, this.col - (nums[0] || 1));
        return;
      case "A":
        this.row = Math.max(0, this.row - (nums[0] || 1));
        return;
      case "B":
        this.row += nums[0] || 1;
        this.ensureLine();
        return;
      case "H":
      case "f":
        this.row = Math.max(0, (nums[0] || 1) - 1);
        this.col = Math.max(0, (nums[1] || 1) - 1);
        this.ensureLine();
        return;
      default:
        return;
    }
  }

  private eraseLine(mode: number) {
    this.ensureLine();
    const line = this.lines[this.row];
    if (mode === 2) {
      this.lines[this.row] = [];
      this.col = 0;
    } else if (mode === 1) {
      for (let i = 0; i <= this.col; i += 1) delete line[i];
    } else {
      line.length = this.col;
    }
  }

  private eraseDisplay(mode: number) {
    if (mode === 2 || mode === 3) {
      this.clear();
    }
  }

  private handleSgr(nums: number[]) {
    if (nums.length === 0) nums = [0];
    for (let i = 0; i < nums.length; i += 1) {
      const code = nums[i];
      if (code === 0) this.style = {};
      else if (code === 1) this.style.bold = true;
      else if (code === 2) this.style.dim = true;
      else if (code === 3) this.style.italic = true;
      else if (code === 4) this.style.underline = true;
      else if (code === 7) this.style.inverse = true;
      else if (code === 22) {
        this.style.bold = false;
        this.style.dim = false;
      } else if (code === 23) this.style.italic = false;
      else if (code === 24) this.style.underline = false;
      else if (code === 27) this.style.inverse = false;
      else if (code === 39) this.style.fg = undefined;
      else if (code === 49) this.style.bg = undefined;
      else if (code >= 30 && code <= 37) this.style.fg = BASIC_COLORS[code - 30];
      else if (code >= 40 && code <= 47) this.style.bg = BASIC_COLORS[code - 40];
      else if (code >= 90 && code <= 97) this.style.fg = BRIGHT_COLORS[code - 90];
      else if (code >= 100 && code <= 107) this.style.bg = BRIGHT_COLORS[code - 100];
      else if ((code === 38 || code === 48) && nums[i + 1] === 5) {
        const color = color256(nums[i + 2]);
        if (code === 38) this.style.fg = color;
        else this.style.bg = color;
        i += 2;
      } else if ((code === 38 || code === 48) && nums[i + 1] === 2) {
        const [r, g, b] = nums.slice(i + 2, i + 5);
        const color = `rgb(${r ?? 255}, ${g ?? 255}, ${b ?? 255})`;
        if (code === 38) this.style.fg = color;
        else this.style.bg = color;
        i += 4;
      }
    }
  }
}
