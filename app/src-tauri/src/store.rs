use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config_toml::{
    build_preview, expand_path, parse_grok_config, upsert_model_config, ModelUpsert,
};
use crate::health::{self, HealthResult};
use crate::models::{AppState, CommandResult, ConfigSnapshot, CreateProviderInput, TokenProfile};
use crate::secret_store;
use crate::speedtest::{self, SpeedTestResult};
use crate::usage::{self, UsageSummary};

pub struct AppStore {
    inner: Mutex<Inner>,
}

/// Reuse usage scans across tray rebuilds / window reopens for a short window.
const USAGE_CACHE_TTL: Duration = Duration::from_secs(45);

struct Inner {
    state: AppState,
    discovered: ConfigSnapshot,
    available_model_ids: Vec<String>,
    status: String,
    busy: bool,
    /// Last health check per profile id.
    health: std::collections::HashMap<uuid::Uuid, HealthResult>,
    /// Last speed test per profile id.
    speed: std::collections::HashMap<uuid::Uuid, SpeedTestResult>,
    /// Cached usage summary: (window_hours, computed_at, summary).
    usage_cache: Option<(u32, Instant, UsageSummary)>,
}

impl AppStore {
    pub fn new() -> Self {
        let mut state = load_or_default();
        refresh_token_flags(&mut state);
        let config_path = state
            .profiles
            .first()
            .map(|p| p.config_path.clone())
            .unwrap_or_else(default_config_path);
        let discovered = read_config_snapshot(&config_path);
        Self {
            inner: Mutex::new(Inner {
                state,
                discovered,
                available_model_ids: Vec::new(),
                status: default_status_message(),
                busy: false,
                health: std::collections::HashMap::new(),
                speed: std::collections::HashMap::new(),
                usage_cache: None,
            }),
        }
    }

    pub fn snapshot(&self) -> CommandResult {
        // Light path: list UI must never block on config reads / vault re-scans.
        let guard = self.inner.lock().expect("store lock");
        guard.to_list_result()
    }

    pub fn list_profiles(&self) -> CommandResult {
        self.snapshot()
    }

    pub fn add_profile(&self) -> CommandResult {
        let mut guard = self.inner.lock().expect("store lock");
        let profile = TokenProfile::new("新供应商");
        guard.state.selected_id = Some(profile.id);
        guard.state.profiles.push(profile);
        persist(&guard.state);
        guard.status = "已创建供应商。填入 Token 后即可启用。".into();
        guard.refresh_discovered_quietly();
        guard.to_result(None)
    }

    pub fn create_provider(&self, input: CreateProviderInput) -> CommandResult {
        let mut profile = TokenProfile::new(input.name.trim());
        if profile.name.is_empty() {
            profile.name = "新供应商".into();
        }
        let model_id = input.model_id.trim().to_string();
        if !model_id.is_empty() {
            profile.model_id = model_id;
        }
        profile.api_model = normalize_opt(input.api_model);
        profile.model_alias = normalize_opt(input.model_alias);
        profile.description = normalize_opt(input.description);
        profile.base_url = normalize_opt(input.base_url);
        profile.env_key = normalize_opt(input.env_key);
        profile.api_backend = normalize_opt(input.api_backend);
        profile.context_window = input.context_window;
        profile.max_completion_tokens = input.max_completion_tokens;
        if let Some(path) = normalize_opt(input.config_path) {
            profile.config_path = path;
        }
        profile.set_as_default = input.set_as_default;

        let token = input
            .token
            .as_ref()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());

        if let Some(ref token) = token {
            if let Err(err) = secret_store::save_token(profile.id, token) {
                let mut guard = self.inner.lock().expect("store lock");
                guard.status = err.to_string();
                return guard.to_result(None);
            }
            profile.token_saved = Some(true);
        }

        let id = profile.id;
        let enable = input.enable && token.is_some();

        {
            let mut guard = self.inner.lock().expect("store lock");
            guard.state.selected_id = Some(id);
            guard.state.profiles.push(profile);
            persist(&guard.state);
            guard.status = "已添加供应商。".into();
            guard.refresh_discovered_quietly();
        }

        if enable {
            return self.apply_token(Some(id), token);
        }

        self.snapshot()
    }

    pub fn remove_profile(&self, id: Uuid) -> CommandResult {
        let mut guard = self.inner.lock().expect("store lock");
        if guard.state.current_id == Some(id) {
            guard.status = "不能删除当前已启用的供应商。请先启用其它供应商。".into();
            return guard.to_result(None);
        }
        if let Err(err) = secret_store::delete_token(id) {
            guard.status = err.to_string();
            return guard.to_result(None);
        }
        guard.state.profiles.retain(|p| p.id != id);
        if guard.state.profiles.is_empty() {
            let profile = TokenProfile::new("新供应商");
            guard.state.selected_id = Some(profile.id);
            guard.state.profiles.push(profile);
            guard.state.current_id = None;
        } else if guard.state.selected_id == Some(id) {
            guard.state.selected_id = guard.state.profiles.first().map(|p| p.id);
        }
        persist(&guard.state);
        guard.status = "已删除供应商及其本地保存的 Token。".into();
        guard.refresh_discovered_quietly();
        guard.to_result(None)
    }

    /// Import `[model.*]` sections from live config as providers (skip existing model IDs).
    pub fn import_from_config(&self, config_path: Option<String>) -> CommandResult {
        let mut guard = self.inner.lock().expect("store lock");
        let path = config_path
            .or_else(|| guard.selected().map(|p| p.config_path.clone()))
            .unwrap_or_else(default_config_path);
        let snap = read_config_snapshot(&path);
        guard.discovered = snap.clone();

        let existing: std::collections::HashSet<String> = guard
            .state
            .profiles
            .iter()
            .map(|p| p.model_id.clone())
            .collect();

        let mut added = 0usize;
        for model in snap.models {
            if existing.contains(&model.id) {
                continue;
            }
            let mut profile = TokenProfile::new(
                model
                    .name
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| model.id.clone()),
            );
            profile.model_id = model.id;
            profile.api_model = model.model;
            profile.model_alias = model.name;
            profile.description = model.description;
            profile.base_url = model.base_url;
            profile.env_key = model.env_key;
            profile.api_backend = model.api_backend;
            profile.context_window = model.context_window;
            profile.max_completion_tokens = model.max_completion_tokens;
            profile.config_path = path.clone();
            profile.set_as_default = snap.default_model_id.as_ref() == Some(&profile.model_id);
            // token_saved = local vault only. config.toml may have api_key, but we
            // never auto-import the raw secret into the vault.
            profile.token_saved = Some(secret_store::has_token(profile.id));
            // Note: user re-enters token if they want to re-apply / test connectivity.
            guard.state.profiles.push(profile);
            added += 1;
        }

        if guard.state.current_id.is_none() {
            if let Some(default_id) = &snap.default_model_id {
                if let Some(id) = guard
                    .state
                    .profiles
                    .iter()
                    .find(|p| &p.model_id == default_id)
                    .map(|p| p.id)
                {
                    guard.state.current_id = Some(id);
                }
            }
        }

        persist(&guard.state);
        guard.status = if added == 0 {
            "没有可导入的新模型段（可能都已存在）。".into()
        } else {
            format!("已从 config.toml 导入 {added} 个供应商。Token 需重新填写后才能启用。")
        };
        guard.to_result(None)
    }

    pub fn rename_profile(&self, id: Uuid, name: String) -> CommandResult {
        let mut guard = self.inner.lock().expect("store lock");
        let cleaned = name.trim().to_string();
        if cleaned.is_empty() {
            guard.status = "名称不能为空。".into();
            return guard.to_result(None);
        }
        if let Some(profile) = guard.state.profiles.iter_mut().find(|p| p.id == id) {
            profile.name = cleaned;
            touch(profile);
            persist(&guard.state);
            guard.status = "已重命名配置档。".into();
        }
        guard.to_result(None)
    }

    pub fn select_profile(&self, id: Uuid) -> CommandResult {
        let mut guard = self.inner.lock().expect("store lock");
        if guard.state.profiles.iter().any(|p| p.id == id) {
            guard.state.selected_id = Some(id);
            persist(&guard.state);
            guard.refresh_discovered_quietly();
            guard.status = "已切换配置档。".into();
        }
        guard.to_result(None)
    }

    pub fn update_profile(&self, patch: ProfilePatch) -> CommandResult {
        let mut guard = self.inner.lock().expect("store lock");
        let Some(id) = patch.id.or(guard.state.selected_id) else {
            guard.status = "没有选中的配置档。".into();
            return guard.to_result(None);
        };
        let Some(profile) = guard.state.profiles.iter_mut().find(|p| p.id == id) else {
            guard.status = "配置档不存在。".into();
            return guard.to_result(None);
        };

        if let Some(name) = patch.name {
            let cleaned = name.trim().to_string();
            if !cleaned.is_empty() {
                profile.name = cleaned;
            }
        }
        if let Some(model_id) = patch.model_id {
            let cleaned = model_id.trim().to_string();
            if !cleaned.is_empty() {
                profile.model_id = cleaned;
            }
        }
        if let Some(api_model) = patch.api_model {
            let cleaned = api_model.trim().to_string();
            profile.api_model = if cleaned.is_empty() {
                None
            } else {
                Some(cleaned)
            };
        }
        if let Some(model_alias) = patch.model_alias {
            profile.model_alias = normalize_opt(Some(model_alias));
        }
        if let Some(description) = patch.description {
            profile.description = normalize_opt(Some(description));
        }
        if let Some(base_url) = patch.base_url {
            profile.base_url = normalize_opt(Some(base_url));
        }
        if let Some(env_key) = patch.env_key {
            profile.env_key = normalize_opt(Some(env_key));
        }
        if let Some(api_backend) = patch.api_backend {
            profile.api_backend = normalize_opt(Some(api_backend));
        }
        if let Some(context_window) = patch.context_window {
            profile.context_window = if context_window == 0 {
                None
            } else {
                Some(context_window)
            };
        }
        if let Some(max_completion_tokens) = patch.max_completion_tokens {
            profile.max_completion_tokens = if max_completion_tokens == 0 {
                None
            } else {
                Some(max_completion_tokens)
            };
        }
        if let Some(set_as_default) = patch.set_as_default {
            profile.set_as_default = set_as_default;
        }
        let config_path_changed = patch.config_path.is_some();
        if let Some(config_path) = patch.config_path {
            let cleaned = config_path.trim().to_string();
            if !cleaned.is_empty() {
                profile.config_path = cleaned;
            }
        }
        touch(profile);
        persist(&guard.state);
        if config_path_changed {
            guard.refresh_discovered_quietly();
        }
        guard.status = "配置已更新。".into();
        guard.to_result(None)
    }

    pub fn read_config_file(&self, config_path: Option<String>) -> CommandResult {
        let mut guard = self.inner.lock().expect("store lock");
        let path = config_path
            .or_else(|| guard.selected().map(|p| p.config_path.clone()))
            .or_else(|| guard.state.profiles.first().map(|p| p.config_path.clone()))
            .unwrap_or_else(default_config_path);
        let expanded = expand_path(&path);
        match fs::read_to_string(&expanded) {
            Ok(text) => {
                guard.discovered = parse_grok_config(&text);
                guard.status = format!("已读取配置文件（{} 字节）。", text.len());
                let mut result = guard.to_result(None);
                result.config_text = Some(text);
                result.config_path = Some(expanded);
                result
            }
            Err(err) => {
                guard.status = format!("读取配置失败：{err}");
                let mut result = guard.to_result(None);
                result.config_text = Some(String::new());
                result.config_path = Some(expanded);
                result
            }
        }
    }

    pub fn write_config_file(
        &self,
        config_path: Option<String>,
        content: String,
    ) -> CommandResult {
        let mut guard = self.inner.lock().expect("store lock");
        let path = config_path
            .or_else(|| guard.selected().map(|p| p.config_path.clone()))
            .or_else(|| guard.state.profiles.first().map(|p| p.config_path.clone()))
            .unwrap_or_else(default_config_path);
        let expanded = expand_path(&path);

        if let Some(parent) = std::path::Path::new(&expanded).parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                guard.status = format!("无法创建配置目录：{err}");
                return guard.to_result(None);
            }
        }

        match fs::write(&expanded, &content) {
            Ok(()) => {
                guard.discovered = parse_grok_config(&content);
                guard.status = "已保存完整 config.toml。".into();
                let mut result = guard.to_result(None);
                result.config_text = Some(content);
                result.config_path = Some(expanded);
                result
            }
            Err(err) => {
                guard.status = format!("写入配置失败：{err}");
                guard.to_result(None)
            }
        }
    }

    pub fn save_token(&self, id: Option<Uuid>, token: String) -> CommandResult {
        let mut guard = self.inner.lock().expect("store lock");
        let Some(id) = id.or(guard.state.selected_id) else {
            guard.status = "没有选中的配置档。".into();
            return guard.to_result(None);
        };
        match secret_store::save_token(id, token.trim()) {
            Ok(()) => {
                if let Some(profile) = guard.state.profiles.iter_mut().find(|p| p.id == id) {
                    profile.token_saved = Some(!token.trim().is_empty());
                    touch(profile);
                }
                persist(&guard.state);
                guard.status = if token.trim().is_empty() {
                    "已从本地存储移除 Token。".into()
                } else {
                    "Token 已保存到本地（启用时才写入 config.toml）。".into()
                };
            }
            Err(err) => guard.status = err.to_string(),
        }
        guard.to_result(None)
    }

    pub fn load_token(&self, id: Option<Uuid>) -> CommandResult {
        let id = {
            let guard = self.inner.lock().expect("store lock");
            match id.or(guard.state.selected_id) {
                Some(id) => id,
                None => {
                    // Need mutable for status — re-lock briefly.
                    drop(guard);
                    let mut g = self.inner.lock().expect("store lock");
                    g.status = "没有选中的配置档。".into();
                    return g.to_result(None);
                }
            }
        };

        // Vault / Keychain outside the store mutex.
        let loaded = secret_store::load_token(id);

        let mut guard = self.inner.lock().expect("store lock");
        match loaded {
            Ok(Some(token)) if !token.is_empty() => {
                if let Some(profile) = guard.state.profiles.iter_mut().find(|p| p.id == id) {
                    profile.token_saved = Some(true);
                }
                guard.status = "已读取本地 Token。".into();
                guard.to_result(Some(token))
            }
            Ok(_) => {
                if let Some(profile) = guard.state.profiles.iter_mut().find(|p| p.id == id) {
                    profile.token_saved = Some(false);
                }
                guard.status = "本地没有此供应商的 Token。".into();
                guard.to_result(Some(String::new()))
            }
            Err(err) => {
                guard.status = err.to_string();
                guard.to_result(None)
            }
        }
    }

    pub fn apply_token(&self, id: Option<Uuid>, draft_token: Option<String>) -> CommandResult {
        // Phase 1: resolve profile under a short lock (no disk I/O while holding it).
        let (id, profile) = {
            let mut guard = self.inner.lock().expect("store lock");
            let Some(id) = id.or(guard.state.selected_id) else {
                guard.status = "没有选中的配置档。".into();
                return guard.to_result(None);
            };
            let Some(profile) = guard.state.profiles.iter().find(|p| p.id == id).cloned() else {
                guard.status = "配置档不存在。".into();
                return guard.to_result(None);
            };
            guard.status = format!("正在启用「{}」…", profile.name);
            (id, profile)
        };

        // Phase 2: token + config file I/O off the mutex so the UI can keep painting.
        let token = draft_token
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .or_else(|| secret_store::load_token(id).ok().flatten())
            .unwrap_or_default();
        let token = token.trim().to_string();
        if token.is_empty() {
            let mut guard = self.inner.lock().expect("store lock");
            guard.status = "请先输入并保存 API Token。".into();
            return guard.to_result(None);
        }

        let path = expand_path(&profile.config_path);
        if path.is_empty() {
            let mut guard = self.inner.lock().expect("store lock");
            guard.status = "请填写 Grok 配置文件路径。".into();
            return guard.to_result(None);
        }

        if let Some(parent) = std::path::Path::new(&path).parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                let mut guard = self.inner.lock().expect("store lock");
                guard.status = format!("无法创建配置目录：{err}");
                return guard.to_result(None);
            }
        }

        let existing = fs::read_to_string(&path).unwrap_or_default();
        let upsert = model_upsert_from_profile(&profile, Some(token.clone()));
        let content = upsert_model_config(&existing, &upsert);
        let write_result = fs::write(&path, content);
        let discovered = if write_result.is_ok() {
            let _ = secret_store::save_token(id, &token);
            Some(read_config_snapshot(&path))
        } else {
            None
        };

        // Phase 3: commit in-memory state under a short lock.
        let mut guard = self.inner.lock().expect("store lock");
        match write_result {
            Ok(()) => {
                if let Some(p) = guard.state.profiles.iter_mut().find(|p| p.id == id) {
                    p.token_saved = Some(true);
                    touch(p);
                }
                guard.state.current_id = Some(id);
                guard.state.selected_id = Some(id);
                persist(&guard.state);
                if let Some(snap) = discovered {
                    guard.discovered = snap;
                }
                let default_note = if profile.set_as_default {
                    format!(" 已设为默认模型（{}）。", profile.model_id)
                } else {
                    String::new()
                };
                guard.status = format!(
                    "已启用 {}。已写入 config.toml。{}",
                    profile.name, default_note
                );
            }
            Err(err) => guard.status = format!("写入失败：{err}"),
        }
        guard.to_result(None)
    }

    pub fn refresh_config(&self, config_path: Option<String>, quietly: bool) -> CommandResult {
        let mut guard = self.inner.lock().expect("store lock");
        let path = config_path
            .or_else(|| guard.selected().map(|p| p.config_path.clone()))
            .unwrap_or_else(default_config_path);
        let expanded = expand_path(&path);
        match fs::read_to_string(&expanded) {
            Ok(content) => {
                guard.discovered = parse_grok_config(&content);
                if !quietly {
                    guard.status = format!(
                        "已读取 {} 个模型{}",
                        guard.discovered.models.len(),
                        guard
                            .discovered
                            .default_model_id
                            .as_ref()
                            .map(|d| format!("，默认：{d}"))
                            .unwrap_or_default()
                    );
                }
            }
            Err(_) => {
                guard.discovered = ConfigSnapshot {
                    models: Vec::new(),
                    default_model_id: None,
                };
                if !quietly {
                    guard.status = format!("未找到配置文件：{expanded}");
                }
            }
        }
        guard.to_result(None)
    }

    /// Probe OpenAI-compatible `{base_url}/models` with the provider's API key.
    pub fn test_connectivity(&self, id: Uuid) -> CommandResult {
        let health = self.check_health(id);
        let mut guard = self.inner.lock().expect("store lock");
        guard.status = health.status_line();
        guard.to_result(None)
    }

    /// Structured health check with explainable failure categories.
    pub fn check_health(&self, id: Uuid) -> HealthResult {
        // 1) Snapshot profile under lock (no I/O).
        let (name, base_url) = {
            let guard = self.inner.lock().expect("store lock");
            let Some(profile) = guard.state.profiles.iter().find(|p| p.id == id) else {
                return HealthResult {
                    profile_id: id.to_string(),
                    name: "未知".into(),
                    ok: false,
                    category: "config".into(),
                    title: "供应商不存在".into(),
                    detail: "配置档已删除或不存在".into(),
                    hint: "刷新列表后重试。".into(),
                    latency_ms: None,
                    status_code: None,
                    url: None,
                    checked_at: 0,
                };
            };
            (profile.name.clone(), profile.base_url.clone())
        };

        // 2) Vault I/O outside store lock — never block list/get_state.
        let token = secret_store::load_token(id)
            .ok()
            .flatten()
            .unwrap_or_default()
            .trim()
            .to_string();
        let token = if token.is_empty() {
            None
        } else {
            Some(token)
        };

        {
            let mut guard = self.inner.lock().expect("store lock");
            if let Some(p) = guard.state.profiles.iter_mut().find(|p| p.id == id) {
                p.token_saved = Some(token.is_some());
            }
            // No persist here: token_saved is a derived badge, not durable state.
            guard.status = format!("正在检查「{}」健康度…", name);
        }

        // 3) Network completely unlocked.
        let result = health::check_provider(
            &id.to_string(),
            &name,
            base_url.as_deref(),
            token.as_deref(),
        );

        let mut guard = self.inner.lock().expect("store lock");
        guard.health.insert(id, result.clone());
        guard.status = result.status_line();
        result
    }


    pub fn last_health(&self) -> Vec<HealthResult> {
        let guard = self.inner.lock().expect("store lock");
        let mut out: Vec<HealthResult> = guard.health.values().cloned().collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// One-click speed test: models ping + streaming TTFT + total latency + 403/CF detection.
    pub fn run_speed_test(&self, id: Uuid) -> SpeedTestResult {
        let (name, base_url, api_model, model_id, api_backend, token) = {
            let mut guard = self.inner.lock().expect("store lock");
            let Some(profile) = guard.state.profiles.iter().find(|p| p.id == id).cloned() else {
                return SpeedTestResult {
                    profile_id: id.to_string(),
                    name: "未知".into(),
                    ok: false,
                    category: "config".into(),
                    title: "供应商不存在".into(),
                    detail: "配置档已删除或不存在".into(),
                    hint: "刷新列表后重试。".into(),
                    models_ms: None,
                    ttft_ms: None,
                    total_ms: None,
                    status_code: None,
                    is_403: false,
                    is_cf_block: false,
                    backend: None,
                    model: None,
                    url: None,
                    preview: None,
                    streamed: false,
                    checked_at: 0,
                };
            };
            let token = secret_store::load_token(id)
                .ok()
                .flatten()
                .unwrap_or_default()
                .trim()
                .to_string();
            if let Some(p) = guard.state.profiles.iter_mut().find(|p| p.id == id) {
                p.token_saved = Some(!token.is_empty());
            }
            persist(&guard.state);
            guard.status = format!("正在测速「{}」…", profile.name);
            (
                profile.name,
                profile.base_url.clone(),
                profile.api_model.clone(),
                profile.model_id.clone(),
                profile.api_backend.clone(),
                if token.is_empty() { None } else { Some(token) },
            )
        };

        let result = speedtest::run_speed_test(
            &id.to_string(),
            &name,
            base_url.as_deref(),
            token.as_deref(),
            api_model.as_deref(),
            &model_id,
            api_backend.as_deref(),
        );

        let mut guard = self.inner.lock().expect("store lock");
        guard.speed.insert(id, result.clone());
        guard.status = result.status_line();
        result
    }

    pub fn last_speed_tests(&self) -> Vec<SpeedTestResult> {
        let guard = self.inner.lock().expect("store lock");
        let mut out: Vec<SpeedTestResult> = guard.speed.values().cloned().collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Summarize local Grok logs. Uses a short TTL cache unless `force`.
    pub fn usage_summary(&self, window_hours: Option<u32>, force: bool) -> UsageSummary {
        let hours = window_hours.unwrap_or(24).clamp(1, 24 * 30);
        if !force {
            if let Ok(guard) = self.inner.lock() {
                if let Some((cached_hours, at, ref summary)) = &guard.usage_cache {
                    if *cached_hours == hours && at.elapsed() < USAGE_CACHE_TTL {
                        return summary.clone();
                    }
                }
            }
        }

        let summary = usage::summarize_usage(hours);
        if let Ok(mut guard) = self.inner.lock() {
            guard.usage_cache = Some((hours, Instant::now(), summary.clone()));
        }
        summary
    }

    /// Return cached usage only — never scans logs. Safe for tray / main thread.
    pub fn usage_cached(&self, window_hours: Option<u32>) -> Option<UsageSummary> {
        let hours = window_hours.unwrap_or(24).clamp(1, 24 * 30);
        let guard = self.inner.lock().ok()?;
        match &guard.usage_cache {
            Some((cached_hours, _at, summary)) if *cached_hours == hours => Some(summary.clone()),
            _ => None,
        }
    }

    pub fn verify_grok(&self) -> CommandResult {
        {
            let mut guard = self.inner.lock().expect("store lock");
            if guard.busy {
                return guard.to_result(None);
            }
            guard.busy = true;
            guard.status = "正在执行 grok models...".into();
        }

        let output = run_grok_models();
        let mut guard = self.inner.lock().expect("store lock");
        guard.busy = false;
        let first = output
            .text
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        guard.status = if output.status == 0 {
            format!(
                "验证成功：{}",
                if first.is_empty() {
                    "grok models 已通过".into()
                } else {
                    first
                }
            )
        } else {
            format!(
                "验证失败：{}",
                if first.is_empty() {
                    "grok models 返回错误".into()
                } else {
                    first
                }
            )
        };
        guard.to_result(None)
    }

    pub fn fetch_available_models(&self) -> CommandResult {
        {
            let mut guard = self.inner.lock().expect("store lock");
            if guard.busy {
                return guard.to_result(None);
            }
            guard.busy = true;
            guard.status = "正在获取可用模型...".into();
        }

        let output = run_grok_models();
        let models: Vec<String> = output
            .text
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if !(trimmed.starts_with('-') || trimmed.starts_with('*')) {
                    return None;
                }
                let name = trimmed[1..].trim().replace(" (default)", "");
                if name.is_empty() {
                    None
                } else {
                    Some(name)
                }
            })
            .collect();

        let mut guard = self.inner.lock().expect("store lock");
        guard.busy = false;
        guard.available_model_ids = models.clone();
        guard.status = if output.status == 0 {
            format!("已获取 {} 个可用模型。", models.len())
        } else {
            let first = output
                .text
                .lines()
                .next()
                .unwrap_or("未知错误")
                .to_string();
            format!("获取模型失败：{first}")
        };
        guard.to_result(None)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilePatch {
    pub id: Option<Uuid>,
    pub name: Option<String>,
    pub model_id: Option<String>,
    pub api_model: Option<String>,
    pub model_alias: Option<String>,
    pub description: Option<String>,
    pub base_url: Option<String>,
    pub env_key: Option<String>,
    pub api_backend: Option<String>,
    pub context_window: Option<u64>,
    pub max_completion_tokens: Option<u64>,
    pub set_as_default: Option<bool>,
    pub config_path: Option<String>,
}

impl Inner {
    fn selected(&self) -> Option<&TokenProfile> {
        let id = self.state.selected_id?;
        self.state.profiles.iter().find(|p| p.id == id)
    }

    fn refresh_discovered_quietly(&mut self) {
        let path = self
            .selected()
            .map(|p| p.config_path.clone())
            .unwrap_or_else(default_config_path);
        self.discovered = read_config_snapshot(&path);
    }

    fn resolved_config_path(&self) -> String {
        self.selected()
            .map(|p| p.config_path.clone())
            .or_else(|| self.state.profiles.first().map(|p| p.config_path.clone()))
            .unwrap_or_else(default_config_path)
    }

    /// Fast path for the main list / tray — no config file read, no vault re-walk.
    fn to_list_result(&self) -> CommandResult {
        let path = expand_path(&self.resolved_config_path());
        CommandResult {
            status: self.status.clone(),
            profiles: self.state.profiles.clone(),
            selected_id: self.state.selected_id,
            current_id: self.state.current_id,
            discovered_models: self.discovered.models.clone(),
            default_model_id: self.discovered.default_model_id.clone(),
            available_model_ids: self.available_model_ids.clone(),
            preview: None,
            token: None,
            config_text: None,
            config_path: Some(path),
            busy: self.busy,
        }
    }

    fn to_result(&self, token: Option<String>) -> CommandResult {
        // Refresh key badges from the in-memory vault only (never Keychain).
        let mut profiles = self.state.profiles.clone();
        for profile in &mut profiles {
            profile.token_saved = Some(secret_store::has_token(profile.id));
        }

        let preview = self.selected().map(|profile| {
            let upsert = model_upsert_from_profile(profile, None);
            build_preview(&profile.config_path, &upsert, &self.discovered)
        });
        let path = expand_path(&self.resolved_config_path());
        // config_text is heavy and only needed by the raw editor — omit by default.
        // Callers that need it use read_config_file.

        CommandResult {
            status: self.status.clone(),
            profiles,
            selected_id: self.state.selected_id,
            current_id: self.state.current_id,
            discovered_models: self.discovered.models.clone(),
            default_model_id: self.discovered.default_model_id.clone(),
            available_model_ids: self.available_model_ids.clone(),
            preview,
            token,
            config_text: None,
            config_path: Some(path),
            busy: self.busy,
        }
    }
}

fn normalize_opt(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn model_upsert_from_profile(profile: &TokenProfile, api_key: Option<String>) -> ModelUpsert {
    // Prefer explicit TOML display name; fall back to local provider name.
    let display_name = profile
        .model_alias
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let n = profile.name.trim();
            if n.is_empty() {
                None
            } else {
                Some(n.to_string())
            }
        });

    ModelUpsert {
        model_id: profile.model_id.trim().to_string(),
        api_key,
        base_url: profile.base_url.clone(),
        api_model: profile.api_model.clone(),
        display_name,
        description: profile.description.clone(),
        env_key: profile.env_key.clone(),
        api_backend: profile.api_backend.clone(),
        context_window: profile.context_window,
        max_completion_tokens: profile.max_completion_tokens,
        set_as_default: profile.set_as_default,
    }
}

fn default_status_message() -> String {
    "Token 保存在本地应用数据中；点「启用」才会写入 Grok 的 config.toml。".into()
}

fn default_config_path() -> String {
    dirs::home_dir()
        .map(|h| h.join(".grok").join("config.toml").to_string_lossy().into_owned())
        .unwrap_or_else(|| "~/.grok/config.toml".into())
}

fn profiles_path() -> PathBuf {
    let base = dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("GrokTokenSwitcher").join("profiles.json")
}

fn load_or_default() -> AppState {
    let path = profiles_path();
    if let Ok(raw) = fs::read_to_string(&path) {
        if let Ok(state) = serde_json::from_str::<AppState>(&raw) {
            if !state.profiles.is_empty() {
                return state;
            }
        }
        // Backward-compatible: older Swift used a bare array of profiles.
        if let Ok(profiles) = serde_json::from_str::<Vec<TokenProfile>>(&raw) {
            if !profiles.is_empty() {
                let selected_id = profiles.first().map(|p| p.id);
                return AppState {
                    profiles,
                    selected_id,
                    current_id: None,
                };
            }
        }
    }
    AppState::default()
}

fn persist(state: &AppState) {
    let path = profiles_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string_pretty(state) {
        let _ = fs::write(path, raw);
    }
}

fn refresh_token_flags(state: &mut AppState) {
    for profile in &mut state.profiles {
        profile.token_saved = Some(secret_store::has_token(profile.id));
    }
}

fn read_config_snapshot(config_path: &str) -> ConfigSnapshot {
    let path = expand_path(config_path);
    match fs::read_to_string(path) {
        Ok(content) => parse_grok_config(&content),
        Err(_) => ConfigSnapshot {
            models: Vec::new(),
            default_model_id: None,
        },
    }
}

fn touch(profile: &mut TokenProfile) {
    use std::time::{SystemTime, UNIX_EPOCH};
    profile.updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
}

struct GrokOutput {
    status: i32,
    text: String,
}

fn run_grok_models() -> GrokOutput {
    match Command::new("grok").arg("models").output() {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            if !output.stderr.is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&String::from_utf8_lossy(&output.stderr));
            }
            GrokOutput {
                status: output.status.code().unwrap_or(-1),
                text,
            }
        }
        Err(err) => GrokOutput {
            status: -1,
            text: format!("无法启动 grok：{err}"),
        },
    }
}
