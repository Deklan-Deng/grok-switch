use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenProfile {
    pub id: Uuid,
    /// Local label in the app (can mirror TOML `name`).
    pub name: String,
    #[serde(default = "default_model_id")]
    pub model_id: String,
    /// TOML `model` — identifier sent to the API.
    #[serde(default)]
    pub api_model: Option<String>,
    #[serde(default = "default_config_path")]
    pub config_path: String,
    #[serde(default)]
    pub base_url: Option<String>,
    /// TOML `name` — picker display name.
    #[serde(default)]
    pub model_alias: Option<String>,
    /// TOML `description`.
    #[serde(default)]
    pub description: Option<String>,
    /// TOML `env_key`.
    #[serde(default)]
    pub env_key: Option<String>,
    /// TOML `api_backend`.
    #[serde(default)]
    pub api_backend: Option<String>,
    /// TOML `context_window`.
    #[serde(default)]
    pub context_window: Option<u64>,
    /// TOML `max_completion_tokens`.
    #[serde(default)]
    pub max_completion_tokens: Option<u64>,
    #[serde(default = "default_true")]
    pub set_as_default: bool,
    #[serde(default)]
    pub token_saved: Option<bool>,
    #[serde(default = "now_ms")]
    pub updated_at: i64,
}

impl TokenProfile {
    /// New local card with empty fields — examples belong in UI placeholders only.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            model_id: String::new(),
            api_model: None,
            config_path: default_config_path(),
            base_url: None,
            model_alias: None,
            description: None,
            env_key: None,
            api_backend: Some("responses".into()),
            context_window: None,
            max_completion_tokens: None,
            set_as_default: true,
            token_saved: None,
            updated_at: now_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokModel {
    pub id: String,
    pub model: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub base_url: Option<String>,
    pub env_key: Option<String>,
    pub api_backend: Option<String>,
    pub context_window: Option<u64>,
    pub max_completion_tokens: Option<u64>,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSnapshot {
    pub models: Vec<GrokModel>,
    pub default_model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub profiles: Vec<TokenProfile>,
    pub selected_id: Option<Uuid>,
    #[serde(default)]
    pub current_id: Option<Uuid>,
}

impl Default for AppState {
    fn default() -> Self {
        let profile = TokenProfile::new("新供应商");
        let selected_id = Some(profile.id);
        Self {
            profiles: vec![profile],
            selected_id,
            current_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    pub status: String,
    pub profiles: Vec<TokenProfile>,
    pub selected_id: Option<Uuid>,
    pub current_id: Option<Uuid>,
    pub discovered_models: Vec<GrokModel>,
    pub default_model_id: Option<String>,
    pub available_model_ids: Vec<String>,
    pub preview: Option<String>,
    pub token: Option<String>,
    pub config_text: Option<String>,
    pub config_path: Option<String>,
    pub busy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProviderInput {
    pub name: String,
    pub model_id: String,
    #[serde(default)]
    pub api_model: Option<String>,
    #[serde(default)]
    pub model_alias: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub env_key: Option<String>,
    #[serde(default)]
    pub api_backend: Option<String>,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub max_completion_tokens: Option<u64>,
    #[serde(default)]
    pub config_path: Option<String>,
    #[serde(default = "default_true")]
    pub set_as_default: bool,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub enable: bool,
}

fn default_model_id() -> String {
    String::new()
}

fn default_true() -> bool {
    true
}

fn default_config_path() -> String {
    dirs::home_dir()
        .map(|h| h.join(".grok").join("config.toml").to_string_lossy().into_owned())
        .unwrap_or_else(|| "~/.grok/config.toml".into())
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
