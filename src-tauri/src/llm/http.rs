//! One HTTPS/HTTP POST through WinHTTP: the system's proxy settings (automatic proxy, as Windows has it),
//! the Windows certificate store (so an organisation's own CA is trusted like in Edge), and optionally the
//! user's Windows sign-in (Negotiate/NTLM) for a server or proxy that asks for it. No new crate, nothing
//! that needs the internet: it works the same on a closed network.

use super::protocol::Url;
use std::time::Duration;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

/// A response larger than this is not an answer we can show.
const MAX_BODY: usize = 8 * 1024 * 1024;

pub struct Request<'a> {
    pub url: &'a Url,
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
    pub timeout: Duration,
    pub allow_invalid_certificates: bool,
    /// Answer a server's Negotiate/NTLM challenge with the signed-in user. A proxy's challenge (407) is
    /// always answered that way, as every Windows program does.
    pub windows_auth: bool,
}

pub struct Response {
    pub status: u32,
    pub body: Vec<u8>,
}

struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn open(handle: *mut core::ffi::c_void, what: &str) -> Result<Handle, String> {
    if handle.is_null() {
        Err(net_error(windows::core::Error::from_win32(), what))
    } else {
        Ok(Handle(handle))
    }
}

/// The `AI-00x` error for a failed WinHTTP call.
fn net_error(e: windows::core::Error, what: &str) -> String {
    let code = (e.code().0 as u32) & 0xFFFF;
    match code {
        ERROR_WINHTTP_TIMEOUT => "AI-003: the AI server did not answer in time".to_string(),
        ERROR_WINHTTP_NAME_NOT_RESOLVED => "AI-002: the server name was not found (check the endpoint, DNS or proxy)".to_string(),
        ERROR_WINHTTP_CANNOT_CONNECT => "AI-002: cannot connect to the AI server (check the address, the port and the firewall)".to_string(),
        ERROR_WINHTTP_SECURE_FAILURE | ERROR_WINHTTP_SECURE_FAILURE_PROXY => {
            "AI-008: the server's certificate is not trusted by this PC (install the organisation's CA, or set allowInvalidCertificates)".to_string()
        }
        // 12030 connection aborted, 12031 reset, 12152 invalid server response
        12030 | 12031 | 12152 => format!("AI-002: the connection to the AI server broke ({what}, {code})"),
        _ => format!("AI-002: {what} failed (WinHTTP {code})"),
    }
}

pub fn post(req: &Request) -> Result<Response, String> {
    unsafe { post_inner(req) }
}

unsafe fn post_inner(req: &Request) -> Result<Response, String> {
    let agent = HSTRING::from(format!("Yuval/{}", env!("CARGO_PKG_VERSION")));
    // automatic proxy (Windows 8.1+: the user's proxy settings, WPAD and PAC), else the WinHTTP default
    let mut session = WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0);
    if session.is_null() {
        session = WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, PCWSTR::null(), PCWSTR::null(), 0);
    }
    let session = open(session, "opening WinHTTP")?;
    let ms = req.timeout.as_millis().min(i32::MAX as u128) as i32;
    let connect_ms = ms.min(15_000);
    let _ = WinHttpSetTimeouts(session.0, connect_ms, connect_ms, ms, ms);
    // TLS 1.2 and 1.3 on Windows 10 too (older builds default to fewer protocols)
    let protocols: u32 = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3;
    if WinHttpSetOption(Some(session.0), WINHTTP_OPTION_SECURE_PROTOCOLS, Some(&protocols.to_ne_bytes())).is_err() {
        let tls12: u32 = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2;
        let _ = WinHttpSetOption(Some(session.0), WINHTTP_OPTION_SECURE_PROTOCOLS, Some(&tls12.to_ne_bytes()));
    }

    let host = HSTRING::from(req.url.host.trim_start_matches('[').trim_end_matches(']'));
    let connect = open(WinHttpConnect(session.0, &host, req.url.port, 0), "connecting")?;
    let flags = if req.url.secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
    let path = HSTRING::from(req.url.path.as_str());
    let request = open(WinHttpOpenRequest(connect.0, windows::core::w!("POST"), &path, PCWSTR::null(), PCWSTR::null(), std::ptr::null(), flags), "opening the request")?;

    if req.allow_invalid_certificates && req.url.secure {
        let ignore: u32 = SECURITY_FLAG_IGNORE_UNKNOWN_CA | SECURITY_FLAG_IGNORE_CERT_DATE_INVALID | SECURITY_FLAG_IGNORE_CERT_CN_INVALID | SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE;
        let _ = WinHttpSetOption(Some(request.0), WINHTTP_OPTION_SECURITY_FLAGS, Some(&ignore.to_ne_bytes()));
    }
    if req.windows_auth {
        let low: u32 = WINHTTP_AUTOLOGON_SECURITY_LEVEL_LOW;
        let _ = WinHttpSetOption(Some(request.0), WINHTTP_OPTION_AUTOLOGON_POLICY, Some(&low.to_ne_bytes()));
    }
    let mut header_text = String::new();
    for (name, value) in req.headers {
        header_text.push_str(name);
        header_text.push_str(": ");
        header_text.push_str(value);
        header_text.push_str("\r\n");
    }
    let header_wide: Vec<u16> = header_text.encode_utf16().collect();
    if !header_wide.is_empty() {
        WinHttpAddRequestHeaders(request.0, &header_wide, WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE).map_err(|e| net_error(e, "setting the headers"))?;
    }

    // a challenge (401 server / 407 proxy) is answered once per target with the signed-in user
    let mut answered_server = false;
    let mut answered_proxy = false;
    for _ in 0..4 {
        let len = req.body.len() as u32;
        WinHttpSendRequest(request.0, None, Some(req.body.as_ptr() as *const core::ffi::c_void), len, len, 0).map_err(|e| net_error(e, "sending"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| net_error(e, "receiving"))?;
        let status = status_code(request.0)?;
        let challenge = match status {
            401 if req.windows_auth && !answered_server => Some(WINHTTP_AUTH_TARGET_SERVER),
            407 if !answered_proxy => Some(WINHTTP_AUTH_TARGET_PROXY),
            _ => None,
        };
        if let Some(target) = challenge {
            if let Some(scheme) = windows_scheme(request.0) {
                drain(request.0);
                if WinHttpSetCredentials(request.0, target, scheme, PCWSTR::null(), PCWSTR::null(), std::ptr::null_mut()).is_ok() {
                    if target == WINHTTP_AUTH_TARGET_SERVER {
                        answered_server = true;
                    } else {
                        answered_proxy = true;
                    }
                    continue;
                }
            }
        }
        let body = read_body(request.0)?;
        return Ok(Response { status, body });
    }
    Err("AI-004: the server kept asking to sign in".into())
}

unsafe fn status_code(request: *mut core::ffi::c_void) -> Result<u32, String> {
    let mut status: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    WinHttpQueryHeaders(
        request,
        WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
        PCWSTR::null(),
        Some(&mut status as *mut u32 as *mut core::ffi::c_void),
        &mut size,
        std::ptr::null_mut(),
    )
    .map_err(|e| net_error(e, "reading the status"))?;
    Ok(status)
}

/// Negotiate (Kerberos, then NTLM) or NTLM, when the challenge offers one; never Basic (it would need a password).
unsafe fn windows_scheme(request: *mut core::ffi::c_void) -> Option<u32> {
    let (mut supported, mut first, mut target) = (0u32, 0u32, 0u32);
    WinHttpQueryAuthSchemes(request, &mut supported, &mut first, &mut target).ok()?;
    [WINHTTP_AUTH_SCHEME_NEGOTIATE.0, WINHTTP_AUTH_SCHEME_NTLM.0].into_iter().find(|s| supported & s != 0)
}

/// Read and drop the body of a challenge response so the connection can be reused for the retry.
unsafe fn drain(request: *mut core::ffi::c_void) {
    let _ = read_body(request);
}

unsafe fn read_body(request: *mut core::ffi::c_void) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    loop {
        let mut available: u32 = 0;
        WinHttpQueryDataAvailable(request, &mut available).map_err(|e| net_error(e, "reading the answer"))?;
        if available == 0 {
            break;
        }
        let start = body.len();
        body.resize(start + available as usize, 0);
        let mut read: u32 = 0;
        WinHttpReadData(request, body[start..].as_mut_ptr() as *mut core::ffi::c_void, available, &mut read).map_err(|e| net_error(e, "reading the answer"))?;
        body.truncate(start + read as usize);
        if read == 0 {
            break;
        }
        if body.len() > MAX_BODY {
            return Err("AI-006: the server's answer is too large".into());
        }
    }
    Ok(body)
}
