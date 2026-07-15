use std::io::{BufRead, BufReader, Read};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// One-click latency probe: models ping + streaming TTFT + total time + 403 detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedTestResult {
    pub profile_id: String,
    pub name: String,
    pub ok: bool,
    /// ok | config | auth | network | rate_limit | not_found | server | blocked | unknown
    pub category: String,
    pub title: String,
    pub detail: String,
    pub hint: String,
    /// GET /models latency (connectivity baseline).
    pub models_ms: Option<u64>,
    /// Time to first stream token / first SSE data event.
    pub ttft_ms: Option<u64>,
    /// Full request time until stream ends (or non-stream body arrives).
    pub total_ms: Option<u64>,
    pub status_code: Option<u16>,
    /// Explicit 403 flag for UI badge.
    pub is_403: bool,
    /// Cloudflare / WAF style block (e.g. error code 1010).
    pub is_cf_block: bool,
    /// Endpoint used: chat_completions | responses
    pub backend: Option<String>,
    pub model: Option<String>,
    pub url: Option<String>,
    /// Short model output preview when available.
    pub preview: Option<String>,
    pub streamed: bool,
    pub checked_at: i64,
}

impl SpeedTestResult {
    pub fn status_line(&self) -> String {
        let mut parts = vec![format!("「{}」{}", self.name, self.title)];
        if let Some(ttft) = self.ttft_ms {
            parts.push(format!("TTFT {ttft}ms"));
        }
        if let Some(total) = self.total_ms {
            parts.push(format!("总 {total}ms"));
        }
        if self.is_403 {
            parts.push("HTTP 403".into());
        } else if let Some(code) = self.status_code {
            parts.push(format!("HTTP {code}"));
        }
        if self.is_cf_block {
            parts.push("CF 拦截".into());
        }
        parts.join(" · ")
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn normalize_base(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn models_url(base: &str) -> String {
    if base.ends_with("/models") {
        base.to_string()
    } else {
        format!("{base}/models")
    }
}

fn browser_like_headers(token: &str) -> reqwest::header::HeaderMap {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, ACCEPT, CONTENT_TYPE, USER_AGENT};
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 GrokSwitch/0.1",
        ),
    );
    headers.insert(ACCEPT, HeaderValue::from_static("application/json, text/event-stream, */*"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Ok(v) = HeaderValue::from_str(&format!("Bearer {token}")) {
        headers.insert(AUTHORIZATION, v);
    }
    headers
}

fn truncate(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max_chars).collect();
        format!("{cut}…")
    }
}

fn first_line_snippet(body: &str, max_chars: usize) -> String {
    let line = body.lines().next().unwrap_or("").trim();
    truncate(line, max_chars)
}

fn is_cf_block(status: u16, body: &str) -> bool {
    let lower = body.to_lowercase();
    status == 403
        && (lower.contains("error code: 1010")
            || lower.contains("error code 1010")
            || lower.contains("cloudflare")
            || lower.contains("attention required")
            || lower.contains("cf-ray"))
}

fn resolve_api_model(api_model: Option<&str>, model_id: &str) -> String {
    api_model
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            // Common pattern: local id embeds real model, else fall back to id itself.
            let id = model_id.trim();
            if id.is_empty() {
                "grok-4.5".into()
            } else {
                id.to_string()
            }
        })
}

fn preferred_backends(api_backend: Option<&str>) -> Vec<&'static str> {
    match api_backend.map(str::trim).unwrap_or("").to_ascii_lowercase().as_str() {
        "responses" => vec!["responses", "chat_completions"],
        "messages" => vec!["chat_completions", "responses"],
        "chat_completions" | "chat.completions" | "" => {
            vec!["chat_completions", "responses"]
        }
        _ => vec!["chat_completions", "responses"],
    }
}

/// Run models ping + streaming completion probe for a provider.
pub fn run_speed_test(
    profile_id: &str,
    name: &str,
    base_url: Option<&str>,
    token: Option<&str>,
    api_model: Option<&str>,
    model_id: &str,
    api_backend: Option<&str>,
) -> SpeedTestResult {
    let checked_at = now_ms();
    let name = name.trim();
    let token = token.map(str::trim).filter(|s| !s.is_empty());
    let base_url = base_url.map(str::trim).filter(|s| !s.is_empty());

    if token.is_none() {
        return SpeedTestResult {
            profile_id: profile_id.into(),
            name: name.into(),
            ok: false,
            category: "config".into(),
            title: "缺少密钥".into(),
            detail: "本地未保存 API Token".into(),
            hint: "打开编辑页填写 Token 并保存，再测速。".into(),
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
            checked_at,
        };
    }

    let Some(base_raw) = base_url else {
        return SpeedTestResult {
            profile_id: profile_id.into(),
            name: name.into(),
            ok: false,
            category: "config".into(),
            title: "缺少 base_url".into(),
            detail: "未配置 API 地址".into(),
            hint: "编辑供应商，填写 API 地址（官方示例：https://api.x.ai/v1）。"
                .into(),
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
            checked_at,
        };
    };

    let base = normalize_base(base_raw);
    let token = token.unwrap();
    let model = resolve_api_model(api_model, model_id);

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
    {
        Ok(c) => c,
        Err(err) => {
            return SpeedTestResult {
                profile_id: profile_id.into(),
                name: name.into(),
                ok: false,
                category: "unknown".into(),
                title: "无法发起请求".into(),
                detail: format!("HTTP 客户端错误：{err}"),
                hint: "重启应用后再试。".into(),
                models_ms: None,
                ttft_ms: None,
                total_ms: None,
                status_code: None,
                is_403: false,
                is_cf_block: false,
                backend: None,
                model: Some(model),
                url: Some(base.clone()),
                preview: None,
                streamed: false,
                checked_at,
            };
        }
    };

    let headers = browser_like_headers(token);

    // 1) Models ping (baseline; failure does not abort completion probe).
    let models_url = models_url(&base);
    let models_started = Instant::now();
    let models_ms = match client.get(&models_url).headers(headers.clone()).send() {
        Ok(resp) => {
            let ms = models_started.elapsed().as_millis() as u64;
            let code = resp.status().as_u16();
            let body = resp.text().unwrap_or_default();
            if code == 403 || is_cf_block(code, &body) {
                // Hard block on /models — still try completion, but flag early.
                let cf = is_cf_block(code, &body);
                // If CF blocks everything, completion will likely fail too; continue for detail.
                let _ = (cf, body);
            }
            Some(ms)
        }
        Err(_) => Some(models_started.elapsed().as_millis() as u64),
    };

    // 2) Completion probe with preferred backend order.
    let mut last_fail: Option<ProbeFail> = None;
    for backend in preferred_backends(api_backend) {
        match probe_completion(&client, &headers, &base, &model, backend) {
            Ok(probe) => {
                return finish_ok(
                    profile_id,
                    name,
                    model,
                    backend,
                    models_ms,
                    probe,
                    checked_at,
                );
            }
            Err(fail) => {
                // Auth / hard block: stop early.
                if fail.status_code == Some(401)
                    || fail.status_code == Some(403)
                    || fail.is_cf_block
                {
                    return finish_fail(
                        profile_id,
                        name,
                        model,
                        Some(backend),
                        models_ms,
                        fail,
                        checked_at,
                    );
                }
                last_fail = Some(fail);
            }
        }
    }

    finish_fail(
        profile_id,
        name,
        model,
        last_fail.as_ref().and_then(|f| f.backend.clone()).as_deref(),
        models_ms,
        last_fail.unwrap_or(ProbeFail {
            category: "unknown".into(),
            title: "测速失败".into(),
            detail: "未获得有效响应".into(),
            hint: "检查 base_url / 模型名 / api_backend 是否与中转兼容。".into(),
            status_code: None,
            is_403: false,
            is_cf_block: false,
            total_ms: None,
            ttft_ms: None,
            url: Some(base),
            backend: None,
            preview: None,
            streamed: false,
        }),
        checked_at,
    )
}

struct ProbeOk {
    ttft_ms: Option<u64>,
    total_ms: u64,
    status_code: u16,
    url: String,
    preview: Option<String>,
    streamed: bool,
}

struct ProbeFail {
    category: String,
    title: String,
    detail: String,
    hint: String,
    status_code: Option<u16>,
    is_403: bool,
    is_cf_block: bool,
    total_ms: Option<u64>,
    ttft_ms: Option<u64>,
    url: Option<String>,
    backend: Option<String>,
    preview: Option<String>,
    streamed: bool,
}

fn finish_ok(
    profile_id: &str,
    name: &str,
    model: String,
    backend: &str,
    models_ms: Option<u64>,
    probe: ProbeOk,
    checked_at: i64,
) -> SpeedTestResult {
    let title = if probe.ttft_ms.is_some() {
        "测速完成".into()
    } else {
        "测速完成（无流式）".into()
    };
    let mut bits = Vec::new();
    if let Some(m) = models_ms {
        bits.push(format!("models {m}ms"));
    }
    if let Some(t) = probe.ttft_ms {
        bits.push(format!("TTFT {t}ms"));
    }
    bits.push(format!("总 {}ms", probe.total_ms));
    bits.push(backend.to_string());
    let detail = bits.join(" · ");
    let hint = grade_hint(probe.ttft_ms, probe.total_ms);

    SpeedTestResult {
        profile_id: profile_id.into(),
        name: name.into(),
        ok: true,
        category: "ok".into(),
        title,
        detail,
        hint,
        models_ms,
        ttft_ms: probe.ttft_ms,
        total_ms: Some(probe.total_ms),
        status_code: Some(probe.status_code),
        is_403: false,
        is_cf_block: false,
        backend: Some(backend.into()),
        model: Some(model),
        url: Some(probe.url),
        preview: probe.preview,
        streamed: probe.streamed,
        checked_at,
    }
}

fn finish_fail(
    profile_id: &str,
    name: &str,
    model: String,
    backend: Option<&str>,
    models_ms: Option<u64>,
    fail: ProbeFail,
    checked_at: i64,
) -> SpeedTestResult {
    SpeedTestResult {
        profile_id: profile_id.into(),
        name: name.into(),
        ok: false,
        category: fail.category,
        title: fail.title,
        detail: fail.detail,
        hint: fail.hint,
        models_ms,
        ttft_ms: fail.ttft_ms,
        total_ms: fail.total_ms,
        status_code: fail.status_code,
        is_403: fail.is_403,
        is_cf_block: fail.is_cf_block,
        backend: backend.map(|s| s.to_string()).or(fail.backend),
        model: Some(model),
        url: fail.url,
        preview: fail.preview,
        streamed: fail.streamed,
        checked_at,
    }
}

fn grade_hint(ttft_ms: Option<u64>, total_ms: u64) -> String {
    let ttft = ttft_ms.unwrap_or(total_ms);
    if ttft < 800 && total_ms < 2500 {
        "速度很好，适合日常 Agent 使用。".into()
    } else if ttft < 2000 && total_ms < 6000 {
        "可用。若 Agent 仍觉得卡，多半是上下文过大，不是连通延迟。".into()
    } else if total_ms < 15000 {
        "偏慢。可换线路 / 官方直连，或对比其它供应商。".into()
    } else {
        "明显偏慢。优先检查中转排队、模型负载与是否被限流。".into()
    }
}

fn probe_completion(
    client: &reqwest::blocking::Client,
    headers: &reqwest::header::HeaderMap,
    base: &str,
    model: &str,
    backend: &str,
) -> Result<ProbeOk, ProbeFail> {
    // Prefer stream for TTFT; fall back to non-stream on hard reject.
    match probe_stream(client, headers, base, model, backend) {
        Ok(ok) => Ok(ok),
        Err(fail) if should_fallback_nonstream(&fail) => {
            probe_nonstream(client, headers, base, model, backend)
        }
        Err(fail) => Err(fail),
    }
}

fn should_fallback_nonstream(fail: &ProbeFail) -> bool {
    match fail.status_code {
        Some(400) | Some(404) | Some(405) | Some(415) | Some(422) => true,
        Some(401) | Some(403) | Some(429) => false,
        _ => {
            // Stream parse / empty body issues — try non-stream once.
            fail.category == "unknown" || fail.category == "network"
        }
    }
}

fn build_stream_body(backend: &str, model: &str) -> Value {
    match backend {
        "responses" => json!({
            "model": model,
            "input": "Reply with exactly: OK",
            "max_output_tokens": 8,
            "stream": true,
            "store": false,
            "temperature": 0
        }),
        _ => json!({
            "model": model,
            "messages": [{"role": "user", "content": "Reply with exactly: OK"}],
            "max_tokens": 8,
            "max_completion_tokens": 8,
            "stream": true,
            "temperature": 0
        }),
    }
}

fn build_nonstream_body(backend: &str, model: &str) -> Value {
    match backend {
        "responses" => json!({
            "model": model,
            "input": "Reply with exactly: OK",
            "max_output_tokens": 8,
            "stream": false,
            "store": false,
            "temperature": 0
        }),
        _ => json!({
            "model": model,
            "messages": [{"role": "user", "content": "Reply with exactly: OK"}],
            "max_tokens": 8,
            "max_completion_tokens": 8,
            "stream": false,
            "temperature": 0
        }),
    }
}

fn url_for(base: &str, backend: &str) -> String {
    match backend {
        "responses" => format!("{base}/responses"),
        _ => format!("{base}/chat/completions"),
    }
}

fn probe_stream(
    client: &reqwest::blocking::Client,
    headers: &reqwest::header::HeaderMap,
    base: &str,
    model: &str,
    backend: &str,
) -> Result<ProbeOk, ProbeFail> {
    let url = url_for(base, backend);
    let body = build_stream_body(backend, model);
    let started = Instant::now();
    let response = client
        .post(&url)
        .headers(headers.clone())
        .json(&body)
        .send()
        .map_err(|err| network_fail(err, started, &url, backend))?;

    let status = response.status();
    let status_code = status.as_u16();

    if !status.is_success() {
        let text = response.text().unwrap_or_default();
        let total_ms = started.elapsed().as_millis() as u64;
        return Err(http_fail(
            status_code,
            &text,
            total_ms,
            None,
            &url,
            backend,
            false,
        ));
    }

    // Headers received — read SSE body for TTFT.
    let mut reader = BufReader::new(response);
    let mut line = String::new();
    let mut ttft_ms: Option<u64> = None;
    let mut preview = String::new();
    let mut saw_done = false;
    let mut raw_head = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(err) => {
                let total_ms = started.elapsed().as_millis() as u64;
                return Err(ProbeFail {
                    category: "network".into(),
                    title: "读取流失败".into(),
                    detail: truncate(&err.to_string(), 160),
                    hint: "连接在流式响应中断开。检查中转稳定性或改用非流式线路。".into(),
                    status_code: Some(status_code),
                    is_403: false,
                    is_cf_block: false,
                    total_ms: Some(total_ms),
                    ttft_ms,
                    url: Some(url),
                    backend: Some(backend.into()),
                    preview: non_empty(preview),
                    streamed: true,
                });
            }
        }

        if raw_head.len() < 400 {
            raw_head.push_str(&line);
        }

        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }

        // Some gateways return JSON body even when stream=true.
        if trimmed.starts_with('{') && !trimmed.starts_with("data:") {
            if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
                if let Some(text) = extract_any_text(&v) {
                    ttft_ms = Some(started.elapsed().as_millis() as u64);
                    preview.push_str(&text);
                }
            }
            // Keep reading until EOF.
            let mut rest = String::new();
            let _ = reader.read_to_string(&mut rest);
            if preview.is_empty() {
                if let Ok(v) = serde_json::from_str::<Value>(&format!("{trimmed}{rest}")) {
                    if let Some(text) = extract_any_text(&v) {
                        if ttft_ms.is_none() {
                            ttft_ms = Some(started.elapsed().as_millis() as u64);
                        }
                        preview.push_str(&text);
                    }
                }
            }
            break;
        }

        let data = if let Some(rest) = trimmed.strip_prefix("data:") {
            rest.trim()
        } else if trimmed.starts_with("event:") {
            continue;
        } else {
            continue;
        };

        if data == "[DONE]" {
            saw_done = true;
            break;
        }

        if let Ok(v) = serde_json::from_str::<Value>(data) {
            // Only count visible assistant output — ignore reasoning/summary deltas.
            // Grok reasoning models stream response.reasoning_summary_text.delta first;
            // user-facing TTFT is response.output_text.delta (or chat delta.content).
            if let Some(piece) = extract_output_delta(&v) {
                if !piece.is_empty() {
                    if ttft_ms.is_none() {
                        ttft_ms = Some(started.elapsed().as_millis() as u64);
                    }
                    if preview.len() < 80 {
                        preview.push_str(&piece);
                    }
                }
            }
            if stream_finished(&v) {
                saw_done = true;
                break;
            }
        }
    }

    let total_ms = started.elapsed().as_millis() as u64;

    // If we got headers 200 but no parseable stream, still report total; mark soft ok if we saw data.
    if ttft_ms.is_none() && preview.is_empty() && !saw_done {
        // Completely empty stream body — treat as soft failure so non-stream fallback can run.
        return Err(ProbeFail {
            category: "unknown".into(),
            title: "流式无内容".into(),
            detail: format!(
                "HTTP {status_code} · {}ms · {}",
                total_ms,
                first_line_snippet(&raw_head, 120)
            ),
            hint: "中转可能不支持 stream。将尝试非流式测速。".into(),
            status_code: Some(status_code),
            is_403: false,
            is_cf_block: false,
            total_ms: Some(total_ms),
            ttft_ms: None,
            url: Some(url),
            backend: Some(backend.into()),
            preview: None,
            streamed: true,
        });
    }

    Ok(ProbeOk {
        ttft_ms,
        total_ms,
        status_code,
        url,
        preview: non_empty(preview),
        streamed: true,
    })
}

fn probe_nonstream(
    client: &reqwest::blocking::Client,
    headers: &reqwest::header::HeaderMap,
    base: &str,
    model: &str,
    backend: &str,
) -> Result<ProbeOk, ProbeFail> {
    let url = url_for(base, backend);
    let body = build_nonstream_body(backend, model);
    let started = Instant::now();
    let response = client
        .post(&url)
        .headers(headers.clone())
        .json(&body)
        .send()
        .map_err(|err| network_fail(err, started, &url, backend))?;

    let status = response.status();
    let status_code = status.as_u16();
    let text = response.text().unwrap_or_default();
    let total_ms = started.elapsed().as_millis() as u64;

    if !status.is_success() {
        return Err(http_fail(
            status_code,
            &text,
            total_ms,
            None,
            &url,
            backend,
            false,
        ));
    }

    let preview = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| extract_any_text(&v));

    Ok(ProbeOk {
        // Non-stream: first token ≈ full body arrival.
        ttft_ms: Some(total_ms),
        total_ms,
        status_code,
        url,
        preview,
        streamed: false,
    })
}

fn network_fail(
    err: reqwest::Error,
    started: Instant,
    url: &str,
    backend: &str,
) -> ProbeFail {
    let total_ms = started.elapsed().as_millis() as u64;
    let msg = err.to_string();
    let lower = msg.to_lowercase();
    let (category, title, hint) = if err.is_timeout() || lower.contains("timed out") {
        (
            "network",
            "请求超时",
            "端点响应过慢。检查中转线路，或增大超时后再试。",
        )
    } else if err.is_connect()
        || lower.contains("dns")
        || lower.contains("resolve")
        || lower.contains("connection refused")
    {
        (
            "network",
            "无法连接",
            "DNS 或网络不通。确认 base_url 与本机代理设置。",
        )
    } else {
        (
            "network",
            "网络错误",
            "请求未能完成。稍后重试或换线路。",
        )
    };
    ProbeFail {
        category: category.into(),
        title: title.into(),
        detail: format!("{total_ms}ms · {}", truncate(&msg, 140)),
        hint: hint.into(),
        status_code: None,
        is_403: false,
        is_cf_block: false,
        total_ms: Some(total_ms),
        ttft_ms: None,
        url: Some(url.into()),
        backend: Some(backend.into()),
        preview: None,
        streamed: false,
    }
}

fn http_fail(
    status_code: u16,
    body: &str,
    total_ms: u64,
    ttft_ms: Option<u64>,
    url: &str,
    backend: &str,
    streamed: bool,
) -> ProbeFail {
    let snippet = first_line_snippet(body, 140);
    let cf = is_cf_block(status_code, body);
    let is_403 = status_code == 403;
    let lower = body.to_lowercase();

    let (category, title, hint) = if cf {
        (
            "blocked",
            "被 Cloudflare 拦截",
            "中转开了 WAF/浏览器指纹校验（常见 error 1010）。换线路，或让供应商放行 API 客户端。",
        )
    } else if status_code == 429
        || lower.contains("rate_limit")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
    {
        (
            "rate_limit",
            "触发限流",
            "请求过频或额度不足。稍后再试或换供应商。",
        )
    } else {
        match status_code {
            401 | 403 => (
                "auth",
                if is_403 { "HTTP 403 拒绝" } else { "鉴权失败" },
                "Token 无效、过期，或无权访问该模型。核对密钥与 base_url。",
            ),
            404 => (
                "not_found",
                "接口不存在",
                "路径可能不对。base_url 通常到 /v1；也可切换 api_backend 再试。",
            ),
            400 | 422 => (
                "unknown",
                "请求被拒绝",
                "模型名或 api_backend 可能与中转不兼容。检查 api_model 字段。",
            ),
            500..=599 => (
                "server",
                "服务端错误",
                "供应商侧故障或上游超时。稍后重试。",
            ),
            _ => (
                "unknown",
                "测速失败",
                "查看 HTTP 状态与返回摘要；必要时换线路。",
            ),
        }
    };

    let detail = if snippet.is_empty() {
        format!("HTTP {status_code} · {total_ms}ms")
    } else {
        format!("HTTP {status_code} · {total_ms}ms · {snippet}")
    };

    ProbeFail {
        category: category.into(),
        title: title.into(),
        detail,
        hint: hint.into(),
        status_code: Some(status_code),
        is_403,
        is_cf_block: cf,
        total_ms: Some(total_ms),
        ttft_ms,
        url: Some(url.into()),
        backend: Some(backend.into()),
        preview: None,
        streamed,
    }
}

fn non_empty(s: String) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(truncate(t, 80))
    }
}

/// Extract only user-visible output tokens from a stream event.
/// Returns None for reasoning / lifecycle events so TTFT stays meaningful.
fn extract_output_delta(v: &Value) -> Option<String> {
    let event_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

    // Explicitly ignore reasoning / summary streams from Responses API.
    if event_type.contains("reasoning") || event_type.contains("summary") {
        return None;
    }

    // responses API: visible text
    if event_type == "response.output_text.delta"
        || event_type == "response.content_part.delta"
        || event_type.ends_with("output_text.delta")
    {
        if let Some(delta) = v.get("delta").and_then(|x| x.as_str()) {
            return Some(delta.to_string());
        }
    }

    // chat.completions stream — only assistant content, not reasoning_content
    if let Some(content) = v
        .pointer("/choices/0/delta/content")
        .and_then(|x| x.as_str())
    {
        if !content.is_empty() {
            return Some(content.to_string());
        }
        // empty string delta is still an output event; ignore for preview but no TTFT credit
        return None;
    }

    // some gateways put full message mid-stream
    if let Some(content) = v
        .pointer("/choices/0/message/content")
        .and_then(|x| x.as_str())
    {
        if !content.is_empty() {
            return Some(content.to_string());
        }
    }

    // Generic delta only when event type clearly looks like output (not bare "delta" on reasoning).
    if event_type.is_empty() {
        // Untyped chat-like chunk without choices already handled; skip bare deltas.
        return None;
    }

    None
}

fn stream_finished(v: &Value) -> bool {
    if v.get("type").and_then(|t| t.as_str()) == Some("response.completed") {
        return true;
    }
    if v.pointer("/choices/0/finish_reason")
        .and_then(|x| x.as_str())
        .is_some()
    {
        // finish_reason present on last chat chunk
        return v
            .pointer("/choices/0/finish_reason")
            .and_then(|x| x.as_str())
            .map(|s| !s.is_empty() && s != "null")
            .unwrap_or(false);
    }
    false
}

fn extract_any_text(v: &Value) -> Option<String> {
    if let Some(s) = v
        .pointer("/choices/0/message/content")
        .and_then(|x| x.as_str())
    {
        return Some(truncate(s, 80));
    }
    if let Some(s) = v.pointer("/choices/0/text").and_then(|x| x.as_str()) {
        return Some(truncate(s, 80));
    }
    // responses: output[].content[].text
    if let Some(arr) = v.get("output").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                for part in content {
                    if let Some(t) = part.get("text").and_then(|x| x.as_str()) {
                        if !t.is_empty() {
                            return Some(truncate(t, 80));
                        }
                    }
                }
            }
            if let Some(t) = item.get("text").and_then(|x| x.as_str()) {
                if !t.is_empty() {
                    return Some(truncate(t, 80));
                }
            }
        }
    }
    if let Some(s) = v.pointer("/output_text").and_then(|x| x.as_str()) {
        return Some(truncate(s, 80));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_responses_first_when_configured() {
        assert_eq!(
            preferred_backends(Some("responses")),
            vec!["responses", "chat_completions"]
        );
        assert_eq!(
            preferred_backends(Some("chat_completions")),
            vec!["chat_completions", "responses"]
        );
    }

    #[test]
    fn detects_cf_1010() {
        assert!(is_cf_block(403, "error code: 1010\n"));
        assert!(!is_cf_block(403, "invalid api key"));
        assert!(!is_cf_block(401, "error code: 1010"));
    }

    #[test]
    fn extracts_chat_delta() {
        let v: Value = serde_json::from_str(
            r#"{"choices":[{"delta":{"content":"OK"}}]}"#,
        )
        .unwrap();
        assert_eq!(extract_output_delta(&v).as_deref(), Some("OK"));
    }

    #[test]
    fn ignores_reasoning_summary_delta() {
        let v: Value = serde_json::from_str(
            r#"{"type":"response.reasoning_summary_text.delta","delta":"The user wants"}"#,
        )
        .unwrap();
        assert!(extract_output_delta(&v).is_none());
    }

    #[test]
    fn extracts_output_text_delta() {
        let v: Value = serde_json::from_str(
            r#"{"type":"response.output_text.delta","delta":"OK"}"#,
        )
        .unwrap();
        assert_eq!(extract_output_delta(&v).as_deref(), Some("OK"));
    }
}
