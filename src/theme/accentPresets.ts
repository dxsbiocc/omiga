import type { ThemeOptions } from "@mui/material/styles";
import { darken, lighten } from "@mui/material/styles";

// ── Seeded blend helpers: deterministic “random” mix per preset + mode ─────

function hexToRgb(hex: string): { r: number; g: number; b: number } | null {
  let h = hex.trim().replace("#", "");
  if (h.length === 3) {
    h = h
      .split("")
      .map((c) => c + c)
      .join("");
  }
  if (h.length !== 6) return null;
  const n = parseInt(h, 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
}

function relativeLuminance(hex: string): number {
  const rgb = hexToRgb(hex);
  if (!rgb) return 0;
  const r = rgb.r / 255;
  const g = rgb.g / 255;
  const b = rgb.b / 255;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function hexToRgbaString(hex: string, alpha: number): string {
  const rgb = hexToRgb(hex);
  if (!rgb) return `rgba(0,0,0,${alpha})`;
  return `rgba(${rgb.r},${rgb.g},${rgb.b},${alpha})`;
}

/** Dark swatches on light gradients: lift toward paper so bands stay soft. */
function lightenStopForGradientSwatch(hex: string): string {
  const lum = relativeLuminance(hex);
  if (lum < 0.2) return mixHex(hex, "#ffffff", 0.78);
  if (lum < 0.38) return mixHex(hex, "#ffffff", 0.42);
  return hex;
}

function shuffleInPlace<T>(arr: T[], rng: () => number): void {
  for (let i = arr.length - 1; i > 0; i--) {
    const j = Math.floor(rng() * (i + 1));
    [arr[i], arr[j]] = [arr[j]!, arr[i]!];
  }
}

/** Linear RGB mix (0 = a, 1 = b). */
function mixHex(a: string, b: string, t: number): string {
  const A = hexToRgb(a);
  const B = hexToRgb(b);
  if (!A || !B) return a;
  const u = Math.max(0, Math.min(1, t));
  const r = Math.round(A.r + (B.r - A.r) * u);
  const g = Math.round(A.g + (B.g - A.g) * u);
  const bl = Math.round(A.b + (B.b - A.b) * u);
  return `#${[r, g, bl].map((x) => x.toString(16).padStart(2, "0")).join("")}`;
}

function hashSeed(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

/** Deterministic PRNG for stable colors across renders. */
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    let t = (a += 0x6d2b79f5);
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * Optional accent palettes (brand-inspired swatches). Merges into `getTheme(mode)`:
 * semantic colors from each preset plus `background` / `surface` from {@link getAccentAppChrome}.
 */
export type AccentPresetId =
  | "asana"
  | "elastic"
  | "your_name"
  | "magica_madoka"
  | "water_color"
  | "wufoo"
  | "socialbro"
  | "redox"
  | "emma"
  | "dribbble";

export const ACCENT_PRESET_IDS: AccentPresetId[] = [
  "asana",
  "elastic",
  "your_name",
  "magica_madoka",
  "water_color",
  "wufoo",
  "socialbro",
  "redox",
  "emma",
  "dribbble",
];

export const ACCENT_PRESET_META: Record<
  AccentPresetId,
  { label: string; description: string; swatches: string[] }
> = {
  asana: {
    label: "Asana",
    description:
      "Mint, cyan, purple, amber, coral — inspired by Asana brand colors.",
    swatches: ["#3be8b0", "#1aafd0", "#6a67ce", "#ffb900", "#fc636b"],
  },
  elastic: {
    label: "Elastic Search",
    description:
      "Pink, yellow, green, teal, blues — inspired by Elastic brand palette.",
    swatches: [
      "#d7689d",
      "#f3d337",
      "#a0c443",
      "#66b5ae",
      "#5da4dc",
    ],
  },
  your_name: {
    label: "Your Name",
    description: "Muted teal, plum, coral, amber, and shell peach.",
    swatches: ["#376888", "#826b88", "#de786a", "#f8b976", "#fcddc9"],
  },
  magica_madoka: {
    label: "Magica Madoka",
    description: "Sakura pink, butter yellow, sky blue, ink, and wine.",
    swatches: ["#ffb6bb", "#ffe691", "#95d5ee", "#585872", "#9e4446"],
  },
  water_color: {
    label: "Water Color",
    description: "Sage, honey, umber, ink black, and rust.",
    swatches: ["#81947a", "#deaa6e", "#8e7967", "#0c0a08", "#832f0e"],
  },
  wufoo: {
    label: "Wufoo",
    description: "Coral, lavender, cyan, butter, and lime.",
    swatches: ["#e66760", "#8c88cd", "#69c5e4", "#ffdf8b", "#61e064"],
  },
  socialbro: {
    label: "SocialBro",
    description: "Teal, tangerine, sage, magenta, and azure.",
    swatches: ["#29c4d0", "#f29556", "#84afa2", "#f24c7c", "#00aaf2"],
  },
  redox: {
    label: "Redox",
    description: "Violet, coral, amber, jade, and navy.",
    swatches: ["#854c9e", "#e5505a", "#ff9f1c", "#01b287", "#0a2239"],
  },
  emma: {
    label: "Emma",
    description: "Slate, aqua, gold, mint, and coral.",
    swatches: ["#314855", "#5cc3e8", "#ffdb00", "#79ceb8", "#e95f5c"],
  },
  dribbble: {
    label: "Dribbble",
    description: "Charcoal, pink, green, orange, and cyan.",
    swatches: ["#444444", "#ea4c89", "#8aba56", "#ff8833", "#00b6e3"],
  },
};

/**
 * Preset preview / app shell: linear gradient from swatches in a **seeded random order**
 * (stable per preset + light/dark). Light uses washed stops; dark uses dark base + translucent swatches.
 */
export function getAccentSwatchGradient(
  preset: AccentPresetId,
  mode: "light" | "dark",
): string {
  const rng = mulberry32(hashSeed(`omiga:swatchGradient:${preset}:${mode}`));
  const colors = [...ACCENT_PRESET_META[preset].swatches];
  shuffleInPlace(colors, rng);
  const angle = 105 + Math.floor(rng() * 75);

  if (mode === "light") {
    const diluted = colors.map(lightenStopForGradientSwatch);
    if (diluted.length === 1) {
      const c = diluted[0]!;
      return `linear-gradient(${angle}deg, ${mixHex(c, "#ffffff", 0.4)} 0%, ${c} 100%)`;
    }
    const stops = diluted.map((c, i) => {
      const pct = (i / (diluted.length - 1)) * 100;
      return `${c} ${pct}%`;
    });
    return `linear-gradient(${angle}deg, ${stops.join(", ")})`;
  }

  const base = "#0a0c10";
  const parts: string[] = [`${base} 0%`];
  const n = colors.length;
  colors.forEach((c, i) => {
    const pct = 10 + ((i + 1) * 78) / Math.max(1, n + 1);
    const a = 0.16 + rng() * 0.22;
    parts.push(`${hexToRgbaString(c, a)} ${pct}%`);
  });
  parts.push(`${base} 100%`);
  return `linear-gradient(${angle}deg, ${parts.join(", ")})`;
}

/** Light / dark / system mode tiles — swatches shuffled by seed per mode. */
export function getColorModePickerGradient(
  mode: "light" | "dark" | "system",
): string {
  const rng = mulberry32(hashSeed(`omiga:colorModeGradient:${mode}`));
  const angle = 118 + Math.floor(rng() * 62);
  if (mode === "light") {
    const colors = ["#ffffff", "#fffbeb", "#fef3c7", "#fefce8", "#fff7ed"];
    shuffleInPlace(colors, rng);
    const stops = colors.map((c, i) => {
      const pct = (i / (colors.length - 1)) * 100;
      return `${c} ${pct}%`;
    });
    return `linear-gradient(${angle}deg, ${stops.join(", ")})`;
  }
  if (mode === "dark") {
    const colors = ["#020617", "#1e293b", "#312e81", "#0f172a", "#1e1b4b"];
    shuffleInPlace(colors, rng);
    const base = "#020617";
    const parts: string[] = [`${base} 0%`];
    colors.forEach((c, i) => {
      const pct = 12 + ((i + 1) * 72) / (colors.length + 1);
      const rgb = hexToRgb(c);
      if (!rgb) return;
      const a = 0.32 + rng() * 0.28;
      parts.push(`rgba(${rgb.r},${rgb.g},${rgb.b},${a}) ${pct}%`);
    });
    parts.push(`${base} 100%`);
    return `linear-gradient(${angle}deg, ${parts.join(", ")})`;
  }
  const colors = ["#e0e7ff", "#334155", "#f1f5f9", "#c7d2fe", "#64748b", "#94a3b8"];
  shuffleInPlace(colors, rng);
  const stops = colors.map((c, i) => {
    const pct = (i / (colors.length - 1)) * 100;
    return `${c} ${pct}%`;
  });
  return `linear-gradient(${angle}deg, ${stops.join(", ")})`;
}

function tone(main: string): {
  main: string;
  light: string;
  dark: string;
  contrastText: string;
} {
  const r = parseInt(main.slice(1, 3), 16) / 255;
  const g = parseInt(main.slice(3, 5), 16) / 255;
  const b = parseInt(main.slice(5, 7), 16) / 255;
  const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  return {
    main,
    light: lighten(main, 0.22),
    dark: darken(main, 0.2),
    contrastText: lum > 0.58 ? "#0f172a" : "#ffffff",
  };
}

/**
 * App chrome (page background + elevated surfaces) per accent preset, keyed by light/dark.
 * Colors are deterministically blended from {@link ACCENT_PRESET_META} swatches (seeded “random”
 * mix) so each preset + mode pair is stable across renders but visually distinct.
 */
export type AccentAppChrome = {
  background: { default: string; paper: string };
  surface: { main: string; light: string; dark: string };
};

/**
 * Blend 3 randomly (seeded) picked swatches, then adapt for light (wash toward white) or dark
 * (wash toward near-black). Paper is slightly elevated; surfaces use MUI lighten/darken.
 */
export function getAccentAppChrome(
  preset: AccentPresetId,
  mode: "light" | "dark",
): AccentAppChrome {
  const swatches = ACCENT_PRESET_META[preset].swatches;
  const rng = mulberry32(hashSeed(`omiga:accent:${preset}:${mode}`));

  const pick = () => swatches[Math.floor(rng() * swatches.length)]!;
  const a = pick();
  const b = pick();
  const c = pick();
  const t1 = 0.18 + rng() * 0.55;
  const t2 = 0.12 + rng() * 0.48;
  const blend = mixHex(mixHex(a, b, t1), c, t2);

  if (mode === "light") {
    const towardWhite = 0.74 + rng() * 0.16;
    const defaultBg = mixHex(blend, "#ffffff", towardWhite);
    const paperLift = 0.22 + rng() * 0.2;
    const paperBg = mixHex(defaultBg, "#ffffff", paperLift);
    return {
      background: { default: defaultBg, paper: paperBg },
      surface: {
        main: darken(defaultBg, 0.045 + rng() * 0.055),
        light: lighten(defaultBg, 0.02 + rng() * 0.04),
        dark: darken(defaultBg, 0.09 + rng() * 0.07),
      },
    };
  }

  const towardBlack = 0.7 + rng() * 0.16;
  const defaultBg = mixHex(blend, "#050508", towardBlack);
  const paperBg = lighten(defaultBg, 0.1 + rng() * 0.09);
  return {
    background: { default: defaultBg, paper: paperBg },
    surface: {
      main: lighten(defaultBg, 0.05 + rng() * 0.05),
      light: lighten(defaultBg, 0.1 + rng() * 0.08),
      dark: darken(defaultBg, 0.04 + rng() * 0.05),
    },
  };
}

function mergeAccentChrome(
  preset: AccentPresetId,
  mode: "light" | "dark",
  palette: NonNullable<ThemeOptions["palette"]>,
): NonNullable<ThemeOptions["palette"]> {
  const chrome = getAccentAppChrome(preset, mode);
  return {
    ...palette,
    /** Required so deepmerge + createPalette consumers always see correct mode */
    mode,
    background: chrome.background,
    surface: chrome.surface,
  };
}

/** Deep-merge into `getTheme(mode)` via `createTheme(base, options)`. */
export function getAccentPresetOptions(
  preset: AccentPresetId,
  mode: "light" | "dark",
): ThemeOptions {
  switch (preset) {
    case "asana":
      return {
        palette: mergeAccentChrome(preset, mode, {
          primary: {
            main: "#1aafd0",
            light: "#4dc4db",
            dark: "#1488a5",
            contrastText: "#ffffff",
          },
          secondary: {
            main: "#6a67ce",
            light: "#8b89d8",
            dark: "#4f4bb8",
            contrastText: "#ffffff",
          },
          success: {
            main: "#3be8b0",
            light: "#6befc8",
            dark: "#2bc49a",
            contrastText: "#0f172a",
          },
          warning: {
            main: "#ffb900",
            light: "#ffca3d",
            dark: "#d99f00",
            contrastText: "#0f172a",
          },
          error: {
            main: "#fc636b",
            light: "#fd8a90",
            dark: "#e04550",
            contrastText: "#ffffff",
          },
          info: {
            main: "#1aafd0",
            light: "#4dc4db",
            dark: "#1488a5",
            contrastText: "#ffffff",
          },
        }),
      };
    case "elastic":
      return {
        palette: mergeAccentChrome(preset, mode, {
          primary: {
            main: "#5da4dc",
            light: "#85bce6",
            dark: "#4589bd",
            contrastText: "#ffffff",
          },
          secondary: {
            main: "#66b5ae",
            light: "#8ec9c4",
            dark: "#4a9a92",
            contrastText: "#0f172a",
          },
          success: {
            main: "#a0c443",
            light: "#b8d06a",
            dark: "#7fa032",
            contrastText: "#0f172a",
          },
          warning: {
            main: "#f3d337",
            light: "#f6de65",
            dark: "#c9b02a",
            contrastText: "#0f172a",
          },
          error: {
            main: "#d7689d",
            light: "#e38fb7",
            dark: "#b84d7f",
            contrastText: "#ffffff",
          },
          info: {
            main: "#4a8ab8",
            light: "#6ba3cb",
            dark: "#356891",
            contrastText: "#ffffff",
          },
        }),
      };
    case "your_name":
      return {
        palette: mergeAccentChrome(preset, mode, {
          primary: tone("#376888"),
          secondary: tone("#826b88"),
          error: tone("#de786a"),
          warning: tone("#f8b976"),
          success: tone("#5a8f82"),
          info: tone("#4a7a9e"),
        }),
      };
    case "magica_madoka":
      return {
        palette: mergeAccentChrome(preset, mode, {
          primary: tone("#ffb6bb"),
          secondary: tone("#95d5ee"),
          warning: tone("#ffe691"),
          error: tone("#9e4446"),
          success: tone("#7ec4a8"),
          info: tone("#585872"),
        }),
      };
    case "water_color":
      return {
        palette: mergeAccentChrome(preset, mode, {
          primary: tone("#81947a"),
          secondary: tone("#8e7967"),
          warning: tone("#deaa6e"),
          error: tone("#832f0e"),
          success: tone("#6b8f5e"),
          /** #0c0a08 — ink from palette */
          info: tone("#0c0a08"),
        }),
      };
    case "wufoo":
      return {
        palette: mergeAccentChrome(preset, mode, {
          primary: tone("#e66760"),
          secondary: tone("#8c88cd"),
          info: tone("#69c5e4"),
          warning: tone("#ffdf8b"),
          success: tone("#61e064"),
          error: tone("#d94a42"),
        }),
      };
    case "socialbro":
      return {
        palette: mergeAccentChrome(preset, mode, {
          primary: tone("#29c4d0"),
          secondary: tone("#f29556"),
          success: tone("#84afa2"),
          error: tone("#f24c7c"),
          info: tone("#00aaf2"),
          warning: tone("#f2b054"),
        }),
      };
    case "redox":
      return {
        palette: mergeAccentChrome(preset, mode, {
          primary: tone("#854c9e"),
          secondary: tone("#e5505a"),
          warning: tone("#ff9f1c"),
          success: tone("#01b287"),
          error: tone("#c73e48"),
          info: tone("#0a2239"),
        }),
      };
    case "emma":
      return {
        palette: mergeAccentChrome(preset, mode, {
          primary: tone("#314855"),
          secondary: tone("#5cc3e8"),
          warning: tone("#ffdb00"),
          success: tone("#79ceb8"),
          error: tone("#e95f5c"),
          info: tone("#4a9eb8"),
        }),
      };
    case "dribbble":
      return {
        palette: mergeAccentChrome(preset, mode, {
          primary: tone("#ea4c89"),
          secondary: tone("#8aba56"),
          warning: tone("#ff8833"),
          info: tone("#00b6e3"),
          error: tone("#d63d6f"),
          success: tone("#6dad3f"),
        }),
      };
    default:
      return {};
  }
}
