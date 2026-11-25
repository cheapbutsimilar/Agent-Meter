/* Themes: presets + user colours -> CSS variables for the dashboard and the widget.
 * Display names here are fallbacks; the UI shows AM_I18N 'preset.<id>' / 'font.<id>'.
 * Everything is derived from three colours (background, text, accent), so any choice
 * keeps the same structure. Global: AM_THEME */
(function (root) {
  'use strict';

  const PRESETS = {
    agentmeter: { name: 'AgentMeter', accent: '#F05C1A', dark: { bg: '#2B2B2B', fg: '#F3F3F3' }, light: { bg: '#F6F5F3', fg: '#1E1E1E' }, accentLight: '#E0531A' },
    midnight: { name: 'Midnight', accent: '#E6B450', dark: { bg: '#10141C', fg: '#BFBDB6' }, light: { bg: '#FAFAF8', fg: '#3D424D' }, accentLight: '#D99A1E' },
    nordic: { name: 'Nordic', accent: '#88C0D0', dark: { bg: '#2E3440', fg: '#E5E9F0' }, light: { bg: '#ECEFF4', fg: '#2E3440' }, accentLight: '#5E81AC' },
    forest: { name: 'Forest', accent: '#4CC38A', dark: { bg: '#17201C', fg: '#DCE5DF' }, light: { bg: '#F2F6F3', fg: '#1D2A23' }, accentLight: '#1F8A5B' },
    graphite: { name: 'Graphite', accent: '#8AB4F8', dark: { bg: '#202124', fg: '#E8EAED' }, light: { bg: '#FFFFFF', fg: '#202124' }, accentLight: '#1A73E8' },
    rose: { name: 'Rose', accent: '#F27A9B', dark: { bg: '#241C20', fg: '#F0E4E8' }, light: { bg: '#FBF6F7', fg: '#2B1F24' }, accentLight: '#D2456C' },
  };

  const FONTS = {
    system: { name: 'System default', css: null },
    pretendard: { name: 'Pretendard', css: "'Pretendard Variable', Pretendard, 'Malgun Gothic', sans-serif" },
    malgun: { name: 'Malgun Gothic', css: "'Malgun Gothic', 'Apple SD Gothic Neo', sans-serif" },
    noto: { name: 'Noto Sans KR', css: "'Noto Sans KR', 'Malgun Gothic', sans-serif" },
    mono: { name: 'Monospace', css: "'D2Coding', 'JetBrains Mono', 'Cascadia Mono', Consolas, monospace" },
  };

  const hex2rgb = (h) => {
    const n = parseInt(String(h).slice(1), 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  };
  const rgb2hex = (c) => '#' + c.map((v) => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, '0')).join('');
  const mix = (a, b, t) => rgb2hex(hex2rgb(a).map((v, i) => v + (hex2rgb(b)[i] - v) * t));
  const csv = (h) => hex2rgb(h).join(', ');
  function luminance(h) {
    const [r, g, b] = hex2rgb(h).map((v) => {
      v /= 255;
      return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  }
  function contrast(a, b) {
    const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
    return (x + 0.05) / (y + 0.05);
  }

  const media = root.matchMedia ? root.matchMedia('(prefers-color-scheme: dark)') : null;
  const isHex = (s) => /^#[0-9a-fA-F]{6}$/.test(s || '');

  /** The colours in effect for the given settings.theme. */
  function resolve(theme) {
    const t = theme || {};
    const scheme = t.mode === 'system' ? (media && !media.matches ? 'light' : 'dark') : t.mode === 'light' ? 'light' : 'dark';
    const p = PRESETS[t.preset] || PRESETS.agentmeter;
    const bg = isHex(t.bg) ? t.bg : p[scheme].bg;
    const fg = isHex(t.fg) ? t.fg : p[scheme].fg;
    const accent = isHex(t.accent) ? t.accent : (scheme === 'light' && p.accentLight) || p.accent;
    return { scheme, bg, fg, accent, preset: p, contrast: contrast(bg, fg) };
  }

  /** Set the CSS variables on this document. */
  function apply(settings) {
    const s = settings || {};
    const c = resolve(s.theme);
    const dark = c.scheme === 'dark';
    const { bg, fg, accent } = c;
    const v = {
      '--bg': bg,
      '--bg-sunken': dark ? mix(bg, '#000000', 0.16) : mix(bg, '#000000', 0.035),
      '--bg-raised': dark ? mix(bg, fg, 0.045) : mix(bg, '#ffffff', 0.7),
      '--bg-hover': mix(bg, fg, dark ? 0.09 : 0.07),
      '--bg-input': dark ? mix(bg, '#000000', 0.1) : mix(bg, '#ffffff', 0.5),
      '--line': `rgba(${csv(fg)}, 0.08)`,
      '--line-strong': `rgba(${csv(fg)}, 0.15)`,
      '--text': fg,
      '--text-2': mix(fg, bg, 0.3),
      '--text-3': mix(fg, bg, 0.5),
      '--accent': accent,
      '--accent-hover': mix(accent, '#ffffff', 0.12),
      '--accent-press': mix(accent, '#000000', 0.12),
      '--accent-soft': `rgba(${csv(accent)}, 0.14)`,
      '--accent-line': `rgba(${csv(accent)}, 0.45)`,
      '--on-accent': luminance(accent) > 0.45 ? '#141414' : '#ffffff',
      '--meter-ok': dark ? mix(fg, bg, 0.06) : mix(fg, bg, 0.3),
      '--meter-ok-track': `rgba(${csv(fg)}, 0.12)`,
      '--meter-warn': accent,
      '--meter-warn-track': `rgba(${csv(accent)}, 0.22)`,
      '--pace': `rgba(${csv(fg)}, 0.75)`,
      '--bg-rgb': csv(bg),
      '--fg-rgb': csv(fg),
      '--accent-rgb': csv(accent),
      '--tooltip-bg': dark ? mix(bg, '#000000', 0.35) : '#ffffff',
      '--scroll-thumb': mix(bg, fg, 0.2),
      '--switch-off': mix(bg, fg, dark ? 0.22 : 0.25),
      '--sim-taskbar': dark ? mix(bg, '#000000', 0.35) : mix(bg, '#000000', 0.08),
      '--sim-app': mix(bg, fg, 0.12),
      '--stripe-a': mix(bg, fg, 0.03),
      '--stripe-b': bg,
      '--warn-text': dark ? '#ffb08a' : '#b4441a',
      '--danger-text': dark ? '#ffb3ad' : '#c42b1c',
      '--shadow': dark ? 'rgba(0, 0, 0, 0.35)' : 'rgba(0, 0, 0, 0.14)',
      '--w-radius': `${Number.isFinite(+s.cornerRadius) ? +s.cornerRadius : 10}px`,
      '--bar-w': `${+s.barWidth || 58}px`,
      '--card-w': `${+s.cardWidth || 312}px`,
    };
    const font = FONTS[(s.theme || {}).font];
    const el = root.document.documentElement;
    for (const [k, val] of Object.entries(v)) el.style.setProperty(k, val);
    if (font && font.css) el.style.setProperty('--font', font.css);
    else el.style.removeProperty('--font');
    el.dataset.scheme = c.scheme;
    el.style.colorScheme = c.scheme;
    return c;
  }

  // follow the OS when the mode is "system"
  let last = null;
  if (media && media.addEventListener) media.addEventListener('change', () => last && apply(last));
  function track(settings) {
    last = settings;
    return apply(settings);
  }

  root.AM_THEME = { PRESETS, FONTS, resolve, apply: track, contrast, mix };
})(typeof self !== 'undefined' ? self : this);
