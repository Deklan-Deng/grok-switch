use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageError {
    pub at: i64,
    pub message: String,
    pub model: Option<String>,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub model: String,
    pub calls: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub reasoning_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub window_hours: u32,
    pub total_calls: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub reasoning_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
    pub avg_tokens_per_sec: Option<f64>,
    pub avg_latency_ms: Option<f64>,
    pub error_count: u64,
    pub rate_limit_count: u64,
    pub recent_errors: Vec<UsageError>,
    pub by_model: Vec<ModelUsage>,
    pub source: String,
    pub updated_at: i64,
    pub has_data: bool,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn home_grok_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".grok")
}

fn unified_log_path() -> PathBuf {
    home_grok_dir().join("logs").join("unified.jsonl")
}

/// Parse ISO-8601-ish timestamps used by Grok logs into epoch ms.
fn parse_ts_ms(raw: &str) -> Option<i64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    // 2026-07-14T08:51:35.612Z or with offset
    let s = s.trim_end_matches('Z');
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let y: i32 = d.next()?.parse().ok()?;
    let mo: u32 = d.next()?.parse().ok()?;
    let day: u32 = d.next()?.parse().ok()?;

    let time = time.split(['+', '-']).next().unwrap_or(time);
    let mut t = time.split(':');
    let h: u32 = t.next()?.parse().ok()?;
    let mi: u32 = t.next()?.parse().ok()?;
    let sec_part = t.next().unwrap_or("0");
    let (sec_s, frac) = sec_part
        .split_once('.')
        .map(|(a, b)| (a, b))
        .unwrap_or((sec_part, "0"));
    let sec: u32 = sec_s.parse().ok()?;
    let mut frac = frac.chars().filter(|c| c.is_ascii_digit()).collect::<String>();
    while frac.len() < 3 {
        frac.push('0');
    }
    let ms: u32 = frac.chars().take(3).collect::<String>().parse().ok()?;

    // Days from civil date (Howard Hinnant algorithm) → unix days
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u32;
    let mp = if mo > 2 { mo - 3 } else { mo + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = (era as i64) * 146097 + doe as i64 - 719468;
    let secs = days * 86400 + (h as i64) * 3600 + (mi as i64) * 60 + sec as i64;
    Some(secs * 1000 + ms as i64)
}

fn read_tail_lines(path: &Path, max_bytes: u64) -> Vec<String> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if len == 0 {
        return Vec::new();
    }
    let start = len.saturating_sub(max_bytes);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = String::new();
    if file.read_to_string(&mut buf).is_err() {
        return Vec::new();
    }
    // If we started mid-line, drop the partial first line.
    let body = if start > 0 {
        buf.split_once('\n').map(|(_, rest)| rest).unwrap_or(&buf)
    } else {
        buf.as_str()
    };
    body.lines().map(|l| l.to_string()).collect()
}

/// Best-effort sid → model from recent session event files.
fn load_session_models(limit_files: usize) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let root = home_grok_dir().join("sessions");
    let Ok(projects) = fs::read_dir(&root) else {
        return map;
    };

    let mut event_files: Vec<(i64, PathBuf)> = Vec::new();
    for project in projects.flatten() {
        let p = project.path();
        if !p.is_dir() {
            continue;
        }
        let Ok(sessions) = fs::read_dir(&p) else {
            continue;
        };
        for session in sessions.flatten() {
            let events = session.path().join("events.jsonl");
            if !events.is_file() {
                continue;
            }
            let mtime = events
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            event_files.push((mtime, events));
        }
    }
    event_files.sort_by(|a, b| b.0.cmp(&a.0));
    event_files.truncate(limit_files);

    for (_, path) in event_files {
        let Ok(file) = File::open(&path) else {
            continue;
        };
        for line in BufReader::new(file).lines().flatten() {
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let sid = v
                .get("session_id")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let model = v
                .get("model_id")
                .or_else(|| v.get("model"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if !sid.is_empty() && !model.is_empty() {
                map.insert(sid.to_string(), model.to_string());
            }
        }
    }
    map
}

fn ctx_model(ctx: &Value) -> Option<String> {
    for key in ["model", "model_id", "current_model_id", "api_model"] {
        if let Some(m) = ctx.get(key).and_then(|x| x.as_str()) {
            let m = m.trim();
            if !m.is_empty() {
                return Some(m.to_string());
            }
        }
    }
    None
}

fn as_u64(v: Option<&Value>) -> u64 {
    match v {
        Some(Value::Number(n)) => n
            .as_u64()
            .or_else(|| n.as_f64().map(|f| f.max(0.0) as u64))
            .unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn as_f64(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

/// Summarize Grok usage from local logs for the last `window_hours`.
pub fn summarize_usage(window_hours: u32) -> UsageSummary {
    let updated_at = now_ms();
    let window_hours = window_hours.clamp(1, 24 * 30);
    let cutoff = updated_at.saturating_sub((window_hours as i64) * 3600 * 1000);
    let path = unified_log_path();
    let source = path.to_string_lossy().into_owned();

    if !path.exists() {
        return UsageSummary {
            window_hours,
            total_calls: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            reasoning_tokens: 0,
            cached_prompt_tokens: 0,
            total_tokens: 0,
            avg_tokens_per_sec: None,
            avg_latency_ms: None,
            error_count: 0,
            rate_limit_count: 0,
            recent_errors: Vec::new(),
            by_model: Vec::new(),
            source,
            updated_at,
            has_data: false,
        };
    }

    let sid_models = load_session_models(40);
    let lines = read_tail_lines(&path, MAX_LOG_BYTES);

    // Track latest known model per sid while scanning chronologically.
    let mut live_sid_model: HashMap<String, String> = sid_models;
    let mut by_model: HashMap<String, ModelUsage> = HashMap::new();

    let mut total_calls = 0u64;
    let mut prompt_tokens = 0u64;
    let mut completion_tokens = 0u64;
    let mut reasoning_tokens = 0u64;
    let mut cached_prompt_tokens = 0u64;
    let mut tps_sum = 0.0f64;
    let mut tps_n = 0u64;
    let mut lat_sum = 0.0f64;
    let mut lat_n = 0u64;
    let mut error_count = 0u64;
    let mut rate_limit_count = 0u64;
    let mut recent_errors: Vec<UsageError> = Vec::new();

    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let ts = v
            .get("ts")
            .and_then(|x| x.as_str())
            .and_then(parse_ts_ms)
            .unwrap_or(0);
        if ts > 0 && ts < cutoff {
            continue;
        }

        let msg = v.get("msg").and_then(|x| x.as_str()).unwrap_or("");
        let lvl = v.get("lvl").and_then(|x| x.as_str()).unwrap_or("");
        let sid = v.get("sid").and_then(|x| x.as_str()).unwrap_or("");
        let ctx = v.get("ctx").cloned().unwrap_or(Value::Null);

        if let Some(model) = ctx_model(&ctx) {
            if !sid.is_empty() {
                live_sid_model.insert(sid.to_string(), model);
            }
        }

        if msg == "shell.turn.inference_done" {
            let p = as_u64(ctx.get("prompt_tokens"));
            let c = as_u64(ctx.get("completion_tokens"));
            let r = as_u64(ctx.get("reasoning_tokens"));
            let cached = as_u64(ctx.get("cached_prompt_tokens"));
            let model = ctx_model(&ctx)
                .or_else(|| {
                    if sid.is_empty() {
                        None
                    } else {
                        live_sid_model.get(sid).cloned()
                    }
                })
                .unwrap_or_else(|| "unknown".into());

            total_calls += 1;
            prompt_tokens += p;
            completion_tokens += c;
            reasoning_tokens += r;
            cached_prompt_tokens += cached;

            if let Some(tps) = as_f64(ctx.get("tokens_per_sec")) {
                tps_sum += tps;
                tps_n += 1;
            }
            if let Some(lat) = as_f64(ctx.get("model_elapsed_ms")) {
                lat_sum += lat;
                lat_n += 1;
            }

            let entry = by_model.entry(model.clone()).or_insert_with(|| ModelUsage {
                model,
                calls: 0,
                prompt_tokens: 0,
                completion_tokens: 0,
                reasoning_tokens: 0,
            });
            entry.calls += 1;
            entry.prompt_tokens += p;
            entry.completion_tokens += c;
            entry.reasoning_tokens += r;
            continue;
        }

        let body_hint = ctx.to_string();
        let is_rate = msg.to_lowercase().contains("rate_limit")
            || body_hint.to_lowercase().contains("rate_limit")
            || body_hint.contains("429")
            || body_hint.contains("请求过于频繁");
        let is_api_err = lvl == "error"
            || msg.to_lowercase().contains("api error")
            || msg.to_lowercase().contains("auth recovery")
            || body_hint.contains("status 401")
            || body_hint.contains("status 403");

        if is_rate || is_api_err {
            error_count += 1;
            if is_rate {
                rate_limit_count += 1;
            }
            let model = ctx_model(&ctx).or_else(|| {
                if sid.is_empty() {
                    None
                } else {
                    live_sid_model.get(sid).cloned()
                }
            });
            let message = if msg.is_empty() {
                truncate_msg(&body_hint, 160)
            } else {
                let extra = first_useful_error_text(&ctx);
                if extra.is_empty() {
                    msg.to_string()
                } else {
                    format!("{msg} · {extra}")
                }
            };
            recent_errors.push(UsageError {
                at: ts,
                message,
                model,
                kind: if is_rate {
                    "rate_limit".into()
                } else {
                    "error".into()
                },
            });
        }
    }

    // Keep newest errors only.
    recent_errors.sort_by(|a, b| b.at.cmp(&a.at));
    recent_errors.truncate(8);

    let mut by_model: Vec<ModelUsage> = by_model.into_values().collect();
    by_model.sort_by(|a, b| b.calls.cmp(&a.calls));
    by_model.truncate(8);

    let total_tokens = prompt_tokens + completion_tokens + reasoning_tokens;
    let has_data = total_calls > 0 || error_count > 0;

    UsageSummary {
        window_hours,
        total_calls,
        prompt_tokens,
        completion_tokens,
        reasoning_tokens,
        cached_prompt_tokens,
        total_tokens,
        avg_tokens_per_sec: if tps_n > 0 {
            Some(tps_sum / tps_n as f64)
        } else {
            None
        },
        avg_latency_ms: if lat_n > 0 {
            Some(lat_sum / lat_n as f64)
        } else {
            None
        },
        error_count,
        rate_limit_count,
        recent_errors,
        by_model,
        source,
        updated_at,
        has_data,
    }
}

fn first_useful_error_text(ctx: &Value) -> String {
    for key in ["error", "message", "detail", "reason", "body"] {
        if let Some(s) = ctx.get(key).and_then(|x| x.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return truncate_msg(t, 120);
            }
        }
    }
    // Flatten nested error.message
    if let Some(s) = ctx
        .pointer("/error/message")
        .and_then(|x| x.as_str())
    {
        return truncate_msg(s, 120);
    }
    String::new()
}

fn truncate_msg(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    let count = s.chars().count();
    if count <= max {
        s
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}
