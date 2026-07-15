use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

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
    /// Session title from latest summary.json when available.
    pub title: Option<String>,
    /// Last model id recorded in summary.json.
    pub model_id: Option<String>,
    /// Number of session subdirectories under this project.
    pub session_count: u32,
    /// Lines in project-level prompt_history.jsonl.
    pub prompt_count: u32,
    /// Whether the project directory still exists on disk.
    pub path_exists: bool,
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

/// Hide console flash when a GUI app shells out on Windows.
fn silence_console(cmd: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

fn which_grok() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("GROK_BINARY") {
        let p = PathBuf::from(custom);
        if p.exists() {
            return Some(p);
        }
    }

    let home_bin = home_grok_dir().join("bin");
    for name in ["grok", "grok.exe"] {
        let p = home_bin.join(name);
        if p.exists() {
            return Some(p);
        }
    }

    // PATH lookup (platform-native).
    #[cfg(target_os = "windows")]
    {
        // Prefer where.exe (System32) over the bare name so PowerShell aliases
        // never intercept us.
        let mut cmd = Command::new("where.exe");
        silence_console(&mut cmd);
        cmd.arg("grok")
            .output()
            .ok()
            .and_then(|o| {
                if !o.status.success() {
                    return None;
                }
                let s = String::from_utf8_lossy(&o.stdout);
                let first = s.lines().next()?.trim();
                if first.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(first))
                }
            })
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("sh")
            .args(["-lc", "command -v grok || which grok"])
            .output()
            .ok()
            .and_then(|o| {
                if !o.status.success() {
                    return None;
                }
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(s))
                }
            })
    }
}

/// Absolute project path: Unix `/…` or Windows `C:\…` / `C:/…`.
fn is_absolute_project_path(path: &str) -> bool {
    let p = path.trim();
    if p.starts_with('/') {
        return true;
    }
    let b = p.as_bytes();
    b.len() >= 3
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b[2] == b'\\' || b[2] == b'/')
}

fn grok_version(bin: &Path) -> Option<String> {
    let mut cmd = Command::new(bin);
    silence_console(&mut cmd);
    cmd.arg("--version")
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
        // Case-insensitive home strip helps Windows drive-letter casing.
        let path_norm = path.replace('\\', "/");
        let home_norm = home_s.replace('\\', "/");
        if let Some(rest) = path_norm
            .strip_prefix(&home_norm)
            .or_else(|| {
                let hl = home_norm.to_ascii_lowercase();
                let pl = path_norm.to_ascii_lowercase();
                pl.strip_prefix(&hl).map(|r| &path_norm[path_norm.len() - r.len()..])
            })
        {
            if rest.is_empty() {
                return "~".into();
            }
            // Prefer ~\ on Windows for display when original used backslashes.
            #[cfg(target_os = "windows")]
            {
                return format!("~{}", rest.replace('/', "\\"));
            }
            #[cfg(not(target_os = "windows"))]
            {
                return format!("~{rest}");
            }
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

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    } else {
        s.to_string()
    }
}

fn last_prompt_and_count(history: &Path) -> (Option<String>, u32) {
    let Ok(text) = fs::read_to_string(history) else {
        return (None, 0);
    };
    let mut last: Option<String> = None;
    let mut count = 0u32;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if let Some(p) = v.get("prompt").and_then(|x| x.as_str()) {
                let cleaned = p.replace('\n', " ").trim().to_string();
                if !cleaned.is_empty() {
                    count += 1;
                    last = Some(truncate_chars(&cleaned, 160));
                }
            }
        }
    }
    (last, count)
}

/// Read title + model from the newest session summary under a project dir.
fn latest_session_meta(project_dir: &Path) -> (Option<String>, Option<String>, Option<String>, u32) {
    let Ok(rd) = fs::read_dir(project_dir) else {
        return (None, None, None, 0);
    };
    let mut best_time: i64 = -1;
    let mut best_id: Option<String> = None;
    let mut session_count = 0u32;

    for sub in rd.flatten() {
        let sp = sub.path();
        if !sp.is_dir() {
            continue;
        }
        let name = sub.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        // Grok session ids look like ULIDs / long hex.
        if !(name.starts_with("019") || name.len() > 20) {
            continue;
        }
        session_count += 1;
        let mt = file_mtime_ms(&sp);
        if mt >= best_time {
            best_time = mt;
            best_id = Some(name);
        }
    }

    let Some(id) = best_id else {
        return (None, None, None, session_count);
    };

    let summary_path = project_dir.join(&id).join("summary.json");
    let mut title = None;
    let mut model_id = None;
    if let Ok(text) = fs::read_to_string(&summary_path) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            title = v
                .get("generated_title")
                .and_then(|x| x.as_str())
                .or_else(|| v.get("session_summary").and_then(|x| x.as_str()))
                .map(|s| truncate_chars(s.trim(), 80))
                .filter(|s| !s.is_empty());
            model_id = v
                .get("current_model_id")
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
    }
    (Some(id), title, model_id, session_count)
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
        if encoded.starts_with('.') {
            continue;
        }
        let cwd = decode_session_cwd(&encoded);
        if cwd.is_empty() || !is_absolute_project_path(&cwd) {
            continue;
        }

        let history = path.join("prompt_history.jsonl");
        let (last_prompt, prompt_count) = last_prompt_and_count(&history);
        let (session_id, title, model_id, session_count) = latest_session_meta(&path);

        let updated = if history.exists() {
            file_mtime_ms(&history)
        } else {
            file_mtime_ms(&path)
        };

        items.push(GrokSessionItem {
            id: session_id.unwrap_or(encoded),
            cwd: cwd.clone(),
            cwd_label: short_path(&cwd),
            path: path.to_string_lossy().into_owned(),
            updated_at: updated,
            last_prompt,
            title,
            model_id,
            session_count,
            prompt_count,
            path_exists: Path::new(&cwd).is_dir(),
        });
    }

    items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    items.truncate(limit.max(1));
    items
}

/// Delete a project session folder under `~/.grok/sessions/`.
/// Accepts either the absolute project cwd or the sessions storage path.
pub fn delete_session_project(path_or_cwd: &str) -> Result<String, String> {
    let raw = path_or_cwd.trim();
    if raw.is_empty() {
        return Err("路径为空".into());
    }

    let root = home_grok_dir().join("sessions");
    let root_canon = root
        .canonicalize()
        .unwrap_or_else(|_| root.clone());

    let candidate = {
        let p = PathBuf::from(raw);
        if p.is_dir() && p.starts_with(&root) {
            p
        } else {
            // Encode cwd the same way Grok stores sessions: percent-encode path.
            let cwd = expand_path(raw);
            let encoded = percent_encode_path(&cwd);
            root.join(encoded)
        }
    };

    if !candidate.exists() {
        return Err(format!("会话目录不存在：{}", candidate.display()));
    }

    let canon = candidate
        .canonicalize()
        .map_err(|e| format!("无法解析路径：{e}"))?;
    if !canon.starts_with(&root_canon) {
        return Err("只能删除 ~/.grok/sessions 下的会话目录".into());
    }
    if canon == root_canon {
        return Err("不能删除 sessions 根目录".into());
    }

    let label = {
        let name = canon
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let cwd = decode_session_cwd(&name);
        if is_absolute_project_path(&cwd) {
            short_path(&cwd)
        } else {
            short_path(&name)
        }
    };

    fs::remove_dir_all(&canon).map_err(|e| format!("删除失败：{e}"))?;
    Ok(format!("已删除会话 · {label}"))
}

fn percent_encode_path(path: &str) -> String {
    path.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
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
        // explorer handles both files and directories reliably with spaces.
        if p.is_dir() {
            Command::new("explorer")
                .arg(&expanded)
                .spawn()
                .map_err(|e| format!("无法打开：{e}"))?;
        } else {
            Command::new("cmd")
                .args(["/C", "start", "", &expanded])
                .spawn()
                .map_err(|e| format!("无法打开：{e}"))?;
        }
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

/// Launch interactive Grok in a platform terminal at cwd, optionally with -m model.
pub fn launch_grok_terminal(cwd: Option<&str>, model: Option<&str>) -> Result<String, String> {
    let workdir = expand_path(cwd.unwrap_or("~"));
    if !Path::new(&workdir).is_dir() {
        return Err(format!("工作目录不存在：{workdir}"));
    }

    let bin = which_grok().ok_or_else(|| "未找到 grok 命令".to_string())?;
    let mut args: Vec<String> = Vec::new();
    if let Some(m) = model.map(str::trim).filter(|s| !s.is_empty()) {
        args.push("-m".into());
        args.push(m.into());
    }
    open_in_terminal(&workdir, &bin, &args)?;
    Ok(format!("已在终端打开 Grok · {}", short_path(&workdir)))
}

/// Resume most recent session for a cwd in a platform terminal.
pub fn resume_session_terminal(cwd: &str) -> Result<String, String> {
    let workdir = expand_path(cwd);
    if !Path::new(&workdir).is_dir() {
        return Err(format!("工作目录不存在：{workdir}"));
    }
    let bin = which_grok().ok_or_else(|| "未找到 grok 命令".to_string())?;
    open_in_terminal(&workdir, &bin, &["--continue".into()])?;
    Ok(format!("已继续最近会话 · {}", short_path(&workdir)))
}

fn open_in_terminal(workdir: &str, bin: &Path, args: &[String]) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = format!(
            "cd {} && {}",
            shell_quote(workdir),
            shell_quote(&bin.to_string_lossy())
        );
        for a in args {
            cmd.push(' ');
            cmd.push_str(&shell_quote(a));
        }
        let script = format!(
            "tell application \"Terminal\"\nactivate\ndo script {}\nend tell",
            apple_script_string(&cmd)
        );
        Command::new("osascript")
            .args(["-e", &script])
            .spawn()
            .map_err(|e| format!("无法启动 Terminal：{e}"))?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        // Build: "C:\path\grok.exe" -m model
        let mut parts = vec![win_quote(&bin.to_string_lossy())];
        for a in args {
            parts.push(win_quote(a));
        }
        let cmdline = parts.join(" ");

        // Prefer Windows Terminal when available. Do NOT silence this process —
        // the whole point is to show a terminal window.
        // `wt -d <dir> cmd /K <cmdline>` keeps the window open after exit.
        if Command::new("wt.exe")
            .args(["-d", workdir, "cmd.exe", "/K", &cmdline])
            .spawn()
            .is_ok()
        {
            return Ok(());
        }

        // Fallback: new cmd.exe window via `start`.
        // Title token "Grok" is required so `start` does not treat a quoted path
        // as the window title.
        let full = format!("cd /d {} && {}", win_quote(workdir), cmdline);
        Command::new("cmd.exe")
            .args(["/C", "start", "Grok", "cmd.exe", "/K", &full])
            .spawn()
            .map_err(|e| format!("无法启动终端：{e}"))?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let mut child = Command::new(bin);
        child.current_dir(workdir).args(args);
        // Try a few common terminal emulators; fall back to direct spawn.
        for (term, flag) in [
            ("x-terminal-emulator", "-e"),
            ("gnome-terminal", "--"),
            ("konsole", "-e"),
            ("xterm", "-e"),
        ] {
            let mut line = vec![bin.to_string_lossy().to_string()];
            line.extend(args.iter().cloned());
            if Command::new(term)
                .arg(flag)
                .args(&line)
                .current_dir(workdir)
                .spawn()
                .is_ok()
            {
                return Ok(());
            }
        }
        child
            .spawn()
            .map_err(|e| format!("无法启动 grok：{e}"))?;
        Ok(())
    }
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

/// Quote for cmd.exe: wrap in double quotes; escape embedded `"`.
/// Always compiled so unit tests can cover Windows quoting on any host.
#[cfg_attr(not(test), allow(dead_code))]
fn win_quote(s: &str) -> String {
    if s.is_empty() {
        return "\"\"".into();
    }
    if !s
        .chars()
        .any(|c| c.is_whitespace() || "^&|<>()%!\"'\\".contains(c))
    {
        return s.to_string();
    }
    format!("\"{}\"", s.replace('"', "\"\""))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_unix_and_windows_paths() {
        assert!(is_absolute_project_path("/Users/kang/proj"));
        assert!(is_absolute_project_path("C:\\Users\\kang"));
        assert!(is_absolute_project_path("c:/Users/kang"));
        assert!(is_absolute_project_path("D:/work"));
        assert!(!is_absolute_project_path("relative/path"));
        assert!(!is_absolute_project_path("C:foo")); // drive-relative, not absolute
        assert!(!is_absolute_project_path(""));
        assert!(!is_absolute_project_path("~/proj"));
    }

    #[test]
    fn decode_percent_encoded_windows_cwd() {
        let enc = "%43%3A%5CUsers%5Ckang%5CProject";
        assert_eq!(decode_session_cwd(enc), "C:\\Users\\kang\\Project");
        assert_eq!(
            decode_session_cwd("%2FUsers%2Fkang%2FProject"),
            "/Users/kang/Project"
        );
    }

    #[test]
    fn expand_home_slash_and_backslash() {
        if let Some(home) = dirs::home_dir() {
            let h = home.to_string_lossy();
            assert_eq!(expand_path("~"), h.as_ref());
            assert!(expand_path("~/a/b").starts_with(h.as_ref()));
            assert!(expand_path("~\\a\\b").starts_with(h.as_ref()));
        }
    }

    #[test]
    fn win_quote_spaces_and_specials() {
        assert_eq!(win_quote("grok"), "grok");
        assert_eq!(win_quote("C:\\Program Files\\grok.exe"), "\"C:\\Program Files\\grok.exe\"");
        assert_eq!(win_quote("a\"b"), "\"a\"\"b\"");
        assert_eq!(win_quote(""), "\"\"");
        assert_eq!(win_quote("a&b"), "\"a&b\"");
    }

    #[test]
    fn percent_encode_roundtrip_drive_path() {
        let path = "C:\\Users\\kang\\My Project";
        let enc = percent_encode_path(path);
        assert_eq!(decode_session_cwd(&enc), path);
        assert!(is_absolute_project_path(&decode_session_cwd(&enc)));
    }
}
