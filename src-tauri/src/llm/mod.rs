//! The user's own AI server (docs/AI_API.md): a language model the organisation runs or allows, behind an
//! OpenAI-compatible, Azure OpenAI or Anthropic API. Off until `ai-config.json` turns it on.
//!
//! Smart search asks it (see `assistant::exec::Sources::ask_ai`): in mode `fallback` only for what the
//! built-in engine did not understand, in mode `always` for every question, with the results the built-in
//! search found when `shareResults` is on. Nothing else in the app talks to it.
//!
//! Privacy: questions, answers and results go to the configured server only. Logs carry the provider, the
//! status, the duration and `AI-00x` codes; never the question, the answer, the endpoint or the key.

pub mod config;
mod http;
pub mod protocol;

pub use config::{AiConfig, AiConfigPatch, AiConfigView, Mode, Provider};
use protocol::Turn;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The settings in force and why they could not be read, if so.
pub struct Loaded {
    pub config: AiConfig,
    pub error: Option<String>,
}

/// Read both files now (they are a few hundred bytes; a change applies to the next question).
pub fn load() -> Loaded {
    let mut error = None;
    let machine = config::machine_file().and_then(|p| config::read_object(&p).map_err(|e| error = Some(format!("{e} ({})", p.display()))).ok().flatten());
    let user = config::user_file().ok().and_then(|p| config::read_object(&p).map_err(|e| error = error.clone().or(Some(e))).ok().flatten());
    Loaded { config: config::merge(machine, user), error }
}

/// The settings when the AI is switched on and complete; `None` when it is off. A switched-on but broken
/// configuration is an `AI-001` error, so the user learns why their AI does not answer.
pub fn active() -> Option<Result<AiConfig, String>> {
    let loaded = load();
    if let Some(e) = loaded.error {
        // a broken file: report it when the user meant the AI to be on
        return (loaded.config.enabled || file_mentions_enabled()).then_some(Err(e));
    }
    if !loaded.config.enabled {
        return None;
    }
    Some(loaded.config.check().map(|_| loaded.config))
}

fn file_mentions_enabled() -> bool {
    config::user_file().ok().and_then(|p| std::fs::read_to_string(p).ok()).is_some_and(|t| t.contains("\"enabled\": true") || t.contains("\"enabled\":true"))
}

/// One question for the model.
pub struct Question<'a> {
    pub text: &'a str,
    pub hebrew: bool,
    /// What the PC's own search found, as plain lines (only when `shareResults` is on).
    pub results: Option<&'a str>,
    pub now: chrono::DateTime<chrono::Local>,
}

/// Earlier exchanges, so "ומה לגבי מחר?" follows the previous question. Memory only.
struct Exchange {
    question: String,
    answer: String,
    at: Instant,
}

const HISTORY_TURNS: usize = 6;
const HISTORY_TTL: Duration = Duration::from_secs(10 * 60);
static HISTORY: Mutex<VecDeque<Exchange>> = Mutex::new(VecDeque::new());

fn history() -> std::sync::MutexGuard<'static, VecDeque<Exchange>> {
    HISTORY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Forget the conversation (smart search switched off).
pub fn forget() {
    history().clear();
}

pub fn system_prompt(c: &AiConfig, q: &Question) -> String {
    use chrono::Datelike;
    let weekday = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][q.now.weekday().num_days_from_monday() as usize];
    let mut s = format!(
        "You are Yuval (יובל), the assistant built into this Windows PC of an organisation. \
         Now it is {weekday} {} (local time).\n\
         Answer in the language of the question ({}). Be brief and direct: one to four short sentences, or a short list. \
         Plain text only: no Markdown headings, tables or bold.\n\
         You cannot open programs or change anything on the PC yourself; when asked to, tell the user how.",
        q.now.format("%Y-%m-%d %H:%M"),
        if q.hebrew { "this question is in Hebrew: answer in Hebrew" } else { "English unless the question is in another language" }
    );
    if q.results.is_some() {
        s.push_str(
            "\nThe PC's own search ran on the user's calendar, mail, notes and files for this question; its results come with the question. \
             Answer from them. If they do not contain the answer, say so briefly.",
        );
    }
    let extra = c.system_prompt.trim();
    if !extra.is_empty() {
        s.push_str("\n\n");
        s.push_str(extra);
    }
    s
}

fn user_turn(q: &Question) -> String {
    match q.results {
        Some(r) if !r.trim().is_empty() => format!("{}\n\n[Search results on this PC]\n{}", q.text.trim(), r.trim()),
        Some(_) => format!("{}\n\n[Search results on this PC]\n(nothing found)", q.text.trim()),
        None => q.text.trim().to_string(),
    }
}

/// Ask the model. Blocking (WinHTTP, bounded by the configured timeout): call it on a worker thread.
pub fn ask(c: &AiConfig, q: &Question) -> Result<String, String> {
    let provider = c.check()?;
    let mut turns: Vec<Turn> = {
        let mut h = history();
        h.retain(|e| e.at.elapsed() < HISTORY_TTL);
        h.iter().flat_map(|e| [Turn { user: true, text: e.question.clone() }, Turn { user: false, text: e.answer.clone() }]).collect()
    };
    turns.push(Turn { user: true, text: user_turn(q) });
    let answer = send(c, provider, &system_prompt(c, q), &turns)?;
    let answer = plain_text(&answer);
    let mut h = history();
    h.push_back(Exchange { question: q.text.trim().to_string(), answer: answer.clone(), at: Instant::now() });
    while h.len() > HISTORY_TURNS {
        h.pop_front();
    }
    Ok(answer)
}

fn send(c: &AiConfig, provider: Provider, system: &str, turns: &[Turn]) -> Result<String, String> {
    let url_text = protocol::request_url(c, provider);
    let url = protocol::parse_url(&url_text).map_err(|e| format!("AI-001: {e}"))?;
    let headers = protocol::headers(c, provider, &c.key());
    let body = serde_json::to_vec(&protocol::body(c, provider, system, turns)).map_err(|e| format!("AI-001: {e}"))?;
    let started = Instant::now();
    let result = http::post(&http::Request {
        url: &url,
        headers: &headers,
        body: &body,
        timeout: Duration::from_secs(c.timeout_seconds() as u64),
        allow_invalid_certificates: c.allow_invalid_certificates,
        windows_auth: c.use_windows_auth,
    });
    let ms = started.elapsed().as_millis();
    let answer = result.and_then(|r| if (200..300).contains(&r.status) { protocol::parse_answer(provider, &r.body) } else { Err(protocol::status_error(r.status, &r.body)) });
    match &answer {
        Ok(text) => crate::dlog!("INFO", "ai", "{} answered in {} ms ({} chars)", provider.as_str(), ms, text.chars().count()),
        Err(e) => crate::dlog!("WARN", "ai", "{} failed after {} ms: {}", provider.as_str(), ms, crate::diagnostics::code_in(e).unwrap_or("AI-002")),
    }
    answer
}

/// The result of "בדוק חיבור" in the settings.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub ok: bool,
    pub ms: u64,
    /// The model's reply (a few words) or the error.
    pub message: String,
    /// The URL the request went to (the user's own setting, shown so a wrong path is easy to see).
    pub url: String,
}

/// Send a tiny question with the settings as they are now (switched on or not).
pub fn test() -> TestResult {
    let loaded = load();
    if let Some(e) = loaded.error {
        return TestResult { ok: false, ms: 0, message: e, url: String::new() };
    }
    let c = loaded.config;
    let provider = match c.check() {
        Ok(p) => p,
        Err(e) => return TestResult { ok: false, ms: 0, message: e, url: String::new() },
    };
    let url = protocol::request_url(&c, provider);
    let started = Instant::now();
    let turns = [Turn { user: true, text: "Reply with the single word OK.".into() }];
    let result = send(&c, provider, "You are a connection test. Reply with the single word OK.", &turns);
    let ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(text) => TestResult { ok: true, ms, message: text.chars().take(200).collect(), url },
        Err(e) => TestResult { ok: false, ms, message: e, url },
    }
}

/// Markdown the island cannot draw, removed: `**bold**`, `__x__`, `#` headings, code fences and backticks.
pub fn plain_text(text: &str) -> String {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            continue;
        }
        let line = if trimmed.starts_with('#') { trimmed.trim_start_matches('#').trim_start() } else { line };
        let line = line.trim_end();
        let line = line.replace("**", "").replace("__", "").replace('`', "");
        let line = if let Some(rest) = line.trim_start().strip_prefix("* ") { format!("• {rest}") } else if let Some(rest) = line.trim_start().strip_prefix("- ") { format!("• {rest}") } else { line };
        out.push(line);
    }
    // no runs of blank lines
    let mut text = String::new();
    let mut blank = false;
    for l in out {
        if l.trim().is_empty() {
            if !blank && !text.is_empty() {
                text.push('\n');
            }
            blank = true;
        } else {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&l);
            blank = false;
        }
    }
    text.trim().to_string()
}

/// The answer as a card: a headline (the first sentence or line, at most `HEADLINE_CHARS`) and the rest.
pub fn split_answer(text: &str) -> (String, String) {
    const HEADLINE_CHARS: usize = 140;
    let text = text.trim();
    let first_line = text.lines().next().unwrap_or("").trim();
    let rest_after_line = text[text.find('\n').map(|i| i + 1).unwrap_or(text.len())..].trim();
    if first_line.chars().count() <= HEADLINE_CHARS {
        // a short first line that only introduces a list ("These are your meetings:") stays the headline
        return (first_line.to_string(), rest_after_line.to_string());
    }
    // a long first line: cut after its first sentence, else at a word near the limit
    let cut = first_line
        .char_indices()
        .take_while(|(i, _)| first_line[..*i].chars().count() < HEADLINE_CHARS)
        .filter(|(i, c)| matches!(c, '.' | '!' | '?') && first_line[*i + c.len_utf8()..].starts_with(' '))
        .map(|(i, c)| i + c.len_utf8())
        .next()
        .unwrap_or_else(|| {
            let limit = first_line.char_indices().nth(HEADLINE_CHARS).map(|(i, _)| i).unwrap_or(first_line.len());
            first_line[..limit].rfind(' ').filter(|i| *i > 0).unwrap_or(limit)
        });
    let head = first_line[..cut].trim().to_string();
    let tail = first_line[cut..].trim();
    let rest = [tail, rest_after_line].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join("\n");
    (head, rest)
}

// ---- the Center's settings page and the tray ----

pub fn view() -> AiConfigView {
    let loaded = load();
    let path = config::user_file().unwrap_or_default();
    AiConfigView::of(&loaded.config, &path, loaded.error)
}

/// Change the user's file (created from the template when there is none) and return the new view.
pub fn update(patch: AiConfigPatch) -> Result<AiConfigView, String> {
    let machine_locked = config::machine_file().and_then(|p| config::read_object(&p).ok().flatten()).and_then(|m| m.get("locked").and_then(|v| v.as_bool())).unwrap_or(false);
    if machine_locked {
        return Err("AI-001: the AI settings are managed by the administrator".into());
    }
    let path = config::user_file()?;
    let mut file = config::read_object(&path)?.unwrap_or_else(config::template);
    config::apply_patch(&mut file, patch)?;
    config::write_object(&path, &file)?;
    crate::dlog!("INFO", "ai", "settings saved");
    Ok(view())
}

/// Open the user's `ai-config.json` in Notepad (written from the template first when there is none).
pub fn open_file() -> Result<(), String> {
    let path = config::user_file()?;
    if !path.exists() {
        config::write_object(&path, &config::template())?;
    }
    let windir = std::env::var_os("SystemRoot").map(std::path::PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| r"C:\Windows".into());
    std::process::Command::new(windir.join("System32").join("notepad.exe"))
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("AI-001: cannot open {}: {e}", config::FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn markdown_becomes_plain_text() {
        let md = "## היום\n\n**3 פגישות**:\n\n\n- 09:00 צוות\n* 11:00 `לקוח`\n```\ncode\n```";
        assert_eq!(plain_text(md), "היום\n\n3 פגישות:\n\n• 09:00 צוות\n• 11:00 לקוח\ncode");
    }

    #[test]
    fn answers_split_into_a_headline_and_the_rest() {
        assert_eq!(split_answer("שלום!"), ("שלום!".into(), "".into()));
        assert_eq!(split_answer("יש לך 3 פגישות:\n• 09:00 צוות\n• 11:00 לקוח"), ("יש לך 3 פגישות:".into(), "• 09:00 צוות\n• 11:00 לקוח".into()));
        let long = format!("{}. {}", "א".repeat(60), "ב ".repeat(100));
        let (h, r) = split_answer(&long);
        assert_eq!(h, format!("{}.", "א".repeat(60)));
        assert!(r.starts_with('ב'));
        let no_stop = "word ".repeat(60);
        let (h, r) = split_answer(&no_stop);
        assert!(h.chars().count() <= 140 && !h.ends_with(' ') && !r.is_empty());
    }

    #[test]
    fn the_prompt_says_the_date_the_language_and_the_results() {
        let c = AiConfig { system_prompt: "Our team is IT.".into(), ..AiConfig::default() };
        let now = chrono::Local.with_ymd_and_hms(2026, 10, 13, 9, 30, 0).unwrap();
        let q = Question { text: "מה יש לי היום?", hebrew: true, results: Some("- [calendar] 10:00 צוות"), now };
        let s = system_prompt(&c, &q);
        assert!(s.contains("Tuesday 2026-10-13 09:30") && s.contains("Hebrew") && s.contains("results") && s.ends_with("Our team is IT."), "{s}");
        assert!(user_turn(&q).contains("[Search results on this PC]\n- [calendar] 10:00 צוות"));
        let bare = Question { results: None, ..q };
        assert_eq!(user_turn(&bare), "מה יש לי היום?");
        assert!(!system_prompt(&AiConfig::default(), &bare).contains("results come"));
    }
}

/// Against a real HTTP server: `node <scratch>/mock/server.cjs 18080`, then
/// `cargo test --lib llm::live -- --ignored --nocapture`.
#[cfg(test)]
mod live {
    use super::*;

    fn cfg(provider: &str, endpoint: &str, key: &str) -> AiConfig {
        AiConfig { enabled: true, provider: provider.into(), endpoint: endpoint.into(), model: "mock-1".into(), api_key: key.into(), timeout_seconds: 5, ..AiConfig::default() }
    }

    fn q(text: &str) -> Question<'_> {
        Question { text, hebrew: true, results: None, now: chrono::Local::now() }
    }

    #[test]
    #[ignore]
    fn the_three_formats_against_the_mock_server() {
        let a = ask(&cfg("openai", "http://127.0.0.1:18080/v1", "sk-open"), &q("מה בירת צרפת?")).unwrap();
        println!("openai: {a}");
        assert!(a.starts_with("תשובה מהשרת המדומה: קיבלתי את השאלה \"מה בירת צרפת?\""), "{a}");
        let a = ask(&cfg("anthropic", "http://127.0.0.1:18080", "sk-ant"), &q("ומה בירת איטליה?")).unwrap();
        println!("anthropic: {a}");
        assert!(a.contains("ומה בירת איטליה?"));
        let a = ask(&cfg("azure", "http://127.0.0.1:18080", "az-key"), &q("hello")).unwrap();
        println!("azure: {a}");
        let e = ask(&cfg("openai", "http://127.0.0.1:18080/v1", "bad-key"), &q("x")).unwrap_err();
        println!("bad key: {e}");
        assert!(e.starts_with("AI-004") && e.contains("invalid api key"), "{e}");
        let started = std::time::Instant::now();
        let e = ask(&cfg("openai", "http://127.0.0.1:18080/v1", "k"), &q("SLOW")).unwrap_err();
        println!("timeout after {:?}: {e}", started.elapsed());
        assert!(e.starts_with("AI-003") && started.elapsed().as_secs() < 9, "{e}");
        let e = ask(&cfg("openai", "http://127.0.0.1:18099/v1", "k"), &q("x")).unwrap_err();
        println!("nothing listening: {e}");
        assert!(e.starts_with("AI-002"), "{e}");
        let e = ask(&cfg("openai", "http://no-such-host.invalid/v1", "k"), &q("x")).unwrap_err();
        println!("no such host: {e}");
        assert!(e.starts_with("AI-002"), "{e}");
        let e = ask(&cfg("openai", "http://127.0.0.1:18080/wrong", "k"), &q("x")).unwrap_err();
        println!("wrong path: {e}");
        assert!(e.starts_with("AI-005") && e.contains("404"), "{e}");
    }
}
