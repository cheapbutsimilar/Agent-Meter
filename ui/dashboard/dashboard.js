/* Dashboard: overview, analytics, widget display options, agents, alerts & hotkeys, general. */
(async () => {
  'use strict';
  const U = window.AM_UTIL;
  const R = window.AM_RENDER;
  const api = window.agentMeter;
  const esc = U.esc;
  const I18N = window.AM_I18N;
  const tr = (k, v) => I18N.t(k, v);
  const $ = (s, el = document) => el.querySelector(s);
  const main = $('#main');

  const S = await api.init();
  /** Language from the settings: the static labels in index.html follow it too. */
  function applyLang() {
    I18N.setLang(S.settings.language);
    I18N.applyDom();
  }
  applyLang();
  let page = S.page || new URLSearchParams(location.search).get('page') || 'overview';
  // "전체 패치노트 보기" from the update window opens 일반 with the notes on top
  const startWithNotes = page === 'notes';
  if (startWithNotes) page = 'general';
  const ui = {
    metric: 'tokens',
    range: 30,
    agents: new Set(['claude', 'codex', 'gemini', 'antigravity', 'grok', 'minimax', 'zai', 'qwen']),
    displayTab: S.settings.mode === 'floating' ? 'floating' : 'taskbar',
    recording: null,
    hotkeyErrors: S.hotkeyErrors || [],
    previewSettings: null,
  };
  const providers = () => (S.usage && S.usage.providers) || [];
  const cfgOf = (id) => (S.settings.providers || {})[id] || {};

  // ---------------------------------------------------------------- small helpers
  let toastTimer = null;
  function toast(msg) {
    const t = $('#toast');
    t.textContent = msg;
    t.classList.add('show');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => t.classList.remove('show'), 2400);
  }

  const sw = (attrs, on) => `<label class="switch"><input type="checkbox" ${attrs} ${on ? 'checked' : ''}/><span></span></label>`;
  const row = (label, sub, control) => `<div class="row"><div class="lb">${label}${sub ? `<small>${sub}</small>` : ''}</div>${control}</div>`;
  const seg = (name, options, value) =>
    `<div class="seg">${options.map(([v, l]) => `<button data-seg="${name}" data-v="${esc(v)}" class="${String(value) === String(v) ? 'active' : ''}">${l}</button>`).join('')}</div>`;
  const pageHead = (extra = '') => `<div class="page-head"><div><h1>${esc(tr(`page.${page}`))}</h1><p>${esc(tr(`page.${page}.sub`))}</p></div><span class="sp"></span>${extra}</div>`;

  // ---------------------------------------------------------------- shown agents & connect
  const isShown = (p) => cfgOf(p.id).visible !== false;
  const FABLE_IDS = ['7d_fable', '5h_fable'];
  /** Agents read with an API key from Settings (no CLI login to connect). */
  const KEYED = ['minimax', 'zai'];
  /** Why an agent's card has no limit bars. */
  function noLimitsNote(p) {
    if (p.id === 'grok') return tr('ov.grokNote');
    if (p.id === 'antigravity' && !p.detected) return tr('ov.agyNote');
    if (p.id === 'codex') return tr('ov.codexNote');
    if (KEYED.includes(p.id)) return (cfgOf(p.id).apiKey || '').trim() ? tr('ov.waiting') : tr('ov.keyNote');
    if (p.id === 'qwen') return p.logFiles ? tr('ov.qwenNote') : tr('qwen.recording');
    return p.detected ? tr('ov.waiting') : tr('ov.noLogs');
  }
  const CONNECT_NAMES = { claude: 'Claude Code', codex: 'Codex', gemini: 'Gemini CLI', antigravity: 'Antigravity CLI' };
  /** "원클릭 연결" when the agent's CLI isn't logged in (or the API says so). */
  function connectBtn(p, { always = false } = {}) {
    const st = (S.setup || {})[p.id];
    if (!st || !st.supported) return '';
    const needs = !st.login || (p.apiError && /로그인|만료|login|expired/i.test(p.apiError));
    if (!needs && !always) return '';
    const label = needs ? tr('connect.btn') : tr('connect.relogin');
    return `<button class="btn sm ${needs ? 'primary' : ''}" data-act="connect" data-id="${p.id}">${label}</button>`;
  }
  function connectText(p) {
    const st = (S.setup || {})[p.id];
    const name = CONNECT_NAMES[p.id];
    if (p.id === 'antigravity' && (!st || !st.supported)) return tr('ag.agy.winOnly');
    if (!st || !st.supported) return tr('connect.notNeeded');
    if (!st.cli) return tr('connect.notInstalled', { name });
    if (!st.login) return tr('connect.needLogin', { name });
    return tr('connect.ok', { name, exp: loginExpiry(p) });
  }
  /** " · 로그인 유효 11/28 14:00까지 (6일 남음)" from the token's own expiry. */
  function loginExpiry(p) {
    const exp = p.authExpiresAt;
    if (!exp) return '';
    const now = Date.now();
    if (exp <= now) return tr('connect.expired', { at: U.fmtDateTime(exp), name: CONNECT_NAMES[p.id] });
    return tr('connect.valid', { at: U.fmtDateTime(exp), left: U.fmtLeft(exp - now), name: CONNECT_NAMES[p.id] });
  }

  function statusOf(p, now) {
    if (!p.enabled) return `<span class="status"><i></i>${esc(tr('status.off'))}</span>`;
    if (U.isActive(p, now)) return `<span class="status live"><i></i>${esc(tr('status.working'))}</span>`;
    if (KEYED.includes(p.id) && !(cfgOf(p.id).apiKey || '').trim()) return `<span class="status"><i></i>${esc(tr('status.noKey'))}</span>`;
    if (!p.detected) return `<span class="status"><i></i>${esc(tr('status.notFound'))}</span>`;
    // Antigravity, MiniMax and Z.AI have no activity log, only their limit readings
    if (!U.hasTokens(p.id)) return `<span class="status"><i></i>${esc(p.limitsAt ? tr('ag.limitsAt', { t: U.relTime(p.limitsAt, now) }) : tr('ov.waiting'))}</span>`;
    return `<span class="status"><i></i>${esc(tr('status.last', { t: U.relTime(p.tokens.lastEventAt, now) }))}</span>`;
  }

  function paceText(w) {
    if (w.elapsed == null || w.source === 'estimate' || w.stale) return '';
    const el = Math.round(w.elapsed * 100);
    const diff = w.usedPercent - el;
    // the forecast uses the recent rate; if it says "runs out before reset" don't call it on pace
    const verdict = diff > 10 || w.exhaustBeforeReset ? `<span class="pace-fast">${esc(tr('pace.fast'))}</span>` : esc(diff < -15 ? tr('pace.slack') : tr('pace.ok'));
    return `<span title="${esc(tr('pace.title', { el, used: Math.round(w.usedPercent) }))}">${tr('pace.text', { el, verdict })}</span>`;
  }

  function limitRow(w, now) {
    const s = S.settings;
    const sev = U.severity(w.usedPercent, s.alertThresholds);
    const val = U.shown(w, s);
    const info = [];
    if (w.resetsAt) info.push(`<span>${esc(tr('lim.reset', { in: U.fmtLeft(w.resetsAt - now), at: U.fmtClock(w.resetsAt, now) }))}</span>`);
    else if (w.source === 'estimate') info.push(`<span>${esc(tr('lim.budget'))}</span>`);
    else if (w.stale) info.push(`<span>${esc(tr('lim.stale'))}</span>`);
    const fc = R.forecastText(w, now);
    if (fc) info.push(`<span class="fc">${esc(tr('lim.forecast', { fc }))}</span>`);
    else if (w.ratePerHour) info.push(`<span>${esc(tr('lim.rate', { r: w.ratePerHour.toFixed(1) }))}</span>`);
    const pace = paceText(w);
    if (pace) info.push(pace);
    const icon = sev === 'crit' ? '⛔ ' : sev === 'warn' ? '⚠ ' : '';
    return (
      `<div class="lim"><span class="lb" title="${esc(U.windowName(w))}">${esc(w.label)}</span>${R.meter(w, s, { pace: true })}` +
      `<span class="val sev-${sev}">${icon}${U.fmtPct(val)}<small>${esc(s.valueMode === 'remaining' ? tr('w.left') : tr('w.used'))}</small></span>` +
      `<div class="info">${info.join('')}</div></div>`
    );
  }

  function sparkline(p) {
    const hist = (p.history || []).slice(-30);
    const W = 600;
    const H = 46;
    const n = hist.length || 1;
    const slot = W / n;
    const bw = Math.min(14, slot - 2);
    const max = Math.max(1, ...hist.map((d) => d.w));
    const color = U.AGENTS[p.id] ? U.AGENTS[p.id].color : '#888';
    let bars = '';
    hist.forEach((d, i) => {
      const h = d.w > 0 ? Math.max(2, (d.w / max) * (H - 2)) : 0;
      const x = i * slot + (slot - bw) / 2;
      const tip = tr('spark.tip', { day: d.day.slice(5).replace('-', '/'), n: U.fmtNum(d.w), cost: U.fmtMoney(d.cost) });
      bars += `<rect class="hit" data-tip="${esc(tip)}" x="${i * slot}" y="0" width="${slot}" height="${H}" fill="transparent"/>`;
      if (h > 0) bars += `<rect pointer-events="none" x="${x.toFixed(1)}" y="${(H - h).toFixed(1)}" width="${bw.toFixed(1)}" height="${h.toFixed(1)}" rx="2" fill="${color}"/>`;
    });
    return `<div class="spark"><div class="ct"><span>${esc(tr('spark.title'))}</span><span>${esc(tr('spark.max', { n: U.fmtNum(max) }))}</span></div><svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="none" role="img" aria-label="${esc(tr('spark.aria', { name: p.name }))}">${bars}</svg></div>`;
  }

  // ---------------------------------------------------------------- overview
  function overview() {
    const now = Date.now();
    const list = providers().filter((p) => p.enabled && isShown(p));
    let top = null;
    const forecasts = [];
    for (const p of list) {
      for (const w of p.windows) {
        if (!w.minor && (!top || w.usedPercent > top.w.usedPercent)) top = { p, w };
        if (w.exhaustBeforeReset) forecasts.push({ p, w });
      }
    }
    const sum = (f) => list.reduce((a, p) => a + (f(p.tokens) || 0), 0);
    const topSev = top ? U.severity(top.w.usedPercent, S.settings.alertThresholds) : 'none';
    const kpis = [
      [
        esc(S.settings.valueMode === 'remaining' ? tr('kpi.leastLeft') : tr('kpi.highest')),
        top ? U.fmtPct(U.shown(top.w, S.settings)) : '—',
        top
          ? `<span class="status ${topSev === 'ok' ? '' : topSev}"><i></i><span>${esc(top.p.name)} ${esc(top.w.label)}${top.w.resetsAt ? esc(tr('kpi.resetIn', { t: U.fmtLeft(top.w.resetsAt - now) })) : ''}</span></span>`
          : esc(tr('kpi.noLimits')),
      ],
      [esc(tr('kpi.today')), U.fmtNum(sum((t) => t.today)), esc(tr('kpi.todayNote'))],
      [
        esc(tr('kpi.cost')),
        U.fmtMoneyHtml(sum((t) => t.costToday)),
        `<span title="${esc(tr('kpi.periods', { w: U.fmtMoney(sum((t) => t.costWeek)), m: U.fmtMoney(sum((t) => t.cost30d)) }).replace(' · ', '\n'))}">${esc(tr('kpi.periods', { w: U.fmtMoneyBrief(sum((t) => t.costWeek)), m: U.fmtMoneyBrief(sum((t) => t.cost30d)) }))}</span>`,
      ],
      [
        esc(tr('kpi.runout')),
        forecasts.length ? esc(tr('kpi.count', { n: forecasts.length })) : esc(tr('kpi.none')),
        forecasts.length
          ? forecasts.map((f) => esc(tr('kpi.fcItem', { name: f.p.name, label: f.w.label, t: U.fmtLeft(f.w.eta - now) }))).join(' · ')
          : esc(tr('kpi.allOk')),
      ],
    ];
    let html = pageHead(
      `<button class="btn" data-act="receipt"><svg viewBox="0 0 24 24"><path d="M6 3h12v18l-3-2-3 2-3-2-3 2z"/><path d="M9 8h6M9 12h6"/></svg>${esc(tr('rc.open'))}</button>`,
    );
    html += `<div class="grid4">${kpis.map(([k, v, s]) => `<div class="card kpi"><div class="k">${k}</div><div class="v">${v}</div><div class="s">${s}</div></div>`).join('')}</div>`;
    const pick = providers()
      .map((p) => {
        const on = isShown(p);
        return `<label class="pick ${on ? 'on' : ''}"><input type="checkbox" data-prov="${p.id}" data-key="visible" ${on ? 'checked' : ''}/>${R.logo(p.id, 14)}${esc(p.name)}</label>`;
      })
      .join('');
    html += `<div class="sec-head"><h2>${esc(tr('ov.agents'))}</h2><div class="picks" title="${esc(tr('ov.picksTitle'))}">${pick}</div></div>`;
    const shown = providers().filter(isShown);
    if (!shown.length) html += `<div class="card note muted">${esc(tr('ov.pickNone'))}</div>`;
    html += `<div class="grid2">`;
    for (const p of shown) {
      const t = p.tokens || {};
      html += `<div class="card agent ${p.enabled ? '' : 'off'}"><div class="ah">${R.logo(p.id, 22)}<span class="nm">${esc(p.name)}</span>`;
      if (p.plan) html += `<span class="tag cap">${esc(p.plan)}</span>`;
      if (p.source && p.source !== 'none') {
        const age = p.limitsAt ? ` · ${U.relTime(p.limitsAt, now)}` : '';
        const stale = p.source === 'api' && p.limitsAt && now - p.limitsAt > 10 * 60 * 1000;
        html += `<span class="tag ${stale ? 'stale' : ''}" title="${esc(tr('ov.sourceTitle'))}">${esc((U.sourceLabel(p.source) || p.source) + age)}</span>`;
      }
      html += `<span class="sp"></span>${statusOf(p, now)}</div>`;
      const offIds = (cfgOf(p.id).hiddenWindows || []).filter((id) => FABLE_IDS.includes(id));
      const wins = p.windows.filter((w) => !offIds.includes(w.id));
      if (wins.length) html += wins.map((w) => limitRow(w, now)).join('');
      else
        html += `<div class="note muted">${esc(noLimitsNote(p))}</div>`;
      // Antigravity, MiniMax and Z.AI write no token logs: no token or cost figures to show
      if (U.hasTokens(p.id)) html +=
        `<div class="astats"><div><div class="k">${esc(tr('ov.today'))}</div><div class="v">${U.fmtNum(t.today)}</div></div>` +
        `<div><div class="k">${esc(tr('ov.last5h'))}</div><div class="v">${U.fmtNum(t.last5h)}</div></div>` +
        `<div><div class="k">${esc(tr('ov.last7d'))}</div><div class="v">${U.fmtNum(t.week)}</div></div>` +
        `<div><div class="k">${esc(tr('ov.costToday'))}</div><div class="v">${U.fmtMoneyHtml(t.costToday)}</div></div></div>`;
      if (U.hasTokens(p.id)) html += sparkline(p);
      const note = [p.apiError, p.commandError, p.scanError].filter(Boolean).map(I18N.apiError).join(' · ');
      const btn = connectBtn(p);
      if (note || btn) html += `<div class="note-row">${note ? `<div class="note">${esc(note)}</div>` : '<span></span>'}${btn}</div>`;
      html += `</div>`;
    }
    html += `</div>`;
    return html;
  }

  // ---------------------------------------------------------------- analytics
  function analytics() {
    const chips = U.STACK_ORDER.map((id) => {
      const a = U.AGENTS[id];
      return `<button class="chip ${ui.agents.has(id) ? 'on' : ''}" data-act="agent-filter" data-id="${id}" aria-pressed="${ui.agents.has(id)}"><span class="sw" style="background:${a.color}"></span>${a.name}</button>`;
    }).join('');
    let html = pageHead(`<button class="btn" data-act="export"><svg viewBox="0 0 24 24"><path d="M12 4v11M7 10l5 5 5-5M5 20h14"/></svg>${esc(tr('an.export'))}</button>`);
    html += `<div class="toolbar">${seg('metric', [['tokens', esc(tr('an.tokens'))], ['cost', esc(tr('an.cost'))]], ui.metric)}${seg('range', [[14, esc(tr('an.d14'))], [30, esc(tr('an.d30'))]], ui.range)}<span class="sp"></span><div class="chips">${chips}</div></div>`;
    html += `<div class="card chart-card"><div class="ch"><b>${esc(ui.metric === 'cost' ? tr('an.dailyCost') : tr('an.dailyTokens'))}</b><span>${esc(ui.metric === 'cost' ? tr('an.costNote') : tr('an.tokNote'))}</span></div><div id="daily"></div></div>`;
    html += `<h2>${esc(tr('an.heat'))}</h2><div class="card" id="heat"></div>`;
    html += `<div class="grid2 fit-tbl" style="margin-top:14px"><div class="card"><h2 style="margin-top:0">${esc(tr('an.byProject'))}</h2>${breakdownTable('projects')}</div><div class="card"><h2 style="margin-top:0">${esc(tr('an.byModel'))}</h2>${breakdownTable('models')}</div></div>`;
    return html;
  }

  function selected() {
    return U.STACK_ORDER.filter((id) => ui.agents.has(id)).map((id) => providers().find((p) => p.id === id)).filter((p) => p && p.enabled);
  }

  function niceMax(v) {
    if (v <= 0) return 1;
    const e = Math.pow(10, Math.floor(Math.log10(v)));
    for (const m of [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10]) if (m * e >= v) return m * e;
    return 10 * e;
  }

  function drawDaily() {
    const host = $('#daily');
    if (!host) return;
    const list = selected();
    const W = Math.max(400, host.clientWidth);
    const H = 240;
    const padL = 52;
    const padB = 24;
    const padT = 8;
    const plotW = W - padL - 8;
    const plotH = H - padB - padT;
    const n = ui.range;
    const val = (d) => (ui.metric === 'cost' ? d.cost : d.w);
    const fmt = (v) => (ui.metric === 'cost' ? U.fmtMoneyShort(v) : U.fmtNum(v));
    const days = [];
    for (let i = 0; i < n; i++) {
      const per = list.map((p) => {
        const h = (p.history || []).slice(-n);
        return { p, v: h[i] ? val(h[i]) : 0, day: h[i] ? h[i].day : '' };
      });
      days.push({ day: (per.find((x) => x.day) || {}).day || '', per, total: per.reduce((a, x) => a + x.v, 0) });
    }
    const max = niceMax(Math.max(0, ...days.map((d) => d.total)));
    const y = (v) => padT + plotH - (v / max) * plotH;
    let svg = '';
    for (let g = 0; g <= 4; g++) {
      const v = (max / 4) * g;
      svg += `<line class="grid" x1="${padL}" x2="${W - 8}" y1="${y(v).toFixed(1)}" y2="${y(v).toFixed(1)}"/>`;
      svg += `<text x="${padL - 8}" y="${(y(v) + 4).toFixed(1)}" text-anchor="end">${fmt(v)}</text>`;
    }
    const slot = plotW / n;
    const bw = Math.min(24, slot * 0.68);
    days.forEach((d, i) => {
      const x = padL + i * slot + (slot - bw) / 2;
      let base = padT + plotH;
      const segs = d.per.filter((s) => s.v > 0);
      segs.forEach((s, k) => {
        const h = (s.v / max) * plotH;
        const top = base - h;
        const gap = k < segs.length - 1 ? 0 : 0;
        const isTop = k === segs.length - 1;
        const hh = Math.max(0, h - (k > 0 ? 2 : 0)) - gap;
        const yy = top;
        const r = isTop ? Math.min(4, hh / 2, bw / 2) : 0;
        svg += `<path d="M${x} ${yy + hh} V${yy + r} Q${x} ${yy} ${x + r} ${yy} H${x + bw - r} Q${x + bw} ${yy} ${x + bw} ${yy + r} V${yy + hh} Z" fill="${U.AGENTS[s.p.id].color}"/>`;
        base = top;
      });
      const tip =
        `<b>${esc(d.day.slice(5).replace('-', '/'))}</b>` +
        d.per.map((s) => `<div class="tr"><i style="background:${U.AGENTS[s.p.id].color}"></i>${esc(s.p.name)}<b>${fmt(s.v)}</b></div>`).join('') +
        `<div class="tr" style="border-top:1px solid var(--line-strong);margin-top:4px;padding-top:4px">${esc(tr('an.total'))}<b>${fmt(d.total)}</b></div>`;
      svg += `<rect class="hit" data-tiphtml="${esc(tip)}" x="${(padL + i * slot).toFixed(1)}" y="${padT}" width="${slot.toFixed(1)}" height="${plotH}" fill="transparent"/>`;
      if (i % 5 === 0 || i === n - 1) {
        const label = i === n - 1 ? tr('an.today') : d.day.slice(5).replace('-', '/');
        svg += `<text x="${(padL + i * slot + slot / 2).toFixed(1)}" y="${H - 6}" text-anchor="middle">${label}</text>`;
      }
    });
    const legend = list.length
      ? `<div class="legend">${list.map((p) => `<span><i style="background:${U.AGENTS[p.id].color}"></i>${esc(p.name)}</span>`).join('')}</div>`
      : `<div class="empty">${esc(tr('an.pickAgent'))}</div>`;
    host.innerHTML = `${legend}<svg class="chart" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-label="${esc(tr('an.chartAria'))}">${svg}</svg>`;
  }

  function drawHeat() {
    const host = $('#heat');
    if (!host) return;
    const list = selected();
    const grid = Array.from({ length: 7 }, () => new Array(24).fill(0));
    for (const p of list) {
      (p.tokens.heatmap || []).forEach((r, i) => r.forEach((v, h) => (grid[i][h] += v)));
    }
    const max = Math.max(1, ...grid.flat());
    const color = (v) => {
      if (v <= 0) return 'var(--bg-hover)';
      const t = 0.1 + 0.9 * Math.pow(v / max, 0.75);
      return `rgba(var(--accent-rgb, 240, 92, 26), ${t.toFixed(3)})`;
    };
    const wd = tr('an.wd');
    let html = `<div class="heat"><span></span>${Array.from({ length: 24 }, (_, h) => `<span class="hl">${h % 3 === 0 ? h : ''}</span>`).join('')}`;
    for (let r = 0; r < 7; r++) {
      const d = new Date(Date.now() - (6 - r) * U.DAY);
      const label = r === 6 ? tr('an.today') : `${d.getMonth() + 1}/${d.getDate()} ${wd[d.getDay()]}`;
      html += `<span class="rl">${label}</span>`;
      for (let h = 0; h < 24; h++) {
        const v = grid[r][h];
        html += `<span class="c" style="background:${color(v)}" data-tip="${esc(tr('an.heatTip', { label, h, n: U.fmtNum(v) }))}"></span>`;
      }
    }
    html += `</div><div class="heat-scale">${esc(tr('an.less'))} ${[0.05, 0.25, 0.5, 0.75, 1].map((t) => `<i style="background:${color(t * max)}"></i>`).join('')} ${esc(tr('an.more'))}</div>`;
    host.innerHTML = html;
  }

  function breakdownTable(kind) {
    const list = selected();
    const rows = {};
    for (const p of list) {
      for (const b of p.tokens[kind] || []) {
        const key = kind === 'models' ? `${p.id}:${b.name}` : b.name;
        const r = rows[key] || (rows[key] = { name: b.name, agent: p.id, w: 0, cost: 0 });
        r.w += b.w;
        r.cost += b.cost;
      }
    }
    const items = Object.values(rows).sort((a, b) => b.w - a.w).slice(0, 8);
    if (!items.length) return `<div class="empty">${esc(kind === 'projects' ? tr('an.noProjects') : tr('an.noRecords'))}</div>`;
    const total = items.reduce((a, r) => a + r.w, 0) || 1;
    return (
      // fixed columns: the name gives way (ellipsis), numbers never touch
      `<table class="tbl"><colgroup><col/><col class="c-num"/><col class="c-cost"/></colgroup>` +
      `<thead><tr><th>${esc(kind === 'projects' ? tr('an.project') : tr('an.model'))}</th><th class="r">${esc(tr('an.colTokens'))}</th><th class="r">${esc(tr('an.colCost'))}</th></tr></thead><tbody>` +
      items
        .map(
          (r) =>
            `<tr><td class="nm" title="${esc(r.name)}"><div class="nm-t">${kind === 'models' ? `${R.logo(r.agent, 13)} ` : ''}${esc(r.name)}</div><div class="share"><i style="width:${((r.w / total) * 100).toFixed(1)}%"></i></div></td><td class="r">${U.fmtNum(r.w)}</td><td class="r">${U.fmtMoneyHtml(r.cost)}</td></tr>`,
        )
        .join('') +
      `</tbody></table>`
    );
  }

  // ---------------------------------------------------------------- display (widget options)
  // widget display options, labelled by i18n keys opt.<key> / opt.<key>.sub
  const OPT_KEYS = ['logo', 'name', 'label', 'bar', 'percent', 'reset', 'pace', 'forecast', 'tokens', 'cost', 'system', 'minorWindows', 'gear'];
  /** A settings row labelled by an i18n key (its sub-line from `<key>.sub`, if any). */
  const rowT = (key, control, sub) => row(esc(tr(key)), sub !== undefined ? sub : esc(tr(`${key}.sub`) === `${key}.sub` ? '' : tr(`${key}.sub`)), control);
  const layoutOpts = () => [['taskbar', esc(tr('layout.strip'))], ['floating', esc(tr('layout.card'))]];
  const dateLocale = () => (I18N.lang === 'ko' ? 'ko-KR' : 'en-US');

  function display() {
    const s = ui.previewSettings || S.settings;
    const tab = ui.displayTab;
    const o = s[tab];
    let html = pageHead();
    html += `<div class="fake-taskbar"><div class="apps"><i></i><i></i><i></i><i></i></div><div class="slot" id="pv-taskbar"></div><div class="clock">${esc(U.fmtClock(Date.now()))}<br/>${new Date().toLocaleDateString(dateLocale(), { month: 'numeric', day: 'numeric' })}</div></div>`;
    html += `<div class="preview-cap">${esc(tr(S.canDock === false ? 'disp.stripCap.free' : 'disp.stripCap'))}</div>`;
    html += `<div class="preview"><div><h2>${esc(tr('disp.layout'))}</h2><div class="card rows">`;
    html += row(esc(tr('disp.layout')), tr('disp.layout.sub', { k: esc(s.hotkeyMode || tr('disp.none')) }), seg('mode', layoutOpts(), s.mode));
    if (S.canDock !== false) {
      html += row(
        esc(tr('disp.dock')),
        esc(s.mode === 'floating' ? tr('disp.dock.cardOnly') : tr('disp.dock.sub')),
        sw(`data-set="docked" ${s.mode === 'floating' ? 'disabled' : ''}`, s.docked !== false),
      );
    }
    html += rowT('disp.value', seg('valueMode', [['used', esc(tr('disp.value.used'))], ['remaining', esc(tr('disp.value.remaining'))]], s.valueMode));
    html += rowT('disp.reset', seg('resetFormat', [['relative', esc(tr('disp.reset.relative'))], ['clock', esc(tr('disp.reset.clock'))]], s.resetFormat), '');
    html += `</div></div><div><h2>${esc(tr('disp.cardPreview'))}</h2><div class="float-preview" id="pv-floating"></div></div></div>`;

    html += `<h2>${esc(tr('disp.items'))}</h2><div class="toolbar">${seg('displayTab', layoutOpts(), tab)}<span class="sp"></span><button class="btn sm" data-act="opts-reset">${esc(tr('disp.defaults'))}</button></div>`;
    html += `<div class="card"><div class="opts">${OPT_KEYS.map((k) => `<div class="opt"><div class="lb">${esc(tr(`opt.${k}`))}<small>${esc(tr(`opt.${k}.sub`))}</small></div>${sw(`data-opt="${tab}.${k}"`, o[k])}</div>`).join('')}</div></div>`;

    html += `<h2>${esc(tr('disp.behavior'))}</h2><div class="card rows">`;
    html += row(
      esc(tr('disp.moods')),
      esc(tr('disp.moods.sub')),
      `<div class="fx-row">${sw('data-set="moods"', s.moods !== false)}` +
        `<button class="btn sm" data-act="fx-chaos">${esc(tr('disp.moods.chaos'))}</button>` +
        `<button class="btn sm" data-act="fx-mood">${esc(tr('disp.moods.mood'))}</button></div>`,
    );
    html += rowT('disp.onTop', sw('data-set="alwaysOnTop"', s.alwaysOnTop));
    html += rowT('disp.lock', sw('data-set="lockPosition"', s.lockPosition));
    html += rowT('disp.resetPos', `<button class="btn sm" data-act="reset-pos">${esc(tr('disp.resetPos.btn'))}</button>`);
    html += rowT('disp.look', `<button class="btn sm" data-act="goto" data-id="appearance">${esc(tr('disp.look.btn'))}</button>`);
    html += `</div>`;
    return html;
  }

  function drawPreviews() {
    const s = ui.previewSettings || S.settings;
    const tb = $('#pv-taskbar');
    if (tb) {
      tb.innerHTML = R.widget({ usage: S.usage, settings: s, system: S.system, mode: 'taskbar', taskbarHeight: 46 });
      const bar = tb.parentElement;
      const avail = bar.clientWidth - 140 - 70;
      const w = tb.firstElementChild ? tb.firstElementChild.scrollWidth : 0;
      tb.style.zoom = w > avail ? (avail / w).toFixed(3) : '1';
    }
    const fl = $('#pv-floating');
    if (fl) {
      fl.innerHTML = R.widget({ usage: S.usage, settings: s, system: S.system, mode: 'floating' });
      const card = fl.firstElementChild;
      if (card) card.style.zoom = '0.82';
    }
  }

  // ---------------------------------------------------------------- appearance
  const T = window.AM_THEME;
  function applyTheme() {
    const s = ui.previewSettings || S.settings;
    T.apply(s);
    const z = Number((s.theme || {}).uiScale) || 1;
    $('.app').style.zoom = z === 1 ? '' : String(z);
  }
  const SWATCHES = ['#F05C1A', '#E6B450', '#4CC38A', '#3B9EFF', '#8AB4F8', '#9085E9', '#F27A9B', '#E5484D'];
  const colorField = (key, value, presetValue) =>
    `<div class="colorf"><label class="cswatch" style="background:${esc(value)}"><input type="color" data-theme="${key}" value="${esc(value)}" aria-label="${esc(tr('ap.colorAria', { key }))}"/></label>` +
    `<input class="inp hex" data-theme-hex="${key}" value="${esc(value.toUpperCase())}" maxlength="7" spellcheck="false" aria-label="${key} hex"/>` +
    (value.toLowerCase() !== presetValue.toLowerCase() ? `<button class="btn sm ghost" data-act="theme-reset" data-id="${key}" title="${esc(tr('ap.revert.title'))}">${esc(tr('ap.revert'))}</button>` : '') +
    `</div>`;
  function modeTile(id, label, cur) {
    const tile = (bg, fg, ac) => `<span class="mt-win" style="background:${bg};color:${fg}"><i style="background:${T.mix(bg, fg, 0.12)}"></i><b style="background:${ac}"></b><u style="background:${T.mix(bg, fg, 0.35)}"></u><u style="background:${T.mix(bg, fg, 0.35)};width:40%"></u></span>`;
    const p = T.PRESETS[(S.settings.theme || {}).preset] || T.PRESETS.agentmeter;
    const d = tile(p.dark.bg, p.dark.fg, p.accent);
    const l = tile(p.light.bg, p.light.fg, p.accentLight || p.accent);
    const art = id === 'system' ? `<span class="mt-split">${l}${d}</span>` : id === 'light' ? l : d;
    return `<button class="mode-tile ${cur === id ? 'on' : ''}" data-act="theme-mode" data-id="${id}" aria-pressed="${cur === id}">${art}<span class="mt-l">${esc(label)}</span></button>`;
  }
  const range = (key, min, max, step, value, unit) =>
    `<div class="rangef"><input type="range" min="${min}" max="${max}" step="${step}" data-size="${key}" value="${value}"/><b class="num" id="v-${key}">${value}${unit}</b></div>`;

  function appearance() {
    const s = ui.previewSettings || S.settings;
    const t = s.theme || {};
    const c = T.resolve(t);
    const presetColors = T.resolve({ mode: t.mode, preset: t.preset });
    let html = pageHead(`<button class="btn sm" data-act="theme-defaults">${esc(tr('ap.defaults'))}</button>`);
    html += `<h2>${esc(tr('ap.style'))}</h2><div class="card rows">`;
    html += rowT('ap.mode', `<div class="mode-tiles">${modeTile('system', tr('mode.system'), t.mode)}${modeTile('light', tr('mode.light'), t.mode)}${modeTile('dark', tr('mode.dark'), t.mode)}</div>`);
    html += rowT(
      'ap.theme',
      `<select class="inp sel" data-theme-preset aria-label="${esc(tr('ap.theme'))}">${Object.keys(T.PRESETS)
        .map((id) => `<option value="${id}" ${id === (t.preset || 'agentmeter') ? 'selected' : ''}>${esc(tr(`preset.${id}`))}</option>`)
        .join('')}</select>`,
    );
    html += row(
      esc(tr('ap.accent')),
      esc(tr('ap.accent.sub')),
      `<div class="swatches">${SWATCHES.map((h) => `<button class="sw-dot ${h.toLowerCase() === c.accent.toLowerCase() ? 'on' : ''}" style="background:${h}" data-act="theme-swatch" data-id="${h}" title="${h}" aria-label="${h}"></button>`).join('')}</div>${colorField('accent', c.accent, presetColors.accent)}`,
    );
    html += rowT('ap.bg', colorField('bg', c.bg, presetColors.bg));
    const low = c.contrast < 4.5;
    const ratio = c.contrast.toFixed(1);
    html += row(esc(tr('ap.fg')), low ? `<span class="warn">${esc(tr('ap.fg.low', { r: ratio }))}</span>` : esc(tr('ap.fg.sub', { r: ratio })), colorField('fg', c.fg, presetColors.fg));
    html += rowT(
      'ap.font',
      `<select class="inp sel" data-theme-font aria-label="${esc(tr('ap.font'))}">${Object.keys(T.FONTS)
        .map((id) => `<option value="${id}" ${id === (t.font || 'system') ? 'selected' : ''}>${esc(tr(`font.${id}`))}</option>`)
        .join('')}</select>`,
    );
    html += rowT('ap.uiScale', seg('uiScale', [[0.9, '90%'], [1, '100%'], [1.1, '110%'], [1.25, '125%']], t.uiScale || 1));
    html += `</div>`;

    html += `<h2>${esc(tr('ap.widget'))}</h2>`;
    html += `<div class="fake-taskbar"><div class="apps"><i></i><i></i><i></i><i></i></div><div class="slot" id="pv-taskbar"></div><div class="clock">${esc(U.fmtClock(Date.now()))}</div></div>`;
    html += `<div class="preview"><div class="card rows">`;
    html += rowT('ap.opacity', range('opacity', 30, 100, 1, Math.round(s.opacity * 100), '%'));
    html += rowT('ap.scale', seg('scale', [[0.9, '90%'], [1, '100%'], [1.15, '115%'], [1.3, '130%']], s.scale));
    html += rowT('ap.barW', range('barWidth', 40, 120, 2, s.barWidth || 58, 'px'));
    html += rowT('ap.stripH', range('stripHeight', 32, 56, 2, s.stripHeight || 40, 'px'));
    html += rowT('ap.cardW', range('cardWidth', 260, 420, 4, s.cardWidth || 312, 'px'));
    html += rowT('ap.radius', range('cornerRadius', 0, 16, 1, s.cornerRadius ?? 10, 'px'));
    html += `</div><div class="float-preview" id="pv-floating"></div></div>`;
    return html;
  }

  // ---------------------------------------------------------------- agents

  function agents() {
    let html = pageHead();
    html += `<div class="grid2">`;
    for (const p of providers()) {
      const c = cfgOf(p.id);
      const hasApi = ['claude', 'codex', 'gemini', 'antigravity', ...KEYED].includes(p.id);
      const keyed = KEYED.includes(p.id);
      const hidden = c.hiddenWindows || [];
      html += `<div class="card acfg"><div class="ah">${R.logo(p.id, 22)}<span class="nm">${esc(U.AGENTS[p.id].product)}</span><span class="sp"></span>${statusOf(p, Date.now())}</div>`;
      html += `<div class="src">${tr(`ag.${p.id}`)}</div>`;
      html += `<div class="rows">`;
      html += rowT('ag.collect', sw(`data-prov="${p.id}" data-key="enabled"`, c.enabled !== false));
      html += rowT('ag.show', sw(`data-prov="${p.id}" data-key="visible"`, c.visible !== false));
      if (keyed) html += keyRows(p, c);
      else if (p.id === 'qwen') html += qwenRow(p, c);
      else html += row(esc(tr('ag.login')), esc(connectText(p)), connectBtn(p, { always: true }));
      if (hasApi) html += rowT('ag.api', sw(`data-prov="${p.id}" data-key="useApi"`, c.useApi !== false));
      if (p.id === 'antigravity') html += agyRow(p, c);
      if (p.id === 'claude') {
        const fableOn = !FABLE_IDS.some((id) => hidden.includes(id));
        const has = p.windows.some((w) => FABLE_IDS.includes(w.id));
        html += row(esc(tr('ag.models')), esc(has ? tr('ag.models.has') : tr('ag.models.none')), sw('data-fable="1"', fableOn));
      }
      html += `</div>`;
      if (p.windows.length) {
        html += `<div class="sub-h">${esc(tr('ag.windows'))}</div><div class="chips">`;
        html += p.windows
          .map((w) => `<button class="chip ${hidden.includes(w.id) ? '' : 'on'}" data-act="win-toggle" data-prov="${p.id}" data-id="${esc(w.id)}" aria-pressed="${!hidden.includes(w.id)}"><span class="sw" style="background:var(--accent)"></span>${esc(w.label)}</button>`)
          .join('');
        html += `</div>`;
      }
      // Antigravity, MiniMax and Z.AI have no logs to read
      if (U.hasTokens(p.id)) html += agentForm(p, c);
      if (p.id === 'qwen') html += `<div class="note muted">${esc(p.logFiles ? tr('ag.qwenNote') : tr('qwen.recording'))}</div>`;
      const facts = [
        U.hasTokens(p.id) ? tr('ag.logFiles', { n: p.logFiles || 0 }) : null,
        p.limitsAt && p.id !== 'antigravity' ? tr('ag.limitsAt', { t: U.relTime(p.limitsAt) }) : null,
        p.tokens.unpricedWeek ? tr('ag.unpriced', { n: U.fmtNum(p.tokens.unpricedWeek) }) : null,
      ].filter(Boolean);
      if (facts.length) html += `<div class="note muted">${esc(facts.join(' · '))}</div>`;
      const note = [p.apiError, p.commandError, p.scanError].filter(Boolean).map(I18N.apiError).join(' · ');
      if (note) html += `<div class="note">${esc(note)}</div>`;
      html += `</div>`;
    }
    html += `</div>`;
    return html;
  }

  /** Where agy.exe is: found on its own, set by hand, or not yet (Windows only). */
  function agyRow(p, c) {
    const st = (S.setup || {}).antigravity;
    if (st && !st.supported) return '';
    const manual = (c.antigravityPath || '').trim();
    const path = manual || (st && st.path) || p.exePath || '';
    const text = manual ? tr('ag.agy.manual', { path }) : path ? tr('ag.agy.found', { path }) : tr('ag.agy.missing');
    const btns =
      `<div style="display:flex;flex-direction:column;gap:6px;flex:none"><button class="btn sm" data-act="agy-pick">${esc(tr('ag.agy.browse'))}</button>` +
      (manual ? `<button class="btn sm" data-act="agy-auto">${esc(tr('ag.agy.auto'))}</button>` : '') +
      `</div>`;
    return row(esc(tr('ag.agyPath')), esc(text), btns);
  }

  /** MiniMax / Z.AI: the API key and the platform (region) it belongs to. */
  function keyRows(p, c) {
    const region = c.region === 'china' ? 'china' : 'global';
    const opts = ['global', 'china'].map((r) => `<option value="${r}" ${r === region ? 'selected' : ''}>${esc(tr(`region.${r}`))}</option>`).join('');
    return (
      row(
        esc(tr('ag.apiKey')),
        esc(tr(`ag.apiKey.${p.id}`)),
        `<input class="inp key" type="password" autocomplete="off" spellcheck="false" data-prov="${p.id}" data-key="apiKey" value="${esc(c.apiKey || '')}" placeholder="${esc(tr('ag.apiKey.ph'))}"/>`,
      ) + row(esc(tr('ag.region')), esc(tr('ag.region.sub')), `<select class="inp sel" data-prov="${p.id}" data-key="region">${opts}</select>`)
    );
  }

  /** Qwen: where its chat recordings are (found on its own, or picked). */
  function qwenRow(p, c) {
    const manual = (c.logPath || '').trim();
    const text = manual ? tr('ag.qwenPath.manual', { path: manual }) : tr('ag.qwenPath.auto');
    const btns =
      `<div style="display:flex;flex-direction:column;gap:6px;flex:none"><button class="btn sm" data-act="qwen-pick">${esc(tr('ag.agy.browse'))}</button>` +
      (manual ? `<button class="btn sm" data-act="qwen-auto">${esc(tr('ag.agy.auto'))}</button>` : '') +
      `</div>`;
    return row(esc(tr('ag.qwenPath')), esc(text), btns);
  }

  /** Extra log folders, token budgets and a custom command (agents with logs). */
  function agentForm(p, c) {
    let html = `<div class="form2">`;
    const wslEx = `\\\\wsl.localhost\\Ubuntu\\home\\me\\.${p.id === 'claude' ? 'claude' : p.id}`;
    html += `<label class="full">${esc(tr('ag.extra'))} <small>${esc(tr('ag.extra.sub', { ex: wslEx }))}</small><textarea class="inp" data-prov="${p.id}" data-key="extraPaths" spellcheck="false">${esc(c.extraPaths || '')}</textarea></label>`;
    html += `<div class="hint">${esc(tr('ag.budget'))}</div>`;
    html += `<label>${esc(tr('ag.b5h'))}<input class="inp num" type="number" min="0" step="100000" data-prov="${p.id}" data-key="budget5h" value="${Number(c.budget5h) || 0}"/></label>`;
    html += `<label>${esc(tr('ag.bwk'))}<input class="inp num" type="number" min="0" step="1000000" data-prov="${p.id}" data-key="budgetWeek" value="${Number(c.budgetWeek) || 0}"/></label>`;
    html += `<label class="full">${esc(tr('ag.cmd'))} <small>${tr('ag.cmd.sub', { json: esc('{"windows":[{"label","usedPercent","resetsAt"}]}') })}</small><input class="inp" data-prov="${p.id}" data-key="command" value="${esc(c.command || '')}" spellcheck="false" placeholder="${esc(tr('ag.cmd.ph'))}"/></label>`;
    return html + `</div>`;
  }

  // ---------------------------------------------------------------- alerts & hotkeys
  const HOTKEYS = [
    ['hotkeyMode', 'hk.mode'],
    ['hotkeyVisible', 'hk.visible'],
    ['hotkeyDashboard', 'hk.dashboard'],
  ];

  function alerts() {
    const s = S.settings;
    const th = s.alertThresholds || [80, 95];
    let html = pageHead();
    html += `<h2>${esc(tr('al.alerts'))}</h2><div class="card rows">`;
    html += rowT('al.on', sw('data-set="notifications"', s.notifications));
    html += rowT(
      'al.th',
      `<input class="inp num" type="number" min="1" max="100" data-th="0" value="${th[0] ?? 80}" aria-label="${esc(tr('al.th.warn'))}"/> <input class="inp num" type="number" min="1" max="100" data-th="1" value="${th[1] ?? 95}" aria-label="${esc(tr('al.th.crit'))}"/>`,
    );
    html += rowT('al.reset', sw('data-set="notifyReset"', s.notifyReset));
    html += rowT('al.fc', sw('data-set="notifyForecast"', s.notifyForecast));
    html += rowT('al.tray', sw('data-set="trayGauge"', s.trayGauge));
    html += `</div>`;
    html += `<h2>${esc(tr('al.hotkeys'))}</h2><div class="card rows">`;
    for (const [key, label] of HOTKEYS) {
      const rec = ui.recording === key;
      html += rowT(
        label,
        `<button class="btn hk ${rec ? 'rec' : ''}" data-act="hk-record" data-id="${key}">${rec ? esc(tr('hk.press')) : s[key] ? `<kbd>${esc(s[key])}</kbd>` : `<span style="color:var(--text-3)">${esc(tr('hk.none'))}</span>`}</button>` +
          (s[key] ? `<button class="btn sm" data-act="hk-clear" data-id="${key}">${esc(tr('hk.clear'))}</button>` : ''),
      );
    }
    html += `</div>`;
    if (ui.hotkeyErrors.length) html += `<div class="err">${esc(tr('hk.errors'))}${ui.hotkeyErrors.map(esc).join('<br/>')}</div>`;
    html += `<div class="tip-line">${tr('hk.tip')}</div>`;
    return html;
  }

  // ---------------------------------------------------------------- general
  function general() {
    const s = S.settings;
    let html = pageHead();
    const sysName = I18N.detect() === 'ko' ? '한국어' : 'English';
    html += `<h2>${esc(tr('gen.lang.title'))}</h2><div class="card rows">`;
    html += rowT('gen.lang', seg('language', [['auto', `${esc(tr('gen.lang.auto'))} (${sysName})`], ['ko', '한국어'], ['en', 'English']], s.language || 'auto'));
    html += `</div><h2>${esc(tr('gen.start'))}</h2><div class="card rows">`;
    html += rowT('gen.autostart', sw('data-set="launchAtLogin"', s.launchAtLogin));
    html += `</div><h2>${esc(tr('gen.poll'))}</h2><div class="card rows">`;
    html += rowT('gen.logs', `<input class="inp num" type="number" min="5" max="600" data-set="pollLogsSec" value="${s.pollLogsSec}"/>`);
    html += rowT('gen.api', `<input class="inp num" type="number" min="60" max="3600" data-set="pollApiSec" value="${s.pollApiSec}"/>`);
    html += `</div><h2>${esc(tr('gen.costs'))}</h2><div class="card rows">`;
    html += rowT('gen.currency', seg('currency', [['usd', esc(tr('gen.currency.usd'))], ['krw', esc(tr('gen.currency.krw'))], ['both', esc(tr('gen.currency.both'))]], U.currencyOf(s)));
    const m = U.money;
    const rateNote =
      m.rateSource === 'manual'
        ? tr('gen.rate.manual')
        : m.rateSource === 'auto'
          ? tr('gen.rate.auto', { r: Math.round(m.rate).toLocaleString(dateLocale()), t: U.relTime(m.at) })
          : tr('gen.rate.default', { r: m.rate.toLocaleString(dateLocale()) });
    html += row(esc(tr('gen.rate')), esc(rateNote + tr('gen.rate.zero')), `<input class="inp num" type="number" min="0" max="100000" step="10" data-set="krwRate" value="${Number(s.krwRate) || 0}" aria-label="${esc(tr('gen.rate.aria'))}"/>`);
    html += `</div><h2>${esc(tr('gen.data'))}</h2><div class="card rows">`;
    html += row(esc(tr('gen.dataDir')), esc(S.dataDir || ''), `<button class="btn sm" data-act="open-data">${esc(tr('gen.open'))}</button>`);
    html += rowT('gen.export', `<button class="btn sm" data-act="export">${esc(tr('an.export'))}</button>`);
    html += `</div><h2>${esc(tr('gen.updates'))}</h2><div class="card rows">`;
    const u = S.update || {};
    const updBtn =
      u.state === 'available'
        ? `<button class="btn sm primary" data-act="update-install">${esc(tr('upd.now'))}</button>`
        : `<button class="btn sm" data-act="update-check" ${updateBusy() ? 'disabled' : ''}>${esc(tr('upd.check'))}</button>`;
    html += row(`${esc(tr('upd.current'))} <span class="ver">v${esc(S.version || '')}</span>`, updateText(), updBtn);
    html += rowT('upd.notes', `<button class="btn sm" data-act="notes">${esc(tr('upd.notes.btn'))}</button>`);
    const devBuild = u.auto === false;
    html += row(esc(tr('upd.auto')), esc(devBuild ? tr('upd.auto.dev') : tr('upd.auto.sub')), sw('data-set="autoUpdate"', s.autoUpdate !== false));
    html += rowT('upd.silent', sw(`data-set="updateSilent" ${devBuild || s.autoUpdate === false ? 'disabled' : ''}`, !!s.updateSilent));
    html += `</div><h2>${esc(tr('gen.info'))}</h2><div class="card rows">`;
    html += rowT('gen.privacy', '');
    html += rowT('gen.costInfo', '');
    html += rowT('gen.logos', '');
    html += `</div>`;
    return html;
  }

  // ---------------------------------------------------------------- updates
  const updateBusy = () => ['checking', 'downloading', 'installing'].includes((S.update || {}).state);
  function updateText() {
    const u = S.update || {};
    switch (u.state) {
      case 'checking':
        return esc(tr('upd.checking'));
      case 'latest':
        return esc(tr('upd.latest', { t: U.relTime(u.at) }));
      case 'available':
        return `<span class="accent">${esc(tr('upd.available', { v: u.version }))}</span>`;
      case 'downloading':
        return `<span class="accent">${esc(tr('upd.downloading', { v: u.version, p: u.progress || 0 }))}</span>`;
      case 'installing':
        return `<span class="accent">${esc(tr('upd.installing', { v: u.version }))}</span>`;
      case 'error':
        return esc(tr('upd.failed', { e: u.error || '' }));
      default:
        return esc(tr('upd.idle'));
    }
  }
  function updateBadge() {
    const u = S.update || {};
    const el = $('#tb-update');
    const busy = u.state === 'downloading' || u.state === 'installing';
    el.hidden = !(busy || u.state === 'available');
    el.disabled = busy;
    if (busy) el.textContent = u.state === 'installing' ? tr('badge.installing', { v: u.version }) : tr('badge.downloading', { v: u.version, p: u.progress || 0 });
    else if (u.state === 'available') el.textContent = tr('badge.new', { v: u.version });
  }

  // ---------------------------------------------------------------- patch notes
  let changelog = null;
  async function openNotes() {
    const m = $('#notes-modal');
    m.hidden = false;
    if (changelog == null) {
      $('#notes-body').innerHTML = `<div class="empty">${esc(tr('notes.loading'))}</div>`;
      try {
        changelog = U.parseChangelog(await api.changelog());
      } catch (_) {
        changelog = [];
      }
    }
    renderNotes();
    $('#notes-modal .modal-body').scrollTop = 0;
    m.querySelector('[data-close]').focus();
  }
  function closeNotes() {
    $('#notes-modal').hidden = true;
  }
  function renderNotes() {
    const u = S.update || {};
    const cur = `v${S.version || ''}`;
    const list = (items) => `<ul>${items.map((t) => `<li>${esc(t)}</li>`).join('')}</ul>`;
    let html = '';
    if (u.version && ['available', 'downloading', 'installing'].includes(u.state)) {
      const items = U.noteItems(u.notes);
      const busy = u.state !== 'available';
      html +=
        `<section class="rel new"><div class="rh"><b>v${esc(u.version)}</b><span class="tag accent">${esc(tr('notes.newTag'))}</span><span class="sp"></span>` +
        `<button class="btn sm primary" data-act="update-install" ${busy ? 'disabled' : ''}>${busy ? esc(updateText().replace(/<[^>]+>/g, '')) : esc(tr('upd.now'))}</button></div>` +
        `${list(items.length ? items : [tr('notes.fallback')])}</section>`;
    }
    for (const r of changelog || []) {
      if (u.version && r.version === `v${u.version}`) continue;
      const isCur = r.version === cur;
      html +=
        `<section class="rel${isCur ? ' cur' : ''}"><div class="rh"><b>${esc(r.version === '다음 버전' ? tr('notes.next') : r.version)}</b>${isCur ? `<span class="tag">${esc(tr('notes.current'))}</span>` : ''}` +
        `<span class="sp"></span><span class="date">${esc(r.date)}</span></div>${list(r.items)}</section>`;
    }
    $('#notes-body').innerHTML = html || `<div class="empty">${esc(tr('notes.empty'))}</div>`;
  }
  $('#notes-modal').addEventListener('click', (e) => {
    if (e.target.closest('[data-close]') || e.target.id === 'notes-modal') return closeNotes();
    const b = e.target.closest('[data-act="update-install"]');
    if (b && !b.disabled) {
      S.update = { ...(S.update || {}), state: 'downloading', progress: 0 };
      renderNotes();
      updateBadge();
      api.updateInstall();
    }
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !$('#notes-modal').hidden) closeNotes();
  });
  $('#tb-update').addEventListener('click', () => openNotes());

  // ---------------------------------------------------------------- render & events
  const PAGES = { overview, analytics, display, appearance, agents, alerts, general };

  function typing() {
    const a = document.activeElement;
    return a && main.contains(a) && (a.tagName === 'INPUT' || a.tagName === 'TEXTAREA') && a.type !== 'checkbox' && a.type !== 'range';
  }

  function render(keepScroll = true) {
    if (!PAGES[page]) page = 'overview';
    U.setMoney(S.settings, S.fx);
    applyTheme();
    document.querySelectorAll('#nav > button').forEach((b) => b.classList.toggle('active', b.dataset.page === page));
    document.querySelectorAll('#mode-switch button').forEach((b) => b.classList.toggle('active', b.dataset.mode === S.settings.mode));
    $('#mode-hint').innerHTML = S.settings.hotkeyMode ? tr('side.switch', { k: esc(S.settings.hotkeyMode) }) : esc(tr('side.noKey'));
    const top = main.scrollTop;
    main.innerHTML = PAGES[page]();
    if (keepScroll) main.scrollTop = top;
    if (page === 'analytics') {
      drawDaily();
      drawHeat();
    }
    if (page === 'display' || page === 'appearance') drawPreviews();
    updateStatus();
  }

  function updateStatus() {
    $('#tb-status').textContent = S.usage && S.usage.at ? tr('tb.updated', { t: U.relTime(S.usage.at) }) : tr('tb.collecting');
  }

  function go(p) {
    if (p === page) return;
    page = p;
    cancelRecording();
    render(false);
    main.scrollTop = 0;
  }

  const set = (patch) => api.setSettings(patch);

  /** Leaving the recorder any way but a key press (other page, window closed or
   *  hidden) puts the old shortcuts back. */
  function cancelRecording() {
    if (!ui.recording) return;
    ui.recording = null;
    api.hotkeysSuspend(false);
  }
  window.addEventListener('pagehide', cancelRecording);
  window.addEventListener('beforeunload', cancelRecording);
  // hidden, or still on screen but another window took the focus: the recorder can't
  // hear keys any more, so stop it rather than leave the shortcuts switched off
  const abandon = () => {
    if (!ui.recording) return;
    cancelRecording();
    render();
  };
  document.addEventListener('visibilitychange', () => document.hidden && abandon());
  window.addEventListener('blur', abandon);

  main.addEventListener('click', async (e) => {
    const segBtn = e.target.closest('[data-seg]');
    if (segBtn) {
      const name = segBtn.dataset.seg;
      const v = segBtn.dataset.v;
      if (name === 'metric') ui.metric = v;
      else if (name === 'range') ui.range = Number(v);
      else if (name === 'displayTab') ui.displayTab = v;
      else if (name === 'scale') set({ scale: Number(v) });
      else if (name === 'uiScale') set({ theme: { uiScale: Number(v) } });
      else set({ [name]: v });
      if (['metric', 'range', 'displayTab'].includes(name)) render();
      return;
    }
    const el = e.target.closest('[data-act]');
    if (!el) return;
    const id = el.dataset.id;
    switch (el.dataset.act) {
      case 'agent-filter':
        if (ui.agents.has(id)) ui.agents.delete(id);
        else ui.agents.add(id);
        render();
        break;
      case 'export':
        try {
          const path = await api.exportCsv();
          toast(tr('toast.exported', { p: path }));
        } catch (err) {
          toast(tr('toast.exportFail', { e: I18N.apiError(String(err)) }));
        }
        break;
      case 'open-data':
        api.openDataDir();
        break;
      case 'update-check':
        S.update = { ...(S.update || {}), state: 'checking' };
        render();
        api.updateCheck();
        break;
      case 'update-install':
        S.update = { ...(S.update || {}), state: 'downloading', progress: 0 };
        render();
        api.updateInstall();
        break;
      case 'notes':
        openNotes();
        break;
      case 'receipt':
        window.AM_RECEIPT.open(S, { toast });
        break;
      case 'fx-chaos':
      case 'fx-mood': {
        // the card preview plays it for real: 8 seconds of the same timeline the widget runs
        const first = R.shownProviders(S.usage, ui.previewSettings || S.settings)[0];
        if (!first) break;
        if (el.dataset.act === 'fx-chaos') R.previewChaos(first.id);
        else R.previewMood(first.id);
        const t0 = Date.now();
        const tick = () => {
          drawPreviews();
          if (Date.now() - t0 < 8 * 1000) setTimeout(tick, 120);
        };
        tick();
        break;
      }
      case 'opts-reset': {
        const defaults = {
          taskbar: { logo: true, name: false, label: true, bar: true, percent: true, reset: true, pace: false, forecast: true, tokens: false, cost: false, system: true, minorWindows: false, gear: true },
          floating: { logo: true, name: true, label: true, bar: true, percent: true, reset: true, pace: true, forecast: true, tokens: true, cost: true, system: false, minorWindows: false, gear: true },
        };
        set({ [ui.displayTab]: defaults[ui.displayTab] });
        break;
      }
      case 'reset-pos':
        set({ taskbarOffset: null, floatingPos: null, stripPos: null, docked: true });
        toast(tr('toast.posReset'));
        break;
      case 'goto':
        go(id);
        break;
      case 'theme-mode':
        // light and dark need different colours: custom bg/text don't carry over
        set({ theme: { mode: id, bg: '', fg: '' } });
        break;
      case 'theme-swatch':
        set({ theme: { accent: id } });
        break;
      case 'theme-reset':
        set({ theme: { [id]: '' } });
        break;
      case 'theme-defaults':
        set({ theme: { mode: 'dark', preset: 'agentmeter', accent: '', bg: '', fg: '', font: 'system', uiScale: 1 }, opacity: 0.92, scale: 1, barWidth: 58, stripHeight: 40, cardWidth: 312, cornerRadius: 10 });
        toast(tr('toast.lookReset'));
        break;
      case 'connect':
        try {
          await api.setupConnect(id);
          toast(tr('toast.connectStart'));
        } catch (err) {
          toast(tr('toast.connectFail', { e: I18N.apiError(String(err)) }));
        }
        break;
      case 'agy-pick':
        try {
          const path = await api.antigravityPick();
          if (path) {
            await set({ providers: { antigravity: { antigravityPath: path } } });
            toast(tr('toast.agyPicked'));
            loadSetup(true);
            api.refresh();
          }
        } catch (err) {
          toast(tr('toast.agyFail', { e: I18N.apiError(String(err)) }));
        }
        break;
      case 'qwen-pick': {
        const path = await api.qwenPick();
        if (path) {
          await set({ providers: { qwen: { logPath: path } } });
          api.refresh();
        }
        break;
      }
      case 'qwen-auto':
        await set({ providers: { qwen: { logPath: '' } } });
        api.refresh();
        break;
      case 'agy-auto':
        await set({ providers: { antigravity: { antigravityPath: '' } } });
        loadSetup(true);
        api.refresh();
        break;
      case 'win-toggle': {
        const prov = el.dataset.prov;
        const hidden = new Set(cfgOf(prov).hiddenWindows || []);
        if (hidden.has(id)) hidden.delete(id);
        else hidden.add(id);
        set({ providers: { [prov]: { hiddenWindows: [...hidden] } } });
        break;
      }
      case 'hk-record':
        ui.recording = id;
        api.hotkeysSuspend(true);
        render();
        break;
      case 'hk-clear':
        await set({ [id]: '' });
        break;
      default:
    }
  });

  main.addEventListener('change', (e) => {
    const el = e.target;
    const val = el.type === 'checkbox' ? el.checked : el.type === 'number' ? Number(el.value) || 0 : el.value;
    if (el.dataset.opt) {
      const [tab, key] = el.dataset.opt.split('.');
      set({ [tab]: { [key]: val } });
    } else if (el.dataset.set) set({ [el.dataset.set]: val });
    else if (el.dataset.size) {
      const n = Number(el.value);
      ui.previewSettings = null;
      set({ [el.dataset.size]: el.dataset.size === 'opacity' ? n / 100 : n });
    } else if (el.dataset.theme) set({ theme: { [el.dataset.theme]: el.value.toUpperCase() } });
    else if (el.dataset.themeHex != null) {
      const v = el.value.trim();
      const hex = /^#?[0-9a-fA-F]{6}$/.test(v) ? (v[0] === '#' ? v : `#${v}`).toUpperCase() : null;
      if (hex) set({ theme: { [el.dataset.themeHex]: hex } });
      else {
        toast(tr('toast.hex'));
        render();
      }
    } else if (el.dataset.themePreset != null) set({ theme: { preset: el.value, accent: '', bg: '', fg: '' } });
    else if (el.dataset.themeFont != null) set({ theme: { font: el.value } });
    else if (el.dataset.fable) {
      const hidden = new Set(cfgOf('claude').hiddenWindows || []);
      for (const id of FABLE_IDS) val ? hidden.delete(id) : hidden.add(id);
      set({ providers: { claude: { hiddenWindows: [...hidden] } } });
    }
    else if (el.dataset.prov) {
      const done = set({ providers: { [el.dataset.prov]: { [el.dataset.key]: typeof val === 'string' ? val.trim() : val } } });
      // a new key or region is worth asking about right away
      if (['apiKey', 'region'].includes(el.dataset.key)) Promise.resolve(done).then(() => api.refresh());
    }
    else if (el.dataset.th != null) {
      const th = (S.settings.alertThresholds || [80, 95]).slice();
      th[Number(el.dataset.th)] = Number(el.value) || 0;
      set({ alertThresholds: th });
    } else if (el.dataset.range === 'opacity') {
      ui.previewSettings = null;
      set({ opacity: Number(el.value) / 100 });
    }
  });

  main.addEventListener('input', (e) => {
    const el = e.target;
    if (el.dataset.range === 'opacity') {
      ui.previewSettings = Object.assign({}, S.settings, { opacity: Number(el.value) / 100 });
      $('#op-val').textContent = `${el.value}%`;
      drawPreviews();
    }
    if (el.dataset.theme) {
      const base = ui.previewSettings || S.settings;
      ui.previewSettings = Object.assign({}, base, { theme: Object.assign({}, base.theme, { [el.dataset.theme]: el.value }) });
      el.parentElement.style.background = el.value;
      applyTheme();
      drawPreviews();
    }
    if (el.dataset.size) {
      const k = el.dataset.size;
      const n = Number(el.value);
      ui.previewSettings = Object.assign({}, ui.previewSettings || S.settings, { [k]: k === 'opacity' ? n / 100 : n });
      const out = $(`#v-${k}`);
      if (out) out.textContent = `${n}${k === 'opacity' ? '%' : 'px'}`;
      applyTheme();
      drawPreviews();
    }
  });

  // hotkey recorder
  const CODE_KEYS = {
    PageUp: 'PageUp', PageDown: 'PageDown', Home: 'Home', End: 'End', Insert: 'Insert', Delete: 'Delete',
    ArrowUp: 'ArrowUp', ArrowDown: 'ArrowDown', ArrowLeft: 'ArrowLeft', ArrowRight: 'ArrowRight',
    Space: 'Space', Pause: 'Pause', ScrollLock: 'ScrollLock', Backquote: 'Backquote', Minus: 'Minus', Equal: 'Equal',
    BracketLeft: 'BracketLeft', BracketRight: 'BracketRight', Backslash: 'Backslash', Semicolon: 'Semicolon',
    Quote: 'Quote', Comma: 'Comma', Period: 'Period', Slash: 'Slash',
  };
  function accelFrom(e) {
    let key = null;
    if (/^Key[A-Z]$/.test(e.code)) key = e.code.slice(3);
    else if (/^Digit\d$/.test(e.code)) key = e.code.slice(5);
    else if (/^F\d{1,2}$/.test(e.code)) key = e.code;
    else if (/^Numpad\d$/.test(e.code)) key = e.code;
    else key = CODE_KEYS[e.code] || null;
    if (!key) return null;
    const mods = [];
    if (e.ctrlKey) mods.push('Ctrl');
    if (e.altKey) mods.push('Alt');
    if (e.shiftKey) mods.push('Shift');
    if (e.metaKey) mods.push('Super');
    return [...mods, key].join('+');
  }
  document.addEventListener(
    'keydown',
    async (e) => {
      if (!ui.recording) return;
      e.preventDefault();
      e.stopPropagation();
      const key = ui.recording;
      if (e.code === 'Escape') {
        ui.recording = null;
        api.hotkeysSuspend(false);
        render();
        return;
      }
      if (e.code === 'Backspace') {
        ui.recording = null;
        await set({ [key]: '' });
        api.hotkeysSuspend(false);
        return;
      }
      const accel = accelFrom(e);
      if (!accel) return; // modifier only — keep waiting
      ui.recording = null;
      await set({ [key]: accel });
      api.hotkeysSuspend(false);
      toast(tr('toast.hkSet', { k: accel }));
    },
    true,
  );

  // chart tooltips
  const tip = $('#tip');
  main.addEventListener('mousemove', (e) => {
    const t = e.target.closest('[data-tip],[data-tiphtml]');
    if (!t) return tip.classList.remove('show');
    if (t.dataset.tiphtml) tip.innerHTML = t.dataset.tiphtml;
    else tip.textContent = t.dataset.tip;
    const x = Math.min(window.innerWidth - tip.offsetWidth - 10, e.clientX + 14);
    const y = Math.max(8, e.clientY - tip.offsetHeight - 10);
    tip.style.left = `${x}px`;
    tip.style.top = `${y}px`;
    tip.classList.add('show');
  });
  main.addEventListener('mouseleave', () => tip.classList.remove('show'));

  // chrome
  document.querySelectorAll('#nav > button').forEach((b) => b.addEventListener('click', () => go(b.dataset.page)));
  document.querySelectorAll('#mode-switch button').forEach((b) => b.addEventListener('click', () => set({ mode: b.dataset.mode, widgetVisible: true })));
  // repo / sponsor links: webview ignores target=_blank, so go through the open_url command
  document.querySelectorAll('.side-link[data-url]').forEach((b) =>
    b.addEventListener('click', () => {
      const url = b.dataset.url;
      if (/^https:\/\//.test(url)) api.openUrl(url);
    }),
  );
  $('#btn-settings').addEventListener('click', () => go('general'));
  $('#btn-refresh').addEventListener('click', () => {
    api.refresh();
    toast(tr('toast.refreshing'));
  });
  $('#btn-min').addEventListener('click', () => api.minimizeWindow());
  $('#btn-max').addEventListener('click', () => api.toggleMaximize());
  $('#btn-close').addEventListener('click', () => {
    cancelRecording();
    api.closeWindow();
  });
  window.addEventListener('resize', () => {
    if (page === 'analytics') drawDaily();
    if (page === 'display') drawPreviews();
  });

  // login status of each CLI (cheap file checks in the app); refreshed now and then
  let setupAt = 0;
  async function loadSetup(force) {
    if (!api.setupStatus || (!force && Date.now() - setupAt < 20000)) return;
    setupAt = Date.now();
    try {
      S.setup = await api.setupStatus();
      if (!typing() && !ui.recording && (page === 'overview' || page === 'agents')) render();
    } catch (_) {}
  }
  loadSetup(true);
  window.addEventListener('focus', () => loadSetup(true));

  api.on('usage', (u) => {
    S.usage = u;
    loadSetup(false);
    if (!typing() && !ui.recording) render();
    else updateStatus();
  });
  api.on('settings', (s) => {
    const langChanged = (s.language || 'auto') !== (S.settings.language || 'auto');
    S.settings = s;
    ui.previewSettings = null;
    if (langChanged) applyLang();
    if (!typing() && !ui.recording) render();
  });
  api.on('event', (ev) => {
    if (!ev) return;
    if (ev.type === 'navigate' && ev.page) {
      if (ev.page === 'notes') {
        go('general');
        openNotes();
      } else go(ev.page);
    }
    if (ev.type === 'fx') {
      S.fx = ev.fx;
      if (!typing()) render();
    }
    if (ev.type === 'setup') {
      S.setup = ev.status;
      toast(tr('toast.connected', { name: CONNECT_NAMES[ev.agent] || ev.agent }));
      if (!typing()) render();
    }
    if (ev.type === 'update') {
      // keep what is known about a found version through a background "checking" blip
      const keep = ev.update.state === 'checking' && S.update && S.update.version ? S.update : null;
      S.update = keep || ev.update;
      updateBadge();
      if (!$('#notes-modal').hidden) renderNotes();
      if (page === 'general' && !typing()) render();
    }
    if (ev.type === 'hotkeys') {
      ui.hotkeyErrors = ev.errors || [];
      if (page === 'alerts' && !ui.recording) render();
    }
  });

  render(false);
  if (startWithNotes) openNotes();
  $('#tb-ver').textContent = S.version ? `v${S.version}` : '';
  updateBadge();
  setInterval(updateStatus, 5000);
  setInterval(() => {
    if (page === 'overview' && !typing()) render();
  }, 30000);
})();
