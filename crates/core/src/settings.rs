//! User settings (persisted as JSON; field names match the UI).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const PROVIDER_IDS: [&str; 8] = [
    "claude",
    "codex",
    "gemini",
    "antigravity",
    "grok",
    "minimax",
    "zai",
    "qwen",
];
pub const SETTINGS_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderConfig {
    /// Collect usage for this agent at all.
    pub enabled: bool,
    /// Show it in the widget (it stays in the dashboard either way).
    pub visible: bool,
    /// Window ids not to show in the widget (e.g. "7d_opus", "flash").
    pub hidden_windows: Vec<String>,
    /// Use the official usage endpoint where one exists (Claude, Gemini).
    pub use_api: bool,
    /// Extra folders to scan, separated by newlines / commas (e.g. WSL homes).
    pub extra_paths: String,
    /// Token budgets for estimated windows when no official limit is available.
    pub budget5h: f64,
    pub budget_week: f64,
    /// A command printing `{ "windows": [{ "label", "usedPercent", "resetsAt" }] }`.
    pub command: String,
    /// Antigravity only: the official agy.exe's absolute path. Empty finds it on its own
    /// (`where agy`, then the default install folders).
    pub antigravity_path: String,
    /// MiniMax and Z.AI: the API key their usage endpoints take (kept in this settings file).
    pub api_key: String,
    /// MiniMax and Z.AI: which platform the key belongs to, "global" or "china".
    pub region: String,
    /// Qwen only: the Qwen Code home folder (holding `projects/`). Empty: `QWEN_HOME`,
    /// then `~/.qwen`.
    pub log_path: String,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            visible: true,
            hidden_windows: vec![],
            use_api: true,
            extra_paths: String::new(),
            budget5h: 0.0,
            budget_week: 0.0,
            command: String::new(),
            antigravity_path: String::new(),
            api_key: String::new(),
            region: "global".into(),
            log_path: String::new(),
        }
    }
}

/// Which parts of the widget are shown. Each mode has its own set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct DisplayOptions {
    pub logo: bool,
    pub name: bool,
    pub label: bool,
    pub bar: bool,
    pub percent: bool,
    pub reset: bool,
    /// Tick on the bar showing how much of the window has elapsed.
    pub pace: bool,
    /// "Runs out before reset" forecast.
    pub forecast: bool,
    pub tokens: bool,
    pub cost: bool,
    pub system: bool,
    /// Model-specific windows (Opus/Sonnet weekly, Gemini Flash...).
    pub minor_windows: bool,
    /// Gear button that opens the dashboard's widget settings.
    pub gear: bool,
}

impl DisplayOptions {
    pub fn taskbar() -> Self {
        Self {
            logo: true,
            name: false,
            label: true,
            bar: true,
            percent: true,
            reset: true,
            pace: false,
            forecast: true,
            tokens: false,
            cost: false,
            system: true,
            minor_windows: false,
            gear: true,
        }
    }
    pub fn floating() -> Self {
        Self {
            logo: true,
            name: true,
            label: true,
            bar: true,
            percent: true,
            reset: true,
            pace: true,
            forecast: true,
            tokens: true,
            cost: true,
            system: false,
            minor_windows: false,
            gear: true,
        }
    }
}

impl Default for DisplayOptions {
    fn default() -> Self {
        Self::floating()
    }
}

/// Colours and font for the dashboard and the widget. Empty colours follow the preset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Theme {
    /// "system", "light" or "dark".
    pub mode: String,
    pub preset: String,
    /// "#RRGGBB" or "" (preset colour).
    pub accent: String,
    pub bg: String,
    pub fg: String,
    /// "system", "pretendard", "malgun", "noto" or "mono".
    pub font: String,
    /// Dashboard zoom 0.9..1.25.
    pub ui_scale: f64,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            mode: "dark".into(),
            preset: "agentmeter".into(),
            accent: String::new(),
            bg: String::new(),
            fg: String::new(),
            font: "system".into(),
            ui_scale: 1.0,
        }
    }
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Pos {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub v: u32,
    /// Widget layout: "taskbar" = horizontal strip, "floating" = card.
    /// (Names kept from when these were two modes, so old settings files still load.)
    pub mode: String,
    /// The strip sits in the taskbar (snapped) rather than floating on the desktop.
    pub docked: bool,
    pub widget_visible: bool,
    /// Global shortcuts (accelerator strings, empty = off).
    pub hotkey_mode: String,
    pub hotkey_visible: String,
    pub hotkey_dashboard: String,
    /// "used" or "remaining".
    pub value_mode: String,
    /// "relative" (2h 13m) or "clock" (14:30).
    pub reset_format: String,
    /// How API-equivalent costs are shown: "usd", "krw", "both", or "auto" (both when the
    /// UI is Korean, dollars otherwise; the default for fresh installs).
    pub currency: String,
    /// UI language: "auto" (the system language; anything but Korean shows English),
    /// "ko" or "en".
    pub language: String,
    /// Won per dollar; 0 = use the fetched rate.
    pub krw_rate: f64,
    pub taskbar: DisplayOptions,
    pub floating: DisplayOptions,
    /// Widget background opacity 0.3..1.
    pub opacity: f64,
    /// Widget zoom 0.8..1.5.
    pub scale: f64,
    pub theme: Theme,
    /// Length of a bar in the strip, px (40..120).
    pub bar_width: f64,
    /// Height of a strip floating on the desktop, px (32..56).
    pub strip_height: f64,
    /// Width of the card, px (260..420).
    pub card_width: f64,
    /// Widget corner radius, px (0..16).
    pub corner_radius: f64,
    pub always_on_top: bool,
    pub lock_position: bool,
    /// Taskbar mode: distance of the widget's right edge from the monitor's right edge.
    pub taskbar_offset: Option<f64>,
    /// Card: top-left position.
    pub floating_pos: Option<Pos>,
    /// Undocked strip: top-left position.
    pub strip_pos: Option<Pos>,
    pub notifications: bool,
    pub alert_thresholds: Vec<f64>,
    pub notify_reset: bool,
    pub notify_forecast: bool,
    /// Draw the highest usage as a gauge in the tray icon.
    pub tray_gauge: bool,
    pub launch_at_login: bool,
    /// Check GitHub Releases for new versions in the background (and ask, or install
    /// right away with `update_silent`).
    pub auto_update: bool,
    /// Install new versions without asking (the default asks with a popup).
    pub update_silent: bool,
    pub poll_logs_sec: u64,
    pub poll_api_sec: u64,
    pub providers: BTreeMap<String, ProviderConfig>,
}

impl Default for Settings {
    fn default() -> Self {
        let providers = PROVIDER_IDS
            .iter()
            .map(|id| {
                let mut c = ProviderConfig::default();
                if *id == "grok" {
                    c.use_api = false;
                }
                (id.to_string(), c)
            })
            .collect();
        Self {
            v: SETTINGS_VERSION,
            mode: "floating".into(),
            docked: true,
            widget_visible: true,
            hotkey_mode: "PageUp".into(),
            hotkey_visible: "Shift+PageUp".into(),
            hotkey_dashboard: String::new(),
            value_mode: "remaining".into(),
            reset_format: "relative".into(),
            currency: "auto".into(),
            language: "auto".into(),
            krw_rate: 0.0,
            taskbar: DisplayOptions::taskbar(),
            floating: DisplayOptions::floating(),
            opacity: 0.92,
            scale: 1.0,
            theme: Theme::default(),
            bar_width: 58.0,
            strip_height: 40.0,
            card_width: 312.0,
            corner_radius: 10.0,
            always_on_top: true,
            lock_position: false,
            taskbar_offset: None,
            floating_pos: None,
            strip_pos: None,
            notifications: true,
            alert_thresholds: vec![80.0, 95.0],
            notify_reset: true,
            notify_forecast: true,
            tray_gauge: true,
            launch_at_login: false,
            auto_update: true,
            update_silent: false,
            poll_logs_sec: 15,
            poll_api_sec: 180,
            providers,
        }
    }
}

/// Whether this platform has a taskbar the strip can dock into (not macOS).
pub const CAN_DOCK: bool = !cfg!(target_os = "macos");

impl Settings {
    pub fn provider(&self, id: &str) -> ProviderConfig {
        self.providers.get(id).cloned().unwrap_or_default()
    }

    /// Horizontal strip layout (docked or not).
    pub fn is_strip(&self) -> bool {
        self.mode != "floating"
    }

    /// Strip snapped into the taskbar. macOS has no taskbar to dock into (the menu bar
    /// and the Dock are off limits), so there the strip always floats.
    pub fn is_docked(&self) -> bool {
        CAN_DOCK && self.is_strip() && self.docked
    }

    /// Load from JSON, filling in defaults (including providers missing from old files).
    pub fn from_value(v: Option<Value>) -> Self {
        let mut s: Settings = v
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        for id in PROVIDER_IDS {
            s.providers.entry(id.to_string()).or_default();
        }
        // Antigravity used to live in Gemini's settings: move its agy.exe path over, so Gemini
        // goes back to Gemini CLI and Antigravity shows up as its own agent.
        let moved = s
            .providers
            .get_mut("gemini")
            .map(|g| std::mem::take(&mut g.antigravity_path))
            .filter(|p| !p.trim().is_empty());
        if let Some(path) = moved {
            if let Some(a) = s.providers.get_mut("antigravity") {
                if a.antigravity_path.trim().is_empty() {
                    a.antigravity_path = path;
                }
            }
        }
        if s.v < 3 {
            // v3: Codex limits come from its usage API by default (multi-PC use)
            if let Some(c) = s.providers.get_mut("codex") {
                c.use_api = true;
            }
        }
        s.v = SETTINGS_VERSION;
        s.sanitize();
        s
    }

    /// Apply a partial update coming from the UI (objects are merged deeply).
    pub fn merged(&self, patch: &Value) -> Self {
        let mut base = serde_json::to_value(self).unwrap_or(Value::Null);
        deep_merge(&mut base, patch);
        Self::from_value(Some(base))
    }

    fn sanitize(&mut self) {
        for c in self.providers.values_mut() {
            if c.region != "global" && c.region != "china" {
                c.region = "global".into();
            }
            // keys are pasted: drop surrounding blanks and an accidental "Bearer " prefix
            let key = c.api_key.trim();
            let key = key.strip_prefix("Bearer ").unwrap_or(key).trim();
            c.api_key = key
                .chars()
                .filter(|ch| !ch.is_control())
                .take(512)
                .collect();
            c.log_path = c.log_path.trim().chars().take(1024).collect();
        }
        if self.mode != "taskbar" && self.mode != "floating" {
            self.mode = "floating".into();
        }
        if self.value_mode != "used" && self.value_mode != "remaining" {
            self.value_mode = "remaining".into();
        }
        if self.reset_format != "relative" && self.reset_format != "clock" {
            self.reset_format = "relative".into();
        }
        if !matches!(self.currency.as_str(), "usd" | "krw" | "both" | "auto") {
            self.currency = "auto".into();
        }
        if !matches!(self.language.as_str(), "auto" | "ko" | "en") {
            self.language = "auto".into();
        }
        if !self.krw_rate.is_finite() || self.krw_rate < 0.0 || self.krw_rate > 100_000.0 {
            self.krw_rate = 0.0;
        }
        let t = &mut self.theme;
        if !matches!(t.mode.as_str(), "system" | "light" | "dark") {
            t.mode = "dark".into();
        }
        if t.preset.is_empty() || t.preset.len() > 32 {
            t.preset = "agentmeter".into();
        }
        for c in [&mut t.accent, &mut t.bg, &mut t.fg] {
            if !c.is_empty() && !is_hex_color(c) {
                c.clear();
            }
        }
        if !matches!(
            t.font.as_str(),
            "system" | "pretendard" | "malgun" | "noto" | "mono"
        ) {
            t.font = "system".into();
        }
        t.ui_scale = clamp_or(t.ui_scale, 0.9, 1.25, 1.0);
        self.bar_width = clamp_or(self.bar_width, 40.0, 120.0, 58.0);
        self.strip_height = clamp_or(self.strip_height, 32.0, 56.0, 40.0);
        self.card_width = clamp_or(self.card_width, 260.0, 420.0, 312.0);
        self.corner_radius = clamp_or(self.corner_radius, 0.0, 16.0, 10.0);
        if !self.opacity.is_finite() {
            self.opacity = 0.92;
        }
        self.opacity = self.opacity.clamp(0.3, 1.0);
        if !(0.8..=1.5).contains(&self.scale) {
            self.scale = 1.0;
        }
        self.poll_logs_sec = self.poll_logs_sec.clamp(5, 600);
        self.poll_api_sec = self.poll_api_sec.clamp(60, 3600);
        self.alert_thresholds.retain(|t| *t > 0.0 && *t <= 100.0);
        self.alert_thresholds
            .sort_by(|a, b| a.partial_cmp(b).unwrap());
        self.alert_thresholds.dedup();
    }
}

fn clamp_or(v: f64, lo: f64, hi: f64, default: f64) -> f64 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        default
    }
}

pub fn deep_merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                match b.get_mut(k) {
                    Some(slot) if slot.is_object() && v.is_object() => deep_merge(slot, v),
                    _ => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_and_merge() {
        let s = Settings::default();
        assert_eq!(s.mode, "floating");
        assert_eq!(
            (
                s.value_mode.as_str(),
                s.reset_format.as_str(),
                s.currency.as_str()
            ),
            ("remaining", "relative", "auto")
        );
        assert_eq!(s.language, "auto");
        let s = Settings::from_value(Some(json!({ "language": "fr", "currency": "eur" })));
        assert_eq!((s.language.as_str(), s.currency.as_str()), ("auto", "auto"));
        assert_eq!(s.hotkey_mode, "PageUp");
        assert!(s.taskbar.system && !s.floating.system);

        let s = Settings::from_value(Some(
            json!({ "mode": "taskbar", "providers": { "claude": { "enabled": false } } }),
        ));
        assert_eq!(
            s.mode, "taskbar",
            "a saved choice wins over the new defaults"
        );
        assert!(!s.provider("claude").enabled);
        assert!(s.provider("claude").use_api, "missing fields keep defaults");
        assert!(s.providers.contains_key("grok"));
        assert!(s.providers.contains_key("antigravity"));

        let m = s.merged(&json!({
            "providers": { "grok": { "budget5h": 1000, "hiddenWindows": ["7d"] } },
            "floating": { "cost": false },
            "valueMode": "remaining",
            "alertThresholds": [95, 70],
            "opacity": 7
        }));
        assert_eq!(m.provider("grok").budget5h, 1000.0);
        assert_eq!(m.provider("grok").hidden_windows, vec!["7d".to_string()]);
        assert!(!m.provider("claude").enabled, "untouched provider kept");
        assert!(
            !m.floating.cost && m.floating.tokens,
            "nested display options merge"
        );
        assert_eq!(m.value_mode, "remaining");
        assert_eq!(m.alert_thresholds, vec![70.0, 95.0]);
        assert_eq!(m.opacity, 1.0);
        let json = serde_json::to_value(&m).unwrap();
        assert!(json.get("hotkeyMode").is_some());
        assert!(json["taskbar"].get("minorWindows").is_some());
        assert!(json["providers"]["grok"].get("budgetWeek").is_some());

        let t = s.merged(&json!({
            "theme": { "accent": "#12AB34", "bg": "red", "mode": "light", "font": "comic" },
            "cardWidth": 9999, "barWidth": 10
        }));
        assert_eq!(t.theme.accent, "#12AB34");
        assert_eq!(t.theme.bg, "", "only #RRGGBB colours are kept");
        assert_eq!(t.theme.mode, "light");
        assert_eq!(t.theme.font, "system");
        assert_eq!((t.card_width, t.bar_width), (420.0, 40.0));
        assert!(s.docked, "strip starts docked");

        let bad = s.merged(&json!({ "mode": "sideways", "valueMode": "x" }));
        assert_eq!(bad.mode, "floating");
        assert_eq!(bad.value_mode, "remaining");
    }

    #[test]
    fn antigravity_path_moves_out_of_gemini() {
        let s = Settings::from_value(Some(json!({
            "providers": { "gemini": { "antigravityPath": "C:\\agy\\agy.exe", "command": "" } }
        })));
        assert_eq!(s.provider("gemini").antigravity_path, "");
        assert_eq!(
            s.provider("antigravity").antigravity_path,
            "C:\\agy\\agy.exe"
        );
        assert!(s.provider("antigravity").enabled);
    }
}
