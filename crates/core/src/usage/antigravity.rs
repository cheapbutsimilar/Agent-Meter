//! Google Antigravity CLI (`agy`), Windows only.
//! * Quota: the official, Google-signed agy.exe reports its Gemini 5-hour and weekly quotas
//!   through its status line; `agentmeter_antigravity` runs it in an isolated profile and keeps
//!   only the quota, plan and reset times (see docs/antigravity-native.md).
//! * agy.exe: the path set in Agents → Antigravity, or found on its own (`where agy`, then the
//!   default install folders). The CLI writes no token logs we read, so there are no token
//!   counts or costs for it.

use super::common::*;
use std::path::PathBuf;

/// Successful readings at least this far apart (the CLI takes a few seconds to start).
const EVERY_MS: i64 = 180_000;
/// Look for agy.exe again at most this often while it can't be found.
const SEARCH_EVERY_MS: i64 = 10 * 60_000;

#[derive(Default)]
pub struct Antigravity {
    /// The path from the settings, as last seen (a change starts over).
    configured: String,
    found: Option<PathBuf>,
    search_due: i64,
    due: i64,
    last_run: i64,
    failures: u32,
}

fn user_home(ctx: &Ctx) -> PathBuf {
    ctx.env
        .var("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.env.home.clone())
}

/// Antigravity's bucket ids → the shared window ids and labels.
fn window(id: &str) -> Option<(&'static str, &'static str, i64)> {
    match id {
        "gemini-5h" => Some(("5h", "5H", 300)),
        "gemini-weekly" => Some(("7d", "WK", 10080)),
        _ => None,
    }
}

impl Antigravity {
    /// The agy.exe to run: the configured one, else one found on this computer.
    fn exe(&mut self, ctx: &Ctx, now: i64) -> Option<PathBuf> {
        let configured = ctx.cfg.antigravity_path.trim();
        if !configured.is_empty() {
            return Some(PathBuf::from(configured));
        }
        if self.found.as_ref().is_some_and(|p| !p.is_file()) {
            self.found = None;
            self.search_due = 0;
        }
        if self.found.is_none() && (ctx.force || now >= self.search_due) {
            self.search_due = now + SEARCH_EVERY_MS;
            self.found = agentmeter_antigravity::discover(&user_home(ctx));
        }
        self.found.clone()
    }
}

impl Provider for Antigravity {
    fn id(&self) -> &'static str {
        "antigravity"
    }
    fn name(&self) -> &'static str {
        "Antigravity"
    }
    fn has_api(&self) -> bool {
        true
    }

    fn scan(&mut self, _ctx: &mut Ctx, _now: i64) -> usize {
        0
    }

    fn poll(&mut self, ctx: &mut Ctx, now: i64) {
        if !cfg!(windows) {
            return; // the collector needs Windows ConPTY and Authenticode
        }
        let configured = ctx.cfg.antigravity_path.trim().to_string();
        if configured != self.configured {
            self.configured = configured;
            self.due = 0;
            self.failures = 0;
            ctx.state.limits = None;
            ctx.state.plan = None;
            ctx.state.samples.clear();
        }
        let Some(exe) = self.exe(ctx, now) else {
            ctx.state.exe_path = None;
            ctx.state.api_error = Some("agy.exe를 찾지 못했어요".into());
            return;
        };
        ctx.state.exe_path = Some(exe.to_string_lossy().into_owned());
        // a forced refresh retries after a failure, but never runs the CLI more than once a minute
        let retry = ctx.force && self.failures > 0 && now - self.last_run >= 60_000;
        if now < self.due && !retry {
            return;
        }
        self.last_run = now;
        match agentmeter_antigravity::collect(&exe, &user_home(ctx)) {
            Ok(s) => {
                self.failures = 0;
                self.due = s.captured_at + EVERY_MS;
                let windows: Vec<_> = s
                    .buckets
                    .into_iter()
                    .filter_map(|b| {
                        let (id, label, minutes) = window(&b.id)?;
                        Some(
                            UsageWindow::new(
                                id,
                                label,
                                (1.0 - b.remaining_fraction) * 100.0,
                                b.resets_at,
                                "api",
                            )
                            .minutes(minutes),
                        )
                    })
                    .collect();
                for w in &windows {
                    ctx.state
                        .record_sample(&w.id, s.captured_at, w.used_percent);
                }
                ctx.state.limits = Some(Limits {
                    at: s.captured_at,
                    windows,
                });
                ctx.state.plan = s.plan;
                ctx.state.api_error = None;
            }
            Err(e) => {
                self.failures = (self.failures + 1).min(4);
                self.due = now + (EVERY_MS * (1 << self.failures)).min(1_800_000);
                ctx.state.api_error = Some(e);
                // a path found on its own may have gone stale: look again next time
                if ctx.cfg.antigravity_path.trim().is_empty() {
                    self.found = None;
                }
            }
        }
    }

    /// Readings expire after 15 minutes (never shown as a fresh 0%) and at their reset.
    fn windows(&self, st: &ProviderState, now: i64) -> Vec<UsageWindow> {
        match &st.limits {
            Some(l) if now >= l.at && now - l.at < agentmeter_antigravity::MAX_AGE_MS => l
                .windows
                .iter()
                .filter(|w| w.resets_at.is_none_or(|r| r > now))
                .cloned()
                .collect(),
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_expire_without_showing_a_fresh_zero() {
        let p = Antigravity::default();
        let st = ProviderState {
            limits: Some(Limits {
                at: 1000,
                windows: vec![
                    UsageWindow::new("5h", "5H", 42.0, Some(2000), "api"),
                    UsageWindow::new("7d", "WK", 30.0, None, "api"),
                ],
            }),
            ..Default::default()
        };
        assert_eq!(p.windows(&st, 1500).len(), 2);
        assert_eq!(p.windows(&st, 2000).len(), 1);
        assert!(p
            .windows(&st, 1000 + agentmeter_antigravity::MAX_AGE_MS)
            .is_empty());
        assert!(p.windows(&st, 999).is_empty());
    }

    #[test]
    fn bucket_ids_map_to_the_shared_windows() {
        assert_eq!(window("gemini-5h"), Some(("5h", "5H", 300)));
        assert_eq!(window("gemini-weekly"), Some(("7d", "WK", 10080)));
        assert_eq!(window("3p-weekly"), None);
    }
}
