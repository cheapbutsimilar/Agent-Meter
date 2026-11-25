#!/usr/bin/env node
/* README still of the docked strip with only Claude and Codex, no clock or app buttons around it,
 * rendered large (3x) so it reads at full README width. The real widget UI (ui/widget) with the
 * demo data of scripts/mock-api.js.
 *   node docs/screenshots/strip-duo.js   → docs/screenshots/widget-strip-duo.png, en/widget-strip-duo.png
 * Requires Playwright with Chromium. Kept under docs/ so changing it never cuts a release. */
'use strict';

const http = require('http');
const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');

const ROOT = path.join(__dirname, '..', '..');
const OUT = path.join(ROOT, 'docs', 'screenshots');
const MOCK = fs.readFileSync(path.join(ROOT, 'scripts', 'mock-api.js'), 'utf8');
const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'application/javascript', '.css': 'text/css', '.svg': 'image/svg+xml' };

const stage = (lang) => `<!doctype html><html lang="${lang}"><head><meta charset="utf-8"><style>
  :root { color-scheme: dark; }
  html, body { margin: 0; background: #17181b; }
  .pad { display: inline-block; padding: 20px 24px; background: linear-gradient(160deg, #26272b, #1c1d20); }
  iframe { border: 0; display: block; background: transparent; }
  </style></head><body><div class="pad">
  <iframe id="strip" src="/ui/widget/index.html?mode=taskbar&tb=48&lang=${lang}&agents=claude,codex" width="600" height="48"></iframe>
  </div><script>
  addEventListener('message', (e) => {
    const d = e.data || {};
    if (!d.amWidgetSize) return;
    const f = document.getElementById('strip');
    f.width = Math.ceil(d.amWidgetSize[0]);
    f.height = Math.ceil(d.amWidgetSize[1]);
  });
  </script></body></html>`;

async function main() {
  const srv = http.createServer((req, res) => {
    const rel = decodeURIComponent(req.url.split('?')[0]);
    const m = rel.match(/^\/__strip\/(ko|en)$/);
    if (m) {
      res.writeHead(200, { 'content-type': TYPES['.html'] });
      return res.end(stage(m[1]));
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
  await new Promise((r) => srv.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${srv.address().port}`;
  const browser = await chromium.launch();
  for (const [lang, sub] of [['ko', ''], ['en', 'en']]) {
    const p = await browser.newPage({ viewport: { width: 900, height: 200 }, deviceScaleFactor: 3 });
    await p.addInitScript(MOCK);
    await p.goto(`${base}/__strip/${lang}`);
    await p.waitForTimeout(600);
    // a clean strip: no CPU/RAM block or settings button; the pace tick on
    const w = p.frame({ url: /widget/ });
    await w.evaluate(() => window.agentMeter.setSettings({ taskbar: { system: false, gear: false, pace: true } }));
    // Claude's 5-hour and weekly limits only, as in the demo GIF
    await w.evaluate(() => window.agentMeter.__patchProvider('claude', (c) => (c.windows = c.windows.filter((x) => x.id === '5h' || x.id === '7d'))));
    await p.evaluate(() => document.fonts.ready);
    await p.waitForTimeout(500);
    const file = path.join(OUT, sub, 'widget-strip-duo.png');
    await p.locator('.pad').screenshot({ path: file, animations: 'disabled' });
    const size = await p.locator('.pad').boundingBox();
    console.log('saved', path.relative(ROOT, file), `${Math.round(size.width * 3)}x${Math.round(size.height * 3)}`);
    await p.close();
  }
  await browser.close();
  srv.close();
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
