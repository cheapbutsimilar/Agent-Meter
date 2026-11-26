//! Qwen Code: tokens and cost from its local chat recordings. Qwen offers no usage-limit
//! endpoint, so there are no % windows: this agent shows tokens and the API-equivalent cost.
//! * Logs: `<home>/projects/<project>/chats/*.jsonl` and `chats/archive/*.jsonl`, where
//!   `<home>` is the configured folder, else `QWEN_HOME`, else `~/.qwen`. A record kept in
//!   both places counts once (it is keyed by its session and uuid, not by file).
//! * Each line is a Claude Code-style envelope (`uuid`, `sessionId`, `timestamp`, `type`,
//!   `cwd`, `model`); `assistant` records carry Gemini-style `usageMetadata`:
//!   `promptTokenCount` (cache reads included), `candidatesTokenCount`,
//!   `thoughtsTokenCount` (billed as output) and `cachedContentTokenCount`.
//!   Qwen records no cache writes.
//! * Recording needs `--chat-recording` or `general.chatRecording` in Qwen Code's settings.

use super::common::*;
use super::files::{dedup_paths, list_files, split_paths, ListOptions};
use super::ledger::{Ledger, Tokens};
use crate::time::{parse_time, DAY};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Default)]
pub struct Qwen;

impl Qwen {
    /// Qwen Code home folders to read, most specific first.
    pub fn homes(ctx: &Ctx) -> Vec<PathBuf> {
        let mut dirs = split_paths(&ctx.cfg.log_path, &ctx.env.home);
        dirs.push(
            ctx.env
                .var("QWEN_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| ctx.env.home.join(".qwen")),
        );
        dirs.extend(split_paths(&ctx.cfg.extra_paths, &ctx.env.home));
        dedup_paths(dirs)
    }
}

impl Provider for Qwen {
    fn id(&self) -> &'static str {
        "qwen"
    }
    fn name(&self) -> &'static str {
        "Qwen"
    }

    fn scan(&mut self, ctx: &mut Ctx, now: i64) -> usize {
        let since = now - ctx.lookback_days * DAY;
        let mut files = 0;
        for home in Self::homes(ctx) {
            // a folder that is itself `projects/` works too
            let root = if home.join("projects").is_dir() {
                home.join("projects")
            } else {
                home
            };
            let list = list_files(
                &root,
                &ListOptions {
                    matches: &|n| n.ends_with(".jsonl"),
                    skip_dir: None,
                    modified_since: since,
                    max_depth: 5,
                },
            );
            for f in list {
                // only chat recordings: …/chats/x.jsonl or …/chats/archive/x.jsonl
                let in_chats = f.path.parent().is_some_and(|p| {
                    p.ends_with("chats")
                        || (p.ends_with("archive")
                            && p.parent().is_some_and(|q| q.ends_with("chats")))
                });
                if !in_chats {
                    continue;
                }
                files += 1;
                let mtime = f.mtime;
                let project_dir = f
                    .path
                    .ancestors()
                    .find(|a| a.ends_with("chats"))
                    .and_then(|c| c.parent())
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string());
                let ledger = &mut *ctx.ledger;
                let _ = ctx
                    .cursors
                    .read_jsonl(&f, &[b"usageMetadata"], now, &mut |rec, offset| {
                        on_record(ledger, rec, mtime, project_dir.as_deref(), offset, now)
                    });
            }
        }
        files
    }

    fn windows(&self, _st: &ProviderState, _now: i64) -> Vec<UsageWindow> {
        vec![]
    }
}

/// Count one recorded line if it is an assistant turn with usage.
pub fn on_record(
    ledger: &mut Ledger,
    rec: &Value,
    mtime: i64,
    project_dir: Option<&str>,
    offset: u64,
    now: i64,
) {
    if str_of(rec.get("type")) != Some("assistant") {
        return;
    }
    let Some(u) = rec
        .get("usageMetadata")
        .or_else(|| rec.pointer("/message/usageMetadata"))
        .filter(|u| u.is_object())
    else {
        return;
    };
    let prompt = num(u.get("promptTokenCount"));
    let cached = num(u.get("cachedContentTokenCount")).min(prompt);
    let output = num(u.get("candidatesTokenCount")) + num(u.get("thoughtsTokenCount"));
    if prompt <= 0.0 && output <= 0.0 {
        return;
    }
    let ts = rec.get("timestamp").and_then(parse_time).unwrap_or(mtime);
    // the same turn in chats/ and chats/archive/ shares its session and uuid
    let key = match (str_of(rec.get("sessionId")), str_of(rec.get("uuid"))) {
        (Some(s), Some(u)) => format!("{s}:{u}"),
        (None, Some(u)) => u.to_string(),
        _ => format!("{ts}:{prompt}:{output}:{offset}"),
    };
    let project = str_of(rec.get("cwd"))
        .and_then(project_name)
        .or_else(|| project_dir.map(String::from));
    let model = str_of(rec.get("model")).or_else(|| str_of(rec.pointer("/message/model")));
    ledger.record(
        "qwen",
        &key,
        ts,
        Tokens {
            input: prompt - cached,
            cache_read: cached,
            output,
            cache_write: 0.0,
        },
        model,
        project.as_deref(),
        now,
    );
}
