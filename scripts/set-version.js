#!/usr/bin/env node
/* Release step: set src-tauri/tauri.conf.json's version to <major>.<minor>.<GITHUB_RUN_NUMBER>,
 * so every job of one release run builds the same version.
 *   GITHUB_RUN_NUMBER=31 node scripts/set-version.js */
'use strict';

const fs = require('fs');
const path = require('path');

const run = process.env.GITHUB_RUN_NUMBER;
if (!/^\d+$/.test(run || '')) {
  console.error('GITHUB_RUN_NUMBER is not set');
  process.exit(1);
}
const f = path.join(__dirname, '..', 'src-tauri', 'tauri.conf.json');
const c = JSON.parse(fs.readFileSync(f, 'utf8'));
const [major, minor] = c.version.split('.');
c.version = `${major}.${minor}.${run}`;
fs.writeFileSync(f, JSON.stringify(c, null, 2) + '\n');
console.log('version', c.version);
