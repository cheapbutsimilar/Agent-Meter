#!/usr/bin/env node
/* Release step: stamp the "## 다음 버전" section of CHANGELOG.md with the version being
 * built (so the app's bundled patch notes say the right version) and write that section's
 * text to release-notes.md, which becomes the GitHub release body and, through
 * latest.json, the notes in the in-app update popup.
 *   node scripts/release-notes.js            (version from src-tauri/tauri.conf.json) */
'use strict';

const fs = require('fs');
const path = require('path');

const ROOT = path.join(__dirname, '..');
const version = JSON.parse(fs.readFileSync(path.join(ROOT, 'src-tauri', 'tauri.conf.json'), 'utf8')).version;
// release dates in Korean time, like the rest of the app
const date = new Date(Date.now() + 9 * 3600 * 1000).toISOString().slice(0, 10);

const file = path.join(ROOT, 'CHANGELOG.md');
let md = fs.readFileSync(file, 'utf8');
const heading = `## v${version} · ${date}`;
md = md.replace(/^## 다음 버전.*$/m, heading);
fs.writeFileSync(file, md);

// the stamped section, up to the next version heading
const lines = md.split('\n');
const start = lines.findIndex((l) => l.startsWith(`## v${version} `));
let body = [];
if (start >= 0) {
  for (let i = start + 1; i < lines.length && !lines[i].startsWith('## '); i++) body.push(lines[i]);
}
body = body.join('\n').trim();
if (!body) body = '- 버그 수정과 개선';
fs.writeFileSync(path.join(ROOT, 'release-notes.md'), `${body}\n`);
console.log(`${heading}\n${body}`);
