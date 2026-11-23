import React, { createContext, useContext } from 'react';
import { useColorScheme } from 'react-native';

export type ThemeMode = 'system' | 'dark' | 'light';
export type DarkStyleId = 'classic' | 'midnight' | 'forest' | 'plum';
export type LightStyleId = 'classic' | 'ivory' | 'sage' | 'rose';

export interface Palette {
  id: string;
  name: string;
  dark: boolean;
  bg: string;
  surface: string;
  surfaceElevated: string;
  border: string;
  hairline: string;
  text: string;
  muted: string;
  dim: string;
  accent: string;
  accentBg: string;
  green: string;
  greenBg: string;
  amber: string;
  amberBg: string;
  red: string;
  redBg: string;
  codeBg: string;
  inlineCodeBg: string;
  inlineCodeText: string;
  userBubble: string;
  scrollbar: string;
}

export const DARK_THEMES: Record<DarkStyleId, Palette> = {
  classic: {
    id: 'classic',
    name: 'Classic',
    dark: true,
    bg: '#191919',
    surface: '#202020',
    surfaceElevated: '#29292c',
    border: '#323230',
    hairline: '#262628',
    text: '#e8e8e6',
    muted: '#a3a39f',
    dim: '#777773',
    accent: '#8b7cff',
    accentBg: 'rgba(139, 124, 255, 0.16)',
    green: '#4cc386',
    greenBg: 'rgba(76, 195, 134, 0.14)',
    amber: '#e7b152',
    amberBg: 'rgba(231, 177, 82, 0.16)',
    red: '#ef7a70',
    redBg: 'rgba(239, 122, 112, 0.14)',
    codeBg: '#131218',
    inlineCodeBg: '#292339',
    inlineCodeText: '#c7b9ff',
    userBubble: '#26262a',
    scrollbar: 'rgba(255, 255, 255, 0.24)',
  },
  midnight: {
    id: 'midnight',
    name: 'Midnight',
    dark: true,
    bg: '#0f131a',
    surface: '#161c26',
    surfaceElevated: '#1e2634',
    border: '#273142',
    hairline: '#1e2633',
    text: '#e6edf6',
    muted: '#93a2b8',
    dim: '#66758c',
    accent: '#58a6ff',
    accentBg: 'rgba(88, 166, 255, 0.16)',
    green: '#4cc386',
    greenBg: 'rgba(76, 195, 134, 0.14)',
    amber: '#e7b152',
    amberBg: 'rgba(231, 177, 82, 0.16)',
    red: '#ef7a70',
    redBg: 'rgba(239, 122, 112, 0.14)',
    codeBg: '#0a0d13',
    inlineCodeBg: '#1c283a',
    inlineCodeText: '#9cc9ff',
    userBubble: '#1c2432',
    scrollbar: 'rgba(147, 182, 255, 0.28)',
  },
  forest: {
    id: 'forest',
    name: 'Forest',
    dark: true,
    bg: '#111714',
    surface: '#18211c',
    surfaceElevated: '#202c25',
    border: '#293830',
    hairline: '#1f2b24',
    text: '#e5efe9',
    muted: '#93aa9d',
    dim: '#63786c',
    accent: '#4cc386',
    accentBg: 'rgba(76, 195, 134, 0.16)',
    green: '#5cd697',
    greenBg: 'rgba(92, 214, 151, 0.14)',
    amber: '#e7b152',
    amberBg: 'rgba(231, 177, 82, 0.16)',
    red: '#ef7a70',
    redBg: 'rgba(239, 122, 112, 0.14)',
    codeBg: '#0c110e',
    inlineCodeBg: '#1b2d23',
    inlineCodeText: '#9be8be',
    userBubble: '#1e2923',
    scrollbar: 'rgba(130, 220, 170, 0.26)',
  },
  plum: {
    id: 'plum',
    name: 'Plum',
    dark: true,
    bg: '#171219',
    surface: '#211924',
    surfaceElevated: '#2b212f',
    border: '#35293a',
    hairline: '#291f2d',
    text: '#f0e8f4',
    muted: '#a997b3',
    dim: '#786582',
    accent: '#c084fc',
    accentBg: 'rgba(192, 132, 252, 0.16)',
    green: '#4cc386',
    greenBg: 'rgba(76, 195, 134, 0.14)',
    amber: '#e7b152',
    amberBg: 'rgba(231, 177, 82, 0.16)',
    red: '#ef7a70',
    redBg: 'rgba(239, 122, 112, 0.14)',
    codeBg: '#110d13',
    inlineCodeBg: '#2e2038',
    inlineCodeText: '#deb9ff',
    userBubble: '#29202e',
    scrollbar: 'rgba(210, 165, 255, 0.26)',
  },
};

export const LIGHT_THEMES: Record<LightStyleId, Palette> = {
  classic: {
    id: 'classic',
    name: 'Classic',
    dark: false,
    bg: '#f7f7f8',
    surface: '#ffffff',
    surfaceElevated: '#efeff3',
    border: '#dfdfe5',
    hairline: '#e8e8ee',
    text: '#191a1e',
    muted: '#5d606a',
    dim: '#8a8d98',
    accent: '#6d5ce7',
    accentBg: 'rgba(109, 92, 231, 0.12)',
    green: '#2b8a57',
    greenBg: 'rgba(43, 138, 87, 0.12)',
    amber: '#b07219',
    amberBg: 'rgba(176, 114, 25, 0.12)',
    red: '#c92a2a',
    redBg: 'rgba(201, 42, 42, 0.10)',
    codeBg: '#f0f0f5',
    inlineCodeBg: '#ebe7fa',
    inlineCodeText: '#4c3cb8',
    userBubble: '#eceef5',
    scrollbar: 'rgba(0, 0, 0, 0.24)',
  },
  ivory: {
    id: 'ivory',
    name: 'Ivory',
    dark: false,
    bg: '#faf6f0',
    surface: '#ffffff',
    surfaceElevated: '#f2ece3',
    border: '#e6ddd0',
    hairline: '#eee6da',
    text: '#241f1a',
    muted: '#6f6559',
    dim: '#9a8e7f',
    accent: '#c96442',
    accentBg: 'rgba(201, 100, 66, 0.12)',
    green: '#2b8a57',
    greenBg: 'rgba(43, 138, 87, 0.12)',
    amber: '#b07219',
    amberBg: 'rgba(176, 114, 25, 0.12)',
    red: '#c92a2a',
    redBg: 'rgba(201, 42, 42, 0.10)',
    codeBg: '#f3ede4',
    inlineCodeBg: '#f5e4dc',
    inlineCodeText: '#9d4325',
    userBubble: '#f0e8dd',
    scrollbar: 'rgba(90, 60, 30, 0.24)',
  },
  sage: {
    id: 'sage',
    name: 'Sage',
    dark: false,
    bg: '#f3f7f4',
    surface: '#ffffff',
    surfaceElevated: '#e8f0ea',
    border: '#d6e3da',
    hairline: '#e2ece5',
    text: '#18231c',
    muted: '#546b5c',
    dim: '#839a8b',
    accent: '#2b8a57',
    accentBg: 'rgba(43, 138, 87, 0.12)',
    green: '#2b8a57',
    greenBg: 'rgba(43, 138, 87, 0.12)',
    amber: '#b07219',
    amberBg: 'rgba(176, 114, 25, 0.12)',
    red: '#c92a2a',
    redBg: 'rgba(201, 42, 42, 0.10)',
    codeBg: '#ebf2ed',
    inlineCodeBg: '#dcf0e3',
    inlineCodeText: '#1e6940',
    userBubble: '#e6f0e9',
    scrollbar: 'rgba(25, 80, 50, 0.24)',
  },
  rose: {
    id: 'rose',
    name: 'Rose',
    dark: false,
    bg: '#fbf4f6',
    surface: '#ffffff',
    surfaceElevated: '#f3e7eb',
    border: '#ead6dc',
    hairline: '#f1e2e7',
    text: '#26181e',
    muted: '#755863',
    dim: '#a38490',
    accent: '#c2416d',
    accentBg: 'rgba(194, 65, 109, 0.12)',
    green: '#2b8a57',
    greenBg: 'rgba(43, 138, 87, 0.12)',
    amber: '#b07219',
    amberBg: 'rgba(176, 114, 25, 0.12)',
    red: '#c92a2a',
    redBg: 'rgba(201, 42, 42, 0.10)',
    codeBg: '#f4eaed',
    inlineCodeBg: '#f7dde6',
    inlineCodeText: '#96294f',
    userBubble: '#f2e5e9',
    scrollbar: 'rgba(120, 30, 60, 0.24)',
  },
};

export const DARK_PALETTE = DARK_THEMES.classic;
export const LIGHT_PALETTE = LIGHT_THEMES.classic;

export interface ThemeSelection {
  mode: ThemeMode;
  darkStyle: DarkStyleId;
  lightStyle: LightStyleId;
}

const ThemeContext = createContext<ThemeSelection>({
  mode: 'dark',
  darkStyle: 'classic',
  lightStyle: 'classic',
});

export function ThemeSelectionProvider({
  selection,
  children,
}: {
  selection: ThemeSelection;
  children: React.ReactNode;
}) {
  return React.createElement(ThemeContext.Provider, { value: selection }, children);
}

export function resolvePalette(
  selection: ThemeSelection,
  systemScheme: 'light' | 'dark' | null | undefined,
): Palette {
  const effectiveDark =
    selection.mode === 'dark' || (selection.mode === 'system' && systemScheme !== 'light');
  if (effectiveDark) {
    return DARK_THEMES[selection.darkStyle] ?? DARK_THEMES.classic;
  }
  return LIGHT_THEMES[selection.lightStyle] ?? LIGHT_THEMES.classic;
}

export function usePalette(): Palette {
  const selection = useContext(ThemeContext);
  const scheme = useColorScheme();
  return resolvePalette(selection, scheme);
}
