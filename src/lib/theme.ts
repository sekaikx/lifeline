// Single source of truth for colours. CSS reads these as custom properties
// (set on <html> at runtime); the 3-D scene and charts read them directly.
// Series hues follow a CVD-validated categorical order: blue, orange, aqua.

export type ThemeMode = "light" | "dark";

export interface Palette {
  bg: string;
  surface: string;
  surface2: string;
  border: string;
  text: string;
  text2: string;
  muted: string;
  grid: string;
  blue: string;
  orange: string;
  aqua: string;
  critical: string;
  sky: string;
  ground: string;
  groundLine: string;
}

export const palettes: Record<ThemeMode, Palette> = {
  light: {
    bg: "#f3f2ef",
    surface: "#fcfcfb",
    surface2: "#efeee9",
    border: "#dcdad3",
    text: "#0b0b0b",
    text2: "#52514e",
    muted: "#6f6d68",
    grid: "#e6e4de",
    blue: "#2a78d6",
    orange: "#eb6834",
    aqua: "#1baf7a",
    critical: "#c62f2f",
    sky: "#e8edf2",
    ground: "#d3d8c7",
    groundLine: "#aab19c",
  },
  dark: {
    bg: "#121211",
    surface: "#1a1a19",
    surface2: "#242422",
    border: "#33322f",
    text: "#f0efec",
    text2: "#c3c2b7",
    muted: "#8d8b83",
    grid: "#2c2b28",
    blue: "#3987e5",
    orange: "#d95926",
    aqua: "#199e70",
    critical: "#e66767",
    sky: "#0e1115",
    ground: "#1f241c",
    groundLine: "#3a4234",
  },
};

export function applyTheme(mode: ThemeMode): void {
  const root = document.documentElement;
  root.dataset.theme = mode;
  root.style.colorScheme = mode;
  for (const [key, value] of Object.entries(palettes[mode])) {
    root.style.setProperty(`--${key}`, value);
  }
}

export function initialTheme(): ThemeMode {
  try {
    const saved = localStorage.getItem("lifeline.theme");
    if (saved === "light" || saved === "dark") return saved;
  } catch {
    /* storage unavailable */
  }
  return window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
}
