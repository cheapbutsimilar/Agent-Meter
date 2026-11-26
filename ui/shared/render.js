/* Widget renderer shared by the widget window and the dashboard's live preview.
 * Global: AM_RENDER */
(function (root) {
  'use strict';
  const U = root.AM_UTIL;
  const LOGOS = root.AM_LOGOS || {};
  const esc = U.esc;
  const I18N = root.AM_I18N;
  const tr = (k, v) => I18N.t(k, v);

  function logo(id, size) {
    return `<span class="logo ${esc(id)}" style="width:${size}px;height:${size}px">${LOGOS[id] || ''}</span>`;
  }

  function meter(w, settings, opts = {}) {
    const sev = U.severity(w.usedPercent, settings.alertThresholds);
    const val = Math.min(100, U.shown(w, settings));
    // the tick sits where the bar would be at an even pace: elapsed share of the window
    // when showing use, the share still to go when showing what's left
    const at = settings.valueMode === 'remaining' ? 1 - w.elapsed : w.elapsed;
    const pace = opts.pace && w.elapsed != null ? `<b class="pace" style="left:${(Math.max(0, Math.min(1, at)) * 100).toFixed(1)}%"></b>` : '';
    return `<div class="meter sev-${sev}${opts.cls ? ` ${opts.cls}` : ''}"><i style="width:${val.toFixed(1)}%"></i>${pace}</div>`;
  }

  function resetText(w, settings, now, short) {
    if (!w.resetsAt) return w.stale ? (short ? '↻--' : tr('w.resetDone')) : '';
    const left = w.resetsAt - now;
    if (settings.resetFormat === 'clock') return short ? `↻${U.fmtClock(w.resetsAt, now)}` : tr('w.resetAt', { t: U.fmtClock(w.resetsAt, now) });
    return short ? `↻${U.fmtShortLeft(left)}` : tr('w.resetIn', { t: U.fmtLeft(left) });
  }

  function forecastText(w, now) {
    if (!w.exhaustBeforeReset || !w.eta) return '';
    return tr('w.forecast', { t: U.fmtLeft(w.eta - now) });
  }

  /** Plain-text tooltip for a provider (used in the docked widget). */
  function tooltip(p, settings, now) {
    const lines = [`${p.name}${p.plan ? ` · ${p.plan}` : ''}`];
    for (const w of p.windows || []) {
      const used = Math.round(w.usedPercent);
      let line = tr('w.tipLine', { label: w.label, used, left: 100 - used });
      if (w.resetsAt) line += tr('w.tipReset', { at: U.fmtDateTime(w.resetsAt), in: U.fmtLeft(w.resetsAt - now) });
      else if (w.source === 'estimate') line += tr('w.tipEstimate');
      lines.push(line);
      const fc = forecastText(w, now);
      if (fc) lines.push(tr('w.tipForecast', { fc }));
      else if (w.ratePerHour) lines.push(tr('w.tipRate', { r: w.ratePerHour.toFixed(1) }));
    }
    const t = p.tokens || {};
    lines.push(tr('w.tipToday', { n: U.fmtNum(t.today), cost: U.fmtMoney(t.costToday) }));
    if (p.apiError) lines.push(`! ${I18N.apiError(p.apiError)}`);
    return lines.join('\n');
  }

  /** Korean UI only: a little speech bubble when an agent's tightest shown limit is
   * nearly out (≤ 5% left) or gone (0%). Windows without a reading or already reset don't count.
   * The bubble shows once per crossing for MEME_MS, fading in and out; re-renders in between
   * pick the animation up where it is (negative delay), so it never restarts or flickers. */
  const MEME_MS = 5000;
  const memeSeen = {}; // provider id -> { level: 'hot' | 'done', at }
  let memeNew = 0; // bubbles that started since the last takeMemeEvents()

  function memeLevel(ws) {
    let least = null;
    for (const w of ws) {
      if (w.stale || typeof w.usedPercent !== 'number' || !Number.isFinite(w.usedPercent)) continue;
      const left = Math.max(0, 100 - w.usedPercent);
      if (least === null || left < least) least = left;
    }
    if (least === null || least > 5) return '';
    return least <= 0 ? 'done' : 'hot';
  }

  function meme(p, ws, now) {
    if (I18N.lang !== 'ko') return '';
    const level = memeLevel(ws);
    const seen = memeSeen[p.id];
    if (!level) {
      delete memeSeen[p.id];
      return '';
    }
    if (!seen || seen.level !== level) {
      memeSeen[p.id] = { level, at: now };
      memeNew++;
    }
    const age = now - memeSeen[p.id].at;
    if (age < 0 || age >= MEME_MS) return '';
    const done = level === 'done';
    return `<span class="meme${done ? ' done' : ''}" style="animation-delay:-${age}ms">${esc(tr(done ? 'w.meme.done' : 'w.meme.hot'))}</span>`;
  }

  /** How many bubbles appeared since the last call (the widget plays a sound for them). */
  function takeMemeEvents() {
    const n = memeNew;
    memeNew = 0;
    return n;
  }

  function shownProviders(usage, settings) {
    return (usage.providers || []).filter((p) => {
      const cfg = (settings.providers || {})[p.id] || {};
      return p.enabled && cfg.visible !== false && p.detected;
    });
  }

  // ------------------------------------------------------------------ taskbar (docked)
  function taskbar(ctx) {
    const { usage, settings, system, now } = ctx;
    const o = settings.taskbar;
    const h = ctx.taskbarHeight || 40;
    const rowsMax = h >= 34 ? 2 : 1;
    const blocks = [];
    for (const p of shownProviders(usage, settings)) {
      const ws = U.visibleWindows(p, settings, o);
      const active = U.isActive(p, now) ? `<span class="live" title="${esc(tr('w.working'))}"></span>` : '';
      const bubble = meme(p, ws, now);
      let html = `<div class="prov${bubble ? ' has-meme' : ''}" data-id="${p.id}" title="${esc(tooltip(p, settings, now))}">${bubble}`;
      if (o.logo) html += `<span class="lg">${logo(p.id, h >= 34 ? 18 : 16)}${active}</span>`;
      if (o.name) html += `<span class="nm">${esc(p.name)}</span>`;
      if (!ws.length) {
        html += `<span class="tok">${U.fmtNum(p.tokens.today)}</span>`;
      }
      for (let i = 0; i < ws.length; i += rowsMax) {
        html += `<div class="grp" style="grid-template-rows:repeat(${Math.min(rowsMax, ws.length - i)},1fr)">`;
        for (const w of ws.slice(i, i + rowsMax)) {
          const sev = U.severity(w.usedPercent, settings.alertThresholds);
          html += `<div class="row">`;
          if (o.reset) html += `<span class="rs">${esc(resetText(w, settings, now, true))}</span>`;
          if (o.label) html += `<span class="lb">${esc(w.label)}</span>`;
          if (o.bar) html += meter(w, settings, { pace: o.pace });
          if (o.percent) html += `<span class="pc sev-${sev}">${U.fmtPct(U.shown(w, settings))}</span>`;
          if (o.forecast && w.exhaustBeforeReset) html += `<span class="fc" title="${esc(forecastText(w, now))}">▲</span>`;
          html += `</div>`;
        }
        html += `</div>`;
      }
      // Antigravity writes no token logs, so it has no token or cost figures
      if ((o.tokens || o.cost) && U.hasTokens(p.id)) {
        html += `<div class="ex">`;
        if (o.tokens) html += `<span>${U.fmtNum(p.tokens.today)}</span>`;
        if (o.cost) html += `<span>${U.fmtMoney(p.tokens.costToday)}</span>`;
        html += `</div>`;
      }
      blocks.push(html + `</div>`);
    }
    if (o.system && system) {
      blocks.push(
        `<div class="prov sys" title="CPU ${Math.round(system.cpu)}% · RAM ${Math.round(system.ram)}%"><div class="grp" style="grid-template-rows:repeat(${rowsMax},1fr)">` +
          `<div class="row"><span class="lb">CPU</span>${sysMeter(system.cpu)}<span class="pc">${Math.round(system.cpu)}%</span></div>` +
          (rowsMax > 1 ? `<div class="row"><span class="lb">RAM</span>${sysMeter(system.ram)}<span class="pc">${Math.round(system.ram)}%</span></div>` : '') +
          `</div></div>`,
      );
    }
    if (!blocks.length) blocks.push(`<div class="prov empty">${esc(tr('w.noAgentsShort'))}</div>`);
    const gl = esc(tr('w.settings'));
    const gear = o.gear !== false ? `<span class="sep"></span><button class="ib gear" data-act="settings" title="${gl}" aria-label="${gl}">${ICON.gear}</button>` : '';
    const free = ctx.docked === false ? ' free' : '';
    return `<div class="w taskbar${free}" style="--h:${h}px;--op:${settings.opacity}">${blocks.join('<span class="sep"></span>')}${gear}</div>`;
  }

  function sysMeter(v) {
    return `<div class="meter sys"><i style="width:${Math.min(100, v || 0)}%"></i></div>`;
  }

  /** How fresh the limits are: the oldest official-API reading among the shown agents. */
  function upd(list, usage, now) {
    const api = list.filter((p) => p.source === 'api' && p.limitsAt).map((p) => p.limitsAt);
    const at = api.length ? Math.min(...api) : usage.at;
    const stale = api.length && now - at > 10 * 60 * 1000;
    return `<span class="upd${stale ? ' stale' : ''}" title="${esc(tr('w.limitsAt'))}">${esc(U.relTime(at, now))}</span>`;
  }

  // ------------------------------------------------------------------ floating (card)
  function floating(ctx) {
    const { usage, settings, system, now } = ctx;
    const o = settings.floating;
    const list = shownProviders(usage, settings);
    let html = `<div class="w floating" style="--op:${settings.opacity}">`;
    html += `<header class="head"><span class="mark"></span><span class="ttl">AgentMeter</span><span class="sp"></span>` +
      upd(list, usage, now) +
      `<button class="ib" data-act="refresh" title="${esc(tr('w.refresh'))}">${ICON.refresh}</button>` +
      `<button class="ib" data-act="dashboard" title="${esc(tr('w.dashboard'))}">${ICON.panel}</button>` +
      (o.gear !== false ? `<button class="ib" data-act="settings" title="${esc(tr('w.settings'))}" aria-label="${esc(tr('w.settings'))}">${ICON.gear}</button>` : '') +
      `</header>`;
    for (const p of list) {
      const ws = U.visibleWindows(p, settings, o);
      html += `<section class="prov" data-id="${p.id}">`;
      html += `<div class="ph">`;
      if (o.logo) html += logo(p.id, 18);
      if (o.name) html += `<span class="nm">${esc(p.name)}</span>`;
      if (p.plan) html += `<span class="plan">${esc(p.plan)}</span>`;
      html += meme(p, ws, now);
      html += `<span class="sp"></span>`;
      if (U.isActive(p, now)) html += `<span class="state live-t"><i class="live"></i>${esc(tr('w.working'))}</span>`;
      html += `</div>`;
      for (const w of ws) {
        const sev = U.severity(w.usedPercent, settings.alertThresholds);
        const val = U.shown(w, settings);
        html += `<div class="wl">`;
        if (o.label) html += `<span class="lb" title="${esc(U.windowName(w))}">${esc(w.label)}</span>`;
        if (o.bar) html += meter(w, settings, { pace: o.pace });
        if (o.percent) html += `<span class="pc sev-${sev}">${U.fmtPct(val)}${settings.valueMode === 'remaining' ? `<small>${esc(tr('w.left'))}</small>` : ''}</span>`;
        html += `</div>`;
        const meta = [];
        if (o.reset) {
          const r = resetText(w, settings, now, false);
          if (r) meta.push(`<span>${esc(r)}</span>`);
        }
        if (o.forecast) {
          const fc = forecastText(w, now);
          if (fc) meta.push(`<span class="fc">▲ ${esc(fc)}</span>`);
        }
        if (meta.length) html += `<div class="meta">${meta.join('')}</div>`;
      }
      if (!ws.length) html += `<div class="meta"><span>${esc(tr(p.id === 'qwen' ? 'w.localOnly' : 'w.noLimits'))}</span></div>`;
      const stats = [];
      const counted = U.hasTokens(p.id);
      if (o.tokens && counted) stats.push(tr('w.today', { n: U.fmtNum(p.tokens.today) }));
      if (o.cost && counted) stats.push(`<b>${U.fmtMoney(p.tokens.costToday)}</b> <span title="${esc(tr('w.costTitle'))}">${esc(tr('w.cost'))}</span>`);
      if (stats.length) html += `<div class="st">${stats.join('<i></i>')}</div>`;
      html += `</section>`;
    }
    if (!list.length) html += `<section class="prov empty">${tr('w.noAgents')}</section>`;
    if (o.system && system) {
      html += `<footer class="sysf"><span>CPU</span>${sysMeter(system.cpu)}<b>${Math.round(system.cpu)}%</b><span>RAM</span>${sysMeter(system.ram)}<b>${Math.round(system.ram)}%</b></footer>`;
    }
    return html + `</div>`;
  }

  const ICON = {
    refresh: '<svg viewBox="0 0 24 24"><path d="M20 12a8 8 0 1 1-2.34-5.66M20 4v5h-5"/></svg>',
    panel: '<svg viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/></svg>',
    gear: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>',
  };

  function widget(ctx) {
    ctx.now = ctx.now || Date.now();
    return ctx.mode === 'floating' ? floating(ctx) : taskbar(ctx);
  }

  root.AM_RENDER = { widget, takeMemeEvents, MEME_MS, logo, meter, tooltip, resetText, forecastText, shownProviders, ICON };
})(typeof self !== 'undefined' ? self : this);
