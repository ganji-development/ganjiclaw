import { useState, useEffect, useCallback, type ReactNode } from 'react';
import { colorThemeMap, DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME, type ColorThemeId } from './colorThemes';
import {
  type ThemeMode,
  type AccentColor,
  type UiFont,
  type MonoFont,
  type StoredTheme,
  type ThemeContextValue,
  ThemeContext,
  loadStored,
  STORAGE_KEY,
  accents,
  applyVars,
  fontVars,
  loadUiFont,
  loadMonoFont,
  resolveColorTheme,
  resolveThemeScheme,
} from './ThemeContextUtils';


export function ThemeProvider({ children }: { children: ReactNode }) {
  const [stored] = useState(loadStored);
  const [theme, setThemeState] = useState<ThemeMode>(stored.theme);
  const [accent, setAccentState] = useState<AccentColor>(stored.accent);
  const [colorTheme, setColorThemeState] = useState<ColorThemeId>(stored.colorTheme);
  const [uiFont, setUiFontState] = useState<UiFont>(stored.uiFont);
  const [monoFont, setMonoFontState] = useState<MonoFont>(stored.monoFont);
  const [uiFontSize, setUiFontSizeState] = useState<number>(stored.uiFontSize);
  const [monoFontSize, setMonoFontSizeState] = useState<number>(stored.monoFontSize);

  const persist = useCallback((s: StoredTheme) => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  }, []);

  const applyAll = useCallback((s: StoredTheme) => {
    const resolvedId = resolveColorTheme(s.theme, s.colorTheme);
    const ct = colorThemeMap[resolvedId];
    const themeVars = ct?.vars ?? colorThemeMap[DEFAULT_DARK_THEME].vars;
    applyVars({
      ...themeVars,
      ...accents[s.accent],
      ...fontVars(s.uiFont, s.monoFont, s.uiFontSize, s.monoFontSize),
    });
  }, []);

  const setTheme = useCallback((t: ThemeMode) => {
    // Auto-select a color theme matching the new scheme so explicit mode choices apply correctly.
    const currentCt = colorThemeMap[colorTheme];
    const targetScheme = t === 'oled' ? 'dark' : t === 'system' ? null : t;
    const newColorTheme: ColorThemeId =
      // System mode should preserve the user's current palette and defer scheme resolution to OS preference.
      targetScheme === null || (currentCt && currentCt.scheme === targetScheme) ? colorTheme : (
        t === 'oled' ? 'oled-black' :
          t === 'light' ? DEFAULT_LIGHT_THEME :
            DEFAULT_DARK_THEME
      );
    setThemeState(t);
    setColorThemeState(newColorTheme);
    const next = { theme: t, accent, colorTheme: newColorTheme, uiFont, monoFont, uiFontSize, monoFontSize };
    applyAll(next);
    persist(next);
  }, [accent, colorTheme, uiFont, monoFont, uiFontSize, monoFontSize, applyAll, persist]);

  const setAccent = useCallback((a: AccentColor) => {
    setAccentState(a);
    const next: StoredTheme = { theme, accent: a, colorTheme, uiFont, monoFont, uiFontSize, monoFontSize };
    applyAll(next);
    persist(next);
  }, [theme, colorTheme, uiFont, monoFont, uiFontSize, monoFontSize, applyAll, persist]);

  const setColorTheme = useCallback((c: ColorThemeId) => {
    setColorThemeState(c);
    // Only update the color theme — do NOT override the user's explicit theme mode.
    // setTheme handles syncing colorTheme when the mode changes.
    const next: StoredTheme = { theme, accent, colorTheme: c, uiFont, monoFont, uiFontSize, monoFontSize };
    applyAll(next);
    persist(next);
  }, [theme, accent, uiFont, monoFont, uiFontSize, monoFontSize, applyAll, persist]);

  const setUiFont = useCallback((f: UiFont) => {
    setUiFontState(f);
    loadUiFont(f);
    const next: StoredTheme = { theme, accent, colorTheme, uiFont: f, monoFont, uiFontSize, monoFontSize };
    applyAll(next);
    persist(next);
  }, [theme, accent, colorTheme, applyAll, persist, monoFont, uiFontSize, monoFontSize]);

  const setMonoFont = useCallback((f: MonoFont) => {
    setMonoFontState(f);
    loadMonoFont(f);
    const next: StoredTheme = { theme, accent, colorTheme, uiFont, monoFont: f, uiFontSize, monoFontSize };
    applyAll(next);
    persist(next);
  }, [theme, accent, colorTheme, applyAll, persist, uiFont, uiFontSize, monoFontSize]);

  const setUiFontSize = useCallback((size: number) => {
    const clamped = Math.min(20, Math.max(12, size));
    setUiFontSizeState(clamped);
    const next: StoredTheme = { theme, accent, colorTheme, uiFont, monoFont, uiFontSize: clamped, monoFontSize };
    applyAll(next);
    persist(next);
  }, [theme, accent, colorTheme, applyAll, persist, uiFont, monoFont, monoFontSize]);

  const setMonoFontSize = useCallback((size: number) => {
    const clamped = Math.min(20, Math.max(12, size));
    setMonoFontSizeState(clamped);
    const next: StoredTheme = { theme, accent, colorTheme, uiFont, monoFont, uiFontSize, monoFontSize: clamped };
    applyAll(next);
    persist(next);
  }, [theme, accent, colorTheme, applyAll, persist, uiFont, monoFont, uiFontSize]);

  useEffect(() => {
    applyAll({ theme, accent, colorTheme, uiFont, monoFont, uiFontSize, monoFontSize });
    loadUiFont(uiFont);
    loadMonoFont(monoFont);
  }, [theme, accent, colorTheme, uiFont, monoFont, uiFontSize, monoFontSize, applyAll]);

  useEffect(() => {
    if (theme !== 'system') return;
    const mq = window.matchMedia('(prefers-color-scheme: light)');
    const handler = () => applyAll({ theme, accent, colorTheme, uiFont, monoFont, uiFontSize, monoFontSize });
    mq.addEventListener('change', handler);
    return () => mq.removeEventListener('change', handler);
  }, [theme, accent, colorTheme, uiFont, monoFont, uiFontSize, monoFontSize, applyAll]);

  const resolvedTheme = resolveThemeScheme(theme, colorTheme);

  const value: ThemeContextValue = {
    theme, accent, colorTheme, uiFont, monoFont, uiFontSize, monoFontSize,
    resolvedTheme, setTheme, setAccent, setColorTheme, setUiFont, setMonoFont, setUiFontSize, setMonoFontSize,
  };

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}
