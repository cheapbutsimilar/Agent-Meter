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

  /** A little speech bubble when an agent's tightest shown limit is nearly out (≤ 5% left)
   * or gone (0%). Windows without a reading or already reset don't count.
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

  // ------------------------------------------------------------------ agent moods
  // Playful life for the floating card only: the strip stays calm so it never gets in the
  // way of the taskbar. Two things can happen:
  //   * chaos encounter — a limit hits 0% (or, rarely, nothing at all): the card bounces and
  //     the agent's logo spins, everything stops dead for a moment, then one flat quip
  //     ("ㅋㅋ" in Korean, "bruh." in every other language) before it quietly goes back to normal.
  //   * a click on an agent — a random little mood (blush, sweat, dizzy, hearts, sleepy).
  // Every stage is keyed to elapsed time, so a re-render in the middle picks the animation up
  // where it is instead of restarting it (the widget re-renders on stage boundaries).
  const CHAOS_FIT = 1800; // bounce + spin
  const CHAOS_STILL = 4800; // everything frozen (3 s of stillness)
  const CHAOS_LINE = 7800; // one quip, then done
  const RANDOM_GAP = 30 * 60 * 1000; // no sudden chaos more often than this
  const MOOD_MS = 2600;
  const MOODS = [
    { id: 'blush', emoji: '😳', keys: ['w.mood.blush', 'w.mood.blush2'] },
    { id: 'sweat', emoji: '💦', keys: ['w.mood.sweat', 'w.mood.sweat2'] },
    { id: 'dizzy', emoji: '💫', keys: ['w.mood.dizzy', 'w.mood.dizzy2'] },
    { id: 'love', emoji: '💗', keys: ['w.mood.love', 'w.mood.love2'] },
    { id: 'sleep', emoji: '💤', keys: ['w.mood.sleep', 'w.mood.sleep2'] },
  ];
  // chaos: {id, at}, mood: {id, at, kind, emoji, line}; the first roll waits for the app to settle
  const moodFx = { chaos: null, mood: null, lastRandom: Date.now(), lastRoll: 0 };
  const chaosSeen = {}; // provider id -> a 0% limit already got its encounter
  const pickOne = (a) => a[Math.floor(Math.random() * a.length)];

  function startChaos(id, now) {
    if (!id) return false;
    moodFx.chaos = { id, at: now };
    moodFx.mood = null;
    return true;
  }

  function startMood(id, now, kind) {
    if (!id) return false;
    const m = (kind && MOODS.find((x) => x.id === kind)) || pickOne(MOODS);
    moodFx.mood = { id, at: now, kind: m.id, emoji: m.emoji, line: pickOne(m.keys) };
    return true;
  }

  /** Where the encounter is right now (null once it is over). */
  function chaosStage(c, now) {
    const age = now - c.at;
    if (age < CHAOS_FIT) return { stage: 'bounce', age: Math.max(0, age) };
    if (age < CHAOS_STILL) return { stage: 'still', age };
    if (age < CHAOS_LINE) return { stage: 'line', age: age - CHAOS_STILL };
    return null;
  }

  /** ms until the encounter's next stage (0 when there is none) — widget.js re-renders then. */
  function chaosNext(now = Date.now()) {
    if (!moodFx.chaos) return 0;
    const age = now - moodFx.chaos.at;
    for (const t of [CHAOS_FIT, CHAOS_STILL, CHAOS_LINE]) if (age < t) return Math.max(40, t - age);
    return 0;
  }

  /** Drop finished effects, spot a limit that just hit 0%, and now and then cause trouble. */
  function advanceFx(list, settings, now) {
    if (moodFx.chaos && !chaosStage(moodFx.chaos, now)) moodFx.chaos = null;
    if (moodFx.mood && now - moodFx.mood.at >= MOOD_MS) moodFx.mood = null;
    if (settings && settings.moods === false) {
      moodFx.chaos = null;
      moodFx.mood = null;
      for (const p of list) delete chaosSeen[p.id];
      return;
    }
    for (const p of list) {
      const done = memeLevel(U.visibleWindows(p, settings, settings.floating)) === 'done';
      if (!done) {
        delete chaosSeen[p.id];
      } else if (!chaosSeen[p.id] && !moodFx.chaos) {
        chaosSeen[p.id] = true;
        startChaos(p.id, now);
      }
    }
    // one roll every few seconds at most, so fast re-renders (the settings preview) don't spam it
    if (!moodFx.chaos && list.length && now - moodFx.lastRoll > 5000 && now - moodFx.lastRandom > RANDOM_GAP) {
      moodFx.lastRoll = now;
      if (Math.random() < 0.04) {
        moodFx.lastRandom = now;
        startChaos(pickOne(list).id, now);
      }
    }
  }

  /** A click on an agent in the card (widget.js). */
  function poke(id, settings, now = Date.now()) {
    if (settings && settings.moods === false) return false;
    return startMood(id, now);
  }

  /** The settings page previews both, without waiting for a 0% limit. */
  function previewChaos(id, now = Date.now()) {
    return startChaos(id, now);
  }
  function previewMood(id, now = Date.now()) {
    return startMood(id, now);
  }

  // ------------------------------------------------------------------ today / 7 days
  /** Token and cost figures follow settings.statsPeriod: today, or the last 7 days. */
  const isWeek = (settings) => settings && settings.statsPeriod === 'week';
  const statTokens = (p, settings) => ((p.tokens || {})[isWeek(settings) ? 'week' : 'today']) || 0;
  const statCost = (p, settings) => ((p.tokens || {})[isWeek(settings) ? 'costWeek' : 'costToday']) || 0;
  /** A small calendar showing 1 (today) or 7 (the last 7 days); a click flips it. */
  function periodIcon(week) {
    return `<svg viewBox="0 0 24 24" class="per"><rect x="4" y="5.5" width="16" height="14.5" rx="2.6"/><path d="M4 9.8h16M8.6 3.5v3.6M15.4 3.5v3.6"/><text x="12" y="17.7" text-anchor="middle">${week ? 7 : 1}</text></svg>`;
  }
  function periodButton(settings) {
    const week = isWeek(settings);
    const t = esc(tr(week ? 'w.period.week' : 'w.period.today'));
    return `<button class="ib period${week ? ' on' : ''}" data-act="period" title="${t}" aria-label="${t}" aria-pressed="${week}">${periodIcon(week)}</button>`;
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
      let html = `<div class="prov" data-id="${p.id}" title="${esc(tooltip(p, settings, now))}">`;
      if (o.logo) html += `<span class="lg">${logo(p.id, h >= 34 ? 18 : 16)}${active}</span>`;
      if (o.name) html += `<span class="nm">${esc(p.name)}</span>`;
      if (bubble) html += bubble;
      if (!ws.length) {
        html += `<span class="tok">${U.fmtNum(statTokens(p, settings))}</span>`;
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
        if (o.tokens) html += `<span>${U.fmtNum(statTokens(p, settings))}</span>`;
        if (o.cost) html += `<span>${U.fmtMoney(statCost(p, settings))}</span>`;
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
    // the today ↔ 7 days switch sits by the gear when the strip shows any tokens or cost
    const showsTokens = o.tokens || o.cost || shownProviders(usage, settings).some((p) => !U.visibleWindows(p, settings, o).length);
    const per = showsTokens ? periodButton(settings) : '';
    const gear = o.gear !== false || per ? `<span class="sep"></span>${per}${o.gear !== false ? `<button class="ib gear" data-act="settings" title="${gl}" aria-label="${gl}">${ICON.gear}</button>` : ''}` : '';
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
    advanceFx(list, settings, now);
    const chaos = moodFx.chaos ? chaosStage(moodFx.chaos, now) : null;
    const timing = chaos && chaos.stage === 'bounce' ? `;animation-delay:-${Math.round(chaos.age)}ms` : '';
    let html = `<div class="w floating${chaos ? ` chaos-${chaos.stage}` : ''}" style="--op:${settings.opacity}${timing}">`;
    html += `<header class="head"><span class="mark"></span><span class="ttl">AgentMeter</span><span class="sp"></span>` +
      upd(list, usage, now) +
      `<button class="ib" data-act="refresh" title="${esc(tr('w.refresh'))}">${ICON.refresh}</button>` +
      periodButton(settings) +
      `<button class="ib" data-act="dashboard" title="${esc(tr('w.dashboard'))}">${ICON.panel}</button>` +
      (o.gear !== false ? `<button class="ib" data-act="settings" title="${esc(tr('w.settings'))}" aria-label="${esc(tr('w.settings'))}">${ICON.gear}</button>` : '') +
      `</header>`;
    for (const p of list) {
      const ws = U.visibleWindows(p, settings, o);
      const mine = chaos && moodFx.chaos.id === p.id ? chaos : null;
      const mood = !chaos && moodFx.mood && moodFx.mood.id === p.id ? moodFx.mood : null;
      html += `<section class="prov${mood ? ` mood-${mood.kind}` : ''}" data-id="${p.id}"` +
        `${mine ? ` data-fx="${mine.stage}"` : ''}${mood ? ` style="animation-delay:-${Math.round(now - mood.at)}ms"` : ''}>`;
      html += `<div class="ph">`;
      if (o.logo) html += logo(p.id, 18);
      if (o.name) html += `<span class="nm">${esc(p.name)}</span>`;
      if (mood) html += `<span class="mo" aria-hidden="true" style="animation-delay:-${Math.round(now - mood.at)}ms">${mood.emoji}</span>`;
      if (p.plan) html += `<span class="plan">${esc(p.plan)}</span>`;
      html += `<span class="sp"></span>`;
      if (U.isActive(p, now)) html += `<span class="state live-t"><i class="live"></i>${esc(tr('w.working'))}</span>`;
      html += `</div>`;
      // during an encounter the 5% bubble keeps quiet: one quip, at the end
      const bubble = mine
        ? mine.stage === 'line'
          ? `<span class="meme quip chaos" style="animation-delay:-${Math.round(mine.age)}ms">${esc(tr('w.chaos.line'))}</span>`
          : ''
        : meme(p, ws, now);
      if (bubble) html += `<div class="mrow">${bubble}</div>`;
      if (mood) html += `<div class="mrow"><span class="meme quip mood" style="animation-delay:-${Math.round(now - mood.at)}ms">${esc(tr(mood.line))}</span></div>`;
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
      const week = isWeek(settings);
      if (o.tokens && counted) stats.push(tr(week ? 'w.week' : 'w.today', { n: U.fmtNum(statTokens(p, settings)) }));
      if (o.cost && counted) stats.push(`<b>${U.fmtMoney(statCost(p, settings))}</b> <span title="${esc(tr('w.costTitle'))}">${esc(tr('w.cost'))}</span>`);
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

  root.AM_RENDER = {
    periodIcon,
    statTokens,
    statCost,
    widget,
    takeMemeEvents,
    MEME_MS,
    logo,
    meter,
    tooltip,
    resetText,
    forecastText,
    shownProviders,
    ICON,
    poke,
    previewChaos,
    previewMood,
    chaosNext,
    MOODS,
    MOOD_MS,
    CHAOS_MS: { fit: CHAOS_FIT, still: CHAOS_STILL, line: CHAOS_LINE },
  };
})(typeof self !== 'undefined' ? self : this);
