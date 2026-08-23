/** Mirrors the types `termdeck-core` serialises across the Tauri boundary. */

export type Mode = "light" | "dark";

export interface Ansi {
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  bright_black: string;
  bright_red: string;
  bright_green: string;
  bright_yellow: string;
  bright_blue: string;
  bright_magenta: string;
  bright_cyan: string;
  bright_white: string;
}

export interface Palette {
  background: string;
  foreground: string;
  cursor: string;
  cursor_text: string;
  selection_background: string;
  selection_foreground: string;
  accent: string;
  subtle: string;
  surface: string;
  ansi: Ansi;
}

export interface Theme {
  id: string;
  name: string;
  family: string;
  mode: Mode;
  tmux: { kind: "catppuccin_flavour"; value: string } | { kind: "palette" };
  palette: Palette;
}

export interface Detection {
  id: string;
  name: string;
  installed: boolean;
  paths: string[];
  detail: string;
  current_theme: string | null;
}

export interface ApplyOptions {
  live: boolean;
  claude_use_ansi: boolean;
  iterm_all_profiles: boolean;
}

export interface Settings {
  theme: string | null;
  light_theme: string;
  dark_theme: string;
  enabled: string[];
  options: ApplyOptions;
}

export type Status = "applied" | "pending" | "skipped" | "failed";

export interface Outcome {
  id: string;
  name: string;
  status: Status;
  message: string;
  changed: string[];
  follow_up: string | null;
}

export interface State {
  themes: Theme[];
  targets: Detection[];
  settings: Settings;
  detected_theme: string | null;
}

/** The ANSI slots in the order a terminal numbers them, for the previews. */
export const ANSI_ORDER: (keyof Ansi)[] = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "bright_black",
  "bright_red",
  "bright_green",
  "bright_yellow",
  "bright_blue",
  "bright_magenta",
  "bright_cyan",
  "bright_white",
];
