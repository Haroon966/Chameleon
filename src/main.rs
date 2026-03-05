//! Minimal terminal emulator: PTY + shell + keyboard → vte parser → crossterm display.
//!
//! Architecture:
//! - Main thread: crossterm raw mode + alternate screen, event loop (keyboard + resize),
//!   writes input to PTY master, redraws from shared screen buffer on timeout or when dirty.
//! - Reader thread: reads from PTY master, feeds bytes into vte::Parser, which calls our
//!   Perform impl to update the shared screen buffer; then signals redraw.
//! - On resize: update PTY size and clear/redraw.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::{
    cursor,
    event::{
        self,
        Event,
        KeyCode,
        KeyEvent,
        KeyModifiers,
        MouseButton,
        MouseEventKind,
    },
    execute, queue,
    terminal::{self, ClearType},
};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use vte::{Params, Perform, Parser};

// -----------------------------------------------------------------------------
// Theme (user-editable via config file)
// -----------------------------------------------------------------------------

/// Parses a hex color string "#rrggbb" or "rrggbb" into (r, g, b). Returns None if invalid.
fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some((r, g, b))
}

const DEFAULT_FG: (u8, u8, u8) = (0xcc, 0xcc, 0xcc);
const DEFAULT_BG: (u8, u8, u8) = (0x1e, 0x1e, 0x1e);

#[derive(Clone, Debug)]
struct Theme {
    default_foreground: (u8, u8, u8),
    default_background: (u8, u8, u8),
    background_opacity: f32,
    font_size: u8,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            default_foreground: DEFAULT_FG,
            default_background: DEFAULT_BG,
            background_opacity: 0.95,
            font_size: 14,
        }
    }
}

#[derive(serde::Deserialize, Default)]
struct ThemeConfigFile {
    #[serde(rename = "theme")]
    theme: Option<ThemeSection>,
    ai: Option<AiSection>,
}

#[derive(serde::Deserialize)]
struct ThemeSection {
    default_foreground: Option<String>,
    default_background: Option<String>,
    background_opacity: Option<f32>,
    font_size: Option<u8>,
}

#[derive(serde::Deserialize, Default, Clone, Debug)]
struct ProviderConfig {
    api_key: Option<String>,
    base_url: Option<String>,
    #[allow(dead_code)]
    model: Option<String>,
}

#[derive(serde::Deserialize, Default, Clone, Debug)]
struct AiProvidersSection {
    openai: Option<ProviderConfig>,
    gemini: Option<ProviderConfig>,
    groq: Option<ProviderConfig>,
}

#[derive(serde::Deserialize)]
struct AiSection {
    model: Option<String>,
    backend: Option<String>,
    base_url: Option<String>,
    #[serde(default)]
    providers: Option<AiProvidersSection>,
}

/// AI backend (Ollama = local; others = API with key).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AiBackend {
    Ollama,
    OpenAi,
    Gemini,
    Groq,
}

impl std::fmt::Display for AiBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiBackend::Ollama => write!(f, "Ollama"),
            AiBackend::OpenAi => write!(f, "OpenAI"),
            AiBackend::Gemini => write!(f, "Gemini"),
            AiBackend::Groq => write!(f, "Groq"),
        }
    }
}

impl AiBackend {
    fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "ollama" => Some(AiBackend::Ollama),
            "openai" => Some(AiBackend::OpenAi),
            "gemini" => Some(AiBackend::Gemini),
            "groq" => Some(AiBackend::Groq),
            _ => None,
        }
    }
}

/// Loaded AI config: Ollama base URL, default backend/model, per-provider keys.
#[derive(Clone, Debug)]
struct AiConfig {
    ollama_base_url: String,
    default_backend: AiBackend,
    default_model: Option<String>,
    providers: AiProvidersSection,
}

fn load_ai_config() -> AiConfig {
    let default_url = "http://127.0.0.1:11434".to_string();
    let config_path = match directories::ProjectDirs::from("", "", "chameleon") {
        Some(dirs) => dirs.config_dir().join("config.toml"),
        None => {
            return AiConfig {
                ollama_base_url: default_url.clone(),
                default_backend: AiBackend::Ollama,
                default_model: None,
                providers: AiProvidersSection::default(),
            }
        }
    };
    let contents = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => {
            return AiConfig {
                ollama_base_url: default_url.clone(),
                default_backend: AiBackend::Ollama,
                default_model: None,
                providers: AiProvidersSection::default(),
            }
        }
    };
    let file_config: ThemeConfigFile = match toml::from_str(&contents) {
        Ok(c) => c,
        Err(_) => {
            return AiConfig {
                ollama_base_url: default_url.clone(),
                default_backend: AiBackend::Ollama,
                default_model: None,
                providers: AiProvidersSection::default(),
            }
        }
    };
    let section = match file_config.ai {
        Some(s) => s,
        None => {
            return AiConfig {
                ollama_base_url: default_url.clone(),
                default_backend: AiBackend::Ollama,
                default_model: None,
                providers: AiProvidersSection::default(),
            }
        }
    };
    let ollama_base_url = section
        .base_url
        .unwrap_or_else(|| default_url.clone());
    let default_backend = section
        .backend
        .as_deref()
        .and_then(AiBackend::from_str)
        .unwrap_or(AiBackend::Ollama);
    let providers = section.providers.unwrap_or_default();
    AiConfig {
        ollama_base_url,
        default_backend,
        default_model: section.model,
        providers,
    }
}

impl AiConfig {
    fn openai_api_key(&self) -> Option<String> {
        std::env::var("OPENAI_API_KEY").ok().or_else(|| {
            self.providers
                .openai
                .as_ref()
                .and_then(|p| p.api_key.clone())
        })
    }
    fn gemini_api_key(&self) -> Option<String> {
        std::env::var("GEMINI_API_KEY").ok().or_else(|| {
            self.providers
                .gemini
                .as_ref()
                .and_then(|p| p.api_key.clone())
        })
    }
    fn groq_api_key(&self) -> Option<String> {
        std::env::var("GROQ_API_KEY").ok().or_else(|| {
            self.providers
                .groq
                .as_ref()
                .and_then(|p| p.api_key.clone())
        })
    }
    fn is_configured(&self, backend: AiBackend) -> bool {
        match backend {
            AiBackend::Ollama => true,
            AiBackend::OpenAi => self.openai_api_key().is_some(),
            AiBackend::Gemini => self.gemini_api_key().is_some(),
            AiBackend::Groq => self.groq_api_key().is_some(),
        }
    }
    fn openai_base_url(&self) -> String {
        self.providers
            .openai
            .as_ref()
            .and_then(|p| p.base_url.clone())
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string())
    }
    fn groq_base_url(&self) -> String {
        self.providers
            .groq
            .as_ref()
            .and_then(|p| p.base_url.clone())
            .unwrap_or_else(|| "https://api.groq.com/openai/v1".to_string())
    }
}

/// Entry in the backend picker: either a backend, "Configure API", or "Remove API".
#[derive(Clone, Debug)]
enum BackendChoice {
    Backend(AiBackend),
    ConfigureApi,
    RemoveApi,
}

/// Build list of backends to show: Ollama if local models exist, each API if configured, Configure API, Remove API (if any API configured).
fn available_backends(ai_config: &AiConfig) -> Vec<BackendChoice> {
    let mut choices = Vec::new();
    if ollama_list_models(&ai_config.ollama_base_url)
        .map(|l| !l.is_empty())
        .unwrap_or(false)
    {
        choices.push(BackendChoice::Backend(AiBackend::Ollama));
    }
    if ai_config.is_configured(AiBackend::OpenAi) {
        choices.push(BackendChoice::Backend(AiBackend::OpenAi));
    }
    if ai_config.is_configured(AiBackend::Gemini) {
        choices.push(BackendChoice::Backend(AiBackend::Gemini));
    }
    if ai_config.is_configured(AiBackend::Groq) {
        choices.push(BackendChoice::Backend(AiBackend::Groq));
    }
    choices.push(BackendChoice::ConfigureApi);
    if ai_config.is_configured(AiBackend::OpenAi)
        || ai_config.is_configured(AiBackend::Gemini)
        || ai_config.is_configured(AiBackend::Groq)
    {
        choices.push(BackendChoice::RemoveApi);
    }
    choices
}

// -----------------------------------------------------------------------------
// Ollama: detect and list models
// -----------------------------------------------------------------------------

const OLLAMA_TIMEOUT_MS: u64 = 5000;

/// Check if Ollama is reachable and return list of model names. Empty list if none.
fn ollama_list_models(base_url: &str) -> Result<Vec<String>, String> {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let response = ureq::get(&url)
        .timeout(std::time::Duration::from_millis(OLLAMA_TIMEOUT_MS))
        .call()
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = response
        .into_json()
        .map_err(|e| e.to_string())?;
    let empty: Vec<serde_json::Value> = Vec::new();
    let models = body
        .get("models")
        .and_then(|m| m.as_array())
        .unwrap_or(&empty);
    let names: Vec<String> = models
        .iter()
        .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
        .collect();
    Ok(names)
}

/// Resolve model to use: config model if set and available, else first from list, else error.
fn ollama_resolve_model(base_url: &str, config_model: Option<&str>) -> Result<String, String> {
    let list = ollama_list_models(base_url)?;
    if list.is_empty() {
        return Err("No models found. Pull a model with: ollama pull <name>".to_string());
    }
    if let Some(want) = config_model {
        if list.iter().any(|n| n == want) {
            return Ok(want.to_string());
        }
        return Err(format!("Configured model '{want}' not found. Available: {}", list.join(", ")));
    }
    Ok(list.into_iter().next().unwrap())
}

const OLLAMA_GENERATE_TIMEOUT_MS: u64 = 30_000;

/// Build prompt for Ollama: system instruction + user message. Ollama uses a single "prompt" field.
fn ollama_build_prompt(system: &str, user: &str) -> String {
    format!("{}\n\n{}", system, user)
}

/// Call Ollama /api/generate, return the response text (one command). Strips markdown code blocks.
fn ollama_generate(base_url: &str, model: &str, prompt: &str) -> Result<String, String> {
    let url = format!("{}/api/generate", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": false
    });
    let response = ureq::post(&url)
        .timeout(std::time::Duration::from_millis(OLLAMA_GENERATE_TIMEOUT_MS))
        .send_json(body)
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = response.into_json().map_err(|e| e.to_string())?;
    let text = body
        .get("response")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    // Strip markdown code blocks if present
    let text = text
        .strip_prefix("```")
        .and_then(|s| s.strip_suffix("```"))
        .map(|s| s.trim())
        .unwrap_or(text.as_str())
        .to_string();
    let text = text
        .strip_prefix("bash")
        .or_else(|| text.strip_prefix("sh"))
        .map(|s| s.trim())
        .unwrap_or(text.as_str())
        .to_string();
    if text.is_empty() {
        return Err("Model returned empty response".to_string());
    }
    Ok(text)
}

// -----------------------------------------------------------------------------
// OpenAI: chat completions (and OpenAI-compatible, e.g. Groq)
// -----------------------------------------------------------------------------

const OPENAI_TIMEOUT_MS: u64 = 30_000;

fn strip_code_blocks(text: &str) -> String {
    let text = text.trim().to_string();
    let text = text
        .strip_prefix("```")
        .and_then(|s| s.strip_suffix("```"))
        .map(|s| s.trim())
        .unwrap_or(text.as_str())
        .to_string();
    let text = text
        .strip_prefix("bash")
        .or_else(|| text.strip_prefix("sh"))
        .map(|s| s.trim())
        .unwrap_or(text.as_str())
        .to_string();
    text
}

/// OpenAI (and compatible) chat completions; returns single message content.
fn openai_generate(
    base_url: &str,
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
        "max_tokens": 500
    });
    let response = ureq::post(&url)
        .set("Authorization", &format!("Bearer {}", api_key))
        .timeout(std::time::Duration::from_millis(OPENAI_TIMEOUT_MS))
        .send_json(body)
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = response.into_json().map_err(|e| e.to_string())?;
    let text = body
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let text = strip_code_blocks(&text);
    if text.is_empty() {
        return Err("Model returned empty response".to_string());
    }
    Ok(text)
}

/// List OpenAI models (ids). Falls back to a static list on error.
fn openai_list_models(base_url: &str, api_key: &str) -> Vec<String> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let response = ureq::get(&url)
        .set("Authorization", &format!("Bearer {}", api_key))
        .timeout(std::time::Duration::from_millis(OLLAMA_TIMEOUT_MS))
        .call();
    match response {
        Ok(resp) => {
            let body: serde_json::Value = match resp.into_json() {
                Ok(b) => b,
                Err(_) => return openai_default_models(),
            };
            let data = match body.get("data").and_then(|d| d.as_array()) {
                Some(d) => d,
                None => return openai_default_models(),
            };
            let names: Vec<String> = data
                .iter()
                .filter_map(|m| m.get("id").and_then(|id| id.as_str()).map(String::from))
                .filter(|id| id.starts_with("gpt-") || id.contains("gpt"))
                .collect();
            if names.is_empty() {
                openai_default_models()
            } else {
                names
            }
        }
        Err(_) => openai_default_models(),
    }
}

fn openai_default_models() -> Vec<String> {
    vec![
        "gpt-4o".to_string(),
        "gpt-4o-mini".to_string(),
        "gpt-4-turbo".to_string(),
        "gpt-3.5-turbo".to_string(),
    ]
}

// -----------------------------------------------------------------------------
// Gemini: generate and model list (static; API differs from OpenAI)
// -----------------------------------------------------------------------------

fn gemini_list_models() -> Vec<String> {
    vec![
        "gemini-2.0-flash".to_string(),
        "gemini-1.5-pro".to_string(),
        "gemini-1.5-flash".to_string(),
        "gemini-1.0-pro".to_string(),
    ]
}

/// Gemini generate via REST (generativelanguage.googleapis.com). Uses generateContent.
fn gemini_generate(api_key: &str, model: &str, system: &str, user: &str) -> Result<String, String> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, api_key
    );
    let contents = vec![
        serde_json::json!({ "role": "user", "parts": [{ "text": system }] }),
        serde_json::json!({ "role": "user", "parts": [{ "text": user }] }),
    ];
    let body = serde_json::json!({
        "contents": contents,
        "generationConfig": { "maxOutputTokens": 500 }
    });
    let response = ureq::post(&url)
        .timeout(std::time::Duration::from_millis(OPENAI_TIMEOUT_MS))
        .send_json(body)
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = response.into_json().map_err(|e| e.to_string())?;
    let text = body
        .get("candidates")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
        .and_then(|a| a.first())
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let text = strip_code_blocks(&text);
    if text.is_empty() {
        return Err("Model returned empty response".to_string());
    }
    Ok(text)
}

// -----------------------------------------------------------------------------
// Groq: OpenAI-compatible API
// -----------------------------------------------------------------------------

fn groq_list_models() -> Vec<String> {
    vec![
        "llama-3.3-70b-versatile".to_string(),
        "llama-3.1-8b-instant".to_string(),
        "mixtral-8x7b-32768".to_string(),
    ]
}

fn load_theme() -> Theme {
    let mut theme = Theme::default();
    let config_path = match directories::ProjectDirs::from("", "", "chameleon") {
        Some(dirs) => dirs.config_dir().join("config.toml"),
        None => return theme,
    };
    let contents = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return theme,
    };
    let file_config: ThemeConfigFile = match toml::from_str(&contents) {
        Ok(c) => c,
        Err(_) => return theme,
    };
    let Some(section) = file_config.theme else {
        return theme;
    };
    if let Some(ref s) = section.default_foreground {
        if let Some(rgb) = parse_hex(s) {
            theme.default_foreground = rgb;
        }
    }
    if let Some(ref s) = section.default_background {
        if let Some(rgb) = parse_hex(s) {
            theme.default_background = rgb;
        }
    }
    if let Some(o) = section.background_opacity {
        theme.background_opacity = o.clamp(0.0, 1.0);
    }
    if let Some(f) = section.font_size {
        theme.font_size = f.min(72).max(6);
    }
    theme
}

/// Returns the config file path for the theme (for opening in editor). May be None.
fn theme_config_path() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("", "", "chameleon")
        .map(|d| d.config_dir().join("config.toml"))
}

/// Write a provider's API key into config.toml (merge with existing). Key is stored under [ai.providers.<provider>].
fn write_provider_api_key(provider: &str, api_key: &str) -> Result<(), String> {
    let config_path = match theme_config_path() {
        Some(p) => p,
        None => return Err("No config directory".to_string()),
    };
    let contents = std::fs::read_to_string(&config_path).unwrap_or_else(|_| String::new());
    let contents = if contents.trim().is_empty() {
        DEFAULT_CONFIG.to_string()
    } else {
        contents
    };
    let mut root: toml::Value = toml::from_str(&contents).unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new()));
    let root_table = root.as_table_mut().ok_or("config root is not a table")?;
    let ai_table = root_table
        .entry("ai".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let ai = ai_table.as_table_mut().ok_or("ai is not a table")?;
    let providers_table = ai
        .entry("providers".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let providers = providers_table.as_table_mut().ok_or("ai.providers is not a table")?;
    let provider_table = providers
        .entry(provider.to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let prov = provider_table.as_table_mut().ok_or("provider entry is not a table")?;
    prov.insert("api_key".to_string(), toml::Value::String(api_key.to_string()));
    let new_contents = toml::ser::to_string_pretty(&root).map_err(|e| e.to_string())?;
    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&config_path, new_contents).map_err(|e| e.to_string())?;
    Ok(())
}

/// Remove a provider's API key from config.toml (deletes [ai.providers.<provider>]).
/// If the key was set only via env var, the backend will still appear until the env var is unset.
fn remove_provider_api_key(provider: &str) -> Result<(), String> {
    let config_path = match theme_config_path() {
        Some(p) => p,
        None => return Err("No config directory".to_string()),
    };
    let contents = std::fs::read_to_string(&config_path).unwrap_or_else(|_| String::new());
    let contents = if contents.trim().is_empty() {
        return Ok(());
    } else {
        contents
    };
    let mut root: toml::Value = toml::from_str(&contents).map_err(|e| e.to_string())?;
    let root_table = root.as_table_mut().ok_or("config root is not a table")?;
    let Some(ai_table) = root_table.get_mut("ai") else {
        return Ok(());
    };
    let ai = ai_table.as_table_mut().ok_or("ai is not a table")?;
    let Some(providers_table) = ai.get_mut("providers") else {
        return Ok(());
    };
    let providers = providers_table.as_table_mut().ok_or("ai.providers is not a table")?;
    providers.remove(provider);
    let new_contents = toml::ser::to_string_pretty(&root).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, new_contents).map_err(|e| e.to_string())?;
    Ok(())
}

/// Default config content written when the file does not exist.
const DEFAULT_CONFIG: &str = r##"# Chameleon theme — edit and save; press Ctrl+Shift+T to reopen this file.
[theme]
default_foreground = "#cccccc"
default_background = "#1e1e1e"
background_opacity = 0.95
font_size = 14
"##;

/// Open the theme config file in $EDITOR, then reload theme into `theme`. Restores terminal state
/// (alternate screen, raw mode) after the editor exits.
fn open_theme_config_and_reload(
    stdout: &mut io::Stdout,
    theme: &Arc<Mutex<Theme>>,
) -> io::Result<()> {
    let config_path = match theme_config_path() {
        Some(p) => p,
        None => return Ok(()),
    };
    // Restore normal terminal so the editor gets a usable TTY
    execute!(
        stdout,
        event::DisableMouseCapture,
        cursor::Show,
        terminal::LeaveAlternateScreen
    )?;
    stdout.flush()?;
    let _ = terminal::disable_raw_mode();

    // Ensure config dir and default file exist
    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
        if !config_path.exists() {
            let _ = std::fs::write(&config_path, DEFAULT_CONFIG);
        }
    }

    let editor = std::env::var("EDITOR")
        .unwrap_or_else(|_| std::env::var("VISUAL").unwrap_or_else(|_| "nano".to_string()));
    let parts: Vec<&str> = editor.split_whitespace().collect();
    let (bin, args) = parts
        .split_first()
        .map(|(b, rest)| (*b, rest))
        .unwrap_or(("nano", &[][..]));
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args).arg(&config_path);
    let _ = cmd.status();

    let _ = terminal::enable_raw_mode();
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
    let new_theme = load_theme();
    if let Ok(mut t) = theme.lock() {
        *t = new_theme;
    }
    execute!(stdout, terminal::Clear(ClearType::All))?;
    if let Ok(t) = theme.lock() {
        let (r, g, b) = t.default_background;
        execute!(
            stdout,
            crossterm::style::SetBackgroundColor(crossterm::style::Color::Rgb { r, g, b })
        )?;
    }
    execute!(stdout, event::EnableMouseCapture)?;
    stdout.flush()?;
    Ok(())
}

// -----------------------------------------------------------------------------
// Screen buffer (shared between vte Perform and main thread)
// -----------------------------------------------------------------------------

/// Single cell: character and basic attributes (minimal — no truecolor).
#[derive(Clone, Copy, Debug)]
struct Cell {
    ch: char,
    fg: u8, // 0–7 standard colors (we map to crossterm)
    bg: u8,
    bold: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: 7,
            bg: 0,
            bold: false,
        }
    }
}

/// Terminal screen state: grid + cursor + size. Protected by Mutex for reader thread.
struct Screen {
    /// Grid [row][col]. Row 0 = top.
    grid: Vec<Vec<Cell>>,
    rows: usize,
    cols: usize,
    cursor_row: usize,
    cursor_col: usize,
    /// Current attributes for new characters
    cur_fg: u8,
    cur_bg: u8,
    cur_bold: bool,
}

impl Screen {
    fn new(rows: usize, cols: usize) -> Self {
        let mut grid = Vec::with_capacity(rows);
        for _ in 0..rows {
            grid.push(vec![Cell::default(); cols]);
        }
        Self {
            grid,
            rows,
            cols,
            cursor_row: 0,
            cursor_col: 0,
            cur_fg: 7,
            cur_bg: 0,
            cur_bold: false,
        }
    }

    fn resize(&mut self, rows: usize, cols: usize) {
        self.rows = rows;
        self.cols = cols;
        self.grid.resize(rows, Vec::new());
        for row in &mut self.grid {
            row.resize(cols, Cell::default());
        }
        self.clamp_cursor();
    }

    fn clamp_cursor(&mut self) {
        if self.cursor_row >= self.rows {
            self.cursor_row = self.rows.saturating_sub(1);
        }
        if self.cursor_col >= self.cols {
            self.cursor_col = self.cols.saturating_sub(1);
        }
    }

    fn put_cell(&mut self, row: usize, col: usize, cell: Cell) {
        if row < self.rows && col < self.cols {
            self.grid[row][col] = cell;
        }
    }

    fn put_char(&mut self, c: char) {
        if self.cursor_row >= self.rows || self.cursor_col >= self.cols {
            return;
        }
        self.grid[self.cursor_row][self.cursor_col] = Cell {
            ch: c,
            fg: self.cur_fg,
            bg: self.cur_bg,
            bold: self.cur_bold,
        };
        self.cursor_col += 1;
        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.cursor_row += 1;
            if self.cursor_row >= self.rows {
                self.scroll_up();
                self.cursor_row = self.rows.saturating_sub(1);
            }
        }
    }

    fn scroll_up(&mut self) {
        if self.rows == 0 {
            return;
        }
        self.grid.remove(0);
        self.grid.push(vec![Cell::default(); self.cols]);
    }

    fn scroll_down(&mut self) {
        if self.rows == 0 {
            return;
        }
        self.grid.pop();
        self.grid.insert(0, vec![Cell::default(); self.cols]);
    }

    fn erase_from_cursor_to_end_of_screen(&mut self) {
        for r in self.cursor_row..self.rows {
            for c in 0..self.cols {
                let col_start = if r == self.cursor_row { self.cursor_col } else { 0 };
                if c >= col_start {
                    self.put_cell(r, c, Cell::default());
                }
            }
        }
    }

    fn erase_from_start_to_cursor(&mut self) {
        for r in 0..=self.cursor_row {
            let col_end = if r == self.cursor_row {
                self.cursor_col.saturating_add(1)
            } else {
                self.cols
            };
            for c in 0..col_end {
                self.put_cell(r, c, Cell::default());
            }
        }
    }

    fn erase_entire_screen(&mut self) {
        for row in &mut self.grid {
            for c in row.iter_mut() {
                *c = Cell::default();
            }
        }
    }

    fn erase_from_cursor_to_end_of_line(&mut self) {
        if self.cursor_row < self.rows {
            for c in self.cursor_col..self.cols {
                self.put_cell(self.cursor_row, c, Cell::default());
            }
        }
    }

    fn erase_from_start_to_cursor_in_line(&mut self) {
        if self.cursor_row < self.rows {
            for c in 0..=self.cursor_col {
                self.put_cell(self.cursor_row, c, Cell::default());
            }
        }
    }

    fn erase_entire_line(&mut self) {
        if self.cursor_row < self.rows {
            for c in 0..self.cols {
                self.put_cell(self.cursor_row, c, Cell::default());
            }
        }
    }

    /// Last N rows of the grid as plain text (trimmed, newline-joined). Reserved for future use.
    #[allow(dead_code)]
    fn get_recent_text(&self, last_n_rows: usize) -> String {
        let start = self.rows.saturating_sub(last_n_rows);
        let mut lines = Vec::new();
        for r in start..self.rows {
            if r < self.grid.len() {
                let row = &self.grid[r];
                let line: String = row.iter().map(|c| c.ch).collect::<String>().trim_end().to_string();
                lines.push(line);
            }
        }
        lines.join("\n")
    }

}

// -----------------------------------------------------------------------------
// Helper: collect CSI parameters as u16s (vte Params iter yields &[u16] subparameters)
// -----------------------------------------------------------------------------

fn params_to_vec(params: &Params) -> Vec<u16> {
    let mut v = Vec::new();
    for p in params.iter() {
        v.extend_from_slice(p);
    }
    v
}

// -----------------------------------------------------------------------------
// Perform implementation (vte → screen buffer)
// -----------------------------------------------------------------------------

struct TerminalPerform {
    screen: Arc<Mutex<Screen>>,
}

impl Perform for TerminalPerform {
    fn print(&mut self, c: char) {
        if let Ok(mut s) = self.screen.lock() {
            s.put_char(c);
        }
    }

    fn execute(&mut self, byte: u8) {
        if let Ok(mut s) = self.screen.lock() {
            match byte {
                0x07 => {} // BEL - ignore or beep
                0x08 => {
                    // BS
                    s.cursor_col = s.cursor_col.saturating_sub(1);
                }
                0x09 => {
                    // TAB - advance to next multiple of 8
                    s.cursor_col = (s.cursor_col + 8) / 8 * 8;
                    if s.cursor_col >= s.cols {
                        s.cursor_col = 0;
                        s.cursor_row += 1;
                        if s.cursor_row >= s.rows {
                            s.scroll_up();
                            s.cursor_row = s.rows.saturating_sub(1);
                        }
                    }
                }
                0x0a | 0x0b | 0x0c => {
                    // LF, VT, FF
                    s.cursor_row += 1;
                    if s.cursor_row >= s.rows {
                        s.scroll_up();
                        s.cursor_row = s.rows.saturating_sub(1);
                    }
                }
                0x0d => {
                    // CR
                    s.cursor_col = 0;
                }
                _ => {}
            }
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        if let Ok(mut s) = self.screen.lock() {
            let p = params_to_vec(params);
            let default = |i: usize| p.get(i).copied().unwrap_or(1);

            match action {
                'H' | 'f' => {
                    // CUP - cursor position (1-based)
                    let row = default(0).saturating_sub(1) as usize;
                    let col = default(1).saturating_sub(1) as usize;
                    s.cursor_row = row.min(s.rows.saturating_sub(1));
                    s.cursor_col = col.min(s.cols.saturating_sub(1));
                }
                'A' => {
                    // CUU - cursor up
                    s.cursor_row = s.cursor_row.saturating_sub(default(0) as usize);
                }
                'B' => {
                    // CUD - cursor down
                    s.cursor_row = (s.cursor_row + default(0) as usize).min(s.rows.saturating_sub(1));
                }
                'C' => {
                    // CUF - cursor forward
                    s.cursor_col = (s.cursor_col + default(0) as usize).min(s.cols.saturating_sub(1));
                }
                'D' => {
                    // CUB - cursor back
                    s.cursor_col = s.cursor_col.saturating_sub(default(0) as usize);
                }
                'G' => {
                    // CHA - cursor horizontal absolute (1-based)
                    let col = default(0).saturating_sub(1) as usize;
                    s.cursor_col = col.min(s.cols.saturating_sub(1));
                }
                'd' => {
                    // VPA - line position absolute (1-based)
                    let row = default(0).saturating_sub(1) as usize;
                    s.cursor_row = row.min(s.rows.saturating_sub(1));
                }
                'J' => {
                    // ED - erase in display
                    match default(0) {
                        0 => s.erase_from_cursor_to_end_of_screen(),
                        1 => s.erase_from_start_to_cursor(),
                        2 => s.erase_entire_screen(),
                        _ => {}
                    }
                }
                'K' => {
                    // EL - erase in line
                    match default(0) {
                        0 => s.erase_from_cursor_to_end_of_line(),
                        1 => s.erase_from_start_to_cursor_in_line(),
                        2 => s.erase_entire_line(),
                        _ => {}
                    }
                }
                'm' => {
                    // SGR
                    let mut i = 0;
                    while i < p.len() {
                        let code = p[i];
                        match code {
                            0 => {
                                s.cur_fg = 7;
                                s.cur_bg = 0;
                                s.cur_bold = false;
                            }
                            1 => s.cur_bold = true,
                            7 => {} // reverse (skip for minimal)
                            27 => {} // not reverse
                            30..=37 => s.cur_fg = (code - 30) as u8,
                            38 => {
                                // set fg (skip 256/24bit for minimal)
                                if i + 1 < p.len() && p[i + 1] == 5 && i + 2 < p.len() {
                                    s.cur_fg = p[i + 2] as u8 % 8;
                                    i += 2;
                                }
                                i += 1;
                            }
                            39 => s.cur_fg = 7,
                            40..=47 => s.cur_bg = (code - 40) as u8,
                            48 => {
                                if i + 1 < p.len() && p[i + 1] == 5 && i + 2 < p.len() {
                                    s.cur_bg = p[i + 2] as u8 % 8;
                                    i += 2;
                                }
                                i += 1;
                            }
                            49 => s.cur_bg = 0,
                            _ => {}
                        }
                        i += 1;
                    }
                }
                _ => {}
            }
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        if let Ok(mut s) = self.screen.lock() {
            match byte {
                b'D' => {
                    // IND - index, scroll up / move down
                    s.cursor_row += 1;
                    if s.cursor_row >= s.rows {
                        s.scroll_up();
                        s.cursor_row = s.rows.saturating_sub(1);
                    }
                }
                b'M' => {
                    // RI - reverse index
                    if s.cursor_row > 0 {
                        s.cursor_row -= 1;
                    } else {
                        s.scroll_down();
                    }
                }
                b'E' => {
                    // NEL - next line
                    s.cursor_col = 0;
                    s.cursor_row += 1;
                    if s.cursor_row >= s.rows {
                        s.scroll_up();
                        s.cursor_row = s.rows.saturating_sub(1);
                    }
                }
                b'H' => {
                    // HT
                    s.cursor_col = (s.cursor_col + 8) / 8 * 8;
                    if s.cursor_col >= s.cols {
                        s.cursor_col = 0;
                        s.cursor_row += 1;
                        if s.cursor_row >= s.rows {
                            s.scroll_up();
                            s.cursor_row = s.rows.saturating_sub(1);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

// Fix Screen methods that take &mut self and extra arg (ED 1)
impl Screen {
}

// -----------------------------------------------------------------------------
// Selection (for copy)
// -----------------------------------------------------------------------------

/// Stream selection (traditional terminal): start and end position in reading order (row-major).
/// Selecting from (1,5) to (2,3) selects end of line 1 + start of line 2, not a rectangle.
#[derive(Clone, Copy, Debug, Default)]
struct Selection {
    start_row: usize,
    start_col: usize,
    end_row: usize,
    end_col: usize,
}

impl Selection {
    fn is_empty(&self) -> bool {
        self.start_row == self.end_row && self.start_col == self.end_col
    }

    /// True if (r1, c1) is before or equal to (r2, c2) in stream (reading) order.
    fn stream_order_before(r1: usize, c1: usize, r2: usize, c2: usize) -> bool {
        r1 < r2 || (r1 == r2 && c1 <= c2)
    }

    /// Normalize to (first_row, first_col, last_row, last_col) in stream order.
    fn normalized(&self) -> (usize, usize, usize, usize) {
        let (r1, c1, r2, c2) = if Self::stream_order_before(
            self.start_row, self.start_col,
            self.end_row, self.end_col,
        ) {
            (self.start_row, self.start_col, self.end_row, self.end_col)
        } else {
            (self.end_row, self.end_col, self.start_row, self.start_col)
        };
        (r1, c1, r2, c2)
    }

    /// True if cell (r, c) is inside the stream selection [first..=last].
    fn contains_cell(&self, r: usize, c: usize) -> bool {
        let (r1, c1, r2, c2) = self.normalized();
        let after_start = r > r1 || (r == r1 && c >= c1);
        let before_end = r < r2 || (r == r2 && c <= c2);
        after_start && before_end
    }

    /// Extract selected text in reading order (like traditional terminal copy).
    fn extract_from(&self, screen: &Screen) -> String {
        let (r1, c1, r2, c2) = self.normalized();
        let mut lines = Vec::new();
        for r in r1..=r2 {
            let start_c = if r == r1 { c1 } else { 0 };
            let end_c = if r == r2 { c2 } else { screen.cols.saturating_sub(1) };
            let mut line = String::new();
            if r < screen.grid.len() {
                for c in start_c..=end_c.min(screen.grid[r].len().saturating_sub(1)) {
                    line.push(screen.grid[r][c].ch);
                }
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }
}

/// True if the character is part of a "word" for double-click selection (alphanumeric + underscore).
fn is_word_char(ch: char) -> bool {
    ch.is_alphabetic() || ch.is_ascii_digit() || ch == '_'
}

/// Word at (row, col) in stream order: (start_row, start_col, end_row, end_col).
fn selection_word_at(screen: &Screen, row: usize, col: usize) -> (usize, usize, usize, usize) {
    if row >= screen.grid.len() {
        return (row, col, row, col);
    }
    let line = &screen.grid[row];
    if col >= line.len() {
        return (row, col, row, col);
    }
    if !is_word_char(line[col].ch) {
        return (row, col, row, col);
    }
    let mut start_col = col;
    while start_col > 0 && is_word_char(line[start_col - 1].ch) {
        start_col -= 1;
    }
    let mut end_col = col;
    while end_col + 1 < line.len() && is_word_char(line[end_col + 1].ch) {
        end_col += 1;
    }
    (row, start_col, row, end_col)
}

/// Full line at row in stream order: (start_row, start_col, end_row, end_col).
fn selection_line_at(screen: &Screen, row: usize) -> (usize, usize, usize, usize) {
    let end_col = if row < screen.grid.len() {
        screen.grid[row].len().saturating_sub(1)
    } else {
        0
    };
    (row, 0, row, end_col)
}

// -----------------------------------------------------------------------------
// AI mode and bar (command generation, suggest fix)
// -----------------------------------------------------------------------------

/// Step in the Configure API wizard.
#[derive(Clone, Debug)]
enum ConfigApiStep {
    ChooseProvider { selected: usize },
    EnterKey {
        provider: AiBackend,
        key_buffer: String,
    },
    Done { message: String },
}

#[derive(Clone, Debug)]
enum AiMode {
    Idle,
    PromptInput { buffer: String },
    Thinking,
    SuggestionReady { command: String },
    Error { message: String },
    BackendPicker {
        choices: Vec<BackendChoice>,
        selected: usize,
    },
    ModelPicker {
        backend: AiBackend,
        models: Vec<String>,
        selected: usize,
    },
    ConfigApiWizard { step: ConfigApiStep },
    RemoveApiPicker {
        backends: Vec<AiBackend>,
        selected: usize,
    },
}

const AI_BOX_WIDTH: usize = 64;
const AI_BOX_HEIGHT: usize = 8;

/// Renders the AI command modal: clean centered box with header, input area, and footer.
fn render_ai_bar(
    ai_mode: &AiMode,
    term_rows: usize,
    term_cols: usize,
    theme: &Theme,
    current_model: Option<&str>,
    stdout: &mut io::Stdout,
) -> io::Result<()> {
    if matches!(ai_mode, AiMode::Idle) {
        return Ok(());
    }
    let w = AI_BOX_WIDTH.min(term_cols.saturating_sub(2));
    let h = AI_BOX_HEIGHT.min(term_rows.saturating_sub(2));
    let start_col = term_cols.saturating_sub(w) / 2;
    let start_row = term_rows.saturating_sub(h);
    let w_u = w as u16;
    let h_u = h as u16;
    let inner_w = w.saturating_sub(2);

    let fg = crossterm::style::Color::Rgb {
        r: theme.default_foreground.0,
        g: theme.default_foreground.1,
        b: theme.default_foreground.2,
    };
    let bg = crossterm::style::Color::Rgb {
        r: theme.default_background.0,
        g: theme.default_background.1,
        b: theme.default_background.2,
    };
    let dim = crossterm::style::Color::Rgb { r: 0x66, g: 0x66, b: 0x66 };

    let top_left = start_col as u16;
    let top_row = start_row as u16;

    // ─── Top border (rounded) ───
    queue!(stdout, cursor::MoveTo(top_left, top_row), crossterm::style::SetForegroundColor(dim), crossterm::style::SetBackgroundColor(bg))?;
    queue!(stdout, crossterm::style::Print('╭'), crossterm::style::Print("─".repeat(inner_w)), crossterm::style::Print('╮'))?;

    // Row 1: title + optional model name + close
    let r1 = top_row + 1;
    let title = match ai_mode {
        AiMode::BackendPicker { .. } => "  Select backend",
        AiMode::ModelPicker { .. } => "  Select model",
        AiMode::ConfigApiWizard { .. } => "  Configure API",
        AiMode::RemoveApiPicker { .. } => "  Remove API",
        _ => "  Command instructions",
    };
    queue!(stdout, cursor::MoveTo(top_left, r1), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
    queue!(stdout, crossterm::style::SetForegroundColor(fg), crossterm::style::Print(title))?;
    let mut used = title.len();
    if let Some(model) = current_model {
        let model_label = format!(" · {}", model);
        let max_model = inner_w.saturating_sub(used).saturating_sub(6);
        let show = if model_label.len() > max_model {
            format!("{}…", &model_label[..max_model.saturating_sub(1)])
        } else {
            model_label
        };
        queue!(stdout, crossterm::style::SetForegroundColor(dim), crossterm::style::Print(&show))?;
        used += show.len();
    }
    let gap = inner_w.saturating_sub(used).saturating_sub(6);
    queue!(stdout, crossterm::style::SetForegroundColor(bg), crossterm::style::Print(" ".repeat(gap)))?;
    queue!(stdout, cursor::MoveTo(top_left + w_u - 6, r1), crossterm::style::SetForegroundColor(dim), crossterm::style::Print(" Esc │"))?;

    // Row 2: separator
    let r2 = top_row + 2;
    queue!(stdout, cursor::MoveTo(top_left, r2), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('├'), crossterm::style::Print("─".repeat(inner_w)), crossterm::style::Print('┤'))?;

    // Row 3: main content (input or message)
    let r3 = top_row + 3;
    let (line1, line2, show_placeholder) = match ai_mode {
        AiMode::PromptInput { buffer } => {
            let max_len = inner_w.saturating_sub(6);
            let display = if buffer.len() > max_len {
                format!("{}…", &buffer[buffer.len().saturating_sub(max_len.saturating_sub(1))..])
            } else {
                buffer.clone()
            };
            let line2 = current_model
                .map(|m| {
                    let s = format!("  model: {}", m);
                    if s.len() > inner_w {
                        format!("{}…", &s[..inner_w.saturating_sub(1)])
                    } else {
                        s
                    }
                })
                .unwrap_or_default();
            (format!("  › {}_", display), line2, buffer.is_empty())
        }
        AiMode::Thinking => ("  Thinking…".to_string(), String::new(), false),
        AiMode::SuggestionReady { command } => {
            let max_len = inner_w.saturating_sub(4);
            let display = if command.len() > max_len {
                format!("{}…", &command[..max_len.saturating_sub(1)])
            } else {
                command.clone()
            };
            (format!("  {}", display), "  Enter run · Esc dismiss".to_string(), false)
        }
        AiMode::Error { message } => {
            let max_len = inner_w.saturating_sub(10);
            let display = if message.len() > max_len {
                format!("{}…", &message[..max_len.saturating_sub(1)])
            } else {
                message.clone()
            };
            (format!("  {}", display), "  Esc to close".to_string(), false)
        }
        AiMode::BackendPicker { .. } => (String::new(), "  Enter select · Esc cancel".to_string(), false),
        AiMode::ModelPicker { .. } => (String::new(), "  Enter select · Esc cancel".to_string(), false),
        AiMode::RemoveApiPicker { .. } => (String::new(), "  Enter remove · Esc cancel".to_string(), false),
        AiMode::ConfigApiWizard { step } => match step {
            ConfigApiStep::ChooseProvider { .. } => (String::new(), "  Enter select · Esc cancel".to_string(), false),
            ConfigApiStep::EnterKey { provider, key_buffer } => {
                let prompt = format!("  API key for {}: {}_", provider, key_buffer);
                (prompt, "  Enter save · Esc cancel".to_string(), false)
            }
            ConfigApiStep::Done { message } => {
                let msg = if message.len() > inner_w.saturating_sub(4) {
                    format!("{}…", &message[..inner_w.saturating_sub(4)])
                } else {
                    message.clone()
                };
                (format!("  {}", msg), "  Enter to close".to_string(), false)
            }
        },
        AiMode::Idle => unreachable!(),
    };

    let content_rows: usize = 4;
    if let AiMode::BackendPicker { choices, selected } = ai_mode {
        let labels: Vec<String> = choices
            .iter()
            .map(|c| match c {
                BackendChoice::Backend(b) => b.to_string(),
                BackendChoice::ConfigureApi => "Configure API".to_string(),
                BackendChoice::RemoveApi => "Remove API".to_string(),
            })
            .collect();
        let visible_len = content_rows.saturating_sub(1).min(labels.len());
        let start = selected.saturating_sub(visible_len.saturating_sub(1)).min(labels.len().saturating_sub(visible_len).max(0));
        let end = (start + visible_len).min(labels.len());
        for (i, name) in labels[start..end].iter().enumerate() {
            let row = top_row + 3 + i as u16;
            let idx = start + i;
            let max_name_len = inner_w.saturating_sub(6);
            let truncated = if name.len() > max_name_len {
                format!("{}…", &name[..max_name_len])
            } else {
                name.clone()
            };
            queue!(stdout, cursor::MoveTo(top_left, row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
            if idx == *selected {
                queue!(stdout, crossterm::style::SetForegroundColor(fg), crossterm::style::Print("  › "))?;
                queue!(stdout, crossterm::style::Print(&truncated))?;
            } else {
                queue!(stdout, crossterm::style::SetForegroundColor(fg), crossterm::style::Print("    "))?;
                queue!(stdout, crossterm::style::Print(&truncated))?;
            }
            queue!(stdout, crossterm::style::Print(" ".repeat(inner_w.saturating_sub(4 + truncated.len()))))?;
            queue!(stdout, cursor::MoveTo(top_left + w_u - 1, row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        }
        let footer_row = top_row + 3 + visible_len as u16;
        queue!(stdout, cursor::MoveTo(top_left, footer_row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        queue!(stdout, crossterm::style::Print(&format!("{:<width$}", "  Enter select · Esc cancel", width = inner_w)))?;
        queue!(stdout, cursor::MoveTo(top_left + w_u - 1, footer_row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        for i in (visible_len + 1)..(h.saturating_sub(1).saturating_sub(3)) {
            let r = top_row + 3 + i as u16;
            queue!(stdout, cursor::MoveTo(top_left, r), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
            queue!(stdout, crossterm::style::Print(" ".repeat(inner_w)))?;
            queue!(stdout, cursor::MoveTo(top_left + w_u - 1, r), crossterm::style::Print('│'))?;
        }
    } else if let AiMode::RemoveApiPicker { backends, selected } = ai_mode {
        let labels: Vec<String> = backends.iter().map(|b| b.to_string()).collect();
        let visible_len = content_rows.saturating_sub(1).min(labels.len());
        let start = selected.saturating_sub(visible_len.saturating_sub(1)).min(labels.len().saturating_sub(visible_len).max(0));
        let end = (start + visible_len).min(labels.len());
        for (i, name) in labels[start..end].iter().enumerate() {
            let row = top_row + 3 + i as u16;
            let idx = start + i;
            let max_name_len = inner_w.saturating_sub(6);
            let truncated = if name.len() > max_name_len {
                format!("{}…", &name[..max_name_len])
            } else {
                name.clone()
            };
            queue!(stdout, cursor::MoveTo(top_left, row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
            if idx == *selected {
                queue!(stdout, crossterm::style::SetForegroundColor(fg), crossterm::style::Print("  › "))?;
                queue!(stdout, crossterm::style::Print(&truncated))?;
            } else {
                queue!(stdout, crossterm::style::SetForegroundColor(fg), crossterm::style::Print("    "))?;
                queue!(stdout, crossterm::style::Print(&truncated))?;
            }
            queue!(stdout, crossterm::style::Print(" ".repeat(inner_w.saturating_sub(4 + truncated.len()))))?;
            queue!(stdout, cursor::MoveTo(top_left + w_u - 1, row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        }
        let footer_row = top_row + 3 + visible_len as u16;
        queue!(stdout, cursor::MoveTo(top_left, footer_row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        queue!(stdout, crossterm::style::Print(&format!("{:<width$}", "  Enter remove · Esc cancel", width = inner_w)))?;
        queue!(stdout, cursor::MoveTo(top_left + w_u - 1, footer_row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        for i in (visible_len + 1)..(h.saturating_sub(1).saturating_sub(3)) {
            let r = top_row + 3 + i as u16;
            queue!(stdout, cursor::MoveTo(top_left, r), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
            queue!(stdout, crossterm::style::Print(" ".repeat(inner_w)))?;
            queue!(stdout, cursor::MoveTo(top_left + w_u - 1, r), crossterm::style::Print('│'))?;
        }
    } else if let AiMode::ConfigApiWizard { step: ConfigApiStep::ChooseProvider { selected } } = ai_mode {
        const WIZARD_PROVIDERS: [&str; 3] = ["OpenAI", "Gemini", "Groq"];
        let labels: Vec<String> = WIZARD_PROVIDERS.iter().map(|s| (*s).to_string()).collect();
        let visible_len = content_rows.saturating_sub(1).min(labels.len());
        let start = selected.saturating_sub(visible_len.saturating_sub(1)).min(labels.len().saturating_sub(visible_len).max(0));
        let end = (start + visible_len).min(labels.len());
        for (i, name) in labels[start..end].iter().enumerate() {
            let row = top_row + 3 + i as u16;
            let idx = start + i;
            let max_name_len = inner_w.saturating_sub(6);
            let truncated = if name.len() > max_name_len {
                format!("{}…", &name[..max_name_len])
            } else {
                name.clone()
            };
            queue!(stdout, cursor::MoveTo(top_left, row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
            if idx == *selected {
                queue!(stdout, crossterm::style::SetForegroundColor(fg), crossterm::style::Print("  › "))?;
                queue!(stdout, crossterm::style::Print(&truncated))?;
            } else {
                queue!(stdout, crossterm::style::SetForegroundColor(fg), crossterm::style::Print("    "))?;
                queue!(stdout, crossterm::style::Print(&truncated))?;
            }
            queue!(stdout, crossterm::style::Print(" ".repeat(inner_w.saturating_sub(4 + truncated.len()))))?;
            queue!(stdout, cursor::MoveTo(top_left + w_u - 1, row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        }
        let footer_row = top_row + 3 + visible_len as u16;
        queue!(stdout, cursor::MoveTo(top_left, footer_row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        queue!(stdout, crossterm::style::Print(&format!("{:<width$}", "  Enter select · Esc cancel", width = inner_w)))?;
        queue!(stdout, cursor::MoveTo(top_left + w_u - 1, footer_row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        for i in (visible_len + 1)..(h.saturating_sub(1).saturating_sub(3)) {
            let r = top_row + 3 + i as u16;
            queue!(stdout, cursor::MoveTo(top_left, r), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
            queue!(stdout, crossterm::style::Print(" ".repeat(inner_w)))?;
            queue!(stdout, cursor::MoveTo(top_left + w_u - 1, r), crossterm::style::Print('│'))?;
        }
    } else if let AiMode::ConfigApiWizard { step: ConfigApiStep::EnterKey { .. } | ConfigApiStep::Done { .. } } = ai_mode {
        queue!(stdout, cursor::MoveTo(top_left, r3), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        queue!(stdout, crossterm::style::SetForegroundColor(fg), crossterm::style::Print(&format!("{:<width$}", line1, width = inner_w)))?;
        queue!(stdout, cursor::MoveTo(top_left + w_u - 1, r3), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        let r4 = top_row + 4;
        queue!(stdout, cursor::MoveTo(top_left, r4), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        queue!(stdout, crossterm::style::SetForegroundColor(dim), crossterm::style::Print(&format!("{:<width$}", line2, width = inner_w)))?;
        queue!(stdout, cursor::MoveTo(top_left + w_u - 1, r4), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        for i in 5..h.saturating_sub(1) {
            let r = top_row + i as u16;
            queue!(stdout, cursor::MoveTo(top_left, r), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
            queue!(stdout, crossterm::style::Print(" ".repeat(inner_w)))?;
            queue!(stdout, cursor::MoveTo(top_left + w_u - 1, r), crossterm::style::Print('│'))?;
        }
    } else if let AiMode::ModelPicker { models, selected, .. } = ai_mode {
        let visible_len = content_rows.saturating_sub(1).min(models.len());
        let start = selected.saturating_sub(visible_len.saturating_sub(1)).min(models.len().saturating_sub(visible_len));
        let end = (start + visible_len).min(models.len());
        for (i, name) in models[start..end].iter().enumerate() {
            let row = top_row + 3 + i as u16;
            let idx = start + i;
            let max_name_len = inner_w.saturating_sub(6);
            let truncated = if name.len() > max_name_len {
                format!("{}…", &name[..max_name_len])
            } else {
                name.clone()
            };
            queue!(stdout, cursor::MoveTo(top_left, row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
            if idx == *selected {
                queue!(stdout, crossterm::style::SetForegroundColor(fg), crossterm::style::Print("  › "))?;
                queue!(stdout, crossterm::style::Print(&truncated))?;
            } else {
                queue!(stdout, crossterm::style::SetForegroundColor(fg), crossterm::style::Print("    "))?;
                queue!(stdout, crossterm::style::Print(&truncated))?;
            }
            queue!(stdout, crossterm::style::Print(" ".repeat(inner_w.saturating_sub(4 + truncated.len()))))?;
            queue!(stdout, cursor::MoveTo(top_left + w_u - 1, row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        }
        let footer_row = top_row + 3 + visible_len as u16;
        queue!(stdout, cursor::MoveTo(top_left, footer_row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        queue!(stdout, crossterm::style::Print(&format!("{:<width$}", "  Enter select · Esc cancel", width = inner_w)))?;
        queue!(stdout, cursor::MoveTo(top_left + w_u - 1, footer_row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        for i in (visible_len + 1)..(h.saturating_sub(1).saturating_sub(3)) {
            let r = top_row + 3 + i as u16;
            queue!(stdout, cursor::MoveTo(top_left, r), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
            queue!(stdout, crossterm::style::Print(" ".repeat(inner_w)))?;
            queue!(stdout, cursor::MoveTo(top_left + w_u - 1, r), crossterm::style::Print('│'))?;
        }
    } else {
        queue!(stdout, cursor::MoveTo(top_left, r3), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        if show_placeholder {
        queue!(stdout, crossterm::style::SetForegroundColor(dim), crossterm::style::Print("  › "))?;
        queue!(stdout, crossterm::style::SetForegroundColor(crossterm::style::Color::Rgb { r: 0x55, g: 0x55, b: 0x55 }), crossterm::style::Print("Describe what you want to run…"))?;
        queue!(stdout, crossterm::style::SetForegroundColor(fg), crossterm::style::Print('_'))?;
        let used = 4 + 31 + 1;
        queue!(stdout, crossterm::style::Print(" ".repeat(inner_w.saturating_sub(used))))?;
    } else {
        queue!(stdout, crossterm::style::SetForegroundColor(fg), crossterm::style::Print(&format!("{:<width$}", line1, width = inner_w)))?;
    }
    queue!(stdout, cursor::MoveTo(top_left + w_u - 1, r3), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;

    // Row 4: optional second line
    let r4 = top_row + 4;
    queue!(stdout, cursor::MoveTo(top_left, r4), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
    queue!(stdout, crossterm::style::SetForegroundColor(dim), crossterm::style::Print(&format!("{:<width$}", line2, width = inner_w)))?;
    queue!(stdout, cursor::MoveTo(top_left + w_u - 1, r4), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;

    // Empty rows
    for i in 5..h.saturating_sub(1) {
        let r = top_row + i as u16;
        queue!(stdout, cursor::MoveTo(top_left, r), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('│'))?;
        queue!(stdout, crossterm::style::Print(" ".repeat(inner_w)))?;
        queue!(stdout, cursor::MoveTo(top_left + w_u - 1, r), crossterm::style::Print('│'))?;
    }
    }

    // Bottom separator
    let bottom_row = top_row + h_u - 1;
    queue!(stdout, cursor::MoveTo(top_left, bottom_row), crossterm::style::SetForegroundColor(dim), crossterm::style::Print('╰'), crossterm::style::Print("─".repeat(inner_w)), crossterm::style::Print('╯'))?;

    stdout.flush()?;
    Ok(())
}

// -----------------------------------------------------------------------------
// Render screen buffer to crossterm
// -----------------------------------------------------------------------------

fn render(
    screen: &Screen,
    selection: Option<&Selection>,
    art_height: usize,
    theme: &Theme,
    ai_bar_visible: bool,
    current_model: Option<&str>,
    stdout: &mut io::Stdout,
) -> io::Result<()> {
    let (term_rows, term_cols) = terminal::size()
        .map(|(c, r)| (r as usize, c as usize))
        .unwrap_or((24, 80));
    let art_line_list = art_lines();
    let art_width = art_line_list.iter().map(|l| l.len()).max().unwrap_or(0);
    let start_col = term_cols.saturating_sub(art_width) / 2;

    // Draw art at top (rows 0..art_height)
    for (i, line) in art_line_list.iter().enumerate() {
        if i < art_height {
            queue!(stdout, cursor::MoveTo(start_col as u16, i as u16), crossterm::style::Print(line))?;
        }
    }

    // Draw terminal grid below the art. When AI bar is visible, skip last row so bar fits.
    let grid_rows = if ai_bar_visible {
        screen.rows.saturating_sub(1)
    } else {
        screen.rows
    };
    let top_offset = art_height;
    queue!(stdout, cursor::MoveTo(0, top_offset as u16))?;
    let mut last_fg = 255;
    let mut last_bg = 255;
    let mut last_bold = false;
    for (r, row) in screen.grid.iter().take(grid_rows).enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let in_selection = selection
                .filter(|s| !s.is_empty())
                .map(|s| s.contains_cell(r, c))
                .unwrap_or(false);
            let draw_r = top_offset + r;
            let draw_c = c;

            if in_selection {
                queue!(
                    stdout,
                    crossterm::style::SetForegroundColor(crossterm::style::Color::Black),
                    crossterm::style::SetBackgroundColor(crossterm::style::Color::Grey),
                    crossterm::style::SetAttribute(crossterm::style::Attribute::NormalIntensity)
                )?;
                last_fg = 255;
                last_bg = 255;
                last_bold = false;
            } else if cell.fg != last_fg || cell.bg != last_bg || cell.bold != last_bold {
                let fg = match cell.fg {
                    0 => crossterm::style::Color::Black,
                    1 => crossterm::style::Color::DarkRed,
                    2 => crossterm::style::Color::DarkGreen,
                    3 => crossterm::style::Color::DarkYellow,
                    4 => crossterm::style::Color::DarkBlue,
                    5 => crossterm::style::Color::DarkMagenta,
                    6 => crossterm::style::Color::DarkCyan,
                    7 => crossterm::style::Color::Rgb {
                        r: theme.default_foreground.0,
                        g: theme.default_foreground.1,
                        b: theme.default_foreground.2,
                    },
                    _ => crossterm::style::Color::Rgb {
                        r: theme.default_foreground.0,
                        g: theme.default_foreground.1,
                        b: theme.default_foreground.2,
                    },
                };
                let bg = match cell.bg {
                    0 => crossterm::style::Color::Rgb {
                        r: theme.default_background.0,
                        g: theme.default_background.1,
                        b: theme.default_background.2,
                    },
                    1 => crossterm::style::Color::DarkRed,
                    2 => crossterm::style::Color::DarkGreen,
                    3 => crossterm::style::Color::DarkYellow,
                    4 => crossterm::style::Color::DarkBlue,
                    5 => crossterm::style::Color::DarkMagenta,
                    6 => crossterm::style::Color::DarkCyan,
                    7 => crossterm::style::Color::Grey,
                    _ => crossterm::style::Color::Rgb {
                        r: theme.default_background.0,
                        g: theme.default_background.1,
                        b: theme.default_background.2,
                    },
                };
                queue!(
                    stdout,
                    crossterm::style::SetForegroundColor(fg),
                    crossterm::style::SetBackgroundColor(bg)
                )?;
                if cell.bold {
                    queue!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::Bold))?;
                } else if last_bold {
                    queue!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::NormalIntensity))?;
                }
                last_fg = cell.fg;
                last_bg = cell.bg;
                last_bold = cell.bold;
            }
            queue!(stdout, cursor::MoveTo(draw_c as u16, draw_r as u16), crossterm::style::Print(cell.ch))?;
        }
    }
    // Bottom left: show selected AI model name when set
    if let Some(name) = current_model {
        let bottom_row = (term_rows.saturating_sub(1)) as u16;
        let label = format!(" {}", name);
        let display = if label.len() > term_cols {
            format!("{}…", &label[..term_cols.saturating_sub(1)])
        } else {
            label
        };
        queue!(stdout, cursor::MoveTo(0, bottom_row))?;
        queue!(
            stdout,
            crossterm::style::SetForegroundColor(crossterm::style::Color::Rgb {
                r: theme.default_foreground.0,
                g: theme.default_foreground.1,
                b: theme.default_foreground.2,
            }),
            crossterm::style::SetBackgroundColor(crossterm::style::Color::Rgb {
                r: theme.default_background.0,
                g: theme.default_background.1,
                b: theme.default_background.2,
            })
        )?;
        queue!(stdout, crossterm::style::Print(&display))?;
    }

    queue!(
        stdout,
        cursor::MoveTo(
            screen.cursor_col as u16,
            (top_offset + screen.cursor_row) as u16
        ),
        cursor::Show
    )?;
    stdout.flush()?;
    Ok(())
}

// -----------------------------------------------------------------------------
// Welcome screen
// -----------------------------------------------------------------------------

const WELCOME_ART: &str = r#"
  ██████╗ ██╗  ██╗ █████╗ ███╗   ███╗███████╗██╗     ███████╗ ██████╗ ███╗   ██╗
 ██╔════╝ ██║  ██║██╔══██╗████╗ ████║██╔════╝██║     ██╔════╝██╔═══██╗████╗  ██║
 ██║  ███╗███████║███████║██╔████╔██║█████╗  ██║     █████╗  ██║   ██║██╔██╗ ██║
 ██║   ██║██╔══██║██╔══██║██║╚██╔╝██║██╔══╝  ██║     ██╔══╝  ██║   ██║██║╚██╗██║
 ╚██████╔╝██║  ██║██║  ██║██║ ╚═╝ ██║███████╗███████╗███████╗╚██████╔╝██║ ╚████║
  ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝     ╚═╝╚══════╝╚══════╝╚══════╝ ╚═════╝ ╚═╝  ╚═══╝
"#;

fn art_lines() -> Vec<&'static str> {
    WELCOME_ART.trim_start_matches('\n').lines().collect()
}

// -----------------------------------------------------------------------------
// Main
// -----------------------------------------------------------------------------

fn main() -> io::Result<()> {
    let (term_rows, term_cols) = terminal::size()
        .map(|(c, r)| (r as usize, c as usize))
        .unwrap_or((24, 80));

    let art_height = art_lines().len();
    let rows = term_rows.saturating_sub(art_height).max(1);
    let cols = term_cols;

    let theme = Arc::new(Mutex::new(load_theme()));

    let _guard = RawModeGuard::new()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        terminal::Clear(ClearType::All),
        cursor::Hide
    )?;
    // Set default background from theme so cleared screen and empty cells use it
    if let Ok(t) = theme.lock() {
        let (r, g, b) = t.default_background;
        execute!(
            stdout,
            crossterm::style::SetBackgroundColor(crossterm::style::Color::Rgb { r, g, b })
        )?;
    }

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let cmd = CommandBuilder::new(shell.clone());
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let master = pair.master;
    let mut pty_writer = master
        .take_writer()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let mut pty_reader = master
        .try_clone_reader()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let screen = Arc::new(Mutex::new(Screen::new(rows, cols)));
    let performer = Arc::new(Mutex::new(TerminalPerform {
        screen: Arc::clone(&screen),
    }));
    let running = Arc::new(AtomicBool::new(true));

    let performer_reader = Arc::clone(&performer);
    let running_reader = Arc::clone(&running);
    let reader_handle = thread::spawn(move || {
        let mut parser = Parser::new();
        let mut buf = [0u8; 4096];
        while running_reader.load(Ordering::Relaxed) {
            match pty_reader.read(&mut buf) {
                Ok(0) => {
                    running_reader.store(false, Ordering::Relaxed);
                    break;
                }
                Ok(n) => {
                    for b in &buf[..n] {
                        if let Ok(mut perf) = performer_reader.lock() {
                            parser.advance(&mut *perf, *b);
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });

    execute!(stdout, event::EnableMouseCapture)?;

    // Selection state: drag updates end; Ctrl+Shift+C copies. Copy on release (traditional).
    // Double-click = word, triple-click = line.
    let mut selection: Option<Selection> = None;
    let mut selecting = false;
    let mut last_click: Option<(usize, usize, u8, Instant)> = None;
    const DOUBLE_CLICK_MS: u64 = 400;

    // AI command generation and suggest-fix (reload after wizard saves so new keys apply without restart)
    let mut ai_config = load_ai_config();
    let mut backend_override: Option<AiBackend> = None;
    let mut model_override: Option<String> = None;
    let (ai_tx, ai_rx) = mpsc::channel();
    let mut ai_mode = AiMode::Idle;
    const CMD_SYSTEM: &str = "You are a shell assistant. Reply with exactly one shell command, no explanation, no markdown code blocks.";

    while running.load(Ordering::Relaxed) {
        // Exit when shell exits (Ctrl+D or `exit`)
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        if event::poll(Duration::from_millis(16)).unwrap_or(false) {
            match event::read() {
                Ok(Event::Key(KeyEvent {
                    code,
                    modifiers,
                    ..
                })) => {
                    // Ctrl+Shift+C: copy selection to clipboard (do not send to PTY)
                    if modifiers.contains(KeyModifiers::CONTROL)
                        && modifiers.contains(KeyModifiers::SHIFT)
                        && matches!(code, KeyCode::Char(c) if c == 'c' || c == 'C')
                    {
                        if let Some(ref sel) = selection {
                            if let Ok(s) = screen.lock() {
                                let text = sel.extract_from(&s);
                                if !text.is_empty() {
                                    if let Ok(mut clip) = arboard::Clipboard::new() {
                                        let _ = clip.set_text(&text);
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    // Ctrl+Shift+T: open theme config in $EDITOR and reload theme
                    if modifiers.contains(KeyModifiers::CONTROL)
                        && modifiers.contains(KeyModifiers::SHIFT)
                        && matches!(code, KeyCode::Char(c) if c == 't' || c == 'T')
                    {
                        let _ = open_theme_config_and_reload(&mut stdout, &theme);
                        continue;
                    }

                    // Ctrl+K: open AI prompt to generate commands (only when Idle)
                    if modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(code, KeyCode::Char(c) if c == 'k' || c == 'K')
                    {
                        if matches!(ai_mode, AiMode::Idle) {
                            ai_mode = AiMode::PromptInput {
                                buffer: String::new(),
                            };
                        }
                        continue;
                    }

                    // When in AI mode, handle keys locally (do not send to PTY)
                    if !matches!(ai_mode, AiMode::Idle) {
                        // Poll for worker result when Thinking
                        if matches!(ai_mode, AiMode::Thinking) {
                            if let Ok(res) = ai_rx.try_recv() {
                                ai_mode = match res {
                                    Ok(cmd) => AiMode::SuggestionReady { command: cmd },
                                    Err(e) => AiMode::Error { message: e },
                                };
                            }
                        }
                        let mut exit_wizard_to_idle = false;
                        let mut reload_ai_config = false;
                        match &mut ai_mode {
                            AiMode::PromptInput { buffer } => {
                                match code {
                                    KeyCode::Char(c) if !c.is_control() => {
                                        buffer.push(c);
                                    }
                                    KeyCode::Backspace => {
                                        buffer.pop();
                                    }
                                    KeyCode::Enter => {
                                        let prompt = buffer.trim().to_string();
                                        if prompt.is_empty() {
                                            ai_mode = AiMode::Idle;
                                        } else if prompt == "/model" || prompt == "/models" {
                                            let choices = available_backends(&ai_config);
                                            ai_mode = AiMode::BackendPicker {
                                                choices,
                                                selected: 0,
                                            };
                                        } else {
                                            let backend = backend_override.unwrap_or(ai_config.default_backend);
                                            let tx = ai_tx.clone();
                                            let config = ai_config.clone();
                                            let prompt_clone = prompt.clone();
                                            let model_opt = model_override.clone().or_else(|| config.default_model.clone());
                                            thread::spawn(move || {
                                                let res = match backend {
                                                    AiBackend::Ollama => {
                                                        let model = match ollama_resolve_model(
                                                            &config.ollama_base_url,
                                                            model_opt.as_deref(),
                                                        ) {
                                                            Ok(m) => m,
                                                            Err(e) => {
                                                                let _ = tx.send(Err(e));
                                                                return;
                                                            }
                                                        };
                                                        let full_prompt = ollama_build_prompt(CMD_SYSTEM, &prompt_clone);
                                                        ollama_generate(&config.ollama_base_url, &model, &full_prompt)
                                                    }
                                                    AiBackend::OpenAi => {
                                                        let api_key = match config.openai_api_key() {
                                                            Some(k) => k,
                                                            None => {
                                                                let _ = tx.send(Err("Set OPENAI_API_KEY or configure in /model → Configure API".to_string()));
                                                                return;
                                                            }
                                                        };
                                                        let model = model_opt.unwrap_or_else(|| "gpt-4o-mini".to_string());
                                                        let base = config.openai_base_url();
                                                        openai_generate(&base, &api_key, &model, CMD_SYSTEM, &prompt_clone)
                                                    }
                                                    AiBackend::Gemini => {
                                                        let api_key = match config.gemini_api_key() {
                                                            Some(k) => k,
                                                            None => {
                                                                let _ = tx.send(Err("Set GEMINI_API_KEY or configure in /model → Configure API".to_string()));
                                                                return;
                                                            }
                                                        };
                                                        let model = model_opt.unwrap_or_else(|| "gemini-2.0-flash".to_string());
                                                        gemini_generate(&api_key, &model, CMD_SYSTEM, &prompt_clone)
                                                    }
                                                    AiBackend::Groq => {
                                                        let api_key = match config.groq_api_key() {
                                                            Some(k) => k,
                                                            None => {
                                                                let _ = tx.send(Err("Set GROQ_API_KEY or configure in /model → Configure API".to_string()));
                                                                return;
                                                            }
                                                        };
                                                        let model = model_opt.unwrap_or_else(|| "llama-3.3-70b-versatile".to_string());
                                                        let base = config.groq_base_url();
                                                        openai_generate(&base, &api_key, &model, CMD_SYSTEM, &prompt_clone)
                                                    }
                                                };
                                                let _ = tx.send(res);
                                            });
                                            ai_mode = AiMode::Thinking;
                                        }
                                    }
                                    KeyCode::Esc => {
                                        ai_mode = AiMode::Idle;
                                    }
                                    _ => {}
                                }
                            }
                            AiMode::SuggestionReady { command } => {
                                if code == KeyCode::Enter {
                                    // Inject command into PTY and run
                                    for c in command.chars() {
                                        let bytes = key_to_bytes(KeyCode::Char(c), KeyModifiers::empty());
                                        for b in bytes {
                                            let _ = pty_writer.write_all(&[b]);
                                        }
                                    }
                                    let _ = pty_writer.write_all(&[b'\r']);
                                    let _ = pty_writer.flush();
                                    ai_mode = AiMode::Idle;
                                } else if code == KeyCode::Esc {
                                    ai_mode = AiMode::Idle;
                                }
                            }
                            AiMode::Error { .. } => {
                                if code == KeyCode::Enter || code == KeyCode::Esc {
                                    ai_mode = AiMode::Idle;
                                }
                            }
                            AiMode::BackendPicker { choices, selected } => {
                                match code {
                                    KeyCode::Up => {
                                        *selected = selected.saturating_sub(1);
                                    }
                                    KeyCode::Down => {
                                        *selected = (*selected + 1).min(choices.len().saturating_sub(1));
                                    }
                                    KeyCode::Enter => {
                                        let choice = choices[*selected].clone();
                                        match choice {
                                            BackendChoice::ConfigureApi => {
                                                ai_mode = AiMode::ConfigApiWizard {
                                                    step: ConfigApiStep::ChooseProvider { selected: 0 },
                                                };
                                            }
                                            BackendChoice::RemoveApi => {
                                                let backends: Vec<AiBackend> = [AiBackend::OpenAi, AiBackend::Gemini, AiBackend::Groq]
                                                    .into_iter()
                                                    .filter(|b| ai_config.is_configured(*b))
                                                    .collect();
                                                ai_mode = AiMode::RemoveApiPicker {
                                                    backends,
                                                    selected: 0,
                                                };
                                            }
                                            BackendChoice::Backend(backend) => {
                                                let models = match backend {
                                                    AiBackend::Ollama => {
                                                        match ollama_list_models(&ai_config.ollama_base_url) {
                                                            Ok(list) if !list.is_empty() => list,
                                                            Ok(_) => {
                                                                ai_mode = AiMode::Error {
                                                                    message: "No Ollama models. Pull one with: ollama pull <name>".to_string(),
                                                                };
                                                                continue;
                                                            }
                                                            Err(e) => {
                                                                ai_mode = AiMode::Error { message: e };
                                                                continue;
                                                            }
                                                        }
                                                    }
                                                    AiBackend::OpenAi => {
                                                        let key = ai_config.openai_api_key().unwrap_or_default();
                                                        openai_list_models(&ai_config.openai_base_url(), &key)
                                                    }
                                                    AiBackend::Gemini => gemini_list_models(),
                                                    AiBackend::Groq => groq_list_models(),
                                                };
                                                ai_mode = AiMode::ModelPicker {
                                                    backend,
                                                    models,
                                                    selected: 0,
                                                };
                                            }
                                        }
                                    }
                                    KeyCode::Esc => {
                                        ai_mode = AiMode::Idle;
                                    }
                                    _ => {}
                                }
                            }
                            AiMode::ModelPicker { backend, models, selected } => {
                                match code {
                                    KeyCode::Up => {
                                        *selected = selected.saturating_sub(1);
                                    }
                                    KeyCode::Down => {
                                        *selected = (*selected + 1).min(models.len().saturating_sub(1));
                                    }
                                    KeyCode::Enter => {
                                        let chosen = models[*selected].clone();
                                        backend_override = Some(*backend);
                                        model_override = Some(chosen);
                                        ai_mode = AiMode::Idle;
                                    }
                                    KeyCode::Esc => {
                                        ai_mode = AiMode::Idle;
                                    }
                                    _ => {}
                                }
                            }
                            AiMode::RemoveApiPicker { backends, selected } => {
                                match code {
                                    KeyCode::Up => {
                                        *selected = selected.saturating_sub(1);
                                    }
                                    KeyCode::Down => {
                                        *selected = (*selected + 1).min(backends.len().saturating_sub(1));
                                    }
                                    KeyCode::Enter => {
                                        let backend = backends[*selected];
                                        let provider_name = match backend {
                                            AiBackend::OpenAi => "openai",
                                            AiBackend::Gemini => "gemini",
                                            AiBackend::Groq => "groq",
                                            AiBackend::Ollama => "ollama",
                                        };
                                        match remove_provider_api_key(provider_name) {
                                            Ok(()) => {
                                                reload_ai_config = true;
                                                ai_mode = AiMode::Idle;
                                            }
                                            Err(e) => {
                                                ai_mode = AiMode::Error {
                                                    message: format!("Remove failed: {}", e),
                                                };
                                            }
                                        }
                                    }
                                    KeyCode::Esc => {
                                        ai_mode = AiMode::Idle;
                                    }
                                    _ => {}
                                }
                            }
                            AiMode::ConfigApiWizard { step } => {
                                match step {
                                    ConfigApiStep::ChooseProvider { selected } => {
                                        match code {
                                            KeyCode::Up => {
                                                *selected = selected.saturating_sub(1);
                                            }
                                            KeyCode::Down => {
                                                *selected = (*selected + 1).min(2);
                                            }
                                            KeyCode::Enter => {
                                                const WIZARD_BACKENDS: [AiBackend; 3] =
                                                    [AiBackend::OpenAi, AiBackend::Gemini, AiBackend::Groq];
                                                let provider = WIZARD_BACKENDS[*selected];
                                                ai_mode = AiMode::ConfigApiWizard {
                                                    step: ConfigApiStep::EnterKey {
                                                        provider,
                                                        key_buffer: String::new(),
                                                    },
                                                };
                                            }
                                            KeyCode::Esc => {
                                                ai_mode = AiMode::Idle;
                                            }
                                            _ => {}
                                        }
                                    }
                                    ConfigApiStep::EnterKey { provider, key_buffer } => {
                                        match code {
                                            KeyCode::Char(c) if !c.is_control() => {
                                                key_buffer.push(c);
                                            }
                                            KeyCode::Backspace => {
                                                key_buffer.pop();
                                            }
                                            KeyCode::Enter => {
                                                let key = key_buffer.trim();
                                                if key.is_empty() {
                                                    ai_mode = AiMode::ConfigApiWizard {
                                                        step: ConfigApiStep::Done {
                                                            message: format!(
                                                                "Set {} env var or add key in config.",
                                                                match provider {
                                                                    AiBackend::OpenAi => "OPENAI_API_KEY",
                                                                    AiBackend::Gemini => "GEMINI_API_KEY",
                                                                    AiBackend::Groq => "GROQ_API_KEY",
                                                                    AiBackend::Ollama => "N/A (Ollama needs no key)",
                                                                }
                                                            ),
                                                        },
                                                    };
                                                } else {
                                                    let provider_name = match provider {
                                                        AiBackend::OpenAi => "openai",
                                                        AiBackend::Gemini => "gemini",
                                                        AiBackend::Groq => "groq",
                                                        AiBackend::Ollama => "ollama",
                                                    };
                                                    match write_provider_api_key(provider_name, key) {
                                                        Ok(()) => {
                                                            reload_ai_config = true;
                                                            ai_mode = AiMode::ConfigApiWizard {
                                                                step: ConfigApiStep::Done {
                                                                    message: format!("{} configured. Use /model to select.", provider),
                                                                },
                                                            };
                                                        }
                                                        Err(e) => {
                                                            ai_mode = AiMode::ConfigApiWizard {
                                                                step: ConfigApiStep::Done {
                                                                    message: format!("Save failed: {}", e),
                                                                },
                                                            };
                                                        }
                                                    }
                                                }
                                            }
                                            KeyCode::Esc => {
                                                ai_mode = AiMode::Idle;
                                            }
                                            _ => {}
                                        }
                                    }
                                    ConfigApiStep::Done { .. } => {
                                        if code == KeyCode::Enter || code == KeyCode::Esc {
                                            exit_wizard_to_idle = true;
                                        }
                                    }
                                }
                            }
                            AiMode::Thinking | AiMode::Idle => {}
                        }
                        if exit_wizard_to_idle {
                            ai_mode = AiMode::Idle;
                        }
                        if reload_ai_config {
                            ai_config = load_ai_config();
                        }
                        continue;
                    }

                    let bytes = key_to_bytes(code, modifiers);
                    for b in bytes {
                        let _ = pty_writer.write_all(&[b]);
                    }
                    let _ = pty_writer.flush();
                }
                Ok(Event::Mouse(me)) => {
                    // Terminal row 0..art_height is the banner; grid starts at art_height
                    let row = me.row.saturating_sub(art_height as u16) as usize;
                    let col = me.column as usize;
                    match me.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            selecting = true;
                            if let Ok(s) = screen.lock() {
                                let rows = s.rows;
                                let cols = s.cols;
                                let r = row.min(rows.saturating_sub(1));
                                let c = col.min(cols.saturating_sub(1));
                                let now = Instant::now();
                                let click_count = match last_click {
                                    Some((lr, lc, n, t))
                                        if lr == r && lc == c
                                            && now.duration_since(t).as_millis() < DOUBLE_CLICK_MS as u128 =>
                                    {
                                        (n + 1).min(3)
                                    }
                                    _ => 1,
                                };
                                last_click = Some((r, c, click_count, now));
                                selection = Some(if click_count == 3 {
                                    let (r1, c1, r2, c2) = selection_line_at(&s, r);
                                    Selection {
                                        start_row: r1,
                                        start_col: c1,
                                        end_row: r2,
                                        end_col: c2,
                                    }
                                } else if click_count == 2 {
                                    let (r1, c1, r2, c2) = selection_word_at(&s, r, c);
                                    Selection {
                                        start_row: r1,
                                        start_col: c1,
                                        end_row: r2,
                                        end_col: c2,
                                    }
                                } else {
                                    Selection {
                                        start_row: r,
                                        start_col: c,
                                        end_row: r,
                                        end_col: c,
                                    }
                                });
                            } else {
                                selection = Some(Selection {
                                    start_row: row,
                                    start_col: col,
                                    end_row: row,
                                    end_col: col,
                                });
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if selecting {
                                if let Some(ref mut sel) = selection {
                                    sel.end_row = row;
                                    sel.end_col = col;
                                }
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            if let (Some(ref sel), Ok(s)) = (selection.as_ref(), screen.lock()) {
                                if !sel.is_empty() {
                                    let text = sel.extract_from(&s);
                                    if !text.is_empty() {
                                        if let Ok(mut clip) = arboard::Clipboard::new() {
                                            let _ = clip.set_text(&text);
                                        }
                                    }
                                }
                            }
                            selecting = false;
                        }
                        _ => {}
                    }
                }
                Ok(Event::Resize(c, r)) => {
                    let new_term_rows = r as usize;
                    let new_term_cols = c as usize;
                    let new_rows = new_term_rows.saturating_sub(art_height).max(1);
                    let new_cols = new_term_cols;
                    let _ = master.resize(PtySize {
                        rows: new_rows as u16,
                        cols: c,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                    if let Ok(mut s) = screen.lock() {
                        s.resize(new_rows, new_cols);
                    }
                    selection = None;
                }
                _ => {}
            }
        }

        // When Thinking, poll for worker result even without a key event
        if matches!(ai_mode, AiMode::Thinking) {
            if let Ok(res) = ai_rx.try_recv() {
                ai_mode = match res {
                    Ok(cmd) => AiMode::SuggestionReady { command: cmd },
                    Err(e) => AiMode::Error { message: e },
                };
            }
        }

        // Redraw every frame: art at top, then terminal grid; AI modal overlays center when not Idle
        let backend = backend_override.unwrap_or(ai_config.default_backend);
        let current_model_display = model_override
            .as_deref()
            .or(ai_config.default_model.as_deref())
            .map(|m| format!("{} · {}", backend, m));
        let current_model = current_model_display.as_deref();
        if let (Ok(s), Ok(t)) = (screen.lock(), theme.lock()) {
            let _ = render(&s, selection.as_ref(), art_height, &*t, false, current_model, &mut stdout);
        }
        if !matches!(ai_mode, AiMode::Idle) {
            let (term_rows, term_cols) = terminal::size()
                .map(|(c, r)| (r as usize, c as usize))
                .unwrap_or((24, 80));
            if let Ok(t) = theme.lock() {
                let _ = render_ai_bar(&ai_mode, term_rows, term_cols, &*t, current_model, &mut stdout);
            }
        }
    }

    running.store(false, Ordering::Relaxed);
    let _ = reader_handle.join();

    execute!(
        stdout,
        event::DisableMouseCapture,
        cursor::Show,
        terminal::LeaveAlternateScreen
    )?;
    Ok(())
}

/// Restore terminal on drop.
struct RawModeGuard;

impl RawModeGuard {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

fn key_to_bytes(code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    match code {
        KeyCode::Char('c') if ctrl => vec![0x03],
        KeyCode::Char('z') if ctrl => vec![0x1a],
        KeyCode::Char('d') if ctrl => vec![0x04],
        KeyCode::Char('\\') if ctrl => vec![0x1c],
        KeyCode::Char(c) if ctrl && c >= 'a' && c <= 'z' => {
            vec![(c as u8) - b'a' + 1]
        }
        KeyCode::Char(c) if ctrl && c >= '@' && c <= '_' => {
            vec![(c as u8) - b'@']
        }
        KeyCode::Char(c) => vec![c as u8],
        KeyCode::Enter => vec![b'\r'],
        KeyCode::BackTab => vec![0x1b, b'[', b'Z'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => vec![0x1b, b'[', b'A'],
        KeyCode::Down => vec![0x1b, b'[', b'B'],
        KeyCode::Right => vec![0x1b, b'[', b'C'],
        KeyCode::Left => vec![0x1b, b'[', b'D'],
        KeyCode::Home => vec![0x1b, b'[', b'H'],
        KeyCode::End => vec![0x1b, b'[', b'F'],
        KeyCode::PageUp => vec![0x1b, b'[', b'5', b'~'],
        KeyCode::PageDown => vec![0x1b, b'[', b'6', b'~'],
        KeyCode::Delete => vec![0x1b, b'[', b'3', b'~'],
        KeyCode::Insert => vec![0x1b, b'[', b'2', b'~'],
        _ => vec![],
    }
}
