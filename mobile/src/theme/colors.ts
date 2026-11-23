import { useColorScheme } from 'react-native';

export interface Palette {
  dark: boolean;
  bg: string;
  surface: string;
  surfaceElevated: string;
  border: string;
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
}

export const DARK_PALETTE: Palette = {
  dark: true,
  bg: '#0f1117',
  surface: '#161b22',
  surfaceElevated: '#1d2430',
  border: '#2b3240',
  text: '#e6edf3',
  muted: '#9aa4b2',
  dim: '#6e7785',
  accent: '#58a6ff',
  accentBg: 'rgba(88, 166, 255, 0.14)',
  green: '#3fb950',
  greenBg: 'rgba(63, 185, 80, 0.14)',
  amber: '#d29922',
  amberBg: 'rgba(210, 153, 34, 0.16)',
  red: '#f85149',
  redBg: 'rgba(248, 81, 73, 0.14)',
  codeBg: '#0b0e14',
};

export const LIGHT_PALETTE: Palette = {
  dark: false,
  bg: '#f6f8fa',
  surface: '#ffffff',
  surfaceElevated: '#f0f3f7',
  border: '#d8dee4',
  text: '#1f2328',
  muted: '#59636e',
  dim: '#818b98',
  accent: '#0969da',
  accentBg: 'rgba(9, 105, 218, 0.10)',
  green: '#1a7f37',
  greenBg: 'rgba(26, 127, 55, 0.10)',
  amber: '#9a6700',
  amberBg: 'rgba(154, 103, 0, 0.12)',
  red: '#cf222e',
  redBg: 'rgba(207, 34, 46, 0.10)',
  codeBg: '#eef2f6',
};

export function usePalette(): Palette {
  const scheme = useColorScheme();
  return scheme === 'light' ? LIGHT_PALETTE : DARK_PALETTE;
}
