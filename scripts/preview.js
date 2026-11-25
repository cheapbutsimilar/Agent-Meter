#!/usr/bin/env node
/* README images in Korean (docs/screenshots) and English (docs/screenshots/en), rendered
 * from the real UI with demo data (scripts/mock-api.js):
 *   hero.png          dashboard + card widget + docked strip on a desktop
 *   widget-card.png   the card layout          widget-strip.png  the strip on a taskbar
 *   themes.png        the card in four themes  dashboard-<page>.png  every dashboard page
 *   node scripts/preview.js [outDir]        (default: docs/screenshots)
 * Requires Playwright with a Chromium build. Pretendard, if installed, is used as in the app. */
'use strict';

const http = require('http');
const fs = require('fs');
const path = require('path');

let chromium;
try {
  ({ chromium } = require('playwright'));
} catch {
  console.error('playwright 가 필요합니다: npm i -D playwright && npx playwright install chromium');
  process.exit(1);
}

const ROOT = path.join(__dirname, '..');
const OUT = path.resolve(process.argv[2] || path.join(ROOT, 'docs', 'screenshots'));
const MOCK = fs.readFileSync(path.join(__dirname, 'mock-api.js'), 'utf8');
const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'application/javascript', '.css': 'text/css', '.png': 'image/png', '.svg': 'image/svg+xml' };
const PAGES = ['overview', 'analytics', 'display', 'appearance', 'agents', 'alerts', 'general'];

// --------------------------------------------------------------- stage pages
// Composed on the same origin as the UI so the widget frames can report their size.
const WALLPAPER = `
  radial-gradient(1200px 700px at 12% 0%, rgba(240, 92, 26, 0.22), transparent 60%),
  radial-gradient(900px 600px at 100% 100%, rgba(80, 110, 170, 0.20), transparent 60%),
  linear-gradient(160deg, #202020, #161616)`;

const BASE_CSS = `
  * { box-sizing: border-box; }
  html, body { margin: 0; }
  body { font-family: 'Pretendard Variable', Pretendard, 'Segoe UI', system-ui, sans-serif; color: #f3f3f3;
         background: ${WALLPAPER}; -webkit-font-smoothing: antialiased; }
  iframe { border: 0; display: block; background: transparent; }
  .win { border-radius: 12px; overflow: hidden; box-shadow: 0 30px 80px rgba(0,0,0,.55), 0 0 0 1px rgba(255,255,255,.07); }
  .card { filter: drop-shadow(0 24px 40px rgba(0,0,0,.5)); }
  .taskbar { height: 48px; display: flex; align-items: center; gap: 10px; padding: 0 14px;
             background: rgba(28,28,28,.92); border-top: 1px solid rgba(255,255,255,.07); backdrop-filter: blur(20px); }
  .taskbar iframe { flex: none; }
  .taskbar .apps { display: flex; gap: 6px; margin: 0 auto; }
  .taskbar .apps i { width: 32px; height: 32px; border-radius: 7px; background: rgba(255,255,255,.08); }
  .taskbar .apps i:first-child { background: rgba(255,255,255,.16); }
  .taskbar .clock { font-size: 11px; line-height: 1.35; text-align: right; color: #d0d0d0; margin-left: 12px; white-space: nowrap; }
`;

const SIZE_JS = `
  // widget frames report their size like the real window does
  addEventListener('message', (e) => {
    const d = e.data || {};
    if (!d.amWidgetSize) return;
    const f = [...document.querySelectorAll('iframe')].find((x) => x.contentWindow === e.source);
    if (!f) return;
    f.width = Math.ceil(d.amWidgetSize[0]);
    f.height = Math.ceil(d.amWidgetSize[1]);
    f.dataset.ready = '1';
  });`;

const page = (body, css = '', lang = 'ko') => `<!doctype html><html lang="${lang}"><head><meta charset="utf-8"><style>${BASE_CSS}${css}</style></head><body>${body}<script>${SIZE_JS}</script></body></html>`;

// Scene text per language (the UI inside the frames follows ?lang=).
const TEXT = {
  ko: {
    tagline: 'AI 코딩 에이전트 사용량 모니터',
    clock: '오후 3:42<br/>2026-11-23',
    themes: ['AgentMeter · 다크', '노르딕 · 라이트', '미드나잇 · 다크', '로즈 · 라이트'],
  },
  en: {
    tagline: 'Usage monitor for AI coding agents',
    clock: '3:42 PM<br/>11/23/2026',
    themes: ['AgentMeter · Dark', 'Nordic · Light', 'Midnight · Dark', 'Rose · Light'],
  },
};

/** The composed scenes for one language, at /__stage/<lang>/<name>. */
function stages(lang) {
  const T = TEXT[lang];
  const widget = (mode, q = '') => `/ui/widget/index.html?mode=${mode}${mode === 'taskbar' ? '&tb=48' : ''}&lang=${lang}${q}`;
  const dash = (pg) => `/ui/dashboard/index.html?page=${pg}&lang=${lang}`;
  const clock = `<div class="clock">${T.clock}</div>`;
  const out = {
    // 1440x820: the dashboard on the right, the card on the left, the strip docked below
    hero: page(
      `<div class="hero">
         <div class="title"><div class="mark"></div><div><b>AgentMeter</b><span>${T.tagline}</span></div></div>
         <div class="dash win"><iframe src="${dash('overview')}" width="1060" height="720"></iframe></div>
         <div class="float card"><iframe src="${widget('floating', '&agents=claude,codex')}" width="312" height="460"></iframe></div>
         <div class="bar taskbar"><div class="apps"><i></i><i></i><i></i></div>
           <iframe src="${widget('taskbar')}" width="600" height="48"></iframe>${clock}</div>
       </div>`,
      `body { width: 1440px; height: 820px; overflow: hidden; }
       .hero { position: relative; width: 1440px; height: 820px; }
       .title { position: absolute; left: 64px; top: 64px; display: flex; gap: 16px; align-items: center; }
       .title .mark { width: 34px; height: 34px; border-radius: 9px; background: #F05C1A; box-shadow: inset 0 0 0 8px rgba(0,0,0,.18); }
       .title b { display: block; font-size: 34px; font-weight: 800; letter-spacing: -.02em; }
       .title span { display: block; margin-top: 4px; font-size: 16px; color: #b4b4b4; font-weight: 500; }
       .dash { position: absolute; right: 48px; top: 48px; transform: scale(.9); transform-origin: top right; }
       .float { position: absolute; left: 64px; top: 176px; }
       .bar { position: absolute; left: 0; right: 0; bottom: 0; }`,
      lang,
    ),
    card: page(
      `<div class="pad"><div class="card"><iframe src="${widget('floating')}" width="312" height="560"></iframe></div></div>`,
      `body { display: inline-block; } .pad { padding: 40px 48px 52px; display: inline-block; }`,
      lang,
    ),
    strip: page(
      `<div class="pad"><div class="taskbar"><div class="apps"><i></i><i></i><i></i><i></i><i></i><i></i></div>
         <iframe src="${widget('taskbar')}" width="600" height="48"></iframe>${clock}</div></div>`,
      `body { width: 1500px; } .pad { padding: 64px 0 0; }`,
      lang,
    ),
    themes: page(
      `<div class="grid">${[
        ['agentmeter', 'dark'],
        ['nordic', 'light'],
        ['midnight', 'dark'],
        ['rose', 'light'],
      ]
        .map(
          ([p, sc], i) =>
            `<figure><div class="card"><iframe src="${widget('floating', `&preset=${p}&scheme=${sc}`)}" width="312" height="560"></iframe></div><figcaption>${T.themes[i]}</figcaption></figure>`,
        )
        .join('')}</div>`,
      `body { display: inline-block; } .grid { display: flex; gap: 28px; padding: 40px 44px 36px; align-items: flex-start; }
       figure { margin: 0; } figcaption { margin-top: 14px; text-align: center; font-size: 14px; font-weight: 600; color: #c8c8c8; }`,
      lang,
    ),
  };
  // each dashboard page as a window on the wallpaper
  for (const pg of PAGES) {
    out[`dash-${pg}`] = page(
      `<div class="pad"><div class="win"><iframe src="${dash(pg)}" width="1060" height="720"></iframe></div></div>`,
      `body { display: inline-block; } .pad { padding: 36px 44px 52px; display: inline-block; }`,
      lang,
    );
  }
  return out;
}

// Korean images in docs/screenshots, English ones in docs/screenshots/en
const LANGS = { ko: '', en: 'en' };
const STAGES = {};
for (const lang of Object.keys(LANGS)) {
  for (const [name, html] of Object.entries(stages(lang))) STAGES[`/__stage/${lang}/${name}`] = html;
}

function serve() {
  return new Promise((resolve) => {
    const srv = http.createServer((req, res) => {
      const rel = decodeURIComponent(req.url.split('?')[0]);
      if (STAGES[rel]) {
        res.writeHead(200, { 'content-type': TYPES['.html'] });
        return res.end(STAGES[rel]);
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

async function main() {
  const srv = await serve();
  const base = `http://127.0.0.1:${srv.address().port}`;
  const browser = await chromium.launch();
  const errors = [];

  async function open(name, url, viewport) {
    const p = await browser.newPage({ viewport, deviceScaleFactor: 2 });
    p.on('pageerror', (e) => errors.push(`${name}: ${e.message}`));
    p.on('console', (m) => m.type() === 'error' && errors.push(`${name}: ${m.text()}`));
    await p.addInitScript(MOCK); // runs in every frame
    await p.goto(`${base}${url}`);
    await p.evaluate(() => document.fonts.ready);
    await p.waitForTimeout(900);
    return p;
  }

  for (const [lang, sub] of Object.entries(LANGS)) {
    const dir = path.join(OUT, sub);
    fs.mkdirSync(dir, { recursive: true });
    const save = async (target, name) => {
      await target.screenshot({ path: path.join(dir, `${name}.png`) });
      console.log('saved', path.join(sub, name));
    };
    const stage = (name) => `/__stage/${lang}/${name}`;

    // composed scenes
    const hero = await open('hero', stage('hero'), { width: 1440, height: 820 });
    await save(hero, 'hero');
    await hero.close();
    for (const [name, sel] of [['card', '.pad'], ['strip', '.pad'], ['themes', '.grid']]) {
      const p = await open(name, stage(name), { width: 1500, height: 900 });
      await save(p.locator(sel), name === 'themes' ? 'themes' : `widget-${name}`);
      await p.close();
    }
    // every dashboard page, as a window
    for (const pg of PAGES) {
      const p = await open(pg, stage(`dash-${pg}`), { width: 1200, height: 860 });
      await save(p.locator('.pad'), `dashboard-${pg}`);
      await p.close();
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
