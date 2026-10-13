//! The AI connection's settings: `ai-config.json`.
//!
//! Two files, read on every question (they are small): a machine-wide one an administrator may deploy,
//! `%ProgramData%\Yuval\ai-config.json`, and the user's own, `%LOCALAPPDATA%\Yuval\ai-config.json`, whose
//! keys override the machine's one key at a time. A machine file with `"locked": true` is used alone.
//!
//! The key: `apiKey` as typed in the file, `"env:NAME"` for an environment variable, or `apiKeyProtected`,
//! the key encrypted for this Windows user (DPAPI) that the settings page writes. The key never reaches a
//! page, a log line or the Center: [`AiConfigView`] only says whether one is set.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "ai-config.json";
const DEFAULT_TIMEOUT_SECONDS: u32 = 45;
/// The Center waits 120 s for a question (IslandClient.SearchTimeout): the AI must answer well within it.
const MAX_TIMEOUT_SECONDS: u32 = 90;
const DEFAULT_MAX_TOKENS: u32 = 1024;
const MAX_TOKENS_LIMIT: u32 = 64_000;
pub const DEFAULT_AZURE_API_VERSION: &str = "2024-10-21";

/// Which wire format the server speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    /// `POST …/chat/completions` with `Authorization: Bearer` (OpenAI, vLLM, Ollama, LM Studio, LiteLLM, …).
    OpenAi,
    /// Azure OpenAI: the same body, the key in an `api-key` header, `api-version` in the URL.
    Azure,
    /// Anthropic Messages API: `POST …/v1/messages`, `x-api-key` + `anthropic-version`.
    Anthropic,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Provider> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "openai" | "openai-compatible" | "compatible" => Some(Provider::OpenAi),
            "azure" | "azure-openai" => Some(Provider::Azure),
            "anthropic" | "claude" => Some(Provider::Anthropic),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Provider::OpenAi => "openai",
            Provider::Azure => "azure",
            Provider::Anthropic => "anthropic",
        }
    }
}

/// When a question goes to the AI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Only what the built-in engine did not understand.
    Fallback,
    /// Every question: the built-in engine still searches, and the AI writes the answer (with the results
    /// found when `shareResults` is on).
    Always,
}

impl Mode {
    pub fn parse(s: &str) -> Mode {
        match s.trim().to_ascii_lowercase().as_str() {
            "always" | "all" => Mode::Always,
            _ => Mode::Fallback,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Fallback => "fallback",
            Mode::Always => "always",
        }
    }
}

/// The file as written (camelCase). Unknown keys (`_readme`, comments as keys) are ignored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiConfig {
    pub enabled: bool,
    pub provider: String,
    pub endpoint: String,
    pub api_key: String,
    pub api_key_protected: String,
    pub model: String,
    pub mode: String,
    pub share_results: bool,
    pub timeout_seconds: u32,
    pub max_tokens: u32,
    pub temperature: Option<f64>,
    pub system_prompt: String,
    pub headers: BTreeMap<String, String>,
    pub api_version: String,
    pub allow_invalid_certificates: bool,
    pub use_windows_auth: bool,
    /// Machine file only: the user's file is ignored.
    pub locked: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig {
            enabled: false,
            provider: "openai".into(),
            endpoint: String::new(),
            api_key: String::new(),
            api_key_protected: String::new(),
            model: String::new(),
            mode: "fallback".into(),
            share_results: true,
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            max_tokens: DEFAULT_MAX_TOKENS,
            temperature: None,
            system_prompt: String::new(),
            headers: BTreeMap::new(),
            api_version: String::new(),
            allow_invalid_certificates: false,
            use_windows_auth: false,
            locked: false,
        }
    }
}

impl AiConfig {
    pub fn provider(&self) -> Option<Provider> {
        Provider::parse(&self.provider)
    }

    pub fn mode(&self) -> Mode {
        Mode::parse(&self.mode)
    }

    pub fn timeout_seconds(&self) -> u32 {
        match self.timeout_seconds {
            0 => DEFAULT_TIMEOUT_SECONDS,
            s => s.min(MAX_TIMEOUT_SECONDS),
        }
    }

    pub fn max_tokens(&self) -> u32 {
        self.max_tokens.min(MAX_TOKENS_LIMIT)
    }

    /// The key to send: the file's `apiKey` (or the environment variable it names), else the protected one.
    pub fn key(&self) -> String {
        let plain = self.api_key.trim();
        if let Some(var) = plain.strip_prefix("env:") {
            return std::env::var(var.trim()).unwrap_or_default().trim().to_string();
        }
        if !plain.is_empty() {
            return plain.to_string();
        }
        if self.api_key_protected.trim().is_empty() {
            return String::new();
        }
        unprotect(self.api_key_protected.trim()).unwrap_or_default()
    }

    pub fn has_key(&self) -> bool {
        !self.api_key.trim().is_empty() || !self.api_key_protected.trim().is_empty()
    }

    /// What is missing or wrong, as an `AI-001` error; `Ok` when a question can be sent.
    pub fn check(&self) -> Result<Provider, String> {
        let provider = self.provider().ok_or_else(|| format!("AI-001: unknown provider \"{}\" (openai, azure, anthropic)", self.provider))?;
        if self.endpoint.trim().is_empty() {
            return Err("AI-001: no endpoint (the AI server's address)".into());
        }
        super::protocol::parse_url(&super::protocol::request_url(self, provider)).map_err(|e| format!("AI-001: {e}"))?;
        let model_in_url = provider == Provider::Azure && self.endpoint.contains("/deployments/");
        if self.model.trim().is_empty() && !model_in_url {
            return Err("AI-001: no model".into());
        }
        if provider != Provider::OpenAi && self.key().is_empty() && !self.use_windows_auth {
            return Err("AI-001: no API key".into());
        }
        for (name, value) in &self.headers {
            if !valid_header_name(name) || value.contains(['\r', '\n']) {
                return Err(format!("AI-001: invalid header \"{name}\""));
            }
        }
        Ok(provider)
    }
}

pub fn valid_header_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 100 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// What the settings page sees: everything but the key.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfigView {
    pub enabled: bool,
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub mode: String,
    pub share_results: bool,
    pub timeout_seconds: u32,
    pub max_tokens: u32,
    pub temperature: Option<f64>,
    pub system_prompt: String,
    pub headers: BTreeMap<String, String>,
    pub api_version: String,
    pub allow_invalid_certificates: bool,
    pub use_windows_auth: bool,
    pub has_key: bool,
    /// The administrator's file is in force: the page shows the values but cannot change them.
    pub locked: bool,
    /// Where the user's file is (the settings page shows it and can open it).
    pub file_path: String,
    /// The files could not be read (invalid JSON): the code and a short reason.
    pub error: Option<String>,
}

impl AiConfigView {
    pub fn of(c: &AiConfig, file_path: &Path, error: Option<String>) -> Self {
        AiConfigView {
            enabled: c.enabled,
            provider: c.provider().map(Provider::as_str).unwrap_or(&c.provider).to_string(),
            endpoint: c.endpoint.clone(),
            model: c.model.clone(),
            mode: c.mode().as_str().to_string(),
            share_results: c.share_results,
            timeout_seconds: c.timeout_seconds(),
            max_tokens: c.max_tokens(),
            temperature: c.temperature,
            system_prompt: c.system_prompt.clone(),
            headers: c.headers.clone(),
            api_version: c.api_version.clone(),
            allow_invalid_certificates: c.allow_invalid_certificates,
            use_windows_auth: c.use_windows_auth,
            has_key: c.has_key(),
            locked: c.locked,
            file_path: file_path.to_string_lossy().into_owned(),
            error,
        }
    }
}

/// A change from the settings page. Absent fields stay; `apiKey: ""` removes the key, a non-empty one is
/// stored encrypted for this user.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfigPatch {
    pub enabled: Option<bool>,
    pub provider: Option<String>,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub share_results: Option<bool>,
    pub timeout_seconds: Option<u32>,
    pub max_tokens: Option<u32>,
    #[serde(default, deserialize_with = "nullable")]
    pub temperature: Option<Option<f64>>,
    pub system_prompt: Option<String>,
    pub headers: Option<BTreeMap<String, String>>,
    pub api_version: Option<String>,
    pub allow_invalid_certificates: Option<bool>,
    pub use_windows_auth: Option<bool>,
}

/// `"temperature": null` clears it (Some(None)); an absent key leaves it (None).
fn nullable<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<f64>>, D::Error> {
    Option::<f64>::deserialize(d).map(Some)
}

const MAX_TEXT: usize = 2_000;
const MAX_PROMPT: usize = 8_000;

fn clip(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect()
}

/// Apply `patch` to the user's file object (keys the patch does not name, `_readme` included, stay as they are).
pub fn apply_patch(file: &mut Map<String, Value>, patch: AiConfigPatch) -> Result<(), String> {
    let mut set = |k: &str, v: Value| {
        file.insert(k.to_string(), v);
    };
    if let Some(v) = patch.enabled {
        set("enabled", v.into());
    }
    if let Some(v) = patch.provider {
        let p = Provider::parse(&v).ok_or_else(|| "AI-001: unknown provider".to_string())?;
        set("provider", p.as_str().into());
    }
    if let Some(v) = patch.endpoint {
        set("endpoint", clip(&v, MAX_TEXT).into());
    }
    if let Some(v) = patch.model {
        set("model", clip(&v, 200).into());
    }
    if let Some(v) = patch.mode {
        set("mode", Mode::parse(&v).as_str().into());
    }
    if let Some(v) = patch.share_results {
        set("shareResults", v.into());
    }
    if let Some(v) = patch.timeout_seconds {
        set("timeoutSeconds", v.clamp(5, MAX_TIMEOUT_SECONDS).into());
    }
    if let Some(v) = patch.max_tokens {
        set("maxTokens", v.min(MAX_TOKENS_LIMIT).into());
    }
    if let Some(v) = patch.temperature {
        set("temperature", v.filter(|t| t.is_finite()).map(|t| t.clamp(0.0, 2.0)).map_or(Value::Null, Value::from));
    }
    if let Some(v) = patch.system_prompt {
        set("systemPrompt", clip(&v, MAX_PROMPT).into());
    }
    if let Some(v) = patch.headers {
        for (name, value) in &v {
            if !valid_header_name(name) || value.contains(['\r', '\n']) {
                return Err(format!("AI-001: invalid header \"{name}\""));
            }
        }
        set("headers", serde_json::to_value(v).map_err(|e| format!("AI-001: {e}"))?);
    }
    if let Some(v) = patch.api_version {
        set("apiVersion", clip(&v, 50).into());
    }
    if let Some(v) = patch.allow_invalid_certificates {
        set("allowInvalidCertificates", v.into());
    }
    if let Some(v) = patch.use_windows_auth {
        set("useWindowsAuth", v.into());
    }
    if let Some(key) = patch.api_key {
        let key = key.trim().to_string();
        // a key typed in the page replaces whatever the file had, in either form
        file.insert("apiKey".into(), "".into());
        if key.is_empty() {
            file.insert("apiKeyProtected".into(), "".into());
        } else {
            file.insert("apiKeyProtected".into(), protect(&key)?.into());
        }
    }
    Ok(())
}

/// `%ProgramData%\Yuval\ai-config.json` (an administrator's settings for every user).
pub fn machine_file() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(PathBuf::from).filter(|p| p.is_absolute()).map(|p| p.join(crate::paths::APP_DIR).join(FILE_NAME))
}

/// `%LOCALAPPDATA%\Yuval\ai-config.json`.
pub fn user_file() -> Result<PathBuf, String> {
    Ok(crate::paths::root()?.join(FILE_NAME))
}

/// The JSON object in `path`; `Ok(None)` when there is no file. A file that is not a JSON object is an error.
pub fn read_object(path: &Path) -> Result<Option<Map<String, Value>>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("AI-001: {} cannot be read: {}", FILE_NAME, e)),
    };
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(Some(map)),
        Ok(_) => Err(format!("AI-001: {FILE_NAME} must be a JSON object {{ … }}")),
        Err(e) => Err(format!("AI-001: {FILE_NAME} is not valid JSON (line {}, column {})", e.line(), e.column())),
    }
}

/// The machine object, then the user's keys over it (unless the machine file is locked).
pub fn merge(machine: Option<Map<String, Value>>, user: Option<Map<String, Value>>) -> AiConfig {
    let locked = machine.as_ref().and_then(|m| m.get("locked")).and_then(Value::as_bool).unwrap_or(false);
    let mut merged = machine.unwrap_or_default();
    if !locked {
        for (k, v) in user.unwrap_or_default() {
            if k != "locked" {
                merged.insert(k, v);
            }
        }
    }
    let mut config: AiConfig = serde_json::from_value(Value::Object(merged.clone())).unwrap_or_else(|_| lenient(&merged));
    config.locked = locked;
    config
}

/// A file with a key of the wrong type still gives the rest: each key is read on its own.
fn lenient(map: &Map<String, Value>) -> AiConfig {
    let mut out = serde_json::to_value(AiConfig::default()).unwrap_or(Value::Null);
    if let Value::Object(o) = &mut out {
        for (k, v) in map {
            let mut probe = Map::new();
            probe.insert(k.clone(), v.clone());
            if serde_json::from_value::<AiConfig>(Value::Object(probe)).is_ok() {
                o.insert(k.clone(), v.clone());
            }
        }
    }
    serde_json::from_value(out).unwrap_or_default()
}

/// Write `map` to `path` through a temp file (a crash never leaves half a file).
pub fn write_object(path: &Path, map: &Map<String, Value>) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("AI-001: cannot create the folder: {e}"))?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(map.clone())).map_err(|e| format!("AI-001: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| format!("AI-001: cannot write {FILE_NAME}: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("AI-001: cannot write {FILE_NAME}: {e}")
    })
}

/// The file a user gets the first time (every key with its default, and how to fill them in).
pub fn template() -> Map<String, Value> {
    let readme = [
        "הגדרות החיבור של יובל לשרת AI. שמרו את הקובץ, והשאלה הבאה כבר תשתמש בהגדרות החדשות (אין צורך להפעיל מחדש).",
        "enabled: true כדי להפעיל.",
        "provider: openai (כל שרת תואם OpenAI: vLLM, Ollama, LM Studio, LiteLLM, OpenAI), azure (Azure OpenAI) או anthropic (Claude).",
        "endpoint: כתובת השרת, למשל http://ai-server:8000/v1 או הכתובת המלאה של chat/completions.",
        "apiKey: המפתח (או env:NAME כדי לקרוא אותו ממשתנה סביבה). במסך ההגדרות של מרכז יובל המפתח נשמר מוצפן.",
        "model: שם המודל (ב-Azure: שם ה-deployment).",
        "mode: fallback = רק שאלות שהמנוע המובנה לא הבין; always = כל שאלה.",
        "shareResults: לשלוח ל-AI את התוצאות שנמצאו (כותרות, שעות, שולחים) כדי שיענה עליהן.",
        "headers: כותרות HTTP נוספות לשרת, למשל {\"X-Team\": \"it\"}.",
        "allowInvalidCertificates: true רק לשרת פנימי עם תעודה שהמחשב לא מכיר.",
        "useWindowsAuth: true כדי להזדהות מול השרת עם משתמש Windows (Negotiate/NTLM).",
    ];
    let mut map = Map::new();
    map.insert("_readme".into(), Value::Array(readme.iter().map(|s| Value::from(*s)).collect()));
    if let Ok(Value::Object(defaults)) = serde_json::to_value(AiConfig::default()) {
        for (k, v) in defaults {
            if k != "locked" {
                map.insert(k, v);
            }
        }
    }
    map
}

// ---- DPAPI: the key encrypted for this Windows user (hex of the blob) ----

const ENTROPY: &[u8] = b"Yuval.ai-config.apiKey";

pub fn protect(secret: &str) -> Result<String, String> {
    dpapi(secret.as_bytes(), true).map(|b| hex(&b)).map_err(|e| format!("AI-001: the key could not be encrypted: {e}"))
}

pub fn unprotect(hex_blob: &str) -> Option<String> {
    let blob = unhex(hex_blob)?;
    dpapi(&blob, false).ok().and_then(|b| String::from_utf8(b).ok())
}

fn dpapi(data: &[u8], encrypt: bool) -> windows::core::Result<Vec<u8>> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let entropy = CRYPT_INTEGER_BLOB { cbData: ENTROPY.len() as u32, pbData: ENTROPY.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        if encrypt {
            CryptProtectData(&input, windows::core::w!("Yuval AI key"), Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)?;
        } else {
            CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)?;
        }
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(out.pbData as *mut core::ffi::c_void));
        Ok(bytes)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(v: Value) -> Option<Map<String, Value>> {
        match v {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }

    #[test]
    fn the_user_file_overrides_the_machine_file_key_by_key() {
        let c = merge(
            obj(json!({ "enabled": true, "endpoint": "http://ai:8000/v1", "model": "llama" })),
            obj(json!({ "model": "qwen", "mode": "always" })),
        );
        assert!(c.enabled);
        assert_eq!(c.endpoint, "http://ai:8000/v1");
        assert_eq!(c.model, "qwen");
        assert_eq!(c.mode(), Mode::Always);
        assert!(!c.locked);
    }

    #[test]
    fn a_locked_machine_file_ignores_the_user() {
        let c = merge(obj(json!({ "locked": true, "enabled": true, "model": "a" })), obj(json!({ "model": "b", "enabled": false })));
        assert!(c.locked && c.enabled);
        assert_eq!(c.model, "a");
        // and a user file cannot lock itself
        assert!(!merge(None, obj(json!({ "locked": true }))).locked);
    }

    #[test]
    fn a_key_of_the_wrong_type_does_not_lose_the_others() {
        let c = merge(None, obj(json!({ "enabled": true, "timeoutSeconds": "soon", "model": "m", "_readme": ["x"] })));
        assert!(c.enabled);
        assert_eq!(c.model, "m");
        assert_eq!(c.timeout_seconds(), DEFAULT_TIMEOUT_SECONDS);
    }

    #[test]
    fn defaults_and_limits() {
        let c = AiConfig { timeout_seconds: 0, max_tokens: 1_000_000, ..AiConfig::default() };
        assert_eq!(c.timeout_seconds(), DEFAULT_TIMEOUT_SECONDS);
        assert_eq!(c.max_tokens(), MAX_TOKENS_LIMIT);
        assert_eq!(AiConfig { timeout_seconds: 9999, ..AiConfig::default() }.timeout_seconds(), MAX_TIMEOUT_SECONDS);
        assert_eq!(Mode::parse("whatever"), Mode::Fallback);
        assert_eq!(Provider::parse("Claude"), Some(Provider::Anthropic));
        assert_eq!(Provider::parse("gemini"), None);
    }

    #[test]
    fn check_names_what_is_missing() {
        let mut c = AiConfig { enabled: true, ..AiConfig::default() };
        assert!(c.check().unwrap_err().contains("endpoint"));
        c.endpoint = "ftp://x".into();
        assert!(c.check().unwrap_err().starts_with("AI-001"));
        c.endpoint = "http://ai:8000/v1".into();
        assert!(c.check().unwrap_err().contains("model"));
        c.model = "llama".into();
        assert_eq!(c.check(), Ok(Provider::OpenAi), "an OpenAI-compatible server may need no key");
        c.provider = "anthropic".into();
        assert!(c.check().unwrap_err().contains("key"));
        c.api_key = "sk-1".into();
        assert_eq!(c.check(), Ok(Provider::Anthropic));
        c.headers.insert("Bad Header".into(), "x".into());
        assert!(c.check().unwrap_err().contains("header"));
    }

    #[test]
    fn azure_takes_the_model_from_a_deployment_url() {
        let c = AiConfig {
            provider: "azure".into(),
            endpoint: "https://x.openai.azure.com/openai/deployments/gpt/chat/completions?api-version=2024-10-21".into(),
            api_key: "k".into(),
            ..AiConfig::default()
        };
        assert_eq!(c.check(), Ok(Provider::Azure));
    }

    #[test]
    fn env_keys_are_read_from_the_environment() {
        std::env::set_var("YUVAL_TEST_AI_KEY", " secret ");
        let c = AiConfig { api_key: "env:YUVAL_TEST_AI_KEY".into(), ..AiConfig::default() };
        assert_eq!(c.key(), "secret");
        assert_eq!(AiConfig { api_key: "env:YUVAL_NOT_SET_X".into(), ..AiConfig::default() }.key(), "");
    }

    #[test]
    fn the_protected_key_round_trips_for_this_user() {
        let blob = protect("sk-test-123").unwrap();
        assert!(!blob.contains("sk-test"));
        assert_eq!(unprotect(&blob).as_deref(), Some("sk-test-123"));
        let c = AiConfig { api_key_protected: blob, ..AiConfig::default() };
        assert_eq!(c.key(), "sk-test-123");
        assert!(c.has_key());
        assert_eq!(unprotect("zz"), None);
    }

    #[test]
    fn a_patch_keeps_other_keys_and_encrypts_the_key() {
        let mut file = template();
        let patch: AiConfigPatch = serde_json::from_value(json!({ "enabled": true, "apiKey": "sk-9", "provider": "Claude", "temperature": null, "timeoutSeconds": 1 })).unwrap();
        apply_patch(&mut file, patch).unwrap();
        assert!(file.contains_key("_readme"));
        assert_eq!(file["provider"], "anthropic");
        assert_eq!(file["apiKey"], "");
        assert_eq!(file["timeoutSeconds"], 5);
        let c = merge(None, Some(file.clone()));
        assert_eq!(c.key(), "sk-9");
        let clear: AiConfigPatch = serde_json::from_value(json!({ "apiKey": "" })).unwrap();
        apply_patch(&mut file, clear).unwrap();
        assert!(!merge(None, Some(file)).has_key());
        let bad: AiConfigPatch = serde_json::from_value(json!({ "headers": { "X": "a\r\nb" } })).unwrap();
        assert!(apply_patch(&mut Map::new(), bad).is_err());
    }

    #[test]
    fn invalid_json_is_reported_with_its_place() {
        let dir = std::env::temp_dir().join(format!("yuval-ai-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(FILE_NAME);
        assert_eq!(read_object(&p), Ok(None));
        std::fs::write(&p, "\u{feff}{ \"enabled\": true, }").unwrap();
        let e = read_object(&p).unwrap_err();
        assert!(e.starts_with("AI-001") && e.contains("line 1"), "{e}");
        write_object(&p, &template()).unwrap();
        let back = read_object(&p).unwrap().unwrap();
        assert_eq!(back["mode"], "fallback");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
