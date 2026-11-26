#!/usr/bin/env node
/* README demo GIF: a whole 1920x1080 desktop with the strip docked next to the taskbar clock (all
 * five agents), then a camera that zooms in about 2.4x and pans along it to the clock so every number reads, PageUp switching it to
 * the card (shown at 1.6x, panning from its top to its bottom), a double-click that opens the
 * dashboard and clicks through four of its pages, and back, and the limits, reset countdowns, pace ticks and run-out forecast moving as
 * time passes (a time-lapse).
 * A reproduction, not a screen recording: the real widget UI (ui/widget) is rendered with the
 * demo data of scripts/mock-api.js, frame by frame, and the frames are joined with ffmpeg.
 *   node docs/screenshots/demo-gif.js        → docs/screenshots/demo.gif, docs/screenshots/en/demo.gif
 * Requires Playwright with Chromium, ffmpeg and gifsicle. Kept under docs/ so changing it never cuts a release.
 * Size budget: GitHub's image proxy (camo) refuses anything over 5 MB, so each GIF must stay under
 * MAX_BYTES; gifsicle's light lossy pass (identical frames merged, timing kept) gets it there. */
'use strict';

const http = require('http');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync } = require('child_process');
const { chromium } = require('playwright');

const ROOT = path.join(__dirname, '..', '..');
const OUT = path.join(ROOT, 'docs', 'screenshots');
const MOCK = fs.readFileSync(path.join(ROOT, 'scripts', 'mock-api.js'), 'utf8');
const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'application/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.md': 'text/plain; charset=utf-8' };

const VIEW = { width: 880, height: 495 }; // CSS px; captured at 1.3636x → 1200 x 675 (16:9)
/** The desktop the wide shot shows whole: a 1920 x 1080 screen at 100%. */
const DESK = { width: 1920, height: 1080 };
const SCALE = 1200 / 880;

const TEXT = {
  ko: {
    a: ['작업표시줄 시계 옆에 딱 붙어서', 'Claude · Codex · Gemini · Antigravity · Grok 한도를 한 줄로'],
    b: ['남은 양 · 리셋까지 남은 시간 · 소진 예측', '쓰는 동안 숫자와 막대가 실시간으로 따라와요'],
    c: ['PageUp 한 번이면 카드형으로', '토큰 · 비용까지 자세히'],
    d: ['다시 PageUp, 가로형으로', '평소엔 시계 옆에 조용히 붙어 있어요'],
    e: ['더블클릭하면 대시보드', '분석 · 위젯 · 테마 · 에이전트를 톡톡 넘겨보세요'],
    clock: (h, m) => `${h < 12 ? '오전' : '오후'} ${((h + 11) % 12) + 1}:${String(m).padStart(2, '0')}<br>2026-11-25`,
    lapse: '타임랩스',
  },
  en: {
    a: ['Docked right next to the taskbar clock', 'Claude, Codex, Gemini, Antigravity and Grok in one line'],
    b: ['Remaining, time to reset, run-out forecast', 'Numbers and bars follow along as you work'],
    c: ['Press PageUp for the card', 'With tokens and cost in detail'],
    d: ['PageUp again: back to the strip', 'Quietly docked next to the clock'],
    e: ['Double-click for the dashboard', 'Click through analytics, widget, theme and agents'],
    clock: (h, m) => `${((h + 11) % 12) + 1}:${String(m).padStart(2, '0')} ${h < 12 ? 'AM' : 'PM'}<br>11/25/2026`,
    lapse: 'time-lapse',
  },
};

const ICONS = {
  wifi: '<svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="#e8e8e8" stroke-width="1.4" stroke-linecap="round"><path d="M1.5 6a9.5 9.5 0 0 1 13 0M3.8 8.6a6.2 6.2 0 0 1 8.4 0M6 11.1a3 3 0 0 1 4 0"/><circle cx="8" cy="13.3" r=".9" fill="#e8e8e8" stroke="none"/></svg>',
  vol: '<svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="#e8e8e8" stroke-width="1.4" stroke-linejoin="round" stroke-linecap="round"><path d="M2 6h2.5L8 3v10l-3.5-3H2z"/><path d="M10.5 5.5a3.5 3.5 0 0 1 0 5M12.5 3.8a6 6 0 0 1 0 8.4"/></svg>',
  bat: '<svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="#e8e8e8" stroke-width="1.3"><rect x="1.5" y="4.5" width="11.5" height="7" rx="1.6"/><rect x="3" y="6" width="7" height="4" rx=".6" fill="#e8e8e8" stroke="none"/><path d="M14.5 7v2" stroke-linecap="round"/></svg>',
};

function stage(lang) {
  const q = `&lang=${lang}`;
  return `<!doctype html><html lang="${lang}"><head><meta charset="utf-8"><style>
  :root { color-scheme: dark; }
  * { box-sizing: border-box; }
  html, body { margin: 0; width: ${VIEW.width}px; height: ${VIEW.height}px; overflow: hidden; }
  body { position: relative; font-family: 'Pretendard Variable', Pretendard, 'Segoe UI', system-ui, sans-serif; color: #f3f3f3;
         background: radial-gradient(700px 420px at 0% 0%, #3a2418, transparent 70%), linear-gradient(160deg, #1f2024, #17181b);
         -webkit-font-smoothing: antialiased; }
  iframe { border: 0; display: block; background: transparent; position: absolute; }
  .cap { position: absolute; left: 40px; top: 40px; z-index: 2; }
  .cap .brand { display: flex; align-items: center; gap: 9px; font-size: 14px; font-weight: 700; color: #cfcfcf; letter-spacing: .01em; }
  .cap .brand i { width: 16px; height: 16px; border-radius: 5px; background: #F05C1A; box-shadow: inset 0 0 0 4px rgba(0,0,0,.18); }
  .cap h1 { margin: 18px 0 0; font-size: 30px; line-height: 1.2; font-weight: 800; letter-spacing: -.02em; }
  .cap p { margin: 10px 0 0; font-size: 17px; color: #b9b9b9; font-weight: 500; }
  /* while the big card fills the right side the caption steps down a size so they never meet */
  .cap.small { max-width: 300px; }
  .cap.small h1 { margin-top: 14px; font-size: 22px; }
  .cap.small p { margin-top: 8px; font-size: 15px; }
  .key { position: absolute; z-index: 2; left: 40px; top: 180px; display: flex; align-items: center; gap: 12px; opacity: 0; }
  .key b { display: inline-block; padding: 9px 14px 11px; border-radius: 9px; font-size: 17px; font-weight: 700; color: #f5f5f5;
           background: linear-gradient(#3b3d42, #2c2e32); border: 1px solid #55585e; box-shadow: 0 4px 0 #18191b, 0 8px 18px rgba(0,0,0,.45); }
  .key.down b { transform: translateY(3px); box-shadow: 0 1px 0 #18191b, 0 3px 8px rgba(0,0,0,.45); background: linear-gradient(#4a3226, #3a281f); border-color: #F05C1A; }
  .lapse { position: absolute; z-index: 2; right: 16px; top: 12px; padding: 5px 10px; border-radius: 999px; background: rgba(23,24,27,.82); font-size: 12px; font-weight: 600; color: #9a9a9a; display: flex; gap: 6px; align-items: center; }
  /* the big card reaches the top right corner, so over it the badge sits by the brand instead */
  body.cardshot .lapse { right: auto; left: 160px; top: 36px; }
  /* the dashboard fills the frame below a thin band: the caption shrinks to one line in it */
  .cap.dash { top: 13px; display: flex; align-items: baseline; gap: 12px; }
  .cap.dash .brand { font-size: 12px; gap: 7px; }
  .cap.dash .brand i { width: 12px; height: 12px; border-radius: 4px; box-shadow: inset 0 0 0 3px rgba(0,0,0,.18); }
  .cap.dash h1 { margin: 0; font-size: 16px; letter-spacing: -.01em; }
  .cap.dash p { margin: 0; font-size: 13px; }
  .win { position: absolute; width: 1060px; height: 720px; border-radius: 12px; overflow: hidden; opacity: 0; transform-origin: 0 0;
         box-shadow: 0 30px 80px rgba(0,0,0,.55), 0 0 0 1px rgba(255,255,255,.07); }
  .win iframe { position: static; width: 1060px; height: 720px; }
  #cursor { position: absolute; z-index: 5; left: 0; top: 0; width: 20px; height: 24px; opacity: 0; transform-origin: 2px 2px; pointer-events: none;
            filter: drop-shadow(0 2px 3px rgba(0,0,0,.5)); }
  .ripple { position: absolute; z-index: 4; border: 2px solid #F05C1A; border-radius: 50%; opacity: 0; pointer-events: none; }
  .lapse i { width: 7px; height: 7px; border-radius: 50%; background: #e5484d; }
  .bar { position: absolute; left: 0; right: 0; bottom: 0; height: 48px; background: rgba(32,32,34,.96); border-top: 1px solid rgba(255,255,255,.08); }
  .bar .apps { position: absolute; left: 12px; top: 8px; display: flex; gap: 6px; }
  .bar .apps i { width: 32px; height: 32px; border-radius: 7px; background: rgba(255,255,255,.08); }
  .bar .apps i:nth-child(2) { background: rgba(255,255,255,.16); }
  .tray { position: absolute; right: 8px; top: 0; height: 48px; display: flex; align-items: center; gap: 4px; }
  .tray .chev { font-size: 12px; color: #d8d8d8; padding: 0 8px; transform: scaleX(1.3); }
  .tray .ic { display: flex; gap: 10px; padding: 7px 6px; border-radius: 6px; }
  .tray .clock { font-size: 12px; line-height: 1.4; text-align: right; color: #e6e6e6; padding: 0 4px 0 6px; white-space: nowrap; }
  #world { position: absolute; left: 0; top: 0; width: ${DESK.width}px; height: ${DESK.height}px; transform-origin: 0 0; }
  /* the wallpaper stays put on the body while the camera moves, so frames differ only where things move */
  .card { filter: drop-shadow(0 18px 34px rgba(0,0,0,.55)); }
  </style></head><body>
  <div class="cap" id="cap"><div class="brand"><i></i>AgentMeter</div><h1 id="h1"></h1><p id="p"></p></div>
  <div class="key" id="key"><b>PageUp</b></div>
  <div class="lapse"><i></i>${TEXT[lang].lapse}</div>
  <div class="ripple" id="rip0"></div><div class="ripple" id="rip1"></div>
  <svg id="cursor" viewBox="0 0 20 24"><path d="M2 2v17.5l4.6-4.4 3 6.9 3.1-1.3-3-6.8H16z" fill="#fff" stroke="#111" stroke-width="1.4" stroke-linejoin="round"/></svg>
  <div id="world">
  <iframe id="card" class="card" src="/ui/widget/index.html?mode=floating${q}" width="312" height="560"></iframe>
  <div class="win" id="win"><iframe id="dash" src="/ui/dashboard/index.html?page=overview${q}"></iframe></div>
  <div class="bar"><div class="apps"><i></i><i></i><i></i><i></i><i></i></div>
    <iframe id="strip" src="/ui/widget/index.html?mode=taskbar&tb=48${q}" width="900" height="48"></iframe>
    <div class="tray" id="tray"><span class="ic">${ICONS.wifi}${ICONS.vol}${ICONS.bat}</span><span class="clock" id="clock"></span></div>
  </div>
  </div>
  <script>
  addEventListener('message', (e) => {
    const d = e.data || {};
    if (!d.amWidgetSize) return;
    const f = [...document.querySelectorAll('iframe')].find((x) => x.contentWindow === e.source);
    if (f) { f.width = Math.ceil(d.amWidgetSize[0]); f.height = Math.ceil(d.amWidgetSize[1]); f.dataset.ready = '1'; window.layout && layout(); }
  });
  </script></body></html>`;
}

function serve(stages) {
  return new Promise((resolve) => {
    const srv = http.createServer((req, res) => {
      const rel = decodeURIComponent(req.url.split('?')[0]);
      if (stages[rel]) {
        res.writeHead(200, { 'content-type': TYPES['.html'] });
        return res.end(stages[rel]);
      }
      const file = path.join(ROOT, rel);
      if (!file.startsWith(ROOT)) {
        res.writeHead(403);
        return res.end();
      }
      fs.readFile(file, (err, data) => {
        if (err) {
          res.writeHead(404);
          return res.end();
        }
        res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream' });
        res.end(data);
      });
    });
    srv.listen(0, '127.0.0.1', () => resolve(srv));
  });
}

/** Runs in the stage page: lays out the scene and applies one frame of the time-lapse. */
function stageApi(desk) {
  const $ = (id) => document.getElementById(id);
  const MIN = 60000;
  const HOUR = 60 * MIN;
  window.layout = () => {
    const strip = $('strip');
    const tray = $('tray');
    const card = $('card');
    const trayW = tray.offsetWidth; // untransformed, whatever the camera is doing
    // the world is the desktop; the strip sits just left of the tray, as docked
    const sw = Number(strip.width);
    const W = desk.width;
    const H = desk.height;
    const left = W - 8 - trayW - 6 - sw;
    strip.style.left = `${left}px`;
    strip.style.top = '0px';
    card.style.left = `${W - 16 - Number(card.width)}px`;
    card.style.top = `${H - 48 - 14 - Number(card.height)}px`;
    // cameras as the world's visible left edge (x), scale (s) and, optionally, top edge (y);
    // without y a shot is anchored to the bottom of the desktop
    const Z = 2.4;
    // the card shots: the card at 1.6x, right-aligned, taller than the frame, so one shot on its
    // top (40 px below the frame's top edge, clear of the time-lapse badge) and one on its
    // bottom (16 px above the frame's bottom edge), with a slow pan between them
    const SC = 1.6;
    const cardTop = H - 48 - 14 - Number(card.height);
    const cardBottom = H - 48 - 14;
    const cx = W - innerWidth / SC;
    window.CAMS = {
      wide: { x: 0, s: innerWidth / W },
      zl: { x: left - 10, s: Z }, // zoomed on the start of the strip
      zr: { x: W - innerWidth / Z, s: Z }, // zoomed on its end and the clock
      cardTop: { x: cx, s: SC, y: cardTop - 40 / SC },
      cardBot: { x: cx, s: SC, y: cardBottom + 16 / SC - innerHeight / SC },
    };
    // the dashboard window at its real size (1060 x 720 world px), its right edge where the
    // card's is, standing on the taskbar. The camera holds at 0.85x like a zoomed screen
    // recording: the sidebar and page titles stay readable, the right and bottom run off the
    // frame a little, and a thin band on top holds the caption
    const win = $('win');
    const wx = W - 16 - 1060;
    const wy = H - 48 - 14 - 720;
    win.style.left = `${wx}px`;
    win.style.top = `${wy}px`;
    const SD = 0.85;
    CAMS.dash = { x: wx - 20 / SD, s: SD, y: wy - 52 / SD };
    // cursor targets in world px: on the card, and on each sidebar tab of the dashboard
    const doc = $('dash').contentDocument;
    window.TARGETS = { cardIn: [W - 16 - 230, cardBottom - 40], card: [W - 16 - 150, cardBottom - 120] };
    for (const b of doc.querySelectorAll('#nav > button[data-page]')) {
      const r = b.getBoundingClientRect();
      TARGETS[`tab:${b.dataset.page}`] = [wx + r.left + 34, wy + r.top + r.height / 2 + 4];
    }
    window.WIN = { x: wx, y: wy, cardX: W - 16 - Number(card.width), cardY: cardTop };
    window.WORLD = { W, H, Z, SC, SD, stripW: sw, cardW: Number(card.width), cardH: Number(card.height) };
  };
  const frames = () => ['strip', 'card'].map((id) => $(id).contentWindow.agentMeter);
  window.demoInit = async () => {
    const [strip] = frames();
    // the strip without CPU/RAM, as most people dock it
    await strip.setSettings({ taskbar: { system: false, pace: true } });
    for (const api of frames()) {
      api.__patchProvider('claude', (p) => (p.windows = p.windows.filter((w) => w.id === '5h' || w.id === '7d')));
    }
    // the sidebar's hover look, switchable from here (a synthetic pointer can't hover an iframe)
    const doc = $('dash').contentDocument;
    const st = doc.createElement('style');
    st.textContent = '.side > button.am-hover:not(.active) { background: var(--bg-hover); color: var(--text); }';
    doc.head.appendChild(st);
  };
  /** t: minutes into the time-lapse. mode: 0 = strip, 1 = card, in between while switching. */
  window.demoFrame = (f) => {
    const now = Date.now();
    const t = f.t;
    const c5 = 63 + (21.4 / 60) * t;
    const x5 = 41 + (9.5 / 60) * t;
    const patch = (p) => {
      const w = (id) => p.windows.find((x) => x.id === id);
      if (p.id === 'claude') {
        const a = w('5h');
        a.usedPercent = Math.min(100, c5);
        a.resetsAt = now + 2 * HOUR + 13 * MIN - t * MIN;
        a.eta = now + ((100 - c5) / 21.4) * HOUR;
        a.exhaustBeforeReset = a.eta < a.resetsAt;
        a.elapsed = 1 - (a.resetsAt - now) / (300 * MIN);
        const b = w('7d');
        b.usedPercent = 22 + (0.4 / 60) * t * 6;
        b.resetsAt = now + 4 * 24 * HOUR + 2 * HOUR - t * MIN;
        p.tokens.today = 4_820_000 + t * 62_000;
        p.tokens.costToday = 38.42 + t * 0.51;
        p.tokens.lastEventAt = now - 5000;
      }
      if (p.id === 'codex') {
        const a = w('5h');
        a.usedPercent = x5;
        a.resetsAt = now + 3 * HOUR + 40 * MIN - t * MIN;
        const b = w('7d');
        b.usedPercent = 37 + (1.1 / 60) * t * 3;
        b.resetsAt = now + 6 * 24 * HOUR + 5 * HOUR - t * MIN;
        p.tokens.today = 1_930_000 + t * 21_000;
        p.tokens.costToday = 4.1 + t * 0.05;
        p.tokens.lastEventAt = now - 8000;
      }
      if (p.id === 'gemini') {
        const a = w('pro');
        a.usedPercent = 52 + (6 / 60) * t;
        a.resetsAt = now + 7 * HOUR + 5 * MIN - t * MIN;
        a.elapsed = 1 - (a.resetsAt - now) / (1440 * MIN);
        a.exhaustBeforeReset = false;
        p.tokens.lastEventAt = now - 15000;
      }
      if (p.id === 'antigravity') {
        const a = w('5h');
        a.usedPercent = 54 + (11 / 60) * t;
        a.resetsAt = now + 3 * HOUR + 12 * MIN - t * MIN;
        a.elapsed = 1 - (a.resetsAt - now) / (300 * MIN);
        const b = w('7d');
        b.usedPercent = 29 + (0.5 / 60) * t * 4;
        b.resetsAt = now + 5 * 24 * HOUR + 7 * HOUR - t * MIN;
        b.elapsed = 1 - (b.resetsAt - now) / (10080 * MIN);
      }
      if (p.id === 'grok') {
        w('5h').usedPercent = 13 + (4 / 60) * t;
      }
      p.limitsAt = now - 20000;
    };
    for (const api of frames()) for (const id of ['claude', 'codex', 'gemini', 'antigravity', 'grok']) api.__patchProvider(id, patch);
    $('h1').textContent = f.cap[0];
    $('p').textContent = f.cap[1];
    $('cap').classList.toggle('small', !!f.small && !f.dashCap);
    $('cap').classList.toggle('dash', !!f.dashCap);
    document.body.classList.toggle('cardshot', f.mode >= 0.5 && !f.dashCap);
    $('clock').innerHTML = f.clock;
    $('key').style.opacity = f.key;
    $('key').classList.toggle('down', !!f.down);
    // switching: the strip sinks into the taskbar while the card rises above it
    const m = f.mode;
    const strip = $('strip');
    strip.style.opacity = String(1 - Math.min(1, m * 1.6));
    strip.style.transform = `translateY(${m * 14}px)`;
    const card = $('card');
    const k = Math.max(0, (m - 0.25) / 0.75);
    // the dashboard opens out of the card: the window grows from the card's corner while the
    // card fades into it (e: 0 = card, 1 = window)
    const e = f.dash || 0;
    card.style.opacity = String(k * (1 - Math.min(1, e * 2.2)));
    card.style.transform = `translateY(${(1 - k) * 36}px) scale(${(0.94 + 0.06 * k) * (1 + 0.06 * e)})`;
    card.style.transformOrigin = '100% 100%';
    const win = $('win');
    const ws = WORLD.cardW / 1060 + (1 - WORLD.cardW / 1060) * e;
    win.style.opacity = String(Math.min(1, e * 1.8));
    win.style.visibility = e > 0 ? 'visible' : 'hidden';
    win.style.transform = `translate(${(WIN.cardX - WIN.x) * (1 - e)}px, ${(WIN.cardY - WIN.y) * (1 - e)}px) scale(${ws})`;
    const doc = $('dash').contentDocument;
    if (f.click) doc.querySelector(`#nav > button[data-page="${f.click}"]`).click();
    for (const b of doc.querySelectorAll('#nav > button')) b.classList.toggle('am-hover', b.dataset.page === f.hover);
    // camera: between two shots, moving the visible rectangle's left edge and width
    const A = CAMS[f.cam[0]];
    const B = CAMS[f.cam[1] || f.cam[0]];
    const c = f.cam[2] == null ? 0 : f.cam[2];
    const w = innerWidth / A.s + (innerWidth / B.s - innerWidth / A.s) * c;
    const x = A.x + (B.x - A.x) * c;
    const s = innerWidth / w;
    // bottom-anchored shots keep the desktop's bottom edge in frame all the way; a shot with
    // its own top edge moves there in step with x
    const y =
      A.y == null && B.y == null
        ? WORLD.H - innerHeight / s
        : (A.y ?? WORLD.H - innerHeight / A.s) + ((B.y ?? WORLD.H - innerHeight / B.s) - (A.y ?? WORLD.H - innerHeight / A.s)) * c;
    $('world').style.transform = `scale(${s}) translate(${-x}px, ${-y}px)`;
    // the pointer and its click rings live on the screen, placed through the camera
    const toScreen = (pt) => [(pt[0] - x) * s, (pt[1] - y) * s];
    const cur = $('cursor');
    if (f.cur) {
      const a = TARGETS[f.cur.from];
      const b = TARGETS[f.cur.to];
      const [px, py] = toScreen([a[0] + (b[0] - a[0]) * f.cur.k, a[1] + (b[1] - a[1]) * f.cur.k]);
      cur.style.opacity = String(f.cur.o);
      cur.style.transform = `translate(${px - 2}px, ${py - 2}px) scale(${f.cur.press ? 0.86 : 1})`;
      (f.cur.rings || []).forEach((age, i) => {
        const rp = $(`rip${i}`);
        const q = Math.min(1, age / 0.22);
        const d = 8 + 18 * (1 - Math.pow(1 - q, 3));
        rp.style.width = rp.style.height = `${d}px`;
        rp.style.left = `${px - d / 2}px`;
        rp.style.top = `${py - d / 2}px`;
        rp.style.opacity = String(age < 0.22 ? 1 - q : 0);
      });
      for (let i = (f.cur.rings || []).length; i < 2; i++) $(`rip${i}`).style.opacity = '0';
    } else {
      cur.style.opacity = '0';
      $('rip0').style.opacity = $('rip1').style.opacity = '0';
    }
  };
}

/** Largest GIF GitHub's README image proxy still shows comfortably (its hard limit is 5 MiB). */
const MAX_BYTES = 4.5 * 1024 * 1024;

/** Frames per second of the GIF (40 ms per frame, a whole number of GIF centiseconds). */
const FPS = 25;

/** cubic-bezier(x1, y1, x2, y2) as a function of progress 0..1 (like CSS timing functions). */
function bezier(x1, y1, x2, y2) {
  const at = (a, b, t) => 3 * a * t * (1 - t) * (1 - t) + 3 * b * t * t * (1 - t) + t * t * t;
  return (x) => {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    let lo = 0;
    let hi = 1;
    for (let i = 0; i < 40; i++) {
      const mid = (lo + hi) / 2;
      if (at(x1, x2, mid) < x) lo = mid;
      else hi = mid;
    }
    return at(y1, y2, (lo + hi) / 2);
  };
}
/** Motion-graphics tension (easeInOutQuint-like): a slow start, a quick middle, a soft landing. */
const TENSION = bezier(0.85, 0, 0.15, 1);
/** The gentler curve for zooming in and out of the whole desktop. */
const SOFT = (x) => (x < 0.5 ? 2 * x * x : 1 - Math.pow(-2 * x + 2, 2) / 2);
const OUT_CUBIC = (x) => 1 - Math.pow(1 - x, 3);

/** Dashboard tabs clicked through, in order (the first one the window opens on is overview). */
const TABS = ['analytics', 'display', 'appearance', 'agents'];
/** Per tab: the pointer travels, then rests on the page; 0.62 s a step on average (15.5 frames
 * at 25 fps, so steps alternate 15 and 16 frames). */
const TAB_MOVE = 0.16;
const TAB_FRAMES = [15, 16];

/** The timed segments: [{name, dur, cam, cap, ...}]. Shots: wide / zl / zr / cardTop / cardBot / dash. */
function segments(T) {
  // dur: seconds; cam: [from, to]; mode: [from, to]; dt: time-lapse minutes over the segment;
  // key: [from, to] opacity of the PageUp key; down: seconds the key stays pressed;
  // dash: [from, to] window opening; cur: pointer {from, to, o: [from, to], clicks: [s], tab}
  const S = (name, dur, cam, cap, o = {}) => ({ name, dur, cam, cap, mode: [0, 0], dt: 0, key: [0, 0], down: 0, ...o });
  const C = { mode: [1, 1] };
  const out = [
    // ① the whole desktop: the strip docked next to the clock
    S('wide', 1.1, ['wide', 'wide'], T.a, { dt: 3.2 }),
    S('zoom in', 0.9, ['wide', 'zl'], T.a, { dt: 2.8, curve: SOFT }),
    // ② zoomed in: pan along the strip to the clock while the limits move
    S('strip start', 0.55, ['zl', 'zl'], T.b, { dt: 4 }),
    S('strip pan', 2.2, ['zl', 'zr'], T.b, { dt: 24 }),
    S('strip end', 0.55, ['zr', 'zr'], T.b, { dt: 4 }),
    // ③ PageUp: the strip sinks, the card rises, the camera pulls back onto the card's top
    S('PageUp', 0.3, ['zr', 'zr'], T.c, { key: [1, 1] }),
    S('to card', 0.72, ['zr', 'cardTop'], T.c, { mode: [0, 1], key: [1, 1], down: 0.27 }),
    // the card, big: a beat on Claude and Codex, a quick pan down to Gemini, Antigravity and Grok
    S('card top', 0.4, ['cardTop', 'cardTop'], T.c, { ...C, dt: 1.6, key: [1, 0] }),
    S('card pan', 1.8, ['cardTop', 'cardBot'], T.c, { ...C, dt: 7.2 }),
    S('card bottom', 0.3, ['cardBot', 'cardBot'], T.c, { ...C, dt: 1.2 }),
    // ④ double-click the card: the dashboard opens out of it and four pages get a quick look
    // (the caption stays in the left column until the window takes the frame)
    S('double-click', 0.46, ['cardBot', 'cardBot'], T.e, { ...C, cur: { from: 'cardIn', to: 'card', o: [0, 1], clicks: [0.16, 0.3] } }),
    S('open', 0.45, ['cardBot', 'dash'], T.e, { ...C, dashCap: true, dash: [0, 1], cur: { from: 'card', to: 'card', o: [1, 1] } }),
  ];
  let at = 'card';
  for (const [i, tab] of TABS.entries()) {
    out.push(S(`tab ${tab}`, TAB_FRAMES[i % 2] / FPS, ['dash', 'dash'], T.e, { ...C, dashCap: true, dash: [1, 1], cur: { from: at, to: `tab:${tab}`, o: [1, 1], clicks: [TAB_MOVE], tab } }));
    at = `tab:${tab}`;
  }
  out.push(
    S('close', 0.42, ['dash', 'cardBot'], T.e, { ...C, dashCap: true, dash: [1, 0], cur: { from: at, to: at, o: [1, 0] } }),
    // ⑤ PageUp again: back to the strip, zoomed on the clock end, then pan back along it
    S('PageUp', 0.3, ['cardBot', 'cardBot'], T.d, { ...C, key: [1, 1] }),
    S('to strip', 0.72, ['cardBot', 'zr'], T.d, { mode: [1, 0], key: [1, 1], down: 0.27 }),
    S('strip back', 1.6, ['zr', 'zl'], T.d, { dt: 9.6, key: [1, 0] }),
    S('strip start', 0.3, ['zl', 'zl'], T.d),
    // zoom back out so the loop starts where it ends
    S('zoom out', 0.6, ['zl', 'wide'], T.d, { curve: SOFT }),
  );
  return out;
}

/** The frame list, sampled at FPS from the segments; about 16 s.
 * Each frame: {t, mode, cam: [from, to, k], key, down, cap, small, dashCap, dash, cur, click, hover, clock, ms}. */
function timeline(T) {
  const lerp = (a, b, k) => a + (b - a) * k;
  const clock = (t) => {
    const m = 15 * 60 + 42 + Math.floor(t);
    return T.clock(Math.floor(m / 60), m % 60);
  };
  const ms = 1000 / FPS;
  const out = [];
  let t = 0;
  for (const g of segments(T)) {
    const n = Math.max(1, Math.round(g.dur * FPS));
    const moving = g.cam[0] !== g.cam[1] || g.mode[0] !== g.mode[1] || (g.dash && g.dash[0] !== g.dash[1]);
    const curve = g.curve || TENSION;
    for (let i = 1; i <= n; i++) {
      const x = i / n;
      const k = moving ? curve(x) : x;
      const keyX = Math.min(1, x * 2.5); // the key fades out over the first part of a segment
      const sec = (i - 1) / FPS; // time into the segment at this frame
      let cur = null;
      let click = null;
      let hover = null;
      if (g.cur) {
        const c = g.cur;
        const clicks = c.clicks || [];
        const rings = clicks.filter((at) => sec >= at).map((at) => sec - at);
        cur = {
          from: c.from,
          to: c.to,
          k: c.from === c.to ? 1 : OUT_CUBIC(Math.min(1, sec / TAB_MOVE)),
          o: lerp(c.o[0], c.o[1], Math.min(1, x * 2)),
          press: clicks.some((at) => sec >= at && sec < at + 0.08),
          rings: rings.reverse(),
        };
        if (c.tab) {
          // hover lights up as the pointer reaches the tab; the click lands on arrival
          if (sec >= TAB_MOVE * 0.6) hover = c.tab;
          if (clicks.some((at) => sec >= at && sec - at < 1 / FPS)) click = c.tab;
        }
      }
      out.push({
        t: t + g.dt * x,
        cam: [g.cam[0], g.cam[1], k],
        mode: lerp(g.mode[0], g.mode[1], k),
        dash: g.dash ? lerp(g.dash[0], g.dash[1], k) : 0,
        key: lerp(g.key[0], g.key[1], keyX),
        down: sec < g.down,
        cap: g.cap,
        small: g.cap === T.c || g.cap === T.d || g.cap === T.e,
        dashCap: !!g.dashCap,
        cur,
        click,
        hover,
        clock: clock(t + g.dt * x),
        ms,
      });
    }
    t += g.dt;
  }
  return out;
}

async function main() {
  const stages = {};
  for (const lang of Object.keys(TEXT)) stages[`/__demo/${lang}`] = stage(lang);
  const srv = await serve(stages);
  const base = `http://127.0.0.1:${srv.address().port}`;
  const browser = await chromium.launch();
  const errors = [];
  for (const [lang, sub] of [['ko', ''], ['en', 'en']]) {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'am-demo-'));
    const p = await browser.newPage({ viewport: VIEW, deviceScaleFactor: SCALE });
    p.on('pageerror', (e) => errors.push(`${lang}: ${e.message}`));
    await p.addInitScript(MOCK);
    await p.goto(`${base}/__demo/${lang}`);
    await p.waitForFunction(() => ['card', 'strip'].every((id) => document.getElementById(id).dataset.ready));
    await p.waitForFunction(() => document.getElementById('dash').contentDocument?.querySelector('#nav > button.active'));
    await p.evaluate(stageApi, DESK);
    await p.evaluate(() => document.fonts.ready);
    await p.evaluate(() => window.demoInit());
    await p.waitForTimeout(400);
    await p.evaluate(() => window.layout());
    const frames = timeline(TEXT[lang]);
    for (let i = 0; i < frames.length; i++) {
      await p.evaluate((f) => window.demoFrame(f), frames[i]);
      await p.evaluate(() => window.layout());
      // a clicked dashboard tab renders its page (charts included) before the shot
      if (frames[i].click) await p.waitForTimeout(250);
      const file = path.join(tmp, `f${String(i).padStart(4, '0')}.png`);
      await p.screenshot({ path: file, animations: 'disabled' });
    }
    const { Z: zoom, SC: cardZoom, stripW, cardW, cardH } = await p.evaluate(() => window.WORLD);
    await p.close();
    const dir = path.join(OUT, sub);
    fs.mkdirSync(dir, { recursive: true });
    const gif = path.join(dir, 'demo.gif');
    execFileSync('ffmpeg', [
      '-y', '-loglevel', 'error', '-framerate', String(FPS), '-i', path.join(tmp, 'f%04d.png'),
      '-filter_complex', '[0:v]split[a][b];[a]palettegen=max_colors=64:stats_mode=full[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle',
      '-loop', '0', gif,
    ]);
    // a light lossy LZW pass: text stays crisp, the file drops by about a third
    execFileSync('gifsicle', ['-b', '-O3', '--lossy=20', gif]);
    const bytes = fs.statSync(gif).size;
    if (bytes > MAX_BYTES) errors.push(`${lang}: ${bytes} bytes is over the ${MAX_BYTES}-byte budget`);
    const total = frames.reduce((a, f) => a + f.ms, 0);
    const strip = frames.filter((f) => f.mode < 0.5).reduce((a, f) => a + f.ms, 0);
    console.log('saved', path.relative(ROOT, gif), `${bytes} bytes (${(bytes / 1048576).toFixed(2)} MiB)`, `${frames.length} frames @ ${FPS} fps`,
      `${(total / 1000).toFixed(1)} s (strip ${Math.round((strip / total) * 100)}%)`, `zoom ${zoom}x, strip text at ${(zoom * SCALE).toFixed(2)}x its real size, zoomed strip ${Math.round(stripW * zoom * SCALE)} px across the pan`,
      `card ${cardW}x${cardH} at ${cardZoom}x (${Math.round(((cardW * cardZoom) / VIEW.width) * 100)}% of the frame width)`);
    fs.rmSync(tmp, { recursive: true, force: true });
    if (lang === 'ko') {
      let at = 0;
      for (const g of segments(TEXT[lang])) {
        const n = Math.max(1, Math.round(g.dur * FPS));
        console.log(`  ${at.toFixed(2).padStart(6)}s  ${(n / FPS).toFixed(2)}s  ${g.name}`);
        at += n / FPS;
      }
    }
  }
  await browser.close();
  srv.close();
  if (errors.length) {
    console.error('\nPage errors:\n' + errors.join('\n'));
    process.exitCode = 1;
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
