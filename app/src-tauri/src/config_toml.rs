use crate::models::{ConfigSnapshot, GrokModel};

#[derive(Debug, Clone, Default)]
pub struct ModelUpsert {
    pub model_id: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub api_model: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub env_key: Option<String>,
    pub api_backend: Option<String>,
    pub context_window: Option<u64>,
    pub max_completion_tokens: Option<u64>,
    pub set_as_default: bool,
}

pub fn expand_path(path: &str) -> String {
    let trimmed = path.trim();
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    if trimmed == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.to_string_lossy().into_owned();
        }
    }
    trimmed.to_string()
}

fn toml_value(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Render a TOML key segment. Model IDs are user input and may contain dots,
/// which TOML otherwise interprets as nested table keys.
fn toml_key_segment(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        value.to_string()
    } else {
        toml_value(value)
    }
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_opt(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse_u64(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok()
}

pub fn parse_grok_config(content: &str) -> ConfigSnapshot {
    let mut models: Vec<GrokModel> = Vec::new();
    let mut current: Option<GrokModel> = None;
    let mut in_models_section = false;
    let mut default_model_id: Option<String> = None;

    let flush = |models: &mut Vec<GrokModel>, current: &mut Option<GrokModel>| {
        if let Some(model) = current.take() {
            models.push(model);
        }
    };

    for raw_line in content.split('\n') {
        let line = raw_line.trim();

        if line.starts_with("[model.") && line.ends_with(']') {
            flush(&mut models, &mut current);
            let id = unquote(line.trim_start_matches("[model.").trim_end_matches(']'));
            current = Some(GrokModel {
                id,
                model: None,
                name: None,
                description: None,
                base_url: None,
                env_key: None,
                api_backend: None,
                context_window: None,
                max_completion_tokens: None,
                has_api_key: false,
            });
            in_models_section = false;
            continue;
        }

        if line == "[models]" {
            flush(&mut models, &mut current);
            in_models_section = true;
            continue;
        }

        if line.starts_with('[') {
            flush(&mut models, &mut current);
            in_models_section = false;
            continue;
        }

        let Some(eq) = line.find('=') else {
            continue;
        };
        let key = line[..eq].trim();
        let raw_value = line[eq + 1..].split('#').next().unwrap_or("").trim();
        let value = unquote(raw_value);

        if in_models_section && key == "default" {
            default_model_id = Some(value);
            continue;
        }

        let Some(model) = current.as_mut() else {
            continue;
        };

        match key {
            "model" => model.model = Some(value),
            "name" => model.name = Some(value),
            "description" => model.description = Some(value),
            "base_url" => model.base_url = Some(value),
            "env_key" => model.env_key = Some(value),
            "api_backend" => model.api_backend = Some(value),
            "context_window" => model.context_window = parse_u64(&value),
            "max_completion_tokens" => model.max_completion_tokens = parse_u64(&value),
            "api_key" => model.has_api_key = true,
            _ => {}
        }
    }

    flush(&mut models, &mut current);

    ConfigSnapshot {
        models,
        default_model_id,
    }
}

pub fn upsert_model_config(config: &str, upsert: &ModelUpsert) -> String {
    let mut content = upsert_model_section(config, upsert);
    if let Some(base_url) = normalize_opt(upsert.base_url.as_deref()) {
        content = set_models_base_url(&content, &base_url);
    }
    if upsert.set_as_default {
        content = set_models_default(&content, &upsert.model_id);
    }
    content
}

fn upsert_model_section(config: &str, upsert: &ModelUpsert) -> String {
    let section = format!("[model.{}]", toml_key_segment(&upsert.model_id));
    let mut lines: Vec<String> = if config.is_empty() {
        Vec::new()
    } else {
        config.split('\n').map(str::to_string).collect()
    };

    // Order mirrors common official custom-model examples.
    let mut values: Vec<(String, String)> = Vec::new();
    if let Some(v) = normalize_opt(upsert.api_model.as_deref()) {
        values.push(("model".into(), toml_value(&v)));
    }
    if let Some(v) = normalize_opt(upsert.base_url.as_deref()) {
        values.push(("base_url".into(), toml_value(&v)));
    }
    if let Some(v) = normalize_opt(upsert.display_name.as_deref()) {
        values.push(("name".into(), toml_value(&v)));
    }
    if let Some(v) = normalize_opt(upsert.description.as_deref()) {
        values.push(("description".into(), toml_value(&v)));
    }
    if let Some(v) = normalize_opt(upsert.api_key.as_deref()) {
        values.push(("api_key".into(), toml_value(&v)));
    }
    if let Some(v) = normalize_opt(upsert.env_key.as_deref()) {
        values.push(("env_key".into(), toml_value(&v)));
    }
    if let Some(v) = normalize_opt(upsert.api_backend.as_deref()) {
        values.push(("api_backend".into(), toml_value(&v)));
    }
    if let Some(n) = upsert.context_window {
        values.push(("context_window".into(), n.to_string()));
    }
    if let Some(n) = upsert.max_completion_tokens {
        values.push(("max_completion_tokens".into(), n.to_string()));
    }

    let start = lines.iter().position(|line| line.trim() == section);

    let Some(start) = start else {
        let mut out = config.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&section);
        out.push('\n');
        for (key, value) in values {
            out.push_str(&format!("{key} = {value}\n"));
        }
        return out;
    };

    let mut end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| line.trim().starts_with('['))
        .map(|(i, _)| i)
        .unwrap_or(lines.len());

    for (key, value) in values {
        let key_index = lines[start..end].iter().position(|line| {
            let t = line.trim();
            t.starts_with(&format!("{key} ")) || t.starts_with(&format!("{key}="))
        });

        let rendered = format!("{key} = {value}");
        if let Some(rel) = key_index {
            lines[start + rel] = rendered;
        } else {
            lines.insert(start + 1, rendered);
            end += 1;
        }
    }

    lines.join("\n")
}

/// A configured models endpoint makes Grok use API-key authentication rather
/// than a cached grok.com session, which is necessary for provider switching.
fn set_models_base_url(config: &str, base_url: &str) -> String {
    let rendered = format!("models_base_url = {}", toml_value(base_url));
    let mut lines: Vec<String> = if config.is_empty() {
        Vec::new()
    } else {
        config.split('\n').map(str::to_string).collect()
    };

    if let Some(start) = lines.iter().position(|line| line.trim() == "[endpoints]") {
        let end = lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, line)| line.trim().starts_with('['))
            .map(|(i, _)| i)
            .unwrap_or(lines.len());
        if let Some(rel) = lines[start..end].iter().position(|line| {
            let line = line.trim();
            line.starts_with("models_base_url ") || line.starts_with("models_base_url=")
        }) {
            lines[start + rel] = rendered;
        } else {
            lines.insert(start + 1, rendered);
        }
        return lines.join("\n");
    }

    let mut out = config.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str("[endpoints]\n");
    out.push_str(&rendered);
    out.push('\n');
    out
}

pub fn set_models_default(config: &str, model_id: &str) -> String {
    let default_line = format!("default = {}", toml_value(model_id));
    let mut lines: Vec<String> = if config.is_empty() {
        Vec::new()
    } else {
        config.split('\n').map(str::to_string).collect()
    };

    let start = lines.iter().position(|line| line.trim() == "[models]");
    if let Some(start) = start {
        let end = lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, line)| line.trim().starts_with('['))
            .map(|(i, _)| i)
            .unwrap_or(lines.len());

        if let Some(rel) = lines[start..end].iter().position(|line| {
            let t = line.trim();
            t.starts_with("default ") || t.starts_with("default=")
        }) {
            lines[start + rel] = default_line;
        } else {
            lines.insert(start + 1, default_line);
        }
        return lines.join("\n");
    }

    let mut out = config.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str("[models]\n");
    out.push_str(&default_line);
    out.push('\n');
    out
}

pub fn build_preview(config_path: &str, upsert: &ModelUpsert, snapshot: &ConfigSnapshot) -> String {
    let path = expand_path(config_path);
    let current = snapshot.models.iter().find(|m| m.id == upsert.model_id);

    let mut lines = vec![
        format!("目标：{path}"),
        format!("模型段：[model.{}]", upsert.model_id),
    ];

    lines.push(field_change(
        "model",
        current.and_then(|m| m.model.as_deref()),
        upsert.api_model.as_deref(),
    ));
    lines.push(field_change(
        "name",
        current.and_then(|m| m.name.as_deref()),
        upsert.display_name.as_deref(),
    ));
    lines.push(field_change(
        "description",
        current.and_then(|m| m.description.as_deref()),
        upsert.description.as_deref(),
    ));
    lines.push(field_change(
        "base_url",
        current.and_then(|m| m.base_url.as_deref()),
        upsert.base_url.as_deref(),
    ));
    lines.push(field_change(
        "env_key",
        current.and_then(|m| m.env_key.as_deref()),
        upsert.env_key.as_deref(),
    ));
    lines.push(field_change(
        "api_backend",
        current.and_then(|m| m.api_backend.as_deref()),
        upsert.api_backend.as_deref(),
    ));
    lines.push(field_change(
        "context_window",
        current
            .and_then(|m| m.context_window.map(|n| n.to_string()))
            .as_deref(),
        upsert.context_window.map(|n| n.to_string()).as_deref(),
    ));
    lines.push(field_change(
        "max_completion_tokens",
        current
            .and_then(|m| m.max_completion_tokens.map(|n| n.to_string()))
            .as_deref(),
        upsert
            .max_completion_tokens
            .map(|n| n.to_string())
            .as_deref(),
    ));

    let api_key_action = if current.map(|m| m.has_api_key).unwrap_or(false) {
        if upsert
            .api_key
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        {
            "将替换（已脱敏）".to_string()
        } else {
            "保持（未提供新 Token）".to_string()
        }
    } else if upsert
        .api_key
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
    {
        "将新增（已脱敏）".to_string()
    } else {
        "未设置".to_string()
    };
    lines.push(format!("api_key               {api_key_action}"));

    if upsert.set_as_default {
        let prev = snapshot.default_model_id.as_deref().unwrap_or("未设置");
        lines.push(format!(
            "[models].default      {prev}  ->  {}",
            upsert.model_id
        ));
    } else {
        lines.push(format!(
            "[models].default      保持 {}",
            snapshot.default_model_id.as_deref().unwrap_or("未设置")
        ));
    }

    lines.join("\n")
}

fn field_change(key: &str, current: Option<&str>, next: Option<&str>) -> String {
    let pad = " ".repeat(22usize.saturating_sub(key.len()));
    match normalize_opt(next) {
        Some(target) => format!("{key}{pad}{}  ->  {target}", current.unwrap_or("未设置")),
        None => format!("{key}{pad}保持 {}", current.unwrap_or("未设置")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_writes_extended_fields() {
        let upsert = ModelUpsert {
            model_id: "demo".into(),
            api_key: Some("tok".into()),
            base_url: Some("https://api.x.ai/v1".into()),
            api_model: Some("grok-4.5".into()),
            display_name: Some("Demo".into()),
            description: Some("desc".into()),
            env_key: Some("XAI_API_KEY".into()),
            api_backend: Some("responses".into()),
            context_window: Some(500000),
            max_completion_tokens: Some(32768),
            set_as_default: true,
        };
        let out = upsert_model_config("", &upsert);
        assert!(out.contains("description = \"desc\""));
        assert!(out.contains("env_key = \"XAI_API_KEY\""));
        assert!(out.contains("context_window = 500000"));
        assert!(out.contains("max_completion_tokens = 32768"));
        assert!(out.contains("default = \"demo\""));
        assert!(out.contains("[endpoints]\nmodels_base_url = \"https://api.x.ai/v1\""));
    }

    #[test]
    fn parse_extended_fields() {
        let content = r#"
[model.demo]
model = "grok"
name = "Demo"
description = "x"
base_url = "https://x"
env_key = "XAI_API_KEY"
api_backend = "responses"
context_window = 500000
max_completion_tokens = 32768
api_key = "secret"
"#;
        let snap = parse_grok_config(content);
        assert_eq!(snap.models[0].context_window, Some(500000));
        assert_eq!(snap.models[0].max_completion_tokens, Some(32768));
        assert_eq!(snap.models[0].description.as_deref(), Some("x"));
    }

    #[test]
    fn model_ids_with_dots_are_quoted_and_round_trip() {
        let upsert = ModelUpsert {
            model_id: "grok-4.5".into(),
            api_model: Some("grok-4.5".into()),
            ..Default::default()
        };
        let out = upsert_model_config("", &upsert);
        assert!(out.starts_with("[model.\"grok-4.5\"]"));
        let snapshot = parse_grok_config(&out);
        assert_eq!(snapshot.models[0].id, "grok-4.5");
    }
}
