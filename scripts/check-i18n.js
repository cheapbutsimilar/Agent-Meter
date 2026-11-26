#!/usr/bin/env node
/* Every UI language must have the same keys: ui/shared/i18n.js's dictionaries are compared
 * with each other, and every key used in the UI code must exist.
 *   node scripts/check-i18n.js        (exit 1 on a mismatch; also run by CI) */
'use strict';

const fs = require('fs');
const path = require('path');

const ROOT = path.join(__dirname, '..');
global.self = {};

require(path.join(ROOT, 'ui', 'shared', 'i18n.js'));
const { DICT } = global.self.AM_I18N;

/** Strings that exist in one language on purpose (the widget's Korean-only meme bubble). */
const ONLY = { ko: ['w.meme.hot', 'w.meme.done'] };
const intended = (k) => Object.values(ONLY).some((keys) => keys.includes(k));

let bad = 0;
const langs = Object.keys(DICT);
const all = new Set(langs.flatMap((l) => Object.keys(DICT[l])).filter((k) => !intended(k)));
for (const l of langs) {
  const missing = [...all].filter((k) => !(k in DICT[l]));
  if (missing.length) {
    bad += missing.length;
    console.error(`${l}: ${missing.length} missing key(s)\n  ${missing.join('\n  ')}`);
  }
}

// literal keys used by the UI: tr('…'), t('…'), data-i18n="…", data-i18n-title="…"
const files = ['ui/shared/render.js', 'ui/shared/util.js', 'ui/dashboard/dashboard.js', 'ui/widget/widget.js', 'ui/update/update.js', 'ui/dashboard/index.html', 'ui/update/index.html'];
const used = new Set();
for (const f of files) {
  const p = path.join(ROOT, f);
  if (!fs.existsSync(p)) continue;
  const src = fs.readFileSync(p, 'utf8');
  for (const m of src.matchAll(/\b(?:tr|t)\(\s*'([a-zA-Z0-9_.]+)'/g)) used.add(m[1]);
  for (const m of src.matchAll(/data-i18n(?:-title|-html)?="([a-zA-Z0-9_.]+)"/g)) used.add(m[1]);
}
const unknown = [...used].filter((k) => !intended(k) && !langs.every((l) => k in DICT[l]));
if (unknown.length) {
  bad += unknown.length;
  console.error(`used in the UI but not in every language:\n  ${unknown.join('\n  ')}`);
}

if (bad) process.exit(1);
console.log(`i18n ok: ${langs.join(', ')} · ${all.size} keys each · ${used.size} literal keys used`);
