/* Formatting & small helpers shared by the windows. Global: AM_UTIL */
(function (root) {
  'use strict';

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
  /** settings.currency / settings.krwRate (0 = fetched) + fx = { krw, at } from the app. */
  function setMoney(settings, fx) {
    money.currency = (settings && settings.currency) || 'usd';
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
  /** Compact form for chart axes: one currency, 만/억 for won. */
  function fmtMoneyShort(v) {
    v = Number(v) || 0;
    if (money.currency === 'usd' || money.currency === 'both') return fmtUsd(v);
    const w = v * money.rate;
    if (w >= 1e8) return `₩${(w / 1e8).toFixed(w >= 1e9 ? 0 : 1)}억`;
    if (w >= 1e4) return `₩${(w / 1e4).toFixed(w >= 1e5 ? 0 : 1)}만`;
    return fmtKrw(v);
  }
  /** Compact text for summaries: with "both", the won amount in 만/억 after the dollars. */
  function fmtMoneyBrief(v) {
    v = Number(v) || 0;
    if (money.currency !== 'both') return fmtMoneyShort(v);
    const w = v * money.rate;
    const krw = w >= 1e8 ? `₩${(w / 1e8).toFixed(w >= 1e9 ? 0 : 1)}억` : w >= 1e4 ? `₩${(w / 1e4).toFixed(w >= 1e5 ? 0 : 1)}만` : fmtKrw(v);
    return `${fmtUsd(v)} (${krw})`;
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

  /** "2시간 13분", "3일 4시간", "45분" */
  function fmtLeft(ms) {
    if (ms == null) return '';
    if (ms <= 0) return '곧';
    if (ms >= DAY) {
      const d = Math.floor(ms / DAY);
      const h = Math.floor((ms % DAY) / HOUR);
      return h ? `${d}일 ${h}시간` : `${d}일`;
    }
    if (ms >= HOUR) {
      const h = Math.floor(ms / HOUR);
      const m = Math.floor((ms % HOUR) / MIN);
      return m ? `${h}시간 ${m}분` : `${h}시간`;
    }
    return `${Math.max(1, Math.ceil(ms / MIN))}분`;
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
    if (!ts) return '기록 없음';
    const d = now - ts;
    if (d < 10 * 1000) return '방금';
    if (d < MIN) return `${Math.floor(d / 1000)}초 전`;
    if (d < HOUR) return `${Math.floor(d / MIN)}분 전`;
    if (d < DAY) return `${Math.floor(d / HOUR)}시간 전`;
    return `${Math.floor(d / DAY)}일 전`;
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

  const SOURCE_LABEL = { api: '공식 API', log: '로그', estimate: '예산 추정', command: '명령어', none: '' };

  /** Long name of a window id for tooltips. */
  function windowName(w) {
    const m = w.windowMinutes;
    if (w.id === '5h' || m === 300) return '5시간 한도';
    if (w.id === '7d') return '주간 한도';
    if (w.id === '7d_opus') return 'Opus 주간 한도';
    if (w.id === '7d_sonnet') return 'Sonnet 주간 한도';
    if (w.id === 'pro') return 'Pro 모델 일일 할당량';
    if (w.id === 'flash') return 'Flash 모델 일일 할당량';
    if (w.id === 'day') return '일일 할당량';
    return `${w.label} 한도`;
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
    SOURCE_LABEL,
  };
})(typeof self !== 'undefined' ? self : this);
