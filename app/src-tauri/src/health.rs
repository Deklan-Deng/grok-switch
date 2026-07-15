use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResult {
    pub profile_id: String,
    pub name: String,
    pub ok: bool,
    /// ok | config | auth | network | rate_limit | not_found | server | unknown
    pub category: String,
    pub title: String,
    pub detail: String,
    pub hint: String,
    pub latency_ms: Option<u64>,
    pub status_code: Option<u16>,
    pub url: Option<String>,
    pub checked_at: i64,
}

impl HealthResult {
    pub fn status_line(&self) -> String {
        if self.ok {
            format!("「{}」{} · {}", self.name, self.title, self.detail)
        } else {
            format!(
                "「{}」{} · {} — {}",
                self.name, self.title, self.detail, self.hint
            )
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn models_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/models") {
        base.to_string()
    } else {
        format!("{base}/models")
    }
}

/// Probe a provider with structured, explainable failure categories.
pub fn check_provider(
    profile_id: &str,
    name: &str,
    base_url: Option<&str>,
    token: Option<&str>,
) -> HealthResult {
    let checked_at = now_ms();
    let name = name.trim();
    let token = token.map(str::trim).filter(|s| !s.is_empty());
    let base_url = base_url.map(str::trim).filter(|s| !s.is_empty());

    if token.is_none() {
        return HealthResult {
            profile_id: profile_id.into(),
            name: name.into(),
            ok: false,
            category: "config".into(),
            title: "缺少密钥".into(),
            detail: "本地未保存 API Token".into(),
            hint: "打开编辑页填写 Token 并保存，再测试连通性。".into(),
            latency_ms: None,
            status_code: None,
            url: None,
            checked_at,
        };
    }

    let Some(base) = base_url else {
        return HealthResult {
            profile_id: profile_id.into(),
            name: name.into(),
            ok: false,
            category: "config".into(),
            title: "缺少 base_url".into(),
            detail: "未配置 API 地址".into(),
            hint: "编辑供应商，填写 API 地址（官方示例：https://api.x.ai/v1）。"
                .into(),
            latency_ms: None,
            status_code: None,
            url: None,
            checked_at,
        };
    };

    let url = models_url(base);
    let token = token.unwrap();

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
    {
        Ok(c) => c,
        Err(err) => {
            return HealthResult {
                profile_id: profile_id.into(),
                name: name.into(),
                ok: false,
                category: "unknown".into(),
                title: "无法发起请求".into(),
                detail: format!("HTTP 客户端错误：{err}"),
                hint: "重启应用后再试；若持续失败请检查系统网络代理设置。".into(),
                latency_ms: None,
                status_code: None,
                url: Some(url),
                checked_at,
            };
        }
    };

    let started = Instant::now();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json")
        .send();
    let latency_ms = started.elapsed().as_millis() as u64;

    let response = match response {
        Ok(r) => r,
        Err(err) => {
            let (category, title, hint) = classify_network_error(&err);
            return HealthResult {
                profile_id: profile_id.into(),
                name: name.into(),
                ok: false,
                category: category.into(),
                title: title.into(),
                detail: truncate(&err.to_string(), 160),
                hint: hint.into(),
                latency_ms: Some(latency_ms),
                status_code: None,
                url: Some(url),
                checked_at,
            };
        }
    };

    let status = response.status();
    let status_code = status.as_u16();
    let body = response.text().unwrap_or_default();
    let snippet = first_line_snippet(&body, 140);

    if status.is_success() {
        let model_count = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("data").and_then(|d| d.as_array()).map(|a| a.len()));
        let detail = match model_count {
            Some(n) => format!("{latency_ms}ms · {n} 个模型 · {url}"),
            None => format!("{latency_ms}ms · {url}"),
        };
        return HealthResult {
            profile_id: profile_id.into(),
            name: name.into(),
            ok: true,
            category: "ok".into(),
            title: "连通正常".into(),
            detail,
            hint: "供应商可用，可以安全启用。".into(),
            latency_ms: Some(latency_ms),
            status_code: Some(status_code),
            url: Some(url),
            checked_at,
        };
    }

    let (category, title, hint) = classify_http(status_code, &body);
    let detail = if snippet.is_empty() {
        format!("HTTP {status_code} · {latency_ms}ms")
    } else {
        format!("HTTP {status_code} · {latency_ms}ms · {snippet}")
    };

    HealthResult {
        profile_id: profile_id.into(),
        name: name.into(),
        ok: false,
        category: category.into(),
        title: title.into(),
        detail,
        hint: hint.into(),
        latency_ms: Some(latency_ms),
        status_code: Some(status_code),
        url: Some(url),
        checked_at,
    }
}

fn classify_network_error(err: &reqwest::Error) -> (&'static str, &'static str, &'static str) {
    let msg = err.to_string().to_lowercase();
    if err.is_timeout() || msg.contains("timed out") {
        (
            "network",
            "请求超时",
            "端点响应过慢或不可达。检查 base_url、代理，或换一条线路。",
        )
    } else if err.is_connect()
        || msg.contains("connection refused")
        || msg.contains("dns")
        || msg.contains("resolve")
        || msg.contains("nodename")
    {
        (
            "network",
            "无法连接",
            "DNS 或网络不通。确认 base_url 域名正确，以及本机网络/VPN/代理。",
        )
    } else if msg.contains("certificate") || msg.contains("ssl") || msg.contains("tls") {
        (
            "network",
            "证书错误",
            "TLS 握手失败。可能是自签证书、代理解密 HTTPS，或系统时间不准。",
        )
    } else {
        (
            "network",
            "网络错误",
            "请求未能完成。稍后重试；仍失败时检查防火墙与代理。",
        )
    }
}

fn classify_http(code: u16, body: &str) -> (&'static str, &'static str, &'static str) {
    let lower = body.to_lowercase();
    let rate_limited = code == 429
        || lower.contains("rate_limit")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("请求过于频繁");

    if rate_limited {
        return (
            "rate_limit",
            "触发限流",
            "请求太频繁或额度用尽。稍后再试，或切换到其它供应商。",
        );
    }

    match code {
        401 | 403 => (
            "auth",
            "鉴权失败",
            "Token 无效、过期或无权访问。重新填写密钥，并确认与该 base_url 匹配。",
        ),
        404 => (
            "not_found",
            "接口不存在",
            "路径可能不对。base_url 通常应到 /v1（不要多写 /chat/completions）。",
        ),
        400 | 422 => (
            "unknown",
            "请求被拒绝",
            "服务端认为请求不合法。检查 api_backend / 模型字段是否与该中转兼容。",
        ),
        500..=599 => (
            "server",
            "服务端错误",
            "供应商侧故障。可稍后重试，或切换备用供应商。",
        ),
        _ => (
            "unknown",
            "连通失败",
            "查看详情中的 HTTP 状态与返回摘要；必要时换线路或联系供应商。",
        ),
    }
}

fn first_line_snippet(body: &str, max_chars: usize) -> String {
    let line = body.lines().next().unwrap_or("").trim();
    truncate(line, max_chars)
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
