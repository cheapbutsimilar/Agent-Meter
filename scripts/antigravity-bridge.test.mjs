import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { sanitize, toAgentMeter, toBackgroundMeter, startCollector, MAX_AGE_MS } from './antigravity-bridge.mjs';

const now = Date.parse('2026-11-24T12:00:00Z');
const fixture = {
  product: 'antigravity', plan_tier: 'Pro',
  email: 'private@example.com', transcript_path: 'private-path',
  context_window: { used_percentage: 99 },
  quota: {
    'gemini-weekly': { remaining_fraction: 0.9378, reset_time: '2026-11-30T12:00:00Z' },
    'gemini-5h': { remaining_fraction: 0, reset_in_seconds: 3600 },
    'claude-weekly': { remaining_fraction: 0.5 },
  },
};

test('converts real quota, not context usage; excludes other subscriptions', () => {
  const result = toAgentMeter(sanitize(fixture, now), now);
  assert.equal(result.plan, 'Antigravity · Pro');
  assert.deepEqual(result.windows.map(w => [w.label, w.usedPercent]), [['5H', 100], ['WK', 6.22]]);
  assert.equal(result.windows[0].resetsAt, '2026-11-24T13:00:00.000Z');
});

test('never persists email, conversation, or token material', () => {
  const saved = JSON.stringify(sanitize({ ...fixture, access_token: 'secret' }, now));
  for (const text of ['private', 'secret', 'context_window', 'email', 'access_token'])
    assert.equal(saved.includes(text), false);
});

test('unknown, disabled, amount-only, out-of-range quotas are not zero percent', () => {
  const payload = { product: 'antigravity', quota: {
    'gemini-amount': { remaining_amount: 100 },
    'gemini-disabled': { remaining_fraction: 1, disabled: true },
    'gemini-bad': { remaining_fraction: 2 },
    'gemini-string': { remaining_fraction: '0.5' },
    'gemini-null': null,
  } };
  assert.deepEqual(toAgentMeter(sanitize(payload, now), now).windows, []);
  for (const bucket of [
    { remaining_amount: 100 }, { remaining_fraction: 1, disabled: true },
    { remaining_fraction: 2 }, { remaining_fraction: -1 },
    { remaining_fraction: '0.5' }, { remaining_fraction: NaN }, null,
  ]) {
    const input = { product: 'antigravity', quota: { 'gemini-weekly': bucket } };
    assert.deepEqual(toAgentMeter(sanitize(input, now), now).windows, []);
  }
  assert.throws(() => sanitize({ product: 'other' }, now));
});

test('stale, missing, future and elapsed reset data clears windows', () => {
  const cache = sanitize(fixture, now);
  assert.deepEqual(toAgentMeter(cache, now + MAX_AGE_MS + 1).windows, []);
  assert.deepEqual(toAgentMeter(undefined, now).windows, []);
  assert.deepEqual(toAgentMeter(cache, now - 120000).windows, []);
  const reset = sanitize({ product: 'antigravity', quota: {
    'gemini-weekly': { remaining_fraction: 0.5, reset_time: new Date(now).toISOString() },
  } }, now);
  assert.deepEqual(toAgentMeter(reset, now).windows, []);
});

test('capture/query processes work with spaces and Unicode; invalid cache clears results', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'agentmeter-test-'));
  const file = path.join(dir, '한글 space', 'quota.json');
  const script = fileURLToPath(new URL('./antigravity-bridge.mjs', import.meta.url));
  try {
    const captured = spawnSync(process.execPath, [script, 'capture', file], {
      input: JSON.stringify({ ...fixture, quota: {
        'gemini-weekly': { remaining_fraction: 0.75, reset_in_seconds: 86400 },
      } }), encoding: 'utf8',
    });
    assert.equal(captured.status, 0, captured.stderr);
    const read = () => {
      const result = spawnSync(process.execPath, [script, 'query', file], { encoding: 'utf8' });
      assert.equal(result.status, 0, result.stderr);
      return JSON.parse(result.stdout);
    };
    assert.equal(read().windows[0].usedPercent, 25);
    fs.writeFileSync(file, '{broken');
    assert.deepEqual(read().windows, []);
    fs.unlinkSync(file);
    assert.deepEqual(read().windows, []);
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test('background unavailable data never asks for an open CLI window or invents usage', () => {
  assert.equal(toBackgroundMeter(undefined, undefined, now).plan,
    'Antigravity · background refresh pending');
  const stale = sanitize(fixture, now - MAX_AGE_MS - 1);
  const result = toBackgroundMeter(stale, { state: 'cli_changed' }, now);
  assert.deepEqual(result.windows, []);
  assert.match(result.plan, /reinstall collector/);
});

test('invalid background configuration cannot spawn a program or throw', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'agentmeter-config-test-'));
  const file = path.join(dir, 'collector.json');
  try {
    for (const data of [{}, { pythonw: 1 }, { pythonw: 'relative', worker: 'bad' }]) {
      fs.writeFileSync(file, JSON.stringify(data));
      assert.doesNotThrow(() => startCollector(file));
    }
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});
