// Single source of truth for colours. CSS reads these as custom properties
// (set on <html> at runtime); the 3-D scene and charts read them directly.
// Series hues follow a CVD-validated categorical order: blue, orange, aqua.

export type ThemeMode = "light" | "dark";

export interface Palette {
  bg: string;
  /** Brand blue: selection, focus, primary actions */
  accent: string;
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
    bg: "#f3f4f7",
    accent: "#2563eb",
    surface: "#ffffff",
    surface2: "#eef0f4",
    border: "#dadde5",
    text: "#0b0d14",
    text2: "#4a5062",
    muted: "#6b7183",
    grid: "#e7e9ef",
    blue: "#2a78d6",
    orange: "#eb6834",
    aqua: "#1baf7a",
    critical: "#c62f2f",
    sky: "#e8edf2",
    ground: "#d3d8c7",
    groundLine: "#aab19c",
  },
  dark: {
    bg: "#08090d",
    accent: "#4f8ff7",
    surface: "#10121a",
    surface2: "#181b25",
    border: "#262a37",
    text: "#eef0f6",
    text2: "#b6bbc9",
    muted: "#7f8597",
    grid: "#1d2029",
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
