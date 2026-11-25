/* Formatting & small helpers shared by the windows. Global: AM_UTIL */
(function (root) {
  'use strict';

  const I18N = root.AM_I18N;
  const tr = (k, v) => I18N.t(k, v);
  const MIN = 60 * 1000;
  const HOUR = 60 * MIN;
  const DAY = 24 * HOUR;

  const AGENTS = {
    claude: { id: 'claude', name: 'Claude', product: 'Claude Code', color: 'var(--c-claude)' },
    codex: { id: 'codex', name: 'Codex', product: 'Codex CLI', color: 'var(--c-codex)' },
    gemini: { id: 'gemini', name: 'Gemini', product: 'Gemini CLI', color: 'var(--c-gemini)' },
    grok: { id: 'grok', name: 'Grok', product: 'Grok', color: 'var(--c-grok)' },
  };
  /** Stacking order for charts (adjacent colors validated for color-vision deficiency). */
  const STACK_ORDER = ['gemini', 'claude', 'grok', 'codex'];

  function esc(s) {
    return String(s == null ? '' : s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
  }

  function fmtInt(n) {
    return Math.round(Number(n) || 0).toLocaleString('en-US');
  }

  function fmtNum(n) {
    n = Number(n) || 0;
    const a = Math.abs(n);
    if (a >= 1e9) return `${(n / 1e9).toFixed(a >= 1e10 ? 1 : 2)}B`;
    if (a >= 1e6) return `${(n / 1e6).toFixed(a >= 1e7 ? 1 : 2)}M`;
    if (a >= 1e3) return `${(n / 1e3).toFixed(a >= 1e4 ? 0 : 1)}K`;
    return String(Math.round(n));
  }

  // Costs are kept in USD; the display currency and won rate come from the settings.
  const DEFAULT_KRW = 1400; // only until the first rate is fetched
  const money = { currency: 'usd', rate: DEFAULT_KRW, rateSource: 'default', at: null };
  /** "auto" (the default): both currencies in Korean, dollars in English. */
  function currencyOf(settings) {
    const c = (settings && settings.currency) || 'auto';
    if (c === 'auto') return I18N.lang === 'ko' ? 'both' : 'usd';
    return c;
  }
  /** settings.currency / settings.krwRate (0 = fetched) + fx = { krw, at } from the app. */
  function setMoney(settings, fx) {
    money.currency = currencyOf(settings);
    const manual = Number(settings && settings.krwRate) || 0;
    if (manual > 0) Object.assign(money, { rate: manual, rateSource: 'manual', at: null });
    else if (fx && fx.krw) Object.assign(money, { rate: fx.krw, rateSource: 'auto', at: fx.at });
    else Object.assign(money, { rate: DEFAULT_KRW, rateSource: 'default', at: null });
  }
  function fmtUsd(v) {
    if (v === 0) return '$0';
    if (v < 0.01) return '<$0.01';
    if (v < 100) return `$${v.toFixed(2)}`;
    return `$${Math.round(v).toLocaleString('en-US')}`;
  }
  function fmtKrw(v) {
    const w = v * money.rate;
    if (w === 0) return '₩0';
    if (w < 1) return '<₩1';
    return `₩${Math.round(w).toLocaleString('ko-KR')}`;
  }
  /** Compact won: 만/억 in Korean, K/M elsewhere. */
  function fmtKrwShort(v) {
    const w = v * money.rate;
    if (I18N.lang === 'ko') {
      if (w >= 1e8) return `₩${(w / 1e8).toFixed(w >= 1e9 ? 0 : 1)}억`;
      if (w >= 1e4) return `₩${(w / 1e4).toFixed(w >= 1e5 ? 0 : 1)}만`;
    } else {
      if (w >= 1e6) return `₩${(w / 1e6).toFixed(w >= 1e7 ? 0 : 1)}M`;
      if (w >= 1e4) return `₩${Math.round(w / 1e3)}K`;
    }
    return fmtKrw(v);
  }
  /** Compact form for chart axes: one currency. */
  function fmtMoneyShort(v) {
    v = Number(v) || 0;
    if (money.currency === 'usd' || money.currency === 'both') return fmtUsd(v);
    return fmtKrwShort(v);
  }
  /** Compact text for summaries: with "both", the compact won amount after the dollars. */
  function fmtMoneyBrief(v) {
    v = Number(v) || 0;
    if (money.currency !== 'both') return fmtMoneyShort(v);
    return `${fmtUsd(v)} (${fmtKrwShort(v)})`;
  }
  /** For tight spots: with "both", the won amount goes on a second, smaller line. */
  function fmtMoneyHtml(v) {
    v = Number(v) || 0;
    if (money.currency === 'both') return `${fmtUsd(v)}<small class="alt-cur">${fmtKrw(v)}</small>`;
    return fmtMoney(v);
  }
  function fmtMoney(v) {
    v = Number(v) || 0;
    if (money.currency === 'krw') return fmtKrw(v);
    if (money.currency === 'both') return `${fmtUsd(v)} · ${fmtKrw(v)}`;
    return fmtUsd(v);
  }

  function fmtPct(p) {
    if (p == null || !Number.isFinite(p)) return '—';
    const r = Math.round(p);
    return `${r}%`;
  }

  /** "2시간 13분" / "2h 13m", "3일 4시간" / "3d 4h", "45분" / "45m" */
  function fmtLeft(ms) {
    if (ms == null) return '';
    if (ms <= 0) return tr('time.soon');
    if (ms >= DAY) {
      const d = Math.floor(ms / DAY);
      const h = Math.floor((ms % DAY) / HOUR);
      return h ? tr('time.dh', { d, h }) : tr('time.d', { d });
    }
    if (ms >= HOUR) {
      const h = Math.floor(ms / HOUR);
      const m = Math.floor((ms % HOUR) / MIN);
      return m ? tr('time.hm', { h, m }) : tr('time.h', { h });
    }
    return tr('time.m', { m: Math.max(1, Math.ceil(ms / MIN)) });
  }

  /** compact for the taskbar: "04h", "3d", "45m" */
  function fmtShortLeft(ms) {
    if (ms == null) return '--';
    if (ms <= 0) return '0m';
    if (ms >= 2 * DAY) return `${Math.floor(ms / DAY)}d`;
    if (ms >= HOUR) return `${String(Math.floor(ms / HOUR)).padStart(2, '0')}h`;
    return `${Math.max(1, Math.ceil(ms / MIN))}m`;
  }

  function pad(n) {
    return String(n).padStart(2, '0');
  }

  /** "14:30", or "11/18 14:30" when not today */
  function fmtClock(ts, now = Date.now()) {
    if (!ts) return '';
    const d = new Date(ts);
    const t = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
    const n = new Date(now);
    if (d.toDateString() === n.toDateString()) return t;
    return `${d.getMonth() + 1}/${d.getDate()} ${t}`;
  }

  function fmtDateTime(ts) {
    if (!ts) return '';
    const d = new Date(ts);
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }

  function relTime(ts, now = Date.now()) {
    if (!ts) return tr('rel.none');
    const d = now - ts;
    if (d < 10 * 1000) return tr('rel.now');
    if (d < MIN) return tr('rel.s', { n: Math.floor(d / 1000) });
    if (d < HOUR) return tr('rel.m', { n: Math.floor(d / MIN) });
    if (d < DAY) return tr('rel.h', { n: Math.floor(d / HOUR) });
    return tr('rel.d', { n: Math.floor(d / DAY) });
  }

  function severity(used, thresholds) {
    const [warn, crit] = thresholds && thresholds.length ? [thresholds[0], thresholds[1] ?? 101] : [80, 95];
    if (used == null) return 'none';
    if (used >= crit) return 'crit';
    if (used >= warn) return 'warn';
    return 'ok';
  }

  /** The value to display for a window in the chosen mode. */
  function shown(w, settings) {
    return settings.valueMode === 'remaining' ? Math.max(0, 100 - w.usedPercent) : w.usedPercent;
  }

  /** Windows to show for a provider in the widget. */
  function visibleWindows(p, settings, opts) {
    const cfg = (settings.providers || {})[p.id] || {};
    const hidden = cfg.hiddenWindows || [];
    return (p.windows || []).filter((w) => !hidden.includes(w.id) && (opts.minorWindows || !w.minor));
  }

  function isActive(p, now = Date.now()) {
    return !!(p && p.tokens && p.tokens.lastEventAt && now - p.tokens.lastEventAt < 2 * MIN);
  }

  /** "공식 API" / "Official API" … for a limit source. */
  function sourceLabel(src) {
    return src && src !== 'none' ? tr(`source.${src}`) : '';
  }

  /** Long name of a window id for tooltips ("Fable" -> "Fable model limit (weekly)"). */
  function windowName(w) {
    const m = w.windowMinutes;
    if (w.id === '5h') return tr('win.5h');
    if (w.id === 'gemini-5h') return tr('win.ag5h');
    if (w.id === 'gemini-weekly') return tr('win.agWk');
    if (['7d', '7d_opus', '7d_sonnet', 'pro', 'flash', 'day'].includes(w.id)) return tr(`win.${w.id}`);
    if (/^7d_/.test(w.id)) return tr('win.model7d', { label: w.label });
    if (/^5h_/.test(w.id)) return tr('win.model5h', { label: w.label.replace(/ 5H$/, '') });
    if (m === 300) return tr('win.5h');
    return tr('win.generic', { label: w.label });
  }

  /** CHANGELOG.md -> [{ version, date, items: [line] }] (newest first). "## v0.2.3 · 2026-11-18"
   *  headings, "- " items; the title and HTML comments are skipped. */
  function parseChangelog(md) {
    const out = [];
    const text = String(md || '').replace(/<!--[\s\S]*?-->/g, '');
    for (const raw of text.split('\n')) {
      const line = raw.trim();
      if (line.startsWith('## ')) {
        const head = line.slice(3).trim();
        const [version, date] = head.split('·').map((x) => x.trim());
        out.push({ version: version || head, date: date || '', items: [] });
      } else if (/^[-*•]\s+/.test(line) && out.length) {
        out[out.length - 1].items.push(line.replace(/^[-*•]\s+/, ''));
      }
    }
    return out;
  }
  /** Bullet lines of one release's notes (the update popup). */
  function noteItems(md) {
    return String(md || '')
      .split('\n')
      .map((l) => l.trim())
      .filter((l) => /^[-*•]\s+/.test(l))
      .map((l) => l.replace(/^[-*•]\s+/, ''));
  }

  root.AM_UTIL = {
    parseChangelog,
    noteItems,
    MIN,
    HOUR,
    DAY,
    AGENTS,
    STACK_ORDER,
    esc,
    fmtInt,
    fmtNum,
    fmtMoney,
    setMoney,
    currencyOf,
    fmtMoneyShort,
    fmtMoneyBrief,
    fmtMoneyHtml,
    money,
    fmtPct,
    fmtLeft,
    fmtShortLeft,
    fmtClock,
    fmtDateTime,
    relTime,
    severity,
    shown,
    visibleWindows,
    isActive,
    windowName,
    sourceLabel,
  };
})(typeof self !== 'undefined' ? self : this);
