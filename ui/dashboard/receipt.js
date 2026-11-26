/* Usage receipt: a modern dark receipt card drawn on a canvas — so the PNG that goes to X is
 * exactly what the user sees — with the period picker, the plan-fee inputs and the
 * copy / save / share-on-X actions. Global: AM_RECEIPT */
(function (root) {
  'use strict';
  const U = root.AM_UTIL;
  const tr = (k, v) => root.AM_I18N.t(k, v);
  const esc = U.esc;
  const api = () => root.agentMeter;

  const W = 420; // the card's layout width, in CSS px
  const PAD = 26;
  const SS = 2; // the PNG is 2×, so it stays crisp in a tweet
  const INK = '#F4F2F0';
  const DIM = 'rgba(244, 242, 240, 0.54)';
  const FAINT = 'rgba(244, 242, 240, 0.32)';
  const LINE = 'rgba(255, 255, 255, 0.13)';
  const FONT = `-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Malgun Gothic', 'Apple SD Gothic Neo', 'Noto Sans KR', sans-serif`;
  const MONO = `'JetBrains Mono', 'Cascadia Mono', 'D2Coding', ui-monospace, Consolas, 'SF Mono', monospace`;
  const GITHUB = 'https://github.com/cheapbutsimilar/Agent-Meter';
  const MAX_AGENTS = 4;
  const MAX_MODELS = 4;

  /** Public monthly prices (USD) for the plans the agents report, used until the user types
   *  their own. Longest key first, so "max 20x" is not read as "max". */
  const PLAN_FEES = {
    claude: [
      ['ultra', 200],
      ['max', 100],
      ['pro', 20],
      ['team', 30],
    ],
    codex: [
      ['pro', 200],
      ['plus', 20],
      ['team', 30],
    ],
    gemini: [
      ['ultra', 250],
      ['pro', 20],
    ],
    grok: [['supergrok', 30]],
    minimax: [['plus', 20]],
    zai: [['pro', 20]],
  };

  function planFee(p, settings) {
    const custom = (settings.planFees || {})[p.id];
    if (typeof custom === 'number' && Number.isFinite(custom)) return Math.max(0, custom);
    const plan = String(p.plan || '').toLowerCase();
    for (const [key, fee] of PLAN_FEES[p.id] || []) if (plan.includes(key)) return fee;
    return 0;
  }

  // ---------------------------------------------------------------- data
  const p2 = (n) => String(n).padStart(2, '0');
  const dayKey = (ts) => {
    const d = new Date(ts);
    return `${d.getFullYear()}-${p2(d.getMonth() + 1)}-${p2(d.getDate())}`;
  };
  const usd = (v) =>
    '$' + (Math.abs(v) >= 10000 ? Math.round(v).toLocaleString('en-US') : v.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 }));
  const krw = (v) => '₩' + Math.round(v * (U.money.rate || 1400)).toLocaleString('ko-KR');

  function stats(S, period) {
    const usage = S.usage || { providers: [] };
    const settings = S.settings || {};
    const now = Date.now();
    const month = dayKey(now).slice(0, 7);
    const days = period === 'today' ? 1 : period === 'week' ? 7 : new Date(now).getDate();
    const rows = [];
    for (const p of usage.providers || []) {
      if (p.enabled === false || !U.hasTokens(p.id)) continue;
      const t = p.tokens || {};
      let tokens = 0;
      let cost = 0;
      if (period === 'today') {
        tokens = t.today || 0;
        cost = t.costToday || 0;
      } else if (period === 'week') {
        tokens = t.week || 0;
        cost = t.costWeek || 0;
      } else {
        for (const d of p.history || []) {
          if (String(d.day).startsWith(month)) {
            tokens += d.w || 0;
            cost += d.cost || 0;
          }
        }
      }
      if (tokens > 0) rows.push({ p, tokens, cost, fee: planFee(p, settings) });
    }
    rows.sort((a, b) => b.tokens - a.tokens);
    const tokens = rows.reduce((a, r) => a + r.tokens, 0);
    const cost = rows.reduce((a, r) => a + r.cost, 0);
    const fee = rows.reduce((a, r) => a + r.fee, 0);
    const feeCost = fee * (days / 30);
    const models = [];
    for (const r of rows) for (const m of (r.p.tokens && r.p.tokens.models) || []) models.push({ name: m.name, w: m.w || 0 });
    models.sort((a, b) => b.w - a.w);
    const topModels = models.slice(0, MAX_MODELS);
    return {
      period,
      now,
      rows,
      tokens,
      cost,
      fee,
      feeCost,
      roi: feeCost > 0 ? cost / feeCost : null,
      days,
      topModels,
      modelTotal: topModels.reduce((a, m) => a + m.w, 0) || 1,
    };
  }

  function periodLabel(period, now) {
    const fmt = (d) => d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' }).toUpperCase();
    const d = new Date(now);
    if (period === 'today') return fmt(d);
    const from = period === 'week' ? new Date(now - 6 * U.DAY) : new Date(d.getFullYear(), d.getMonth(), 1);
    return `${fmt(from)} – ${fmt(d)}`;
  }

  // ---------------------------------------------------------------- drawing
  function roundRect(c, x, y, w, h, r) {
    const rr = Math.max(0, Math.min(r, w / 2, h / 2));
    c.beginPath();
    c.moveTo(x + rr, y);
    c.arcTo(x + w, y, x + w, y + h, rr);
    c.arcTo(x + w, y + h, x, y + h, rr);
    c.arcTo(x, y + h, x, y, rr);
    c.arcTo(x, y, x + w, y, rr);
    c.closePath();
  }

  function dotted(c, y) {
    c.save();
    c.strokeStyle = LINE;
    c.lineWidth = 1;
    c.setLineDash([2, 4]);
    c.beginPath();
    c.moveTo(PAD, y + 0.5);
    c.lineTo(W - PAD, y + 0.5);
    c.stroke();
    c.restore();
  }

  function dashed(c, y) {
    c.save();
    c.strokeStyle = LINE;
    c.lineWidth = 1;
    c.setLineDash([7, 4]);
    c.beginPath();
    c.moveTo(PAD, y + 0.5);
    c.lineTo(W - PAD, y + 0.5);
    c.stroke();
    c.restore();
  }

  /** Trim a name to the space it has, with an ellipsis. Set the font before calling. */
  function clip(c, text, max) {
    let s = String(text || '');
    if (c.measureText(s).width <= max) return s;
    while (s.length > 1 && c.measureText(`${s}…`).width > max) s = s.slice(0, -1);
    return `${s}…`;
  }

  function hash(str) {
    let h = 2166136261;
    for (let i = 0; i < str.length; i++) h = Math.imul(h ^ str.charCodeAt(i), 16777619);
    return h >>> 0;
  }

  function barcode(c, x, y, w, h, seed) {
    let s = seed || 1;
    const rnd = () => (s = (Math.imul(s, 1664525) + 1013904223) >>> 0) / 4294967296;
    let cx = x;
    c.fillStyle = 'rgba(244, 242, 240, 0.78)';
    while (cx < x + w - 1) {
      const bw = 1 + Math.floor(rnd() * 3.2);
      if (rnd() > 0.38) c.fillRect(cx, y, bw, h);
      cx += bw + 1 + Math.floor(rnd() * 2.4);
    }
  }

  function rgba(hex, a) {
    const n = parseInt(String(hex).slice(1), 16);
    return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
  }

  function paint(S, period) {
    const d = stats(S, period);
    const accent = accentOf(S);
    const scratch = document.createElement('canvas');
    scratch.width = W * SS;
    scratch.height = 1400 * SS;
    const c = scratch.getContext('2d');
    c.scale(SS, SS);
    let y = PAD;

    // brand + receipt number
    roundRect(c, PAD, y, 16, 16, 5);
    c.fillStyle = accent;
    c.fill();
    c.fillStyle = INK;
    c.font = `800 15px ${FONT}`;
    c.fillText('AgentMeter', PAD + 24, y + 12);
    c.textAlign = 'right';
    c.fillStyle = DIM;
    c.font = `600 8.5px ${FONT}`;
    c.fillText('RECEIPT', W - PAD, y + 7);
    c.fillStyle = FAINT;
    c.font = `500 8px ${MONO}`;
    c.fillText('No. 0235', W - PAD, y + 17);
    c.textAlign = 'left';
    y += 32;

    dotted(c, y);
    y += 18;

    // period
    c.fillStyle = DIM;
    c.font = `600 8px ${FONT}`;
    c.fillText('PERIOD', PAD, y + 8);
    c.fillStyle = INK;
    c.font = `600 11.5px ${FONT}`;
    c.fillText(periodLabel(period, d.now), PAD, y + 24);
    c.textAlign = 'right';
    c.fillStyle = FAINT;
    c.font = `500 8px ${FONT}`;
    c.fillText('API LIST PRICES · USD', W - PAD, y + 24);
    c.textAlign = 'left';
    y += 40;

    // the numbers
    c.fillStyle = DIM;
    c.font = `600 8px ${FONT}`;
    c.fillText('TOTAL TOKENS', PAD, y + 8);
    c.fillStyle = INK;
    const total = U.fmtInt(d.tokens);
    c.font = `700 ${total.length > 11 ? 26 : 34}px ${MONO}`;
    c.fillText(total, PAD, y + 44);
    c.textAlign = 'right';
    c.fillStyle = DIM;
    c.font = `600 8px ${FONT}`;
    c.fillText('API VALUE', W - PAD, y + 8);
    c.fillStyle = accent;
    c.font = `700 17px ${MONO}`;
    c.fillText(usd(d.cost), W - PAD, y + 30);
    c.fillStyle = DIM;
    c.font = `500 11px ${MONO}`;
    c.fillText(krw(d.cost), W - PAD, y + 46);
    c.textAlign = 'left';
    y += 60;

    dotted(c, y);
    y += 20;

    // vs subscription
    c.fillStyle = DIM;
    c.font = `600 8px ${FONT}`;
    c.fillText('VS SUBSCRIPTION', PAD, y + 8);
    if (d.roi) {
      c.fillStyle = accent;
      c.font = `700 26px ${MONO}`;
      c.fillText(`${d.roi.toFixed(1)}×`, PAD, y + 34);
      c.textAlign = 'right';
      c.fillStyle = INK;
      c.font = `600 10.5px ${MONO}`;
      c.fillText(`${usd(d.fee)}/mo`, W - PAD, y + 20);
      c.fillStyle = FAINT;
      c.font = `500 9px ${MONO}`;
      c.fillText(`${d.days}d = ${usd(d.feeCost)}`, W - PAD, y + 34);
      const saved = d.cost - d.feeCost;
      if (saved > 0) {
        c.fillStyle = accent;
        c.font = `600 9.5px ${MONO}`;
        c.fillText(`saved ${usd(saved)}`, W - PAD, y + 48);
      }
      c.textAlign = 'left';
    } else {
      c.fillStyle = FAINT;
      c.font = `600 22px ${MONO}`;
      c.fillText('—', PAD, y + 34);
      c.textAlign = 'right';
      c.fillStyle = FAINT;
      c.font = `500 9px ${FONT}`;
      c.fillText('plan fee not set', W - PAD, y + 30);
      c.textAlign = 'left';
    }
    y += 60;

    dotted(c, y);
    y += 20;

    // top agents
    const barW = W - PAD * 2;
    c.fillStyle = DIM;
    c.font = `600 8px ${FONT}`;
    c.fillText('TOP AGENTS', PAD, y + 8);
    c.textAlign = 'right';
    c.fillText('TOKENS', W - PAD, y + 8);
    c.textAlign = 'left';
    y += 18;
    for (const r of d.rows.slice(0, MAX_AGENTS)) {
      const share = d.tokens ? r.tokens / d.tokens : 0;
      c.fillStyle = INK;
      c.font = `600 11px ${FONT}`;
      c.fillText(clip(c, r.p.name, barW - 130), PAD, y + 11);
      c.textAlign = 'right';
      c.font = `600 11px ${MONO}`;
      c.fillText(U.fmtInt(r.tokens), W - PAD, y + 11);
      c.textAlign = 'left';
      roundRect(c, PAD, y + 17, barW, 2, 1);
      c.fillStyle = 'rgba(255, 255, 255, 0.07)';
      c.fill();
      roundRect(c, PAD, y + 17, Math.max(2, barW * share), 2, 1);
      c.fillStyle = accent;
      c.fill();
      c.textAlign = 'right';
      c.fillStyle = FAINT;
      c.font = `500 8px ${MONO}`;
      c.fillText(`${Math.round(share * 100)}%`, W - PAD, y + 27);
      c.textAlign = 'left';
      y += 33;
    }
    if (!d.rows.length) {
      c.fillStyle = FAINT;
      c.font = `500 10px ${FONT}`;
      c.fillText('no usage in this period', PAD, y + 10);
      y += 22;
    }

    // model mix
    y += 2;
    dotted(c, y);
    y += 20;
    c.fillStyle = DIM;
    c.font = `600 8px ${FONT}`;
    c.fillText('MODEL MIX · 7D', PAD, y + 8);
    c.textAlign = 'right';
    c.fillText('SHARE', W - PAD, y + 8);
    c.textAlign = 'left';
    y += 18;
    for (const m of d.topModels) {
      c.fillStyle = DIM;
      c.font = `500 9.5px ${MONO}`;
      c.fillText(clip(c, m.name, barW - 70), PAD, y + 9);
      c.textAlign = 'right';
      c.fillText(`${Math.round((m.w / d.modelTotal) * 100)}%`, W - PAD, y + 9);
      c.textAlign = 'left';
      y += 16;
    }
    if (!d.topModels.length) {
      c.fillStyle = FAINT;
      c.font = `500 9.5px ${FONT}`;
      c.fillText('—', PAD, y + 9);
      y += 16;
    }

    // signature
    y += 8;
    dashed(c, y);
    y += 18;
    barcode(c, PAD, y, barW, 28, hash(`${d.tokens}|${Math.round(d.cost * 100)}|${period}`));
    y += 42;
    c.fillStyle = FAINT;
    c.font = `500 8px ${MONO}`;
    c.fillText('github.com/cheapbutsimilar/Agent-Meter', PAD, y);
    c.textAlign = 'right';
    c.fillStyle = DIM;
    c.font = `600 8px ${FONT}`;
    c.fillText('THANKS FOR PROMPTING', W - PAD, y);
    c.textAlign = 'left';
    y += 16;
    c.fillStyle = FAINT;
    c.font = `500 7.5px ${FONT}`;
    c.fillText('API value is an estimate at public list prices — not a bill.', PAD, y);

    // the card itself: a dark sheet with a warm glow at the top
    const H = Math.ceil(y + PAD);
    const out = document.createElement('canvas');
    out.width = W * SS;
    out.height = H * SS;
    const o = out.getContext('2d');
    o.scale(SS, SS);
    const bg = o.createLinearGradient(0, 0, W * 0.7, H);
    bg.addColorStop(0, '#1F1D1B');
    bg.addColorStop(0.5, '#161514');
    bg.addColorStop(1, '#101010');
    roundRect(o, 0, 0, W, H, 22);
    o.fillStyle = bg;
    o.fill();
    const glow = o.createLinearGradient(0, 0, 0, 180);
    glow.addColorStop(0, rgba(accent, 0.2));
    glow.addColorStop(1, rgba(accent, 0));
    o.save();
    roundRect(o, 0, 0, W, H, 22);
    o.clip();
    o.fillStyle = glow;
    o.fillRect(0, 0, W, 180);
    o.restore();
    o.drawImage(scratch, 0, 0, W * SS, H * SS, 0, 0, W, H);
    roundRect(o, 0.5, 0.5, W - 1, H - 1, 22);
    o.strokeStyle = 'rgba(255, 255, 255, 0.1)';
    o.lineWidth = 1;
    o.stroke();
    return Object.assign({ dataUrl: out.toDataURL('image/png'), height: H }, d);
  }

  function accentOf(S) {
    const theme = root.AM_THEME && root.AM_THEME.resolve((S.settings || {}).theme);
    return (theme && theme.accent) || '#F05C1A';
  }

  function tweet(d) {
    const x = d.roi ? d.roi.toFixed(1) : '';
    return tr(d.roi ? 'rc.tweet' : 'rc.tweetNoRoi', {
      period: tr(`rc.${d.period}`),
      tokens: U.fmtInt(d.tokens),
      cost: U.fmtMoney(d.cost),
      x,
    });
  }

  // ---------------------------------------------------------------- actions
  async function toClipboard(dataUrl) {
    const a = api();
    if (a && a.receiptCopyPng) return a.receiptCopyPng(dataUrl);
    const blob = await (await fetch(dataUrl)).blob();
    await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })]);
  }

  function download(dataUrl) {
    const link = document.createElement('a');
    link.href = dataUrl;
    link.download = 'AgentMeter-receipt.png';
    link.click();
  }

  // ---------------------------------------------------------------- the window
  let live = null; // { S, opts, period, el, data }

  function toast(msg) {
    if (live && typeof live.opts.toast === 'function') live.opts.toast(msg);
  }

  function syncPeriod() {
    live.el.querySelectorAll('[data-rc-period]').forEach((b) => b.classList.toggle('active', b.dataset.rcPeriod === live.period));
  }

  function draw() {
    const { S, el } = live;
    const d = paint(S, live.period);
    live.data = d;
    const img = el.querySelector('#rc-img');
    img.src = d.dataUrl;
    img.alt = tr('rc.title');
    const box = el.querySelector('#rc-fees');
    if (!d.rows.length) {
      box.innerHTML = `<div class="rc-empty">${esc(tr('rc.noUsage'))}</div>`;
      return;
    }
    box.innerHTML =
      `<div class="rc-fees-h"><b>${esc(tr('rc.feeTitle'))}</b><small>${esc(tr('rc.feeHint'))}</small></div>` +
      d.rows
        .map(
          (r) =>
            `<label class="rc-fee"><span>${esc(r.p.name)}${r.p.plan ? ` <i>${esc(r.p.plan)}</i>` : ''}</span>` +
            `<input type="number" min="0" max="10000" step="1" inputmode="decimal" data-fee="${esc(r.p.id)}" value="${Math.round(r.fee)}"/></label>`,
        )
        .join('');
  }

  async function shareX() {
    try {
      await toClipboard(live.data.dataUrl);
      toast(tr('rc.copied'));
    } catch (_) {
      toast(tr('rc.copyFail'));
    }
    const url = `https://twitter.com/intent/tweet?text=${encodeURIComponent(tweet(live.data))}&url=${encodeURIComponent(GITHUB)}`;
    api().openUrl(url);
  }

  async function copyOnly() {
    try {
      await toClipboard(live.data.dataUrl);
      toast(tr('rc.copied'));
    } catch (_) {
      toast(tr('rc.copyFail'));
    }
  }

  async function save() {
    const a = api();
    try {
      if (a && a.receiptSavePng) {
        const path = await a.receiptSavePng(live.data.dataUrl);
        if (path) toast(tr('rc.saved', { path }));
      } else {
        download(live.data.dataUrl);
      }
    } catch (_) {
      toast(tr('rc.saveFail'));
    }
  }

  function close() {
    if (!live) return;
    live.el.hidden = true;
    live = null;
  }

  function onClick(e) {
    if (!live) return;
    const per = e.target.closest('[data-rc-period]');
    if (per) {
      live.period = per.dataset.rcPeriod;
      syncPeriod();
      draw();
      return;
    }
    if (e.target.closest('[data-close]') || e.target.id === 'receipt-modal') return close();
    const b = e.target.closest('[data-rc]');
    if (!b) return;
    if (b.dataset.rc === 'x') shareX();
    if (b.dataset.rc === 'save') save();
    if (b.dataset.rc === 'copy') copyOnly();
  }

  function onChange(e) {
    if (!live) return;
    const input = e.target.closest('[data-fee]');
    if (!input) return;
    const id = input.dataset.fee;
    const v = Math.max(0, Math.min(10000, Number(input.value) || 0));
    live.S.settings.planFees = Object.assign({}, live.S.settings.planFees, { [id]: v });
    const a = api();
    if (a && a.setSettings) a.setSettings({ planFees: { [id]: v } }).catch(() => {});
    draw();
  }

  function init() {
    const el = document.getElementById('receipt-modal');
    if (!el) return;
    el.addEventListener('click', onClick);
    el.addEventListener('change', onChange);
    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape' && live) close();
    });
  }

  function open(S, opts = {}) {
    const el = document.getElementById('receipt-modal');
    if (!el) return;
    if (live) close();
    live = { S, opts, period: 'week', el, data: null };
    el.hidden = false;
    syncPeriod();
    draw();
    const focus = el.querySelector('[data-close]');
    if (focus) focus.focus();
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();

  root.AM_RECEIPT = { open, close, paint, stats, tweet };
})(typeof self !== 'undefined' ? self : this);
