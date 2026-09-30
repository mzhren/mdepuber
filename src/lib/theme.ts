/**
 * App colour scheme.
 *
 * The resolved theme is written to `<html data-theme="…">` (which drives every
 * CSS token) and mirrored onto the native window via `Window::setTheme`. That
 * second part matters: `prefers-color-scheme` inside the webview follows the
 * window theme, and mdBook's own theme picker keys off `prefers-color-scheme`,
 * so the embedded mdBook preview switches along with the app chrome without a
 * rebuild.
 */

export type Theme = "light" | "dark";

const STORAGE_KEY = "mdepuber-theme";

export const THEMES: Theme[] = ["dark", "light"];

export const THEME_LABEL: Record<Theme, string> = {
  dark: "深色",
  light: "浅色",
};

/** Stored preference if there is one, otherwise whatever the OS reports. */
export function resolveInitialTheme(): Theme {
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved === "light" || saved === "dark") return saved;
  } catch {
    // localStorage can be unavailable; fall through to the system preference.
  }
  return window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

export function persistTheme(theme: Theme): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // Not fatal — the theme just will not survive a restart.
  }
}

/** Apply the theme to the document. */
export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
}

export function otherTheme(theme: Theme): Theme {
  return theme === "dark" ? "light" : "dark";
}
