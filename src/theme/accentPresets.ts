import type { ThemeOptions } from "@mui/material/styles";
import { darken, lighten } from "@mui/material/styles";

/**
 * Optional accent palettes (brand-inspired swatches). Only overrides `palette`;
 * light/dark surfaces still come from `getTheme(mode)`.
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
 * Distinct gradient backdrops for the theme picker cards (light vs dark UI).
 * UI/UX: preview surfaces echo each preset’s palette without affecting app chrome.
 */
export const ACCENT_PICKER_SURFACES: Record<
  AccentPresetId,
  { gradientLight: string; gradientDark: string }
> = {
  asana: {
    gradientLight:
      "linear-gradient(135deg, #ccfbf1 0%, #e0f2fe 38%, #ede9fe 72%, #fffbeb 100%)",
    gradientDark:
      "linear-gradient(135deg, #0f172a 0%, rgba(26,175,208,0.22) 42%, rgba(106,103,206,0.2) 78%, #172554 100%)",
  },
  elastic: {
    gradientLight:
      "linear-gradient(118deg, #fce7f3 0%, #fef9c3 20%, #d9f99d 40%, #a5f3fc 62%, #bfdbfe 84%, #e0e7ff 100%)",
    gradientDark:
      "linear-gradient(118deg, #14101c 0%, rgba(215,104,157,0.2) 26%, rgba(243,211,55,0.1) 48%, rgba(95,164,220,0.22) 72%, rgba(61,122,168,0.22) 100%)",
  },
  your_name: {
    gradientLight:
      "linear-gradient(140deg, rgba(55,104,136,0.14) 0%, rgba(130,107,136,0.12) 38%, rgba(248,185,118,0.2) 72%, #fcddc9 100%)",
    gradientDark:
      "linear-gradient(140deg, #0f172a 0%, rgba(55,104,136,0.28) 42%, rgba(222,120,106,0.15) 78%, #1a1520 100%)",
  },
  magica_madoka: {
    gradientLight:
      "linear-gradient(125deg, #ffe4e6 0%, #fff9db 32%, #e0f2fe 64%, #e8e9f2 100%)",
    gradientDark:
      "linear-gradient(125deg, #1e1b2e 0%, rgba(255,182,187,0.18) 35%, rgba(149,213,238,0.2) 65%, rgba(88,88,114,0.35) 100%)",
  },
  water_color: {
    gradientLight:
      "linear-gradient(145deg, #e8ebe4 0%, #f5e6d3 35%, #d4c9bf 70%, #f0ede8 100%)",
    gradientDark:
      "linear-gradient(145deg, #0a0908 0%, rgba(129,148,122,0.25) 40%, rgba(222,170,110,0.12) 72%, rgba(131,47,14,0.22) 100%)",
  },
  wufoo: {
    gradientLight:
      "linear-gradient(130deg, #fce8e7 0%, #ede9fc 28%, #e0f7fa 55%, #fffbeb 82%, #ecfdf5 100%)",
    gradientDark:
      "linear-gradient(130deg, #1a1520 0%, rgba(230,103,96,0.2) 32%, rgba(140,136,205,0.22) 58%, rgba(105,197,228,0.15) 100%)",
  },
  socialbro: {
    gradientLight:
      "linear-gradient(118deg, #e0f7fa 0%, #fff4e6 30%, #e8f2ef 58%, #fce4ec 85%, #e1f5fe 100%)",
    gradientDark:
      "linear-gradient(118deg, #0f1724 0%, rgba(41,196,208,0.2) 35%, rgba(242,149,86,0.14) 55%, rgba(242,76,124,0.16) 100%)",
  },
  redox: {
    gradientLight:
      "linear-gradient(135deg, #f3e8ff 0%, #ffe4e6 28%, #fff7ed 55%, #ecfdf5 82%, #e0e7ef 100%)",
    gradientDark:
      "linear-gradient(135deg, #060d18 0%, rgba(133,76,158,0.28) 38%, rgba(229,80,90,0.18) 62%, rgba(1,178,135,0.15) 100%)",
  },
  emma: {
    gradientLight:
      "linear-gradient(132deg, #e8f7fc 0%, #fffbeb 35%, #ecfdf8 68%, #fce8e7 100%)",
    gradientDark:
      "linear-gradient(132deg, #0c1418 0%, rgba(92,195,232,0.22) 40%, rgba(255,219,0,0.1) 62%, rgba(121,206,184,0.14) 100%)",
  },
  dribbble: {
    gradientLight:
      "linear-gradient(120deg, #f4f4f4 0%, #fce7f3 22%, #f0fdf4 48%, #fff7ed 72%, #e0f7fa 100%)",
    gradientDark:
      "linear-gradient(120deg, #0f0f0f 0%, rgba(234,76,137,0.22) 35%, rgba(138,186,86,0.14) 58%, rgba(0,182,227,0.2) 100%)",
  },
};

/** Light / dark / system mode tiles — each with its own preview gradient */
export const COLOR_MODE_PICKER_SURFACES: Record<
  "light" | "dark" | "system",
  { gradient: string }
> = {
  light: {
    gradient:
      "linear-gradient(165deg, #ffffff 0%, #fffbeb 40%, #fef3c7 100%)",
  },
  dark: {
    gradient:
      "linear-gradient(165deg, #020617 0%, #1e293b 45%, #312e81 100%)",
  },
  system: {
    gradient:
      "linear-gradient(125deg, #e0e7ff 0%, #334155 38%, #f1f5f9 62%, #c7d2fe 100%)",
  },
};

/** Deep-merge into `getTheme(mode)` via `createTheme(base, options)`. */
export function getAccentPresetOptions(
  preset: AccentPresetId,
  _mode: "light" | "dark",
): ThemeOptions {
  switch (preset) {
    case "asana":
      return {
        palette: {
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
        },
      };
    case "elastic":
      return {
        palette: {
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
        },
      };
    case "your_name":
      return {
        palette: {
          primary: tone("#376888"),
          secondary: tone("#826b88"),
          error: tone("#de786a"),
          warning: tone("#f8b976"),
          success: tone("#5a8f82"),
          info: tone("#4a7a9e"),
        },
      };
    case "magica_madoka":
      return {
        palette: {
          primary: tone("#ffb6bb"),
          secondary: tone("#95d5ee"),
          warning: tone("#ffe691"),
          error: tone("#9e4446"),
          success: tone("#7ec4a8"),
          info: tone("#585872"),
        },
      };
    case "water_color":
      return {
        palette: {
          primary: tone("#81947a"),
          secondary: tone("#8e7967"),
          warning: tone("#deaa6e"),
          error: tone("#832f0e"),
          success: tone("#6b8f5e"),
          /** #0c0a08 — ink from palette */
          info: tone("#0c0a08"),
        },
      };
    case "wufoo":
      return {
        palette: {
          primary: tone("#e66760"),
          secondary: tone("#8c88cd"),
          info: tone("#69c5e4"),
          warning: tone("#ffdf8b"),
          success: tone("#61e064"),
          error: tone("#d94a42"),
        },
      };
    case "socialbro":
      return {
        palette: {
          primary: tone("#29c4d0"),
          secondary: tone("#f29556"),
          success: tone("#84afa2"),
          error: tone("#f24c7c"),
          info: tone("#00aaf2"),
          warning: tone("#f2b054"),
        },
      };
    case "redox":
      return {
        palette: {
          primary: tone("#854c9e"),
          secondary: tone("#e5505a"),
          warning: tone("#ff9f1c"),
          success: tone("#01b287"),
          error: tone("#c73e48"),
          info: tone("#0a2239"),
        },
      };
    case "emma":
      return {
        palette: {
          primary: tone("#314855"),
          secondary: tone("#5cc3e8"),
          warning: tone("#ffdb00"),
          success: tone("#79ceb8"),
          error: tone("#e95f5c"),
          info: tone("#4a9eb8"),
        },
      };
    case "dribbble":
      return {
        palette: {
          primary: tone("#ea4c89"),
          secondary: tone("#8aba56"),
          warning: tone("#ff8833"),
          info: tone("#00b6e3"),
          error: tone("#d63d6f"),
          success: tone("#6dad3f"),
        },
      };
    default:
      return {};
  }
}
