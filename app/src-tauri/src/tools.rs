use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config_toml::{expand_path, parse_grok_config};
use crate::secret_store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckItem {
    pub id: String,
    pub ok: bool,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub checks: Vec<CheckItem>,
    pub ok_count: usize,
    pub total: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokSessionItem {
    pub id: String,
    pub cwd: String,
    pub cwd_label: String,
    pub path: String,
    pub updated_at: i64,
    pub last_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickAskResult {
    pub ok: bool,
    pub command: String,
    pub output: String,
    pub elapsed_ms: u128,
}

fn home_grok_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".grok")
}

fn default_config_path() -> String {
    home_grok_dir()
        .join("config.toml")
        .to_string_lossy()
        .into_owned()
}

fn which_grok() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("GROK_BINARY") {
        let p = PathBuf::from(custom);
        if p.exists() {
            return Some(p);
        }
    }
    let home = home_grok_dir().join("bin").join("grok");
    if home.exists() {
        return Some(home);
    }
    // Fall back to PATH lookup.
    Command::new("/usr/bin/env")
        .args(["which", "grok"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(s))
                }
            } else {
                None
            }
        })
}

fn grok_version(bin: &Path) -> Option<String> {
    Command::new(bin)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
}

fn decode_session_cwd(encoded: &str) -> String {
    // Session dirs are percent-encoded paths, e.g. %2FUsers%2Fkang%2FProject
    let mut out = String::new();
    let bytes = encoded.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &encoded[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn short_path(path: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let home_s = home.to_string_lossy();
        if let Some(rest) = path.strip_prefix(home_s.as_ref()) {
            return format!("~{rest}");
        }
    }
    path.to_string()
}

fn file_mtime_ms(path: &Path) -> i64 {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn last_prompt_from_history(history: &Path) -> Option<String> {
    let text = fs::read_to_string(history).ok()?;
    let mut last: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if let Some(p) = v.get("prompt").and_then(|x| x.as_str()) {
                let cleaned = p.replace('\n', " ").trim().to_string();
                if !cleaned.is_empty() {
                    last = Some(cleaned);
                }
            }
        }
    }
    last.map(|s| {
        if s.chars().count() > 120 {
            let cut: String = s.chars().take(120).collect();
            format!("{cut}…")
        } else {
            s
        }
    })
}

pub fn doctor(config_path: Option<&str>) -> DoctorReport {
    let mut checks = Vec::new();

    let bin = which_grok();
    checks.push(match &bin {
        Some(p) => CheckItem {
            id: "binary".into(),
            ok: true,
            title: "Grok CLI 已安装".into(),
            detail: p.to_string_lossy().into_owned(),
        },
        None => CheckItem {
            id: "binary".into(),
            ok: false,
            title: "未找到 grok 命令".into(),
            detail: "请安装 Grok Build CLI，或设置 GROK_BINARY 环境变量。".into(),
        },
    });

    if let Some(p) = &bin {
        checks.push(match grok_version(p) {
            Some(v) => CheckItem {
                id: "version".into(),
                ok: true,
                title: "CLI 版本".into(),
                detail: v,
            },
            None => CheckItem {
                id: "version".into(),
                ok: false,
                title: "无法读取版本".into(),
                detail: "grok --version 执行失败。".into(),
            },
        });
    }

    let cfg = expand_path(config_path.unwrap_or(&default_config_path()));
    let cfg_path = PathBuf::from(&cfg);
    checks.push(if cfg_path.exists() {
        CheckItem {
            id: "config".into(),
            ok: true,
            title: "配置文件存在".into(),
            detail: cfg.clone(),
        }
    } else {
        CheckItem {
            id: "config".into(),
            ok: false,
            title: "配置文件不存在".into(),
            detail: format!("未找到 {cfg}"),
        }
    });

    if let Ok(content) = fs::read_to_string(&cfg_path) {
        let snap = parse_grok_config(&content);
        checks.push(CheckItem {
            id: "models".into(),
            ok: !snap.models.is_empty(),
            title: format!("自定义模型 {} 个", snap.models.len()),
            detail: snap
                .default_model_id
                .as_ref()
                .map(|d| format!("默认：{d}"))
                .unwrap_or_else(|| "未设置 [models].default".into()),
        });
        for m in &snap.models {
            let has_key = m.has_api_key;
            checks.push(CheckItem {
                id: format!("model.{}", m.id),
                ok: has_key || m.env_key.is_some(),
                title: format!("模型 {}", m.id),
                detail: format!(
                    "{}{}{}",
                    m.base_url
                        .as_ref()
                        .map(|u| format!("base_url={u}"))
                        .unwrap_or_else(|| "无 base_url".into()),
                    if has_key {
                        " · 有 api_key"
                    } else {
                        " · 无 api_key"
                    },
                    m.env_key
                        .as_ref()
                        .map(|e| format!(" · env_key={e}"))
                        .unwrap_or_default()
                ),
            });
        }
    }

    let sessions_dir = home_grok_dir().join("sessions");
    let session_count = fs::read_dir(&sessions_dir)
        .map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).count())
        .unwrap_or(0);
    checks.push(CheckItem {
        id: "sessions".into(),
        ok: sessions_dir.exists(),
        title: format!("会话目录 · {session_count} 个项目"),
        detail: sessions_dir.to_string_lossy().into_owned(),
    });

    let ok_count = checks.iter().filter(|c| c.ok).count();
    let total = checks.len();
    let summary = if ok_count == total {
        "环境正常，可以开始使用 Grok Build。".into()
    } else {
        format!("完成 {ok_count}/{total} 项检查，有项目需要处理。")
    };

    DoctorReport {
        checks,
        ok_count,
        total,
        summary,
    }
}

pub fn list_recent_sessions(limit: usize) -> Vec<GrokSessionItem> {
    let root = home_grok_dir().join("sessions");
    let mut items: Vec<GrokSessionItem> = Vec::new();

    let Ok(entries) = fs::read_dir(&root) else {
        return items;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let encoded = entry.file_name().to_string_lossy().into_owned();
        if !encoded.starts_with('%') && !encoded.contains("%2F") {
            // Still allow non-encoded names, but skip lock files etc.
            if encoded.starts_with('.') {
                continue;
            }
        }
        let cwd = decode_session_cwd(&encoded);
        if cwd.is_empty() || !cwd.starts_with('/') {
            continue;
        }

        let history = path.join("prompt_history.jsonl");
        let updated = if history.exists() {
            file_mtime_ms(&history)
        } else {
            file_mtime_ms(&path)
        };

        // Find newest session subdir id if present.
        let mut session_id = encoded.clone();
        if let Ok(rd) = fs::read_dir(&path) {
            let mut best_time: i64 = -1;
            let mut best_id: Option<String> = None;
            for sub in rd.flatten() {
                let sp = sub.path();
                if sp.is_dir() {
                    let name = sub.file_name().to_string_lossy().into_owned();
                    if name.starts_with("019") || name.len() > 20 {
                        let mt = file_mtime_ms(&sp);
                        if mt >= best_time {
                            best_time = mt;
                            best_id = Some(name);
                        }
                    }
                }
            }
            if let Some(id) = best_id {
                session_id = id;
            }
        }

        items.push(GrokSessionItem {
            id: session_id,
            cwd: cwd.clone(),
            cwd_label: short_path(&cwd),
            path: path.to_string_lossy().into_owned(),
            updated_at: updated,
            last_prompt: last_prompt_from_history(&history),
        });
    }

    items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    items.truncate(limit.max(1));
    items
}

pub fn open_path(path: &str) -> Result<String, String> {
    let expanded = expand_path(path);
    let p = PathBuf::from(&expanded);
    if !p.exists() {
        return Err(format!("路径不存在：{expanded}"));
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&expanded)
            .spawn()
            .map_err(|e| format!("无法打开：{e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", &expanded])
            .spawn()
            .map_err(|e| format!("无法打开：{e}"))?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Command::new("xdg-open")
            .arg(&expanded)
            .spawn()
            .map_err(|e| format!("无法打开：{e}"))?;
    }

    Ok(format!("已打开 {expanded}"))
}

pub fn open_config_dir() -> Result<String, String> {
    open_path(&home_grok_dir().to_string_lossy())
}

pub fn open_config_file(config_path: Option<&str>) -> Result<String, String> {
    let path = expand_path(config_path.unwrap_or(&default_config_path()));
    open_path(&path)
}

/// Launch interactive Grok in Terminal at cwd, optionally with -m model.
pub fn launch_grok_terminal(cwd: Option<&str>, model: Option<&str>) -> Result<String, String> {
    let workdir = expand_path(cwd.unwrap_or("~"));
    if !Path::new(&workdir).is_dir() {
        return Err(format!("工作目录不存在：{workdir}"));
    }

    let bin = which_grok().ok_or_else(|| "未找到 grok 命令".to_string())?;
    let mut cmd = format!("cd {} && {}", shell_quote(&workdir), shell_quote(&bin.to_string_lossy()));
    if let Some(m) = model.map(str::trim).filter(|s| !s.is_empty()) {
        cmd.push_str(" -m ");
        cmd.push_str(&shell_quote(m));
    }

    #[cfg(target_os = "macos")]
    {
        // Open Terminal.app and run the command.
        let script = format!(
            "tell application \"Terminal\"\nactivate\ndo script {}\nend tell",
            apple_script_string(&cmd)
        );
        Command::new("osascript")
            .args(["-e", &script])
            .spawn()
            .map_err(|e| format!("无法启动 Terminal：{e}"))?;
        return Ok(format!("已在 Terminal 打开 Grok · {workdir}"));
    }

    #[cfg(not(target_os = "macos"))]
    {
        Command::new(&bin)
            .current_dir(&workdir)
            .args(model.map(|m| vec!["-m", m]).unwrap_or_default())
            .spawn()
            .map_err(|e| format!("无法启动 grok：{e}"))?;
        Ok(format!("已启动 Grok · {workdir}"))
    }
}

/// Resume most recent session for a cwd in Terminal.
pub fn resume_session_terminal(cwd: &str) -> Result<String, String> {
    let workdir = expand_path(cwd);
    if !Path::new(&workdir).is_dir() {
        return Err(format!("工作目录不存在：{workdir}"));
    }
    let bin = which_grok().ok_or_else(|| "未找到 grok 命令".to_string())?;
    let cmd = format!(
        "cd {} && {} --continue",
        shell_quote(&workdir),
        shell_quote(&bin.to_string_lossy())
    );

    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "tell application \"Terminal\"\nactivate\ndo script {}\nend tell",
            apple_script_string(&cmd)
        );
        Command::new("osascript")
            .args(["-e", &script])
            .spawn()
            .map_err(|e| format!("无法启动 Terminal：{e}"))?;
        return Ok(format!("已继续最近会话 · {}", short_path(&workdir)));
    }

    #[cfg(not(target_os = "macos"))]
    {
        Command::new(&bin)
            .current_dir(&workdir)
            .arg("--continue")
            .spawn()
            .map_err(|e| format!("无法启动 grok：{e}"))?;
        Ok(format!("已继续最近会话 · {}", short_path(&workdir)))
    }
}

/// One-shot ask via `grok -p` (headless).
pub fn quick_ask(prompt: &str, model: Option<&str>, cwd: Option<&str>) -> QuickAskResult {
    let prompt = prompt.trim();
    let workdir = expand_path(cwd.unwrap_or("~"));
    let bin = match which_grok() {
        Some(b) => b,
        None => {
            return QuickAskResult {
                ok: false,
                command: "grok -p …".into(),
                output: "未找到 grok 命令".into(),
                elapsed_ms: 0,
            };
        }
    };

    let mut args: Vec<String> = vec!["-p".into(), prompt.into()];
    if let Some(m) = model.map(str::trim).filter(|s| !s.is_empty()) {
        args.push("-m".into());
        args.push(m.into());
    }

    let command = format!(
        "{} {}",
        bin.file_name().and_then(|s| s.to_str()).unwrap_or("grok"),
        args.iter()
            .map(|a| shell_quote(a))
            .collect::<Vec<_>>()
            .join(" ")
    );

    let started = SystemTime::now();
    let output = Command::new(&bin)
        .args(&args)
        .current_dir(&workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let elapsed_ms = started
        .elapsed()
        .map(|d| d.as_millis())
        .unwrap_or(0);

    match output {
        Ok(o) => {
            let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
            if !o.stderr.is_empty() {
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                text.push_str(&String::from_utf8_lossy(&o.stderr));
            }
            if text.trim().is_empty() {
                text = if o.status.success() {
                    "(无输出)".into()
                } else {
                    format!("命令失败，退出码 {:?}", o.status.code())
                };
            }
            QuickAskResult {
                ok: o.status.success(),
                command,
                output: text,
                elapsed_ms,
            }
        }
        Err(e) => QuickAskResult {
            ok: false,
            command,
            output: format!("无法启动 grok：{e}"),
            elapsed_ms,
        },
    }
}

pub fn copyable_commands(model: Option<&str>, cwd: Option<&str>) -> Vec<(String, String)> {
    let workdir = expand_path(cwd.unwrap_or("~"));
    let m = model.unwrap_or("").trim();
    let model_flag = if m.is_empty() {
        String::new()
    } else {
        format!(" -m {m}")
    };
    vec![
        (
            "交互式启动".into(),
            format!("cd {} && grok{}", shell_quote(&workdir), model_flag),
        ),
        (
            "继续最近会话".into(),
            format!("cd {} && grok --continue", shell_quote(&workdir)),
        ),
        (
            "一次性提问".into(),
            format!(
                "cd {} && grok -p \"你的问题\"{}",
                shell_quote(&workdir),
                model_flag
            ),
        ),
        ("查看模型列表".into(), "grok models".into()),
        ("登录".into(), "grok login".into()),
    ]
}

fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-:@+".contains(c))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn apple_script_string(s: &str) -> String {
    // AppleScript string literal with escaped quotes and backslashes.
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Count how many of the given profile ids have a locally stored token.
#[allow(dead_code)]
pub fn count_local_tokens(ids: &[uuid::Uuid]) -> usize {
    ids.iter().filter(|id| secret_store::has_token(**id)).count()
}
