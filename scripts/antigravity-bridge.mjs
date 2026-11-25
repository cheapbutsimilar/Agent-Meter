// Antigravity's documented statusLine JSON -> AgentMeter user-command JSON.
// No OAuth tokens, prompts, transcripts, or email addresses are persisted.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';

export const MAX_AGE_MS = 15 * 60 * 1000;
const defaultCache = path.join(process.env.LOCALAPPDATA || os.homedir(),
  'AgentMeter', 'antigravity-bridge', 'quota.json');

export function sanitize(payload, now = Date.now()) {
  if (payload?.product !== 'antigravity') throw new Error('Not an Antigravity status payload');
  const quota = {};
  for (const [id, bucket] of Object.entries(payload.quota || {})) {
    if (!['gemini-5h', 'gemini-weekly'].includes(id)) continue;
    if (!bucket || typeof bucket !== 'object') continue;
    const fraction = bucket.remaining_fraction;
    // An absolute remaining_amount has no denominator: never invent a percentage.
    if (bucket.disabled || typeof fraction !== 'number' ||
        !Number.isFinite(fraction) || fraction < 0 || fraction > 1) continue;
    const reset = typeof bucket.reset_time === 'string' ? Date.parse(bucket.reset_time) : NaN;
    const seconds = bucket.reset_in_seconds;
    const resetMs = Number.isFinite(reset) ? reset :
      typeof seconds === 'number' && Number.isFinite(seconds) && seconds >= 0
        ? now + seconds * 1000 : null;
    quota[id] = { remainingFraction: fraction, resetsAt: resetMs };
  }
  return {
    version: 1,
    capturedAt: now,
    plan: typeof payload.plan_tier === 'string' ? payload.plan_tier.slice(0, 80) : null,
    quota,
  };
}

export function toAgentMeter(cache, now = Date.now()) {
  const empty = reason => ({ plan: `Antigravity · ${reason}`, windows: [] });
  if (!cache || cache.version !== 1 || !Number.isFinite(cache.capturedAt))
    return empty('waiting for /usage');
  if (cache.capturedAt > now + 60_000 || now - cache.capturedAt > MAX_AGE_MS)
    return empty('stale - run /usage');
  const windows = [];
  for (const [id, bucket] of Object.entries(cache.quota || {}).sort()) {
    // These go in AgentMeter's Gemini slot, not its separate Claude/Codex subscriptions.
    if (!/gemini/i.test(id)) continue;
    const remaining = bucket?.remainingFraction;
    if (typeof remaining !== 'number' || !Number.isFinite(remaining) ||
        remaining < 0 || remaining > 1) continue;
    if (bucket.resetsAt !== null &&
        (!Number.isFinite(bucket.resetsAt) || bucket.resetsAt <= now)) continue;
    const label = /week/i.test(id) ? 'WK' : /5.?h|five.?hour/i.test(id) ? '5H' :
      /day|daily/i.test(id) ? 'DAY' : `G${windows.length + 1}`;
    windows.push({
      id, label,
      usedPercent: Math.round((1 - remaining) * 10000) / 100,
      resetsAt: bucket.resetsAt === null ? null : new Date(bucket.resetsAt).toISOString(),
    });
  }
  return windows.length
    ? { plan: `Antigravity${cache.plan ? ` · ${cache.plan}` : ''}`, windows }
    : empty('quota unavailable - run /usage');
}

export function saveSnapshot(file, snapshot) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const temp = `${file}.${process.pid}.tmp`;
  try {
    fs.writeFileSync(temp, JSON.stringify(snapshot), { mode: 0o600 });
    fs.renameSync(temp, file);
  } finally {
    if (fs.existsSync(temp)) fs.unlinkSync(temp);
  }
}

export function startCollector(configFile, now = Date.now()) {
  const home = path.dirname(configFile);
  let config;
  try {
    config = JSON.parse(fs.readFileSync(configFile, 'utf8'));
    const status = JSON.parse(fs.readFileSync(path.join(home, 'collector-status.json'), 'utf8'));
    if (Number.isFinite(status.nextAttempt) && status.nextAttempt > now) return;
  } catch { /* First run, or no status yet; worker also locks/throttles. */ }
  if (!config || typeof config.pythonw !== 'string' || typeof config.worker !== 'string' ||
      !path.isAbsolute(config.pythonw) || !path.isAbsolute(config.worker)) return;
  const child = spawn(config.pythonw, ['-I', config.worker, configFile], {
    detached: true, windowsHide: true, stdio: 'ignore',
    cwd: home,
    // Do not pass API keys, proxy configuration, or Node/Python injection options.
    env: Object.fromEntries(Object.entries(process.env).filter(([key]) =>
      ['SYSTEMROOT', 'WINDIR', 'COMSPEC', 'SYSTEMDRIVE', 'OS', 'PROCESSOR_ARCHITECTURE']
        .includes(key.toUpperCase()))),
  });
  child.on('error', () => { /* no sensitive error details; stale cache expires */ });
  child.unref();
}

export function toBackgroundMeter(cache, status, now = Date.now()) {
  const result = toAgentMeter(cache, now);
  if (result.windows.length) return result;
  const labels = {
    cli_changed: 'CLI updated - reinstall collector',
    quota_unavailable: 'quota unavailable - check CLI sign-in',
    collector_error: 'collector error',
  };
  result.plan = `Antigravity · ${labels[status?.state] || 'background refresh pending'}`;
  return result;
}

export function main(args) {
  const [mode, file = defaultCache] = args;
  if (mode === 'capture') {
    const snapshot = sanitize(JSON.parse(fs.readFileSync(0, 'utf8')));
    saveSnapshot(file, snapshot);
    const result = toAgentMeter(snapshot);
    // stack_with_default preserves Antigravity's normal status line.
    console.log(result.windows.length
      ? `AgentMeter: ${result.windows.map(w => `${w.label} ${w.usedPercent}% used`).join(' / ')}`
      : 'AgentMeter: waiting for Gemini quota (/usage)');
  } else if (mode === 'query' || mode === 'query-background') {
    const home = path.dirname(defaultCache);
    const cacheFile = mode === 'query-background' ? path.join(home, 'quota-background.json') : file;
    if (mode === 'query-background') startCollector(path.join(home, 'collector.json'));
    let cache;
    try { cache = JSON.parse(fs.readFileSync(cacheFile, 'utf8')); } catch { /* unavailable */ }
    // Success + empty windows clears AgentMeter's previous command result.
    let status;
    if (mode === 'query-background') {
      try { status = JSON.parse(fs.readFileSync(path.join(home, 'collector-status.json'), 'utf8')); } catch { /* first run */ }
    }
    console.log(JSON.stringify(mode === 'query-background'
      ? toBackgroundMeter(cache, status) : toAgentMeter(cache)));
  } else {
    throw new Error('Usage: node antigravity-bridge.mjs capture|query [cache-path] | query-background');
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(process.argv.slice(2)); }
  catch (error) {
    // Never echo the input payload, which includes private account/session data.
    console.error(`Antigravity bridge: ${error instanceof SyntaxError ? 'invalid JSON' : error.message}`);
    process.exitCode = 1;
  }
}
