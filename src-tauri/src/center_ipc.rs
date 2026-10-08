//! The named pipe the Yuval Center talks to the island through (docs: contract section 4).
//!
//! `\\.\pipe\CompanyIsland.Center.<session>.<sidhash>`, readable and writable by the current
//! user only (protected DACL), no remote clients, one server per user session. The protocol is
//! one UTF-8 JSON object per line (16 MiB at most): requests `{"id","cmd","args"}` get a
//! `{"id","ok","result"|"error"}` answer, and the server pushes `{"event","payload"}` lines
//! (`settings-changed`, `notes-changed` to everyone, `navigate` to Center clients).
//! Rust stays the only writer of `settings.json` and `notes.json`: every command goes through
//! the same functions the island's own commands use.
//!
//! The dispatcher only knows the `CenterBackend` trait, so the tests run it against a fake.
//! Nothing here ever logs a request or response body (only command names and counts).

use crate::{assistant, debug_log, monitors, notes, notifications, rt, settings, window};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions};
use tokio::sync::{mpsc, Notify};

const PROTOCOL: u64 = 1;
/// One protocol line, each way.
const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
/// Pipe instances at once (clients plus the one that is listening).
const MAX_INSTANCES: usize = 4;
/// Lines waiting for one client. A client that falls this far behind is disconnected.
const QUEUE_LINES: usize = 256;
const MAX_LOG_CHARS: usize = 300;
const MAX_REQUEST_ID: u64 = 1 << 53;
const TABS: [&str; 5] = ["calendar", "notifications", "notes", "about", "settings"];

// =============================================================================
// Names and security
// =============================================================================

/// `\\.\pipe\CompanyIsland.Center.<session>.<first 16 hex chars of SHA-256(SID string)>`.
pub fn pipe_name(session: u32, sid: &str) -> String {
    let digest = Sha256::digest(sid.as_bytes());
    let hash: String = digest.iter().take(8).map(|b| format!("{:02x}", b)).collect();
    format!(r"\\.\pipe\CompanyIsland.Center.{session}.{hash}")
}

/// Protected DACL with one entry: generic-all for this user, nobody else (not even Administrators).
pub fn sddl_for(sid: &str) -> String {
    format!("D:P(A;;GA;;;{sid})")
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{
        GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    };
    use windows::Win32::System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeClientSessionId};
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, OpenProcessToken};

    /// The current user's SID as `S-1-5-21-...`.
    pub fn current_user_sid() -> Result<String, String> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(|e| format!("token: {e}"))?;
            let result = (|| {
                let mut needed = 0u32;
                // The first call only reports the size.
                let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
                if needed == 0 {
                    return Err("token size".to_string());
                }
                // u64 elements: the TOKEN_USER header needs 8-byte alignment.
                let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
                GetTokenInformation(token, TokenUser, Some(buffer.as_mut_ptr() as *mut c_void), needed, &mut needed)
                    .map_err(|e| format!("token user: {e}"))?;
                let user = &*(buffer.as_ptr() as *const TOKEN_USER);
                let mut text = PWSTR::null();
                ConvertSidToStringSidW(user.User.Sid, &mut text).map_err(|e| format!("sid text: {e}"))?;
                let sid = text.to_string().map_err(|e| format!("sid text: {e}"));
                let _ = LocalFree(HLOCAL(text.0 as *mut c_void));
                sid
            })();
            let _ = CloseHandle(token);
            result
        }
    }

    /// The Windows session this process runs in.
    pub fn current_session() -> Result<u32, String> {
        let mut session = 0u32;
        unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) }.map_err(|e| format!("session: {e}"))?;
        Ok(session)
    }

    /// Pid and session of the process at the other end of a pipe instance.
    pub fn client_of(pipe: *mut c_void) -> Result<(u32, u32), String> {
        let (mut pid, mut session) = (0u32, 0u32);
        unsafe {
            GetNamedPipeClientProcessId(HANDLE(pipe), &mut pid).map_err(|e| format!("client pid: {e}"))?;
            GetNamedPipeClientSessionId(HANDLE(pipe), &mut session).map_err(|e| format!("client session: {e}"))?;
        }
        Ok((pid, session))
    }

    /// A security descriptor built from SDDL, kept alive for as long as pipe instances are created.
    pub struct Security {
        descriptor: PSECURITY_DESCRIPTOR,
        attributes: SECURITY_ATTRIBUTES,
    }

    // The descriptor is immutable after creation and only ever read by CreateNamedPipe.
    unsafe impl Send for Security {}
    unsafe impl Sync for Security {}

    impl Security {
        pub fn from_sddl(sddl: &str) -> Result<Self, String> {
            let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    PCWSTR(wide.as_ptr()),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    None,
                )
            }
            .map_err(|e| format!("security descriptor: {e}"))?;
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor.0,
                bInheritHandle: false.into(),
            };
            Ok(Self { descriptor, attributes })
        }

        /// Pointer to the SECURITY_ATTRIBUTES, valid while `self` lives.
        pub fn as_ptr(&self) -> *mut c_void {
            &self.attributes as *const SECURITY_ATTRIBUTES as *mut c_void
        }
    }

    impl Drop for Security {
        fn drop(&mut self) {
            unsafe {
                let _ = LocalFree(HLOCAL(self.descriptor.0));
            }
        }
    }
}

use win::Security;

// =============================================================================
// Protocol: requests, commands, responses
// =============================================================================

#[derive(Debug, PartialEq)]
pub struct Request {
    pub id: u64,
    pub cmd: String,
    pub args: Value,
}

fn invalid_args() -> String {
    "APP-031: invalid arguments".to_string()
}

/// Parse one request line. The error carries the request id when one could be read, so the
/// caller can still answer it.
pub fn parse_request(line: &[u8]) -> Result<Request, (Option<u64>, String)> {
    let value: Value =
        serde_json::from_slice(line).map_err(|_| (None, "APP-031: request is not valid JSON".to_string()))?;
    let id = value.get("id").and_then(Value::as_u64).filter(|id| (1..=MAX_REQUEST_ID).contains(id));
    let Some(id) = id else {
        return Err((None, "APP-031: request needs an integer id".to_string()));
    };
    let Some(cmd) = value.get("cmd").and_then(Value::as_str) else {
        return Err((Some(id), "APP-031: request needs a cmd".to_string()));
    };
    let args = match value.get("args") {
        None | Some(Value::Null) => json!({}),
        Some(args @ Value::Object(_)) => args.clone(),
        Some(_) => return Err((Some(id), invalid_args())),
    };
    Ok(Request { id, cmd: cmd.to_string(), args })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// A validated command for the backend (everything except `hello`).
#[derive(Debug)]
pub enum Cmd {
    GetSettings,
    UpdateSettings(settings::SettingsPatch),
    NotesLoad,
    NotesSave(Vec<notes::Note>),
    GetMonitors,
    GetNotificationStatus,
    RequestNotificationAccess,
    OpenLogDir,
    ShowIsland(Option<String>),
    Log { level: LogLevel, message: String },
    /// Smart search (docs/AI_SEARCH.md section 3, "Pipe"). The text is memory only, never logged.
    SearchSubmit { text: String },
    SearchChoose { query_id: String, option_id: String, remember: bool },
    SearchExtend { query_id: String },
    SearchOpen { query_id: String, item_id: String },
    SearchResults { query_id: String },
    SearchHistory,
}

impl Cmd {
    /// The wire name, for debug logs (never the arguments).
    pub fn name(&self) -> &'static str {
        match self {
            Cmd::GetSettings => "getSettings",
            Cmd::UpdateSettings(_) => "updateSettings",
            Cmd::NotesLoad => "notesLoad",
            Cmd::NotesSave(_) => "notesSave",
            Cmd::GetMonitors => "getMonitors",
            Cmd::GetNotificationStatus => "getNotificationStatus",
            Cmd::RequestNotificationAccess => "requestNotificationAccess",
            Cmd::OpenLogDir => "openLogDir",
            Cmd::ShowIsland(_) => "showIsland",
            Cmd::Log { .. } => "log",
            Cmd::SearchSubmit { .. } => "searchSubmit",
            Cmd::SearchChoose { .. } => "searchChoose",
            Cmd::SearchExtend { .. } => "searchExtend",
            Cmd::SearchOpen { .. } => "searchOpen",
            Cmd::SearchResults { .. } => "searchResults",
            Cmd::SearchHistory => "searchHistory",
        }
    }
}

impl Cmd {
    /// Commands that can run for seconds (they search Outlook, files or apps). The connection
    /// runs these on their own task so the requests behind them are not held up.
    pub fn is_slow(&self) -> bool {
        matches!(
            self,
            Cmd::SearchSubmit { .. } | Cmd::SearchChoose { .. } | Cmd::SearchExtend { .. } | Cmd::SearchOpen { .. }
        )
    }
}

/// Clip to `max` characters.
fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((cut, _)) => text[..cut].to_string(),
        None => text.to_string(),
    }
}

/// A question: 1..=500 characters, not blank.
const MAX_QUERY_CHARS: usize = 500;

fn id_arg(args: &Value, name: &str) -> Result<String, String> {
    match args.get(name) {
        Some(Value::String(id)) if notes::valid_id(id) => Ok(id.clone()),
        _ => Err(invalid_args()),
    }
}

/// A clarification choice: `all`, or `mb:<id>` / `cal:<id>` as `assistant::exec` mints them
/// (the part after the prefix follows the same rule as every other id).
fn valid_option_id(id: &str) -> bool {
    id == "all" || id.strip_prefix("mb:").or_else(|| id.strip_prefix("cal:")).is_some_and(notes::valid_id)
}

fn option_id_arg(args: &Value, name: &str) -> Result<String, String> {
    match args.get(name) {
        Some(Value::String(id)) if valid_option_id(id) => Ok(id.clone()),
        _ => Err(invalid_args()),
    }
}

pub fn parse_command(cmd: &str, args: &Value) -> Result<Cmd, String> {
    let arg = |name: &str| args.get(name).filter(|v| !v.is_null());
    Ok(match cmd {
        "getSettings" => Cmd::GetSettings,
        "updateSettings" => {
            let patch = arg("patch").filter(|p| p.is_object()).ok_or_else(invalid_args)?;
            Cmd::UpdateSettings(serde_json::from_value(patch.clone()).map_err(|_| invalid_args())?)
        }
        "notesLoad" => Cmd::NotesLoad,
        "notesSave" => {
            let list = arg("notes").filter(|n| n.is_array()).ok_or_else(invalid_args)?;
            Cmd::NotesSave(serde_json::from_value(list.clone()).map_err(|_| invalid_args())?)
        }
        "getMonitors" => Cmd::GetMonitors,
        "getNotificationStatus" => Cmd::GetNotificationStatus,
        "requestNotificationAccess" => Cmd::RequestNotificationAccess,
        "openLogDir" => Cmd::OpenLogDir,
        "showIsland" => match arg("tab") {
            None => Cmd::ShowIsland(None),
            Some(Value::String(tab)) if TABS.contains(&tab.as_str()) => Cmd::ShowIsland(Some(tab.clone())),
            Some(_) => return Err(invalid_args()),
        },
        "log" => {
            let level = match arg("level").and_then(Value::as_str) {
                Some("info") => LogLevel::Info,
                Some("warn") => LogLevel::Warn,
                Some("error") => LogLevel::Error,
                _ => return Err(invalid_args()),
            };
            let message = arg("message").and_then(Value::as_str).ok_or_else(invalid_args)?;
            Cmd::Log { level, message: clip(message, MAX_LOG_CHARS) }
        }
        "searchSubmit" => {
            let text = arg("text").and_then(Value::as_str).ok_or_else(invalid_args)?;
            if text.trim().is_empty() || text.chars().count() > MAX_QUERY_CHARS {
                return Err(invalid_args());
            }
            Cmd::SearchSubmit { text: text.to_string() }
        }
        "searchChoose" => Cmd::SearchChoose {
            query_id: id_arg(args, "queryId")?,
            option_id: option_id_arg(args, "optionId")?,
            remember: match args.get("remember") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(b)) => *b,
                Some(_) => return Err(invalid_args()),
            },
        },
        "searchExtend" => Cmd::SearchExtend { query_id: id_arg(args, "queryId")? },
        "searchOpen" => Cmd::SearchOpen { query_id: id_arg(args, "queryId")?, item_id: id_arg(args, "itemId")? },
        "searchResults" => Cmd::SearchResults { query_id: id_arg(args, "queryId")? },
        "searchHistory" => Cmd::SearchHistory,
        _ => return Err("APP-031: unknown command".to_string()),
    })
}

fn response(id: u64, result: Result<Value, String>) -> Value {
    match result {
        Ok(result) => json!({ "id": id, "ok": true, "result": result }),
        Err(error) => json!({ "id": id, "ok": false, "error": error }),
    }
}

/// An answer to a line that carried no usable id.
fn id_less_error(error: String) -> Value {
    json!({ "id": null, "ok": false, "error": error })
}

// =============================================================================
// Backend
// =============================================================================

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// What the pipe can do. The real one wraps the app; blocking work is the implementation's job
/// (the real backend runs it on the blocking pool, never on the async reactor).
pub trait CenterBackend: Send + Sync + 'static {
    fn run(&self, cmd: Cmd) -> BoxFuture<Result<Value, String>>;
}

struct AppBackend {
    app: AppHandle,
}

fn to_json<T: Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| format!("APP-001: {e}"))
}

fn run_sync(app: &AppHandle, cmd: Cmd) -> Result<Value, String> {
    match cmd {
        Cmd::GetSettings => to_json(&app.state::<settings::SettingsStore>().get()),
        Cmd::UpdateSettings(patch) => to_json(&settings::apply_patch(app, patch)),
        Cmd::NotesLoad => to_json(&notes::load()?),
        Cmd::NotesSave(list) => to_json(&notes::save(app, list)?),
        Cmd::GetMonitors => {
            let list = monitors::list();
            if list.is_empty() {
                return Err("WIN-503: monitor enumeration failed".to_string());
            }
            to_json(&monitors::infos(&list))
        }
        Cmd::OpenLogDir => debug_log::open_dir().map(|_| Value::Null),
        Cmd::ShowIsland(tab) => {
            window::show(app);
            // "show": the Center asks for the island to be open on that tab; unlike a tray toggle it
            // must not collapse an island that is already open there.
            if let Err(e) = app.emit("island-toggle", serde_json::json!({ "tab": tab, "show": true })) {
                dlog!("WARN", "center", "emit island-toggle failed: {}", e);
            }
            Ok(Value::Null)
        }
        Cmd::Log { level, message } => {
            let level = match level {
                LogLevel::Info => "INFO",
                LogLevel::Warn => "WARN",
                LogLevel::Error => "ERROR",
            };
            debug_log::write(level, "center", &message);
            Ok(Value::Null)
        }
        Cmd::SearchResults { query_id } => to_json(&assistant::results(&query_id)?),
        Cmd::SearchHistory => to_json(&assistant::history()),
        // Answered by the async arms in `run`.
        Cmd::GetNotificationStatus
        | Cmd::RequestNotificationAccess
        | Cmd::SearchSubmit { .. }
        | Cmd::SearchChoose { .. }
        | Cmd::SearchExtend { .. }
        | Cmd::SearchOpen { .. } => Err("APP-001: unexpected command".to_string()),
    }
}

impl CenterBackend for AppBackend {
    fn run(&self, cmd: Cmd) -> BoxFuture<Result<Value, String>> {
        let app = self.app.clone();
        Box::pin(async move {
            match cmd {
                Cmd::GetNotificationStatus => notifications::notifications_get_status(app).await.map(Value::String),
                Cmd::RequestNotificationAccess => {
                    notifications::notifications_request_access(app).await.map(Value::String)
                }
                Cmd::SearchSubmit { text } => {
                    assistant::submit(app, text, "center".into()).await.and_then(|card| to_json(&card))
                }
                Cmd::SearchChoose { query_id, option_id, remember } => {
                    assistant::choose(app, query_id, option_id, remember).await.and_then(|card| to_json(&card))
                }
                Cmd::SearchExtend { query_id } => {
                    assistant::extend(app, query_id).await.and_then(|card| to_json(&card))
                }
                Cmd::SearchOpen { query_id, item_id } => {
                    assistant::open_item(app, query_id, item_id).await.map(|_| Value::Null)
                }
                other => rt::run_blocking("center_command", move || run_sync(&app, other)).await,
            }
        })
    }
}

// =============================================================================
// Clients and broadcast
// =============================================================================

struct Client {
    id: u64,
    pid: u32,
    /// `hello` succeeded: this connection receives events.
    ready: bool,
    /// `hello` said `"client": "center"`: it is also eligible for `navigate`.
    center: bool,
    tx: mpsc::Sender<Arc<str>>,
    kill: Arc<Notify>,
}

/// Every connected client and the way to reach it. Methods are callable from any thread.
pub struct Hub {
    clients: Mutex<Vec<Client>>,
    next_id: AtomicU64,
}

#[derive(Serialize)]
struct EventOut<'a, T: Serialize> {
    event: &'a str,
    payload: &'a T,
}

impl Hub {
    pub fn new() -> Arc<Hub> {
        Arc::new(Hub { clients: Mutex::new(Vec::new()), next_id: AtomicU64::new(1) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Client>> {
        self.clients.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn register(&self, pid: u32, tx: mpsc::Sender<Arc<str>>, kill: Arc<Notify>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.lock().push(Client { id, pid, ready: false, center: false, tx, kill });
        id
    }

    fn unregister(&self, id: u64) {
        self.lock().retain(|c| c.id != id);
    }

    fn mark_hello(&self, id: u64, center: bool) {
        if let Some(client) = self.lock().iter_mut().find(|c| c.id == id) {
            client.ready = true;
            client.center = center;
        }
    }

    #[cfg(test)]
    fn client_count(&self) -> usize {
        self.lock().len()
    }

    /// Cut a client loose: it fell behind or went away.
    fn drop_client(&self, id: u64, kill: &Notify) {
        self.unregister(id);
        kill.notify_one();
    }

    /// Queue a line for one client; a full or closed queue disconnects it.
    fn push(&self, id: u64, tx: &mpsc::Sender<Arc<str>>, kill: &Notify, line: Arc<str>) -> bool {
        if tx.try_send(line).is_ok() {
            return true;
        }
        dlog!("WARN", "center", "APP-031 center client too slow; disconnected");
        self.drop_client(id, kill);
        false
    }

    /// Send an event to every client that said hello. Does nothing without clients.
    pub fn broadcast<T: Serialize>(&self, event: &str, payload: &T) {
        let targets: Vec<(u64, mpsc::Sender<Arc<str>>, Arc<Notify>)> =
            self.lock().iter().filter(|c| c.ready).map(|c| (c.id, c.tx.clone(), c.kill.clone())).collect();
        if targets.is_empty() {
            return;
        }
        let mut line = match serde_json::to_string(&EventOut { event, payload }) {
            Ok(line) => line,
            Err(e) => {
                dlog!("WARN", "center", "APP-031 event {} not serializable: {}", event, e);
                return;
            }
        };
        line.push('\n');
        let line: Arc<str> = Arc::from(line);
        for (id, tx, kill) in targets {
            self.push(id, &tx, &kill, line.clone());
        }
    }

    /// Send `navigate` to the newest Center client and return its pid. `grant` runs with that
    /// pid before the event is queued, so the Center may take the foreground when it gets it.
    pub fn send_navigate(&self, page: &str, mut grant: impl FnMut(u32)) -> Option<u32> {
        let mut line = serde_json::to_string(&EventOut { event: "navigate", payload: &json!({ "page": page }) }).ok()?;
        line.push('\n');
        let line: Arc<str> = Arc::from(line);
        let candidates: Vec<(u64, u32, mpsc::Sender<Arc<str>>, Arc<Notify>)> = self
            .lock()
            .iter()
            .rev()
            .filter(|c| c.ready && c.center)
            .map(|c| (c.id, c.pid, c.tx.clone(), c.kill.clone()))
            .collect();
        for (id, pid, tx, kill) in candidates {
            grant(pid);
            if self.push(id, &tx, &kill, line.clone()) {
                return Some(pid);
            }
        }
        None
    }
}

static HUB: OnceLock<Arc<Hub>> = OnceLock::new();

/// Push an event to every connected Center (no-op while none is connected).
pub fn broadcast<T: Serialize>(event: &str, payload: &T) {
    if let Some(hub) = HUB.get() {
        hub.broadcast(event, payload);
    }
}

/// Ask a connected Center to show `page`; `None` when no Center is connected. See
/// `Hub::send_navigate` for `grant`.
pub fn send_navigate(page: &str, grant: impl FnMut(u32)) -> Option<u32> {
    HUB.get()?.send_navigate(page, grant)
}

// =============================================================================
// One connection
// =============================================================================

enum Line {
    Complete,
    Eof,
    TooLong,
}

/// Read one `\n`-terminated line into `buf` (without the terminator), never holding more than
/// `max` bytes of it. A partial line at end of stream counts as end of stream.
async fn read_line_limited<R: AsyncBufRead + Unpin>(reader: &mut R, buf: &mut Vec<u8>, max: usize) -> std::io::Result<Line> {
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(Line::Eof);
        }
        match chunk.iter().position(|&b| b == b'\n') {
            Some(end) => {
                if buf.len() + end > max {
                    return Ok(Line::TooLong);
                }
                buf.extend_from_slice(&chunk[..end]);
                reader.consume(end + 1);
                return Ok(Line::Complete);
            }
            None => {
                let len = chunk.len();
                if buf.len() + len > max {
                    return Ok(Line::TooLong);
                }
                buf.extend_from_slice(chunk);
                reader.consume(len);
            }
        }
    }
}

struct Session {
    hub: Arc<Hub>,
    backend: Arc<dyn CenterBackend>,
    client_id: u64,
    /// `hello` has succeeded.
    ready: bool,
}

impl Session {
    fn hello(&mut self, args: &Value) -> Result<Value, String> {
        if args.get("protocol").and_then(Value::as_u64) != Some(PROTOCOL) {
            return Err("APP-031: unsupported protocol".to_string());
        }
        let center = args.get("client").and_then(Value::as_str) == Some("center");
        self.ready = true;
        self.hub.mark_hello(self.client_id, center);
        Ok(json!({ "protocol": PROTOCOL, "appVersion": env!("CARGO_PKG_VERSION") }))
    }

    /// Answer a request now, or hand back a slow search command to be run on its own task. `hello`
    /// and every fast command stay inline, so their replies keep the order of the requests.
    async fn dispatch(&mut self, req: Request) -> Dispatch {
        let Request { id, cmd, args } = req;
        if cmd == "hello" {
            return Dispatch::Reply(response(id, self.hello(&args)));
        }
        if !self.ready {
            return Dispatch::Reply(response(id, Err("APP-031: hello required".to_string())));
        }
        match parse_command(&cmd, &args) {
            Ok(command) if command.is_slow() => Dispatch::Slow { id, command },
            Ok(command) => Dispatch::Reply(run_command(&self.backend, id, command).await),
            Err(error) => Dispatch::Reply(response(id, Err(error))),
        }
    }

    async fn dispatch_line(&mut self, line: &[u8]) -> Dispatch {
        match parse_request(line) {
            Ok(req) => self.dispatch(req).await,
            Err((Some(id), error)) => Dispatch::Reply(response(id, Err(error))),
            Err((None, error)) => Dispatch::Reply(id_less_error(error)),
        }
    }

    /// One line in, one line (with its `\n`) out; slow commands are awaited here.
    #[cfg(test)]
    async fn handle_line(&mut self, line: &[u8]) -> Arc<str> {
        let value = match self.dispatch_line(line).await {
            Dispatch::Reply(value) => value,
            Dispatch::Slow { id, command } => run_command(&self.backend, id, command).await,
        };
        reply_line(value)
    }
}

enum Dispatch {
    Reply(Value),
    Slow { id: u64, command: Cmd },
}

async fn run_command(backend: &Arc<dyn CenterBackend>, id: u64, command: Cmd) -> Value {
    dlog!("DEBUG", "center", "command {}", command.name());
    response(id, backend.run(command).await)
}

fn reply_line(value: Value) -> Arc<str> {
    let mut text = value.to_string();
    text.push('\n');
    Arc::from(text)
}

/// Searches a connection may have running at once; more are refused rather than queued.
const MAX_SLOW_IN_FLIGHT: usize = 2;

async fn write_loop<W: AsyncWrite + Unpin>(mut writer: W, mut lines: mpsc::Receiver<Arc<str>>) {
    while let Some(line) = lines.recv().await {
        if writer.write_all(line.as_bytes()).await.is_err() || writer.flush().await.is_err() {
            break;
        }
    }
}

/// Serve one connected client until it goes away, misbehaves or falls behind.
async fn serve_connection<S>(stream: S, pid: u32, hub: Arc<Hub>, backend: Arc<dyn CenterBackend>)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (read_half, write_half) = tokio::io::split(stream);
    let (tx, rx) = mpsc::channel::<Arc<str>>(QUEUE_LINES);
    let kill = Arc::new(Notify::new());
    let client_id = hub.register(pid, tx.clone(), kill.clone());
    dlog!("INFO", "center", "client connected, pid {}", pid);
    crate::center::note_connected();
    let writer = tokio::spawn(write_loop(write_half, rx));

    let mut session = Session { hub: hub.clone(), backend: backend.clone(), client_id, ready: false };
    // Slow search commands run here, off the read loop; aborted when the connection ends.
    let mut slow: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();
    let mut reader = BufReader::new(read_half);
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = tokio::select! {
            read = read_line_limited(&mut reader, &mut line, MAX_LINE_BYTES) => read,
            _ = kill.notified() => break,
        };
        match read {
            Ok(Line::Complete) => {}
            Ok(Line::Eof) | Err(_) => break,
            Ok(Line::TooLong) => {
                dlog!("WARN", "center", "APP-031 request line too long; closing the connection");
                break;
            }
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.is_empty() {
            continue;
        }
        while slow.try_join_next().is_some() {}
        let reply = match session.dispatch_line(&line).await {
            Dispatch::Reply(value) => reply_line(value),
            Dispatch::Slow { id, .. } if slow.len() >= MAX_SLOW_IN_FLIGHT => {
                reply_line(response(id, Err("APP-031: too many searches at once".to_string())))
            }
            Dispatch::Slow { id, command } => {
                let (backend, hub, tx, kill) = (backend.clone(), hub.clone(), tx.clone(), kill.clone());
                slow.spawn(async move {
                    let reply = reply_line(run_command(&backend, id, command).await);
                    hub.push(client_id, &tx, &kill, reply);
                });
                continue;
            }
        };
        if !hub.push(client_id, &tx, &kill, reply) {
            break;
        }
    }
    slow.abort_all();
    hub.unregister(client_id);
    drop(tx);
    // Dropping the write half (with the read half, at the end of this function) closes the pipe.
    writer.abort();
    dlog!("INFO", "center", "client disconnected, pid {}", pid);
}

// =============================================================================
// The server
// =============================================================================

/// One pipe instance. `first` makes creation fail when the name already exists (a squatted name).
fn create_instance(name: &str, first: bool, security: &Security) -> std::io::Result<NamedPipeServer> {
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .max_instances(MAX_INSTANCES)
        .pipe_mode(PipeMode::Byte);
    // SAFETY: `security.as_ptr()` points to a live SECURITY_ATTRIBUTES for the duration of the call.
    unsafe { options.create_with_security_attributes_raw(name, security.as_ptr()) }
}

/// Check where a fresh connection comes from. Returns the client's pid.
fn admit(server: &NamedPipeServer, our_session: u32) -> Result<u32, String> {
    use std::os::windows::io::AsRawHandle;
    let (pid, session) = win::client_of(server.as_raw_handle())?;
    if session != our_session {
        return Err("another Windows session".to_string());
    }
    Ok(pid)
}

async fn accept_loop(
    name: String,
    first: NamedPipeServer,
    security: Arc<Security>,
    our_session: u32,
    hub: Arc<Hub>,
    backend: Arc<dyn CenterBackend>,
) {
    let mut next = Some(first);
    let mut failing = false;
    loop {
        let server = match next.take() {
            Some(server) => server,
            None => match create_instance(&name, false, &security) {
                Ok(server) => {
                    failing = false;
                    server
                }
                Err(e) => {
                    // All instances busy (clients connected) is normal; anything else is logged once.
                    if !failing && e.raw_os_error() != Some(231) {
                        dlog!("WARN", "center", "APP-031 center connection: cannot listen again ({})", e);
                    }
                    failing = true;
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    continue;
                }
            },
        };
        if let Err(e) = server.connect().await {
            dlog!("WARN", "center", "APP-031 center connection: accept failed ({})", e);
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        }
        match admit(&server, our_session) {
            Ok(pid) => {
                tokio::spawn(serve_connection(server, pid, hub.clone(), backend.clone()));
            }
            Err(reason) => {
                dlog!("WARN", "center", "APP-031 center connection refused ({})", reason);
            }
        }
    }
}

/// The listening side, bound but not yet accepting.
pub struct Server {
    name: String,
    first: NamedPipeServer,
    security: Arc<Security>,
    session: u32,
}

impl Server {
    /// Create the first instance of `name`. Must run inside the tokio runtime.
    pub fn bind(name: String, sddl: &str, session: u32) -> Result<Self, String> {
        let security = Arc::new(Security::from_sddl(sddl)?);
        let first = create_instance(&name, true, &security).map_err(|e| format!("cannot create the pipe: {e}"))?;
        Ok(Self { name, first, security, session })
    }

    /// Accept clients for as long as the runtime lives.
    pub async fn run(self, hub: Arc<Hub>, backend: Arc<dyn CenterBackend>) {
        accept_loop(self.name, self.first, self.security, self.session, hub, backend).await;
    }
}

fn bind_for_current_user() -> Result<Server, String> {
    let sid = win::current_user_sid()?;
    let session = win::current_session()?;
    Server::bind(pipe_name(session, &sid), &sddl_for(&sid), session)
}

/// Start serving the Center. A failure (e.g. the name is taken) only costs the Center its
/// connection: the island keeps working.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match bind_for_current_user() {
            Ok(server) => {
                let hub = HUB.get_or_init(Hub::new).clone();
                dlog!("INFO", "center", "pipe server listening");
                server.run(hub, Arc::new(AppBackend { app })).await;
            }
            Err(e) => dlog!("WARN", "center", "APP-031 center connection unavailable ({})", e),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    use tokio::net::windows::named_pipe::ClientOptions;

    const SID: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";

    fn block_on<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_multi_thread().enable_all().worker_threads(2).build().unwrap().block_on(future)
    }

    /// Answers like the app would, from memory.
    #[derive(Default)]
    struct Fake {
        notes: Mutex<Vec<notes::Note>>,
        calls: Mutex<Vec<&'static str>>,
    }

    impl CenterBackend for Fake {
        fn run(&self, cmd: Cmd) -> BoxFuture<Result<Value, String>> {
            self.calls.lock().unwrap().push(cmd.name());
            let result = match cmd {
                Cmd::GetSettings => Ok(json!({ "schemaVersion": 1, "islandDisplay": "full" })),
                Cmd::NotesLoad => to_json(&*self.notes.lock().unwrap()),
                Cmd::NotesSave(list) => {
                    let saved = notes::sanitize(list, 1_800_000_000_000);
                    *self.notes.lock().unwrap() = saved.clone();
                    to_json(&saved)
                }
                Cmd::Log { message, .. } => Ok(json!(message.chars().count())),
                Cmd::GetNotificationStatus => Ok(json!("allowed")),
                Cmd::OpenLogDir | Cmd::ShowIsland(_) => Ok(Value::Null),
                Cmd::SearchSubmit { text } => Ok(json!({ "queryId": "q1", "echoChars": text.chars().count() })),
                Cmd::SearchChoose { query_id, remember, .. } => Ok(json!({ "queryId": query_id, "remember": remember })),
                Cmd::SearchExtend { query_id } | Cmd::SearchResults { query_id } => Ok(json!({ "queryId": query_id })),
                Cmd::SearchOpen { .. } => Ok(Value::Null),
                Cmd::SearchHistory => Ok(json!([])),
                _ => Err("APP-001: not in the fake".to_string()),
            };
            Box::pin(async move { result })
        }
    }

    fn session_with(backend: Arc<dyn CenterBackend>) -> (Session, Arc<Hub>, mpsc::Receiver<Arc<str>>) {
        let hub = Hub::new();
        let (tx, rx) = mpsc::channel(QUEUE_LINES);
        let id = hub.register(4242, tx, Arc::new(Notify::new()));
        (Session { hub: hub.clone(), backend, client_id: id, ready: false }, hub, rx)
    }

    async fn ask(session: &mut Session, line: &str) -> Value {
        serde_json::from_str(session.handle_line(line.as_bytes()).await.trim_end()).unwrap()
    }

    // ----- names and security -----

    #[test]
    fn pipe_name_hashes_the_sid_string() {
        let digest = Sha256::digest(SID.as_bytes());
        let expected: String = digest.iter().map(|b| format!("{:02x}", b)).collect::<String>()[..16].to_string();
        assert_eq!(pipe_name(3, SID), format!(r"\\.\pipe\CompanyIsland.Center.3.{expected}"));
        // The value the C# client must compute for the same input (independent SHA-256 run).
        assert_eq!(expected, "a8c06b3027d3fc4a");
    }

    #[test]
    fn pipe_name_differs_per_session_and_user() {
        assert_ne!(pipe_name(1, SID), pipe_name(2, SID));
        assert_ne!(pipe_name(1, SID), pipe_name(1, "S-1-5-21-9-9-9-1001"));
        let name = pipe_name(12, SID);
        let tail = name.strip_prefix(r"\\.\pipe\CompanyIsland.Center.12.").unwrap();
        assert_eq!(tail.len(), 16);
        assert!(tail.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    }

    #[test]
    fn sddl_is_protected_and_grants_only_the_user() {
        assert_eq!(sddl_for(SID), format!("D:P(A;;GA;;;{SID})"));
    }

    #[test]
    fn this_machine_yields_a_sid_a_session_and_a_valid_descriptor() {
        let sid = win::current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
        win::current_session().unwrap();
        win::Security::from_sddl(&sddl_for(&sid)).unwrap();
        assert!(win::Security::from_sddl("not sddl").is_err());
    }

    // ----- parsing -----

    #[test]
    fn requests_need_an_id_and_a_cmd() {
        let ok = parse_request(br#"{"id": 7, "cmd": "getSettings", "ignored": true}"#).unwrap();
        assert_eq!(ok, Request { id: 7, cmd: "getSettings".into(), args: json!({}) });
        let with_args = parse_request(br#"{"id": 1, "cmd": "log", "args": {"level": "info"}}"#).unwrap();
        assert_eq!(with_args.args, json!({"level": "info"}));
        assert!(parse_request(br#"{"id": 9007199254740992, "cmd": "x"}"#).is_ok());
        for bad in [
            &br#"{"cmd": "x"}"#[..],
            br#"{"id": 0, "cmd": "x"}"#,
            br#"{"id": -1, "cmd": "x"}"#,
            br#"{"id": 1.5, "cmd": "x"}"#,
            br#"{"id": "1", "cmd": "x"}"#,
            br#"{"id": 9007199254740993, "cmd": "x"}"#,
            b"not json",
            b"[]",
        ] {
            assert!(matches!(parse_request(bad), Err((None, _))), "{}", String::from_utf8_lossy(bad));
        }
        assert!(matches!(parse_request(br#"{"id": 3}"#), Err((Some(3), _))));
        assert!(matches!(parse_request(br#"{"id": 3, "cmd": 5}"#), Err((Some(3), _))));
        assert!(matches!(parse_request(br#"{"id": 3, "cmd": "x", "args": [1]}"#), Err((Some(3), _))));
    }

    #[test]
    fn commands_are_validated() {
        let none = json!({});
        for (name, expected) in [
            ("getSettings", "getSettings"),
            ("notesLoad", "notesLoad"),
            ("getMonitors", "getMonitors"),
            ("getNotificationStatus", "getNotificationStatus"),
            ("requestNotificationAccess", "requestNotificationAccess"),
            ("openLogDir", "openLogDir"),
            ("showIsland", "showIsland"),
        ] {
            assert_eq!(parse_command(name, &none).unwrap().name(), expected);
        }
        assert_eq!(parse_command("nope", &none).unwrap_err(), "APP-031: unknown command");
        assert_eq!(parse_command("hello", &none).unwrap_err(), "APP-031: unknown command");

        assert!(parse_command("updateSettings", &json!({"patch": {"islandDisplay": "clock"}})).is_ok());
        assert_eq!(parse_command("updateSettings", &none).unwrap_err(), "APP-031: invalid arguments");
        assert!(parse_command("updateSettings", &json!({"patch": 5})).is_err());
        assert!(parse_command("updateSettings", &json!({"patch": {"reminderMinutes": "soon"}})).is_err());

        assert!(parse_command("notesSave", &json!({"notes": [{"id": "a", "text": "x"}]})).is_ok());
        assert!(parse_command("notesSave", &json!({"notes": []})).is_ok());
        assert!(parse_command("notesSave", &none).is_err());
        assert!(parse_command("notesSave", &json!({"notes": {"id": "a"}})).is_err());
        assert!(parse_command("notesSave", &json!({"notes": [{"id": 5}]})).is_err());

        for tab in TABS {
            assert!(parse_command("showIsland", &json!({"tab": tab})).is_ok(), "{tab}");
        }
        assert!(parse_command("showIsland", &json!({"tab": null})).is_ok());
        assert!(parse_command("showIsland", &json!({"tab": "mail"})).is_err());
        assert!(parse_command("showIsland", &json!({"tab": 3})).is_err());
    }

    #[test]
    fn choice_ids_have_the_shapes_the_assistant_mints() {
        // exec.rs: "mb:" + hash16 (16 hex), "cal:" + a calendar id (hash16), and "all".
        for good in ["mb:0123456789abcdef", "cal:fedcba9876543210", "all", "mb:a_b-c"] {
            match parse_command("searchChoose", &json!({"queryId": "q-1", "optionId": good})) {
                Ok(Cmd::SearchChoose { option_id, .. }) => assert_eq!(option_id, good),
                other => panic!("{good}: {other:?}"),
            }
        }
        for bad in ["", "mb:", "cal:", "mb", "x:0123", "mb:a:b", "mb:a/b", "mb:a b", "MB:abc", "all ", "mb:%s", &"mb:".repeat(40)] {
            assert!(parse_command("searchChoose", &json!({"queryId": "q-1", "optionId": bad})).is_err(), "{bad}");
        }
        let long = format!("cal:{}", "a".repeat(65));
        assert!(parse_command("searchChoose", &json!({"queryId": "q-1", "optionId": long})).is_err());
        // The ids used for queries and items keep the plain rule.
        assert!(parse_command("searchOpen", &json!({"queryId": "q1", "itemId": "mb:0123"})).is_err());
        assert!(parse_command("searchExtend", &json!({"queryId": "cal:1"})).is_err());
    }

    #[test]
    fn search_commands_are_validated() {
        match parse_command("searchSubmit", &json!({"text": "מה יש לאיציק ביומן מחר?"})).unwrap() {
            Cmd::SearchSubmit { text } => assert_eq!(text, "מה יש לאיציק ביומן מחר?"),
            other => panic!("{other:?}"),
        }
        assert!(parse_command("searchSubmit", &json!({"text": "ש".repeat(500)})).is_ok());
        for bad in [json!({}), json!({"text": ""}), json!({"text": "  \n "}), json!({"text": 5}), json!({"text": "ש".repeat(501)})] {
            assert_eq!(parse_command("searchSubmit", &bad).unwrap_err(), "APP-031: invalid arguments", "{bad}");
        }

        match parse_command("searchChoose", &json!({"queryId": "q-1", "optionId": "mb:0123456789abcdef", "remember": true})).unwrap() {
            Cmd::SearchChoose { query_id, option_id, remember } => {
                assert_eq!((query_id.as_str(), option_id.as_str(), remember), ("q-1", "mb:0123456789abcdef", true));
            }
            other => panic!("{other:?}"),
        }
        match parse_command("searchChoose", &json!({"queryId": "q1", "optionId": "all"})).unwrap() {
            Cmd::SearchChoose { remember, .. } => assert!(!remember),
            other => panic!("{other:?}"),
        }
        for bad in [
            json!({"queryId": "q1"}),
            json!({"optionId": "all"}),
            json!({"queryId": "q 1", "optionId": "all"}),
            json!({"queryId": "q1", "optionId": "a/b"}),
            json!({"queryId": "q1", "optionId": "all", "remember": "yes"}),
            json!({"queryId": 7, "optionId": "all"}),
        ] {
            assert!(parse_command("searchChoose", &bad).is_err(), "{bad}");
        }

        assert!(parse_command("searchExtend", &json!({"queryId": "q1"})).is_ok());
        assert!(parse_command("searchExtend", &json!({})).is_err());
        assert!(parse_command("searchExtend", &json!({"queryId": "x".repeat(65)})).is_err());
        assert!(parse_command("searchOpen", &json!({"queryId": "q1", "itemId": "i-9"})).is_ok());
        assert!(parse_command("searchOpen", &json!({"queryId": "q1"})).is_err());
        assert!(parse_command("searchOpen", &json!({"queryId": "q1", "itemId": "C:\\x.exe"})).is_err());
        assert!(parse_command("searchResults", &json!({"queryId": "q1"})).is_ok());
        assert!(parse_command("searchResults", &json!({"queryId": ""})).is_err());
        assert_eq!(parse_command("searchHistory", &json!({})).unwrap().name(), "searchHistory");
    }

    #[test]
    fn search_commands_reach_the_backend() {
        block_on(async {
            let fake = Arc::new(Fake::default());
            let (mut session, _hub, _rx) = session_with(fake.clone());
            ask(&mut session, r#"{"id": 1, "cmd": "hello", "args": {"client": "center", "protocol": 1}}"#).await;
            let submit = ask(&mut session, r#"{"id": 2, "cmd": "searchSubmit", "args": {"text": "שלום"}}"#).await;
            assert_eq!(submit["ok"], json!(true));
            assert_eq!(submit["result"]["echoChars"], json!(4));
            let bad = ask(&mut session, r#"{"id": 3, "cmd": "searchSubmit", "args": {"text": ""}}"#).await;
            assert_eq!(bad, json!({"id": 3, "ok": false, "error": "APP-031: invalid arguments"}));
            let open = ask(&mut session, r#"{"id": 4, "cmd": "searchOpen", "args": {"queryId": "q1", "itemId": "i1"}}"#).await;
            assert_eq!(open, json!({"id": 4, "ok": true, "result": null}));
            let history = ask(&mut session, r#"{"id": 5, "cmd": "searchHistory"}"#).await;
            assert_eq!(history["result"], json!([]));
            let calls = fake.calls.lock().unwrap().clone();
            assert_eq!(calls, vec!["searchSubmit", "searchOpen", "searchHistory"]);
        });
    }

    #[test]
    fn log_messages_are_clipped_and_levels_checked() {
        let long = "ש".repeat(1000);
        match parse_command("log", &json!({"level": "warn", "message": long})).unwrap() {
            Cmd::Log { level, message } => {
                assert_eq!(level, LogLevel::Warn);
                assert_eq!(message.chars().count(), MAX_LOG_CHARS);
            }
            other => panic!("{other:?}"),
        }
        assert!(parse_command("log", &json!({"level": "debug", "message": "x"})).is_err());
        assert!(parse_command("log", &json!({"level": "info"})).is_err());
        assert!(parse_command("log", &json!({"message": "x"})).is_err());
        assert_eq!(clip("abc", 3), "abc");
        assert_eq!(clip("abcd", 3), "abc");
    }

    // ----- dispatcher -----

    #[test]
    fn nothing_but_hello_works_before_hello() {
        block_on(async {
            let fake = Arc::new(Fake::default());
            let (mut session, _hub, _rx) = session_with(fake.clone());
            let early = ask(&mut session, r#"{"id": 1, "cmd": "getSettings"}"#).await;
            assert_eq!(early, json!({"id": 1, "ok": false, "error": "APP-031: hello required"}));
            let unknown_early = ask(&mut session, r#"{"id": 2, "cmd": "whatever"}"#).await;
            assert_eq!(unknown_early["error"], "APP-031: hello required");
            assert!(fake.calls.lock().unwrap().is_empty(), "the backend was never reached");

            let bad = ask(&mut session, r#"{"id": 3, "cmd": "hello", "args": {"client": "center", "protocol": 2}}"#).await;
            assert_eq!(bad, json!({"id": 3, "ok": false, "error": "APP-031: unsupported protocol"}));
            let still = ask(&mut session, r#"{"id": 4, "cmd": "getSettings"}"#).await;
            assert_eq!(still["error"], "APP-031: hello required");

            let hello = ask(&mut session, r#"{"id": 5, "cmd": "hello", "args": {"client": "center", "protocol": 1}}"#).await;
            assert_eq!(hello["id"], 5);
            assert_eq!(hello["ok"], true);
            assert_eq!(hello["result"], json!({"protocol": 1, "appVersion": env!("CARGO_PKG_VERSION")}));
            let settings = ask(&mut session, r#"{"id": 6, "cmd": "getSettings"}"#).await;
            assert_eq!(settings["ok"], true);
            assert_eq!(settings["result"]["islandDisplay"], "full");
        });
    }

    #[test]
    fn unknown_commands_and_bad_arguments_get_errors_after_hello() {
        block_on(async {
            let (mut session, _hub, _rx) = session_with(Arc::new(Fake::default()));
            ask(&mut session, r#"{"id": 1, "cmd": "hello", "args": {"client": "center", "protocol": 1}}"#).await;
            let unknown = ask(&mut session, r#"{"id": 2, "cmd": "formatDisk"}"#).await;
            assert_eq!(unknown, json!({"id": 2, "ok": false, "error": "APP-031: unknown command"}));
            let bad = ask(&mut session, r#"{"id": 3, "cmd": "notesSave", "args": {}}"#).await;
            assert_eq!(bad, json!({"id": 3, "ok": false, "error": "APP-031: invalid arguments"}));
            let garbage = ask(&mut session, "this is not json").await;
            assert_eq!(garbage["id"], Value::Null);
            assert_eq!(garbage["ok"], false);
            // The connection still works afterwards.
            assert_eq!(ask(&mut session, r#"{"id": 4, "cmd": "getSettings"}"#).await["ok"], true);
        });
    }

    #[test]
    fn hello_marks_center_clients_only_when_they_say_so() {
        block_on(async {
            let (mut session, hub, _rx) = session_with(Arc::new(Fake::default()));
            assert_eq!(hub.send_navigate("tour", |_| {}), None, "not ready yet");
            ask(&mut session, r#"{"id": 1, "cmd": "hello", "args": {"client": "probe", "protocol": 1}}"#).await;
            assert_eq!(hub.send_navigate("tour", |_| {}), None, "a probe is not a Center");
            ask(&mut session, r#"{"id": 2, "cmd": "hello", "args": {"client": "center", "protocol": 1}}"#).await;
            assert_eq!(hub.send_navigate("tour", |_| {}), Some(4242));
        });
    }

    // ----- hub -----

    fn client(hub: &Hub, pid: u32, ready: bool, center: bool, capacity: usize) -> (u64, mpsc::Receiver<Arc<str>>, Arc<Notify>) {
        let (tx, rx) = mpsc::channel(capacity);
        let kill = Arc::new(Notify::new());
        let id = hub.register(pid, tx, kill.clone());
        if ready {
            hub.mark_hello(id, center);
        }
        (id, rx, kill)
    }

    #[test]
    fn broadcast_reaches_ready_clients_as_event_lines() {
        let hub = Hub::new();
        let (_, mut ready, _) = client(&hub, 1, true, false, 8);
        let (_, mut waiting, _) = client(&hub, 2, false, false, 8);
        hub.broadcast("settings-changed", &json!({"islandDisplay": "clock"}));
        let line = ready.try_recv().unwrap();
        assert!(line.ends_with('\n'));
        let event: Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(event, json!({"event": "settings-changed", "payload": {"islandDisplay": "clock"}}));
        assert!(waiting.try_recv().is_err(), "no events before hello");
    }

    #[test]
    fn navigate_goes_to_the_newest_center_only_and_grants_first() {
        let hub = Hub::new();
        let (_, mut older, _) = client(&hub, 10, true, true, 8);
        let (_, mut newer, _) = client(&hub, 11, true, true, 8);
        let (_, mut other, _) = client(&hub, 12, true, false, 8);
        let granted = Mutex::new(Vec::new());
        let pid = hub.send_navigate("note:abc", |pid| {
            // the event is not queued yet when the right is granted
            assert!(newer.try_recv().is_err());
            granted.lock().unwrap().push(pid);
        });
        assert_eq!(pid, Some(11));
        assert_eq!(*granted.lock().unwrap(), vec![11]);
        let event: Value = serde_json::from_str(newer.try_recv().unwrap().trim_end()).unwrap();
        assert_eq!(event, json!({"event": "navigate", "payload": {"page": "note:abc"}}));
        assert!(older.try_recv().is_err() && other.try_recv().is_err());
        assert_eq!(hub.send_navigate("x", |_| {}), Some(11));
        assert_eq!(Hub::new().send_navigate("tour", |_| panic!("no client, no grant")), None);
    }

    #[test]
    fn a_full_queue_drops_that_client_only() {
        block_on(async {
            let hub = Hub::new();
            let (slow_id, _slow_rx, slow_kill) = client(&hub, 1, true, false, 1);
            let (_, mut fast_rx, _) = client(&hub, 2, true, false, 8);
            hub.broadcast("notes-changed", &json!([]));
            assert_eq!(hub.client_count(), 2);
            hub.broadcast("notes-changed", &json!([]));
            assert_eq!(hub.client_count(), 1, "the slow client was dropped");
            assert!(hub.lock().iter().all(|c| c.id != slow_id));
            // Its connection is told to close (the permit is stored even before anyone waits).
            tokio::time::timeout(Duration::from_secs(2), slow_kill.notified()).await.unwrap();
            assert!(fast_rx.try_recv().is_ok() && fast_rx.try_recv().is_ok());
        });
    }

    #[test]
    fn a_navigate_that_cannot_be_queued_falls_back_to_the_next_center() {
        let hub = Hub::new();
        let (_, mut older, _) = client(&hub, 10, true, true, 8);
        let (_, _full, _) = client(&hub, 11, true, true, 1);
        hub.broadcast("notes-changed", &json!([])); // fills the newer client's queue
        assert_eq!(hub.send_navigate("welcome", |_| {}), Some(10));
        assert!(older.try_recv().is_ok());
    }

    // ----- framing -----

    #[test]
    fn lines_over_the_limit_are_refused() {
        block_on(async {
            let mut buf = Vec::new();
            let mut ok = BufReader::new(&b"abc\r\ndef\nlast"[..]);
            assert!(matches!(read_line_limited(&mut ok, &mut buf, 5).await.unwrap(), Line::Complete));
            assert_eq!(buf, b"abc\r");
            buf.clear();
            assert!(matches!(read_line_limited(&mut ok, &mut buf, 5).await.unwrap(), Line::Complete));
            assert_eq!(buf, b"def");
            buf.clear();
            assert!(matches!(read_line_limited(&mut ok, &mut buf, 5).await.unwrap(), Line::Eof), "no newline, no line");

            for (data, max, long) in [(&b"123456\n"[..], 5, true), (b"12345\n", 5, false), (b"1234567890", 5, true), (b"12345", 5, false)] {
                let mut buf = Vec::new();
                let mut reader = BufReader::with_capacity(4, data);
                let got = read_line_limited(&mut reader, &mut buf, max).await.unwrap();
                assert_eq!(matches!(got, Line::TooLong), long, "{}", String::from_utf8_lossy(data));
            }
        });
    }

    // ----- a real named pipe -----

    fn test_pipe_name(tag: &str) -> String {
        static N: AtomicU64 = AtomicU64::new(0);
        format!(r"\\.\pipe\CompanyIsland.Center.test.{}.{}.{}", std::process::id(), tag, N.fetch_add(1, Ordering::Relaxed))
    }

    struct Wire {
        reader: BufReader<tokio::io::ReadHalf<tokio::net::windows::named_pipe::NamedPipeClient>>,
        writer: tokio::io::WriteHalf<tokio::net::windows::named_pipe::NamedPipeClient>,
    }

    impl Wire {
        fn connect(name: &str) -> Wire {
            let client = ClientOptions::new().open(name).expect("the pipe is open to the current user");
            let (reader, writer) = tokio::io::split(client);
            Wire { reader: BufReader::new(reader), writer }
        }

        async fn send(&mut self, line: &str) {
            self.writer.write_all(format!("{line}\n").as_bytes()).await.unwrap();
        }

        async fn next(&mut self) -> Value {
            let mut text = String::new();
            let read = tokio::time::timeout(Duration::from_secs(10), self.reader.read_line(&mut text)).await;
            assert!(read.expect("a line arrives in time").unwrap() > 0, "the server closed the connection");
            serde_json::from_str(text.trim_end()).unwrap()
        }
    }

    /// Searches wait on `gate` (one permit lets one finish); everything else answers at once.
    /// `running` counts searches that have started and not yet finished or been dropped.
    struct Gated {
        gate: Arc<tokio::sync::Semaphore>,
        running: Arc<std::sync::atomic::AtomicUsize>,
    }

    struct Running(Arc<std::sync::atomic::AtomicUsize>);

    impl Drop for Running {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl CenterBackend for Gated {
        fn run(&self, cmd: Cmd) -> BoxFuture<Result<Value, String>> {
            if !cmd.is_slow() {
                return Box::pin(async { Ok(json!("fast")) });
            }
            let (gate, running) = (self.gate.clone(), self.running.clone());
            Box::pin(async move {
                running.fetch_add(1, Ordering::SeqCst);
                let _running = Running(running);
                gate.acquire().await.unwrap().forget();
                Ok(json!("slow"))
            })
        }
    }

    fn gated() -> (Arc<Gated>, Arc<tokio::sync::Semaphore>, Arc<std::sync::atomic::AtomicUsize>) {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let running = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        (Arc::new(Gated { gate: gate.clone(), running: running.clone() }), gate, running)
    }

    async fn hello(wire: &mut Wire) {
        wire.send(r#"{"id": 1, "cmd": "hello", "args": {"client": "center", "protocol": 1}}"#).await;
        assert_eq!(wire.next().await["ok"], true);
    }

    #[test]
    fn real_pipe_a_slow_search_does_not_hold_up_other_requests() {
        block_on(async {
            let name = test_pipe_name("slow");
            let (backend, gate, _running) = gated();
            let _hub = start_server(&name, backend);
            let mut wire = Wire::connect(&name);
            hello(&mut wire).await;

            wire.send(r#"{"id": 2, "cmd": "searchSubmit", "args": {"text": "mail from dana"}}"#).await;
            wire.send(r#"{"id": 3, "cmd": "getSettings"}"#).await;
            wire.send(r#"{"id": 4, "cmd": "searchHistory"}"#).await;
            let (a, b) = (wire.next().await, wire.next().await);
            assert_eq!((a["id"].clone(), a["result"].clone()), (json!(3), json!("fast")), "settings is answered while the search runs");
            assert_eq!((b["id"].clone(), b["result"].clone()), (json!(4), json!("fast")), "fast replies keep their order");

            gate.add_permits(1);
            let done = wire.next().await;
            assert_eq!((done["id"].clone(), done["result"].clone()), (json!(2), json!("slow")));
        });
    }

    #[test]
    fn real_pipe_caps_the_searches_in_flight_per_connection() {
        block_on(async {
            let name = test_pipe_name("cap");
            let (backend, gate, _running) = gated();
            let _hub = start_server(&name, backend);
            let mut wire = Wire::connect(&name);
            hello(&mut wire).await;

            for id in 2..=(1 + MAX_SLOW_IN_FLIGHT + 1) {
                wire.send(&format!(r#"{{"id": {id}, "cmd": "searchExtend", "args": {{"queryId": "q1"}}}}"#)).await;
            }
            let refused = wire.next().await;
            assert_eq!(refused["id"], 1 + MAX_SLOW_IN_FLIGHT as u64 + 1);
            assert_eq!(refused["error"], "APP-031: too many searches at once");

            gate.add_permits(MAX_SLOW_IN_FLIGHT);
            let mut ids = vec![wire.next().await["id"].as_u64().unwrap(), wire.next().await["id"].as_u64().unwrap()];
            ids.sort();
            assert_eq!(ids, [2, 3]);

            // Room again once they finished.
            wire.send(r#"{"id": 9, "cmd": "searchExtend", "args": {"queryId": "q1"}}"#).await;
            gate.add_permits(1);
            assert_eq!(wire.next().await["id"], 9);
        });
    }

    #[test]
    fn real_pipe_aborts_running_searches_when_the_client_leaves() {
        block_on(async {
            let name = test_pipe_name("abort");
            let (backend, _gate, running) = gated();
            let hub = start_server(&name, backend);
            let mut wire = Wire::connect(&name);
            hello(&mut wire).await;
            wire.send(r#"{"id": 2, "cmd": "searchSubmit", "args": {"text": "x"}}"#).await;
            let mut waited = 0;
            while running.load(Ordering::SeqCst) == 0 && waited < 100 {
                tokio::time::sleep(Duration::from_millis(20)).await;
                waited += 1;
            }
            assert_eq!(running.load(Ordering::SeqCst), 1, "the search started");

            drop(wire);
            let mut waited = 0;
            while (running.load(Ordering::SeqCst) > 0 || hub.client_count() > 0) && waited < 150 {
                tokio::time::sleep(Duration::from_millis(20)).await;
                waited += 1;
            }
            assert_eq!(running.load(Ordering::SeqCst), 0, "the search future was dropped with the connection");
        });
    }

    fn start_server(name: &str, backend: Arc<dyn CenterBackend>) -> Arc<Hub> {
        let sid = win::current_user_sid().unwrap();
        let session = win::current_session().unwrap();
        let server = Server::bind(name.to_string(), &sddl_for(&sid), session).unwrap();
        let hub = Hub::new();
        tokio::spawn(server.run(hub.clone(), backend));
        hub
    }

    #[test]
    fn real_pipe_hello_settings_notes_and_events() {
        block_on(async {
            let name = test_pipe_name("flow");
            let hub = start_server(&name, Arc::new(Fake::default()));
            let mut wire = Wire::connect(&name);

            wire.send(r#"{"id": 1, "cmd": "getSettings"}"#).await;
            assert_eq!(wire.next().await["error"], "APP-031: hello required");

            wire.send(r#"{"id": 2, "cmd": "hello", "args": {"client": "center", "protocol": 1}}"#).await;
            let hello = wire.next().await;
            assert_eq!((hello["id"].clone(), hello["ok"].clone()), (json!(2), json!(true)));
            assert_eq!(hello["result"]["protocol"], 1);

            wire.send(r#"{"id": 3, "cmd": "getSettings"}"#).await;
            let settings = wire.next().await;
            assert_eq!(settings["id"], 3);
            assert_eq!(settings["result"]["islandDisplay"], "full");

            wire.send(
                r#"{"id": 4, "cmd": "notesSave", "args": {"notes": [
                    {"id": "old", "text": "first", "createdAt": 5, "updatedAt": 10, "pinned": false},
                    {"id": "pin", "text": "second", "createdAt": 5, "updatedAt": 5, "pinned": true},
                    {"id": "bad id", "text": "dropped", "createdAt": 5, "updatedAt": 5, "pinned": false}]}}"#
                    .replace('\n', " ")
                    .as_str(),
            )
            .await;
            let saved = wire.next().await;
            let ids: Vec<&str> = saved["result"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap()).collect();
            assert_eq!(ids, ["pin", "old"], "sanitized, pinned first");

            wire.send(r#"{"id": 5, "cmd": "notesLoad"}"#).await;
            let loaded = wire.next().await;
            assert_eq!(loaded["result"], saved["result"]);

            // An event pushed from "another thread" reaches the client between answers.
            let pusher = hub.clone();
            std::thread::spawn(move || pusher.broadcast("notes-changed", &json!([{"id": "x"}]))).join().unwrap();
            let event = wire.next().await;
            assert_eq!(event, json!({"event": "notes-changed", "payload": [{"id": "x"}]}));

            wire.send(r#"{"id": 6, "cmd": "nope"}"#).await;
            assert_eq!(wire.next().await["error"], "APP-031: unknown command");

            // The Center registered with this process's pid and can be navigated.
            assert_eq!(hub.send_navigate("settings", |_| {}), Some(std::process::id()));
            let nav = wire.next().await;
            assert_eq!(nav, json!({"event": "navigate", "payload": {"page": "settings"}}));
        });
    }

    #[test]
    fn real_pipe_closes_on_an_oversized_line() {
        block_on(async {
            let name = test_pipe_name("big");
            let hub = start_server(&name, Arc::new(Fake::default()));
            let mut wire = Wire::connect(&name);
            wire.send(r#"{"id": 1, "cmd": "hello", "args": {"client": "center", "protocol": 1}}"#).await;
            assert_eq!(wire.next().await["ok"], true);

            // 16 MiB + 1 bytes without a newline.
            let chunk = vec![b'x'; 1024 * 1024];
            let mut written = 0usize;
            while written <= MAX_LINE_BYTES {
                if wire.writer.write_all(&chunk).await.is_err() {
                    break; // the server already hung up
                }
                written += chunk.len();
            }
            let mut rest = Vec::new();
            let closed = tokio::time::timeout(Duration::from_secs(10), wire.reader.read_to_end(&mut rest)).await;
            // A reset instead of a clean end of stream is as good as a close.
            let _ = closed.expect("the server closes the connection");
            assert!(rest.is_empty(), "no answer for an oversized line");
            let mut waited = 0;
            while hub.client_count() > 0 && waited < 100 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                waited += 1;
            }
            assert_eq!(hub.client_count(), 0);
        });
    }

    #[test]
    fn a_second_server_on_the_same_name_is_refused() {
        block_on(async {
            let name = test_pipe_name("dup");
            let _hub = start_server(&name, Arc::new(Fake::default()));
            let sid = win::current_user_sid().unwrap();
            let again = Server::bind(name, &sddl_for(&sid), win::current_session().unwrap());
            assert!(again.is_err(), "first_pipe_instance protects against a squatted name");
        });
    }
}
