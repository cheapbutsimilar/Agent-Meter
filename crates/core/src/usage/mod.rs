//! Usage collection: providers → ledger → snapshot.

pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod common;
pub mod files;
pub mod gemini;
pub mod grok;
pub mod ledger;
pub mod minimax;
pub mod qwen;
pub mod zai;

use crate::settings::Settings;
use crate::time::DAY;
use common::*;
use files::{Cursor, FileCursors};
use ledger::{HistoryDay, Ledger, LedgerData, TokenStats};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Duration;

pub use common::{Env, Http, HttpResponse, NoHttp, UsageWindow};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub plan: Option<String>,
    pub windows: Vec<UsageWindow>,
    /// api | log | estimate | command | none
    pub source: String,
    pub api_error: Option<String>,
    pub scan_error: Option<String>,
    pub command_error: Option<String>,
    pub log_files: usize,
    /// The executable AgentMeter runs for this agent (Antigravity's agy.exe), when found.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub exe_path: Option<String>,
    pub limits_at: Option<i64>,
    /// When the agent's stored login lapses (the agent renews it whenever it runs).
    pub auth_expires_at: Option<i64>,
    pub tokens: TokenStats,
    pub history: Vec<HistoryDay>,
    pub detected: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub at: i64,
    pub providers: Vec<ProviderSnapshot>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct CacheFile {
    v: u32,
    saved_at: i64,
    cursors: Vec<(String, Cursor)>,
    ledger: LedgerData,
    provider_state: BTreeMap<String, ProviderState>,
}

const CACHE_VERSION: u32 = 2;

pub struct RefreshResult {
    pub snapshot: Snapshot,
    /// Weighted tokens per provider processed since the previous refresh.
    pub new_tokens: BTreeMap<String, i64>,
}

/// Earliest re-check of a usage API after new activity shows up in the logs.
const ACTIVE_POLL_MS: i64 = 60_000;
/// A manual refresh asks the API again only if the last ask is at least this old.
const MANUAL_FLOOR_MS: i64 = 15_000;

pub struct UsageService {
    ledger: Ledger,
    cursors: FileCursors,
    providers: Vec<Box<dyn Provider>>,
    states: BTreeMap<String, ProviderState>,
    api_due: HashMap<String, i64>,
    /// When each provider's usage API was last asked.
    api_last: HashMap<String, i64>,
    cmd_due: HashMap<String, i64>,
    backoff: HashMap<String, i64>,
    cache_path: Option<PathBuf>,
    lookback_days: i64,
    env: Env,
    http: Box<dyn Http>,
    last_save: i64,
}

impl UsageService {
    pub fn new(
        cache_path: Option<PathBuf>,
        env: Env,
        http: Box<dyn Http>,
        credit_since: i64,
    ) -> Self {
        let lookback_days = 8;
        let providers: Vec<Box<dyn Provider>> = vec![
            Box::new(claude::Claude::default()),
            Box::new(codex::Codex::default()),
            Box::new(gemini::Gemini),
            Box::new(antigravity::Antigravity::default()),
            Box::new(grok::Grok),
            Box::new(minimax::MiniMax),
            Box::new(zai::Zai),
            Box::new(qwen::Qwen),
        ];
        let states = providers
            .iter()
            .map(|p| (p.id().to_string(), ProviderState::default()))
            .collect();
        Self {
            ledger: Ledger::new(lookback_days, credit_since),
            cursors: FileCursors::default(),
            providers,
            states,
            api_due: HashMap::new(),
            api_last: HashMap::new(),
            cmd_due: HashMap::new(),
            backoff: HashMap::new(),
            cache_path,
            lookback_days,
            env,
            http,
            last_save: 0,
        }
    }

    pub fn set_credit_since(&mut self, ts: i64) {
        self.ledger.credit_since = ts;
    }

    pub fn load(&mut self) {
        let Some(path) = &self.cache_path else { return };
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let Ok(data) = serde_json::from_slice::<CacheFile>(&bytes) else {
            return;
        };
        if data.v != CACHE_VERSION || !self.ledger.load(data.ledger) {
            return; // older format: rebuild from the logs (lookback window)
        }
        self.cursors.load(data.cursors);
        for (id, st) in data.provider_state {
            self.states.insert(id, st);
        }
    }

    /// Persist offsets & ledger (at most every 30 s unless `force`).
    pub fn save(&mut self, now: i64, force: bool) {
        if !force && now - self.last_save < 30_000 {
            return;
        }
        self.last_save = now;
        let Some(path) = &self.cache_path else { return };
        let data = CacheFile {
            v: CACHE_VERSION,
            saved_at: now,
            cursors: self.cursors.to_vec(),
            ledger: self.ledger.to_data(),
            provider_state: self.states.clone(),
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        if let Ok(bytes) = serde_json::to_vec(&data) {
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(&tmp, path);
                self.ledger.dirty = false;
            }
        }
    }

    /// One polling round. `force_api` ignores API intervals (manual refresh).
    /// Usage APIs are asked every `poll_api_sec`, or after `ACTIVE_POLL_MS` when the logs
    /// show new activity; a 429 doubles the wait (up to 4×).
    pub fn refresh(&mut self, settings: &Settings, force_api: bool, now: i64) -> RefreshResult {
        let api_every = settings.poll_api_sec.max(60) as i64 * 1000;
        for p in self.providers.iter_mut() {
            let id = p.id().to_string();
            let cfg = settings.provider(&id);
            if !cfg.enabled {
                continue;
            }
            let state = self.states.entry(id.clone()).or_default();
            let mut ctx = Ctx {
                ledger: &mut self.ledger,
                cursors: &mut self.cursors,
                state,
                cfg: &cfg,
                env: &self.env,
                http: self.http.as_ref(),
                lookback_days: self.lookback_days,
                backoff: false,
                force: force_api,
            };
            let files = p.scan(&mut ctx, now);
            ctx.state.log_files = files;
            ctx.state.scan_error = None;

            let due = self.api_due.get(&id).copied().unwrap_or(0);
            let last = self.api_last.get(&id).copied().unwrap_or(0);
            let mul = self.backoff.get(&id).copied().unwrap_or(1);
            // New activity in the logs since the last check: the limit has moved, so look
            // again soon instead of waiting out the whole interval (not while backing off).
            let active = mul == 1
                && now - last >= ACTIVE_POLL_MS
                && ctx.ledger.last_event(&id).is_some_and(|t| t > last);
            // a manual refresh still respects a short floor, so clicking can't cause a 429
            let manual = force_api && now - last >= MANUAL_FLOOR_MS;
            if p.has_api() && cfg.use_api && (manual || active || now >= due) {
                self.api_due.insert(id.clone(), now + api_every * mul);
                self.api_last.insert(id.clone(), now);
                p.poll(&mut ctx, now);
                let mul = if ctx.backoff { (mul * 2).min(4) } else { 1 };
                self.backoff.insert(id.clone(), mul);
                if mul > 1 {
                    self.api_due.insert(id.clone(), now + api_every * mul);
                }
            }
            if !cfg.command.trim().is_empty()
                && (force_api || now >= self.cmd_due.get(&id).copied().unwrap_or(0))
            {
                self.cmd_due.insert(id.clone(), now + api_every);
                match run_command(&cfg.command, Duration::from_secs(20)) {
                    Ok(v) => {
                        ctx.state.command_error = None;
                        ctx.state.command = Some(CommandLimits {
                            at: now,
                            windows: normalize_command_windows(&v),
                            plan: str_of(v.get("plan")).map(String::from),
                        });
                    }
                    Err(e) => ctx.state.command_error = Some(e),
                }
            }
        }
        self.ledger.prune(now);
        self.cursors.prune(now, self.lookback_days * DAY * 2);
        let new_tokens = self.ledger.take_pending();
        let snapshot = self.snapshot(settings, now);
        self.save(now, false);
        RefreshResult {
            snapshot,
            new_tokens,
        }
    }

    pub fn snapshot(&self, settings: &Settings, now: i64) -> Snapshot {
        let providers = self
            .providers
            .iter()
            .map(|p| {
                let id = p.id();
                let cfg = settings.provider(id);
                let st = self.states.get(id).cloned().unwrap_or_default();
                let (windows, source) = match st
                    .command
                    .as_ref()
                    .filter(|c| !cfg.command.trim().is_empty() && !c.windows.is_empty())
                {
                    Some(c) => (c.windows.clone(), "command".to_string()),
                    None => {
                        let w = p.windows(&st, now);
                        match w.first().map(|f| f.source.clone()) {
                            Some(src) => (w, src),
                            None => {
                                let b = budget_windows(&self.ledger, id, &cfg, now);
                                let src = if b.is_empty() { "none" } else { "estimate" };
                                (b, src.to_string())
                            }
                        }
                    }
                };
                let mut windows = apply_resets(windows, now);
                for w in windows.iter_mut() {
                    enrich_window(w, st.samples.get(&w.id), now);
                }
                let tokens = self.ledger.stats(id, now);
                let detected = !windows.is_empty()
                    || st.log_files > 0
                    || tokens.week > 0
                    || st.exe_path.is_some();
                ProviderSnapshot {
                    id: id.to_string(),
                    name: p.name().to_string(),
                    enabled: cfg.enabled,
                    plan: st
                        .command
                        .as_ref()
                        .and_then(|c| c.plan.clone())
                        .filter(|_| !cfg.command.trim().is_empty())
                        .or(st.plan.clone()),
                    windows,
                    source,
                    api_error: if cfg.use_api && p.has_api() {
                        st.api_error.clone()
                    } else {
                        None
                    },
                    auth_expires_at: st.auth_expires_at.filter(|_| cfg.use_api && p.has_api()),
                    scan_error: st.scan_error.clone(),
                    command_error: if cfg.command.trim().is_empty() {
                        None
                    } else {
                        st.command_error.clone()
                    },
                    log_files: st.log_files,
                    exe_path: st.exe_path.clone().filter(|_| cfg.use_api),
                    limits_at: st.limits.as_ref().map(|l| l.at),
                    tokens,
                    history: self.ledger.history(id, 30, now),
                    detected,
                }
            })
            .collect();
        Snapshot { at: now, providers }
    }

    /// Daily totals of every provider as CSV (UTF-8 with BOM so Excel reads it).
    pub fn export_csv(&self) -> String {
        let mut out = String::from("\u{feff}date,agent,weighted_tokens,input,output,cache_read,cache_write,requests,est_cost_usd\n");
        let mut rows = vec![];
        for p in &self.providers {
            for (day, d) in self.ledger.all_days(p.id()) {
                if d.n == 0 && d.w == 0 {
                    continue;
                }
                rows.push(format!(
                    "{day},{},{},{},{},{},{},{},{:.4}",
                    p.name(),
                    d.w,
                    d.input,
                    d.output,
                    d.cache_read,
                    d.cache_write,
                    d.n,
                    d.cost
                ));
            }
        }
        rows.sort();
        out.push_str(&rows.join("\n"));
        out.push('\n');
        out
    }

    /// Test / debug access.
    pub fn state_mut(&mut self, id: &str) -> &mut ProviderState {
        self.states.entry(id.to_string()).or_default()
    }
}
