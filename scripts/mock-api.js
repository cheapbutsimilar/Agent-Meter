/* Browser-only stand-in for the Tauri bridge (window.agentMeter) with demo data.
 * Used by the live preview server (cargo run -p agentmeter-devserver) and
 * scripts/preview.js (screenshots). Never active inside the real app. */
(function () {
  'use strict';
  if (window.__TAURI_INTERNALS__) return;
  const MIN = 60 * 1000;
  const HOUR = 60 * MIN;
  const DAY = 24 * HOUR;
  const now = Date.now();
  const listeners = {};
  const emit = (ch, data) => (listeners[ch] || []).forEach((cb) => cb(data));
  const bc = typeof BroadcastChannel !== 'undefined' ? new BroadcastChannel('agentmeter-mock') : null;

  // deterministic pseudo-random
  let seed = 7;
  const rnd = () => ((seed = (seed * 16807) % 2147483647) - 1) / 2147483646;

  function history(base, costPerM) {
    const out = [];
    for (let i = 29; i >= 0; i--) {
      const d = new Date(now - i * DAY);
      const k = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
      const weekend = d.getDay() === 0 || d.getDay() === 6;
      const w = Math.round(base * (weekend ? 0.25 : 0.6 + rnd() * 0.8));
      out.push({ day: k, w, cost: (w / 1e6) * costPerM });
    }
    return out;
  }

  function heatmap(scale) {
    const rows = [];
    for (let d = 0; d < 7; d++) {
      const row = [];
      for (let h = 0; h < 24; h++) {
        const work = h >= 10 && h <= 19 ? 1 : h >= 21 || h <= 1 ? 0.45 : 0.05;
        row.push(Math.round(scale * work * (0.4 + rnd())));
      }
      rows.push(row);
    }
    return rows;
  }

  function win(id, label, used, resetsIn, minutes, extra = {}) {
    const resetsAt = resetsIn == null ? null : now + resetsIn;
    const elapsed = resetsAt && minutes ? 1 - resetsIn / (minutes * MIN) : undefined;
    return { id, label, usedPercent: used, resetsAt, source: extra.source || 'api', windowMinutes: minutes, elapsed, ...extra };
  }

  const usage = {
    at: now - 12 * 1000,
    providers: [
      {
        id: 'claude', name: 'Claude', enabled: true, plan: 'Max', source: 'api', authExpiresAt: now + 5 * HOUR, detected: true, logFiles: 42, limitsAt: now - 60 * 1000,
        windows: [
          win('5h', '5H', 63, 2 * HOUR + 13 * MIN, 300, { ratePerHour: 21.4, eta: now + 1 * HOUR + 44 * MIN, exhaustBeforeReset: true }),
          win('7d', 'WK', 22, 4 * DAY + 2 * HOUR, 10080, { ratePerHour: 0.4 }),
          win('7d_opus', 'Opus', 12, 4 * DAY + 2 * HOUR, 10080, { minor: true }),
          win('7d_fable', 'Fable', 35, 4 * DAY + 2 * HOUR, 10080, { ratePerHour: 0.6 }),
        ],
        tokens: {
          today: 4_820_000, last5h: 2_310_000, week: 31_400_000, costToday: 38.42, cost5h: 17.9, costWeek: 241.6, cost30d: 902.3, unpricedWeek: 0,
          lastEventAt: now - 20 * 1000,
          models: [{ name: 'claude-opus-5-5', w: 21_000_000, cost: 190.2 }, { name: 'claude-sonnet-5-5', w: 9_100_000, cost: 48.1 }, { name: 'claude-haiku-5-5', w: 1_300_000, cost: 3.3 }],
          projects: [{ name: 'customer-onboarding-service-v2', w: 24_780_000, cost: 1126.21 }, { name: 'agentmeter', w: 14_200_000, cost: 112.5 }, { name: 'payments-api', w: 9_800_000, cost: 76.0 }, { name: 'design-system', w: 4_900_000, cost: 33.4 }, { name: 'dotfiles', w: 2_500_000, cost: 19.7 }],
          heatmap: heatmap(420_000),
        },
        history: history(5_200_000, 7.8),
      },
      {
        id: 'codex', name: 'Codex', enabled: true, plan: 'plus', source: 'api', authExpiresAt: now + 6 * DAY + 3 * HOUR, detected: true, logFiles: 17, limitsAt: now - 90 * 1000,
        windows: [
          win('5h', '5H', 41, 3 * HOUR + 40 * MIN, 300, { ratePerHour: 9.5 }),
          win('7d', 'WK', 37, 6 * DAY + 5 * HOUR, 10080, { ratePerHour: 1.1 }),
        ],
        tokens: {
          today: 1_930_000, last5h: 1_200_000, week: 9_800_000, costToday: 4.1, cost5h: 2.6, costWeek: 21.9, cost30d: 80.4, unpricedWeek: 0,
          lastEventAt: now - 40 * MIN,
          models: [{ name: 'gpt-5.3-codex', w: 9_800_000, cost: 21.9 }],
          projects: [{ name: 'payments-api', w: 6_100_000, cost: 13.2 }, { name: 'infra', w: 3_700_000, cost: 8.7 }],
          heatmap: heatmap(150_000),
        },
        history: history(1_700_000, 2.2),
      },
      {
        id: 'gemini', name: 'Gemini', enabled: true, plan: 'Gemini Code Assist', source: 'api', detected: true, logFiles: 6, limitsAt: now - 2 * MIN,
        windows: [
          win('pro', 'Pro', 84, 7 * HOUR + 5 * MIN, 1440, { ratePerHour: 4.2, eta: now + 3 * HOUR + 50 * MIN, exhaustBeforeReset: true, detail: 'gemini-3-pro: 84%' }),
          win('flash', 'Flash', 4, 7 * HOUR + 5 * MIN, 1440, { minor: true }),
        ],
        tokens: {
          today: 620_000, last5h: 410_000, week: 2_900_000, costToday: 1.9, cost5h: 1.2, costWeek: 8.4, cost30d: 31.2, unpricedWeek: 0,
          lastEventAt: now - 3 * HOUR,
          models: [{ name: 'gemini-3-pro', w: 2_400_000, cost: 7.6 }, { name: 'gemini-3-flash', w: 500_000, cost: 0.8 }],
          projects: [],
          heatmap: heatmap(60_000),
        },
        history: history(480_000, 3.1),
      },
      {
        id: 'antigravity', name: 'Antigravity', enabled: true, plan: 'Pro', source: 'api', detected: true, logFiles: 0, limitsAt: now - 2 * MIN,
        exePath: 'C:\\Users\\me\\AppData\\Local\\agy\\bin\\agy.exe',
        windows: [
          win('5h', '5H', 46, 3 * HOUR + 12 * MIN, 300, { ratePerHour: 6.1 }),
          win('7d', 'WK', 29, 5 * DAY + 7 * HOUR, 10080, { ratePerHour: 0.5 }),
        ],
        tokens: {
          today: 0, last5h: 0, week: 0, costToday: 0, cost5h: 0, costWeek: 0, cost30d: 0, unpricedWeek: 0,
          lastEventAt: null, models: [], projects: [], heatmap: heatmap(0),
        },
        history: history(0, 0),
      },
      {
        id: 'grok', name: 'Grok', enabled: true, plan: null, source: 'estimate', detected: true, logFiles: 2, limitsAt: null,
        windows: [win('5h', '5H', 13, null, 300, { source: 'estimate' })],
        tokens: {
          today: 132_000, last5h: 132_000, week: 760_000, costToday: 0.06, cost5h: 0.06, costWeek: 0.4, cost30d: 1.6, unpricedWeek: 0,
          lastEventAt: now - 26 * HOUR,
          models: [{ name: 'grok-code-fast-1', w: 760_000, cost: 0.4 }],
          projects: [],
          heatmap: heatmap(12_000),
        },
        history: history(110_000, 0.5),
      },
    ],
  };

  const display = (o) => Object.assign({ logo: true, name: false, label: true, bar: true, percent: true, reset: true, pace: false, forecast: true, tokens: false, cost: false, system: true, minorWindows: false, gear: true }, o);
  const provider = (o) => Object.assign({ enabled: true, visible: true, hiddenWindows: [], useApi: true, extraPaths: '', budget5h: 0, budgetWeek: 0, command: '' }, o);
  let settings = {
    v: 3, mode: 'floating', docked: true, widgetVisible: true, hotkeyMode: 'PageUp', hotkeyVisible: 'Shift+PageUp', hotkeyDashboard: '',
    valueMode: 'remaining', resetFormat: 'relative', currency: 'auto', krwRate: 0, language: 'ko',
    taskbar: display({}),
    floating: display({ name: true, pace: true, tokens: true, cost: true, system: false, minorWindows: false }),
    opacity: 0.92, scale: 1,
    theme: { mode: 'dark', preset: 'agentmeter', accent: '', bg: '', fg: '', font: 'system', uiScale: 1 },
    barWidth: 58, stripHeight: 40, cardWidth: 312, cornerRadius: 10, alwaysOnTop: true, lockPosition: false, taskbarOffset: null, floatingPos: null,
    notifications: true, alertThresholds: [80, 95], notifyReset: true, notifyForecast: true, trayGauge: true,
    launchAtLogin: false, autoUpdate: true, pollLogsSec: 15, pollApiSec: 180,
    providers: {
      claude: provider({}),
      codex: provider({}),
      gemini: provider({}),
      antigravity: provider({ antigravityPath: '' }),
      grok: provider({ useApi: false, budget5h: 1_000_000 }),
    },
  };
  const qs = new URLSearchParams(location.search);
  // the preview page pins each widget frame to one mode
  const pinnedMode = qs.get('mode');
  if (pinnedMode) settings.mode = pinnedMode;
  // screenshots: ?preset=nordic&scheme=light&value=used
  if (qs.get('preset')) settings.theme.preset = qs.get('preset');
  if (qs.get('scheme')) settings.theme.mode = qs.get('scheme');
  if (qs.get('value')) settings.valueMode = qs.get('value');
  if (qs.get('lang')) settings.language = qs.get('lang');
  if (qs.get('agents')) {
    const shown = qs.get('agents').split(',');
    for (const [id, c] of Object.entries(settings.providers)) c.visible = shown.includes(id);
  }

  function merge(base, patch) {
    for (const [k, v] of Object.entries(patch || {})) {
      if (v && typeof v === 'object' && !Array.isArray(v) && base[k] && typeof base[k] === 'object') merge(base[k], v);
      else base[k] = v;
    }
    return base;
  }

  // a found update (?update=available), as the updater reports it
  const NEW_UPDATE = {
    state: 'available', version: '0.2.25', current: '0.2.24', auto: true, at: now,
    notes: '- 새 버전이 나오면 "지금 업데이트 / 나중에"를 고르는 창이 떠요\n- 설정 → 일반 → 업데이트에서 패치노트를 볼 수 있어요\n- 위젯 우클릭 메뉴가 위젯 뒤로 가려지던 문제를 고쳤어요',
  };
  let update = qs.get('update') === 'available' ? { ...NEW_UPDATE } : { state: 'latest', at: now - 40 * MIN };

  window.agentMeter = {
    init: async () => ({
      window: location.pathname.includes('dashboard') ? 'dashboard' : location.pathname.includes('update') ? 'update' : 'widget',
      settings,
      usage,
      system: { cpu: 3, ram: 45 },
      platform: 'windows',
      version: '0.2.24',
      page: qs.get('page'),
      taskbarHeight: Number(qs.get('tb')) || 48,
      hotkeyErrors: [],
      dataDir: 'C:\\Users\\me\\AppData\\Roaming\\dev.agentmeter.app',
      update,
      fx: { krw: 1432.5, at: now - 3 * HOUR },
    }),
    setSettings: async (patch) => {
      settings = merge(JSON.parse(JSON.stringify(settings)), patch);
      emit('settings', settings);
      if (bc) bc.postMessage({ type: 'settings', data: settings });
      return settings;
    },
    updateCheck: () => {
      emit('event', { type: 'update', update: { state: 'checking' } });
      setTimeout(() => emit('event', { type: 'update', update: (update = { ...NEW_UPDATE }) }), 800);
    },
    updateLater: () => {},
    changelog: async () => {
      try {
        return await (await fetch('/CHANGELOG.md')).text();
      } catch (_) {
        return '';
      }
    },
    setupStatus: async () => ({
      claude: { supported: true, cli: true, login: true },
      codex: { supported: true, cli: true, login: true },
      gemini: { supported: true, cli: false, login: false },
      antigravity: { supported: true, cli: true, login: true, path: 'C:\\Users\\me\\AppData\\Local\\agy\\bin\\agy.exe' },
      grok: { supported: false },
    }),
    setupConnect: async (agent) => {
      setTimeout(() => emit('event', { type: 'setup', agent, status: { claude: { supported: true, cli: true, login: true }, codex: { supported: true, cli: true, login: true }, gemini: { supported: true, cli: true, login: true }, grok: { supported: false } } }), 1500);
    },
    antigravityPick: async () => 'C:\\Users\\me\\AppData\\Local\\agy\\bin\\agy.exe',
    updateInstall: () => emit('event', { type: 'update', update: (update = { ...NEW_UPDATE, state: 'downloading', progress: 30 }) }),
    refresh: async () => {
      usage.at = Date.now();
      emit('usage', usage);
    },
    openDashboard: (page) => {
      if (window.top !== window) window.top.postMessage({ amOpenDashboard: page || 'overview' }, location.origin);
    },
    closeWindow: () => {},
    minimizeWindow: () => {},
    toggleMaximize: () => {},
    widgetResize: (w, h) => {
      window.__widgetSize = [w, h];
      if (window.top !== window) window.top.postMessage({ amWidgetSize: [w, h], mode: settings.mode }, location.origin);
    },
    widgetMenu: () => {},
    dragStart: () => {},
    dragMove: () => {},
    dragEnd: () => {},
    hotkeysSuspend: (s) => (window.__hk = window.__hk || []).push(s),
    exportCsv: async () => 'C:\\Users\\me\\AppData\\Roaming\\dev.agentmeter.app\\exports\\agentmeter-usage.csv',
    openDataDir: () => {},
    openUrl: () => {},
    quit: () => {},
    on: (ch, cb) => {
      (listeners[ch] = listeners[ch] || []).push(cb);
      return () => {};
    },
    __emit: emit,
    /** Preview helper: mutate a provider of the demo snapshot and re-render. */
    __patchProvider: (id, fn) => {
      const p = usage.providers.find((x) => x.id === id);
      if (p) fn(p);
      usage.at = Date.now();
      emit('usage', usage);
    },
  };

  if (bc) {
    bc.onmessage = ({ data: msg }) => {
      if (msg && msg.type === 'settings') {
        settings = msg.data;
        if (pinnedMode) settings.mode = pinnedMode;
  // screenshots: ?preset=nordic&scheme=light&value=used
  if (qs.get('preset')) settings.theme.preset = qs.get('preset');
  if (qs.get('scheme')) settings.theme.mode = qs.get('scheme');
  if (qs.get('value')) settings.valueMode = qs.get('value');
  if (qs.get('agents')) {
    const shown = qs.get('agents').split(',');
    for (const [id, c] of Object.entries(settings.providers)) c.visible = shown.includes(id);
  }
        emit('settings', settings);
      }
    };
  }
})();
