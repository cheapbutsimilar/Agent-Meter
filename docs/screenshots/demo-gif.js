#!/usr/bin/env node
/* README demo GIF: the docked strip next to the taskbar clock, PageUp switching it to the card
 * and back, and the limits and burn-down forecast moving as time passes (a time-lapse).
 * A reproduction, not a screen recording: the real widget UI (ui/widget) is rendered with the
 * demo data of scripts/mock-api.js, frame by frame, and the frames are joined with ffmpeg.
 *   node docs/screenshots/demo-gif.js        → docs/screenshots/demo.gif, docs/screenshots/en/demo.gif
 * Requires Playwright with Chromium and ffmpeg. Kept under docs/ so changing it never cuts a release. */
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

const VIEW = { width: 860, height: 480 }; // CSS px; captured at 1.35x → 1161 x 648
const SCALE = 1.35;

const TEXT = {
  ko: {
    a: ['작업표시줄 시계 옆에 딱 붙어서', 'Claude · Codex 한도를 한눈에 보여줘요'],
    c: ['PageUp 한 번이면 카드형으로', '남은 양 · 리셋까지 남은 시간 · 소진 예측'],
    d: ['다시 PageUp, 가로형으로 돌아가요', '쓰는 동안 숫자가 실시간으로 따라와요'],
    clock: (h, m) => `${h < 12 ? '오전' : '오후'} ${((h + 11) % 12) + 1}:${String(m).padStart(2, '0')}<br>2026-11-25`,
    lapse: '타임랩스',
  },
  en: {
    a: ['Docked right next to the taskbar clock', 'Claude and Codex limits at a glance'],
    c: ['Press PageUp for the card', 'Remaining, time to reset, burn-down forecast'],
    d: ['PageUp again: back to the strip', 'The numbers follow along as you work'],
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
  const q = `&lang=${lang}&agents=claude,codex`;
  return `<!doctype html><html lang="${lang}"><head><meta charset="utf-8"><style>
  * { box-sizing: border-box; }
  html, body { margin: 0; width: ${VIEW.width}px; height: ${VIEW.height}px; overflow: hidden; }
  body { position: relative; font-family: 'Pretendard Variable', Pretendard, 'Segoe UI', system-ui, sans-serif; color: #f3f3f3;
         background: radial-gradient(700px 420px at 0% 0%, #3a2418, transparent 70%), linear-gradient(160deg, #1f2024, #17181b);
         -webkit-font-smoothing: antialiased; }
  iframe { border: 0; display: block; background: transparent; position: absolute; }
  .cap { position: absolute; left: 40px; top: 40px; transition: none; }
  .cap .brand { display: flex; align-items: center; gap: 9px; font-size: 14px; font-weight: 700; color: #cfcfcf; letter-spacing: .01em; }
  .cap .brand i { width: 16px; height: 16px; border-radius: 5px; background: #F05C1A; box-shadow: inset 0 0 0 4px rgba(0,0,0,.18); }
  .cap h1 { margin: 18px 0 0; font-size: 30px; line-height: 1.2; font-weight: 800; letter-spacing: -.02em; }
  .cap p { margin: 10px 0 0; font-size: 17px; color: #b9b9b9; font-weight: 500; }
  .key { position: absolute; left: 40px; top: 196px; display: flex; align-items: center; gap: 12px; opacity: 0; }
  .key b { display: inline-block; padding: 9px 14px 11px; border-radius: 9px; font-size: 17px; font-weight: 700; color: #f5f5f5;
           background: linear-gradient(#3b3d42, #2c2e32); border: 1px solid #55585e; box-shadow: 0 4px 0 #18191b, 0 8px 18px rgba(0,0,0,.45); }
  .key.down b { transform: translateY(3px); box-shadow: 0 1px 0 #18191b, 0 3px 8px rgba(0,0,0,.45); background: linear-gradient(#4a3226, #3a281f); border-color: #F05C1A; }
  .lapse { position: absolute; right: 22px; top: 18px; font-size: 12px; font-weight: 600; color: #9a9a9a; display: flex; gap: 6px; align-items: center; }
  .lapse i { width: 7px; height: 7px; border-radius: 50%; background: #e5484d; }
  .bar { position: absolute; left: 0; right: 0; bottom: 0; height: 48px; background: rgba(32,32,34,.96); border-top: 1px solid rgba(255,255,255,.08); }
  .bar .apps { position: absolute; left: 22px; top: 8px; display: flex; gap: 6px; }
  .bar .apps i { width: 32px; height: 32px; border-radius: 7px; background: rgba(255,255,255,.08); }
  .bar .apps i:nth-child(2) { background: rgba(255,255,255,.16); }
  .tray { position: absolute; right: 12px; top: 0; height: 48px; display: flex; align-items: center; gap: 4px; }
  .tray .chev { font-size: 12px; color: #d8d8d8; padding: 0 8px; transform: scaleX(1.3); }
  .tray .ic { display: flex; gap: 10px; padding: 7px 10px; border-radius: 6px; }
  .tray .clock { font-size: 12px; line-height: 1.4; text-align: right; color: #e6e6e6; padding: 0 6px 0 8px; white-space: nowrap; }
  .card { filter: drop-shadow(0 18px 34px rgba(0,0,0,.55)); }
  </style></head><body>
  <div class="cap"><div class="brand"><i></i>AgentMeter</div><h1 id="h1"></h1><p id="p"></p></div>
  <div class="key" id="key"><b>PageUp</b></div>
  <div class="lapse"><i></i>${TEXT[lang].lapse}</div>
  <iframe id="card" class="card" src="/ui/widget/index.html?mode=floating${q}" width="312" height="380"></iframe>
  <div class="bar"><div class="apps"><i></i><i></i><i></i></div>
    <iframe id="strip" src="/ui/widget/index.html?mode=taskbar&tb=48${q}" width="700" height="48"></iframe>
    <div class="tray" id="tray"><span class="chev">^</span><span class="ic">${ICONS.wifi}${ICONS.vol}${ICONS.bat}</span><span class="clock" id="clock"></span></div>
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
function stageApi() {
  const $ = (id) => document.getElementById(id);
  const MIN = 60000;
  const HOUR = 60 * MIN;
  window.layout = () => {
    const strip = $('strip');
    const tray = $('tray');
    const trayW = tray.getBoundingClientRect().width;
    strip.style.left = `${innerWidth - 12 - trayW - 8 - strip.width}px`;
    strip.style.top = '0px';
    const card = $('card');
    card.style.left = `${innerWidth - 16 - card.width}px`;
    card.style.top = `${innerHeight - 48 - 14 - card.height}px`;
  };
  const frames = () => ['strip', 'card'].map((id) => $(id).contentWindow.agentMeter);
  window.demoInit = async () => {
    const [strip] = frames();
    // the strip without CPU/RAM so the two agents fit the scene
    await strip.setSettings({ taskbar: { system: false } });
    for (const api of frames()) {
      api.__patchProvider('claude', (p) => (p.windows = p.windows.filter((w) => w.id === '5h' || w.id === '7d')));
    }
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
      p.limitsAt = now - 20000;
    };
    for (const api of frames()) {
      api.__patchProvider('claude', patch);
      api.__patchProvider('codex', patch);
    }
    $('h1').textContent = f.cap[0];
    $('p').textContent = f.cap[1];
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
    card.style.opacity = String(k);
    card.style.transform = `translateY(${(1 - k) * 36}px) scale(${0.94 + 0.06 * k})`;
    card.style.transformOrigin = '100% 100%';
  };
}

/** The frame list: {t, mode, key, down, cap, clock, ms}. */
function timeline(T) {
  const out = [];
  const ease = (x) => (x < 0.5 ? 2 * x * x : 1 - Math.pow(-2 * x + 2, 2) / 2);
  const clock = (t) => {
    const m = 15 * 60 + 42 + Math.floor(t);
    return T.clock(Math.floor(m / 60), m % 60);
  };
  const push = (o) => out.push({ key: 0, down: false, ...o, clock: clock(o.t) });
  let t = 0;
  // A: the docked strip, limits moving
  for (let i = 0; i < 30; i++) push({ t: (t += 0.6), mode: 0, cap: T.a, ms: 130 });
  push({ t, mode: 0, cap: T.a, ms: 600 });
  // PageUp
  push({ t, mode: 0, cap: T.c, key: 1, ms: 380 });
  push({ t, mode: 0, cap: T.c, key: 1, down: true, ms: 160 });
  for (let i = 1; i <= 9; i++) push({ t, mode: ease(i / 9), cap: T.c, key: 1, down: i < 4, ms: 40 });
  // C: the card, the 5-hour limit runs low and the forecast moves closer
  for (let i = 0; i < 36; i++) push({ t: (t += 0.9), mode: 1, cap: T.c, key: Math.max(0, 1 - i / 6), ms: 130 });
  push({ t, mode: 1, cap: T.c, ms: 900 });
  // PageUp again
  push({ t, mode: 1, cap: T.d, key: 1, ms: 380 });
  push({ t, mode: 1, cap: T.d, key: 1, down: true, ms: 160 });
  for (let i = 1; i <= 9; i++) push({ t, mode: 1 - ease(i / 9), cap: T.d, key: 1, down: i < 4, ms: 40 });
  for (let i = 0; i < 14; i++) push({ t: (t += 0.4), mode: 0, cap: T.d, key: Math.max(0, 1 - i / 6), ms: 130 });
  push({ t, mode: 0, cap: T.d, ms: 1600 });
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
    await p.waitForFunction(() => [...document.querySelectorAll('iframe')].every((f) => f.dataset.ready));
    await p.evaluate(stageApi);
    await p.evaluate(() => document.fonts.ready);
    await p.evaluate(() => window.demoInit());
    await p.waitForTimeout(400);
    await p.evaluate(() => window.layout());
    const frames = timeline(TEXT[lang]);
    let list = '';
    for (let i = 0; i < frames.length; i++) {
      await p.evaluate((f) => window.demoFrame(f), frames[i]);
      await p.evaluate(() => window.layout());
      const file = path.join(tmp, `f${String(i).padStart(4, '0')}.png`);
      await p.screenshot({ path: file, animations: 'disabled' });
      list += `file '${file}'\nduration ${(frames[i].ms / 1000).toFixed(3)}\n`;
    }
    // the concat demuxer needs the last file once more for its duration to count
    list += `file '${path.join(tmp, `f${String(frames.length - 1).padStart(4, '0')}.png`)}'\n`;
    fs.writeFileSync(path.join(tmp, 'list.txt'), list);
    await p.close();
    const dir = path.join(OUT, sub);
    fs.mkdirSync(dir, { recursive: true });
    const gif = path.join(dir, 'demo.gif');
    execFileSync('ffmpeg', [
      '-y', '-loglevel', 'error', '-f', 'concat', '-safe', '0', '-i', path.join(tmp, 'list.txt'),
      '-filter_complex', '[0:v]split[a][b];[a]palettegen=max_colors=160:stats_mode=full[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle',
      '-fps_mode', 'vfr', '-loop', '0', gif,
    ]);
    console.log('saved', path.relative(ROOT, gif), `${(fs.statSync(gif).size / 1048576).toFixed(2)} MB`, `${frames.length} frames`);
    fs.rmSync(tmp, { recursive: true, force: true });
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
