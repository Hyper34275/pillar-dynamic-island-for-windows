//! Late-bound COM automation (IDispatch) over the `windows` crate.
//!
//! Everything here is generic: no Outlook knowledge. Callers own a [`ComApartment`]
//! for the thread, create [`Dispatch`] wrappers around automation objects and read
//! results through the `variant_*` helpers. Error descriptions from EXCEPINFO are
//! never kept (they can contain item content); only HRESULT, scode and the
//! description length are retained.

use chrono::{DateTime, Duration, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use windows::core::{implement, Interface, BSTR, GUID, PCWSTR, VARIANT};
use windows::Win32::Media::Audio::{IMessageFilter, IMessageFilter_Impl};
use windows::Win32::Media::HTASK;
use windows::Win32::System::Com::{
    CLSIDFromProgID, CoInitializeEx, CoUninitialize, IDispatch, COINIT_APARTMENTTHREADED,
    DISPATCH_FLAGS, DISPATCH_METHOD, DISPATCH_PROPERTYGET, DISPATCH_PROPERTYPUT, DISPPARAMS, EXCEPINFO, INTERFACEINFO,
};
use windows::Win32::System::Ole::GetActiveObject;
use windows::Win32::System::Variant::{VT_BOOL, VT_BSTR, VT_DATE, VT_DISPATCH, VT_I2, VT_I4, VT_R8, VT_UNKNOWN};

const LOCALE_USER_DEFAULT: u32 = 0x0400;
const DISPID_PROPERTYPUT: i32 = -3;

pub const DISP_E_EXCEPTION: i32 = 0x8002_0009_u32 as i32;
pub const E_ACCESSDENIED: i32 = 0x8007_0005_u32 as i32;
/// Moniker/ROT lookup failed: Outlook is not (yet) registered as the active object.
pub const MK_E_UNAVAILABLE: i32 = 0x8004_01E3_u32 as i32;
const RPC_E_CALL_REJECTED: i32 = 0x8001_0001_u32 as i32;
const RPC_E_SERVERCALL_REJECTED: i32 = 0x8001_0109_u32 as i32;
const RPC_E_SERVERCALL_RETRYLATER: i32 = 0x8001_010A_u32 as i32;

const DISCONNECTED: [u32; 11] = [
    0x8001_0108, // RPC_E_DISCONNECTED
    0x8007_06BA, // RPC_S_SERVER_UNAVAILABLE
    0x8007_06BE, // RPC_S_CALL_FAILED
    0x8007_06BF, // RPC_S_CALL_FAILED_DNE
    0x8004_01FD, // CO_E_OBJNOTCONNECTED
    0x8004_01FF, // CO_E_RELEASED
    0x8001_0105, // RPC_E_SERVERFAULT
    0x8001_0007, // RPC_E_SERVER_DIED
    0x8001_0012, // RPC_E_SERVER_DIED_DNE
    0x8001_0006, // RPC_E_CONNECTION_TERMINATED
    0x8001_0114, // RPC_E_INVALID_OBJECT
];

/// The server went away or the proxy is dead: drop every cached object and reattach.
pub fn is_disconnected(hr: i32) -> bool {
    DISCONNECTED.contains(&(hr as u32))
}

/// The server rejected the call because it is busy (modal dialog, startup, ...).
pub fn is_busy(hr: i32) -> bool {
    matches!(hr, RPC_E_CALL_REJECTED | RPC_E_SERVERCALL_REJECTED | RPC_E_SERVERCALL_RETRYLATER)
}

/// E_ACCESSDENIED or a VBA-style 0x800A.... runtime error: how the Outlook object
/// model guard and policy blocks surface through automation.
pub fn is_blocked(hr: i32) -> bool {
    hr == E_ACCESSDENIED || (hr as u32) >> 16 == 0x800A
}

#[derive(Debug, Clone)]
pub struct ComError {
    pub op: &'static str,
    pub hr: i32,
    /// From EXCEPINFO when `hr` is DISP_E_EXCEPTION.
    pub scode: Option<i32>,
    pub description_len: usize,
}

impl ComError {
    pub fn new(op: &'static str, hr: i32) -> Self {
        ComError { op, hr, scode: None, description_len: 0 }
    }

    fn from_windows(op: &'static str, e: &windows::core::Error) -> Self {
        Self::new(op, e.code().0)
    }

    /// The scode inside an automation exception when there is one, else the HRESULT.
    pub fn effective(&self) -> i32 {
        self.scode.filter(|s| *s != 0).unwrap_or(self.hr)
    }

    pub fn is_disconnected(&self) -> bool {
        is_disconnected(self.hr) || is_disconnected(self.effective())
    }

    pub fn is_busy(&self) -> bool {
        is_busy(self.hr) || is_busy(self.effective())
    }

    pub fn is_blocked(&self) -> bool {
        is_blocked(self.hr) || is_blocked(self.effective())
    }
}

impl fmt::Display for ComError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} failed hr=0x{:08X}", self.op, self.hr as u32)?;
        if let Some(s) = self.scode {
            write!(f, " scode=0x{:08X} desc_len={}", s as u32, self.description_len)?;
        }
        Ok(())
    }
}

impl std::error::Error for ComError {}

pub type ComResult<T> = Result<T, ComError>;

// =============================================================================
// Apartment + message filter
// =============================================================================

/// Single-threaded apartment for the current thread. Not `Send`: it must be dropped
/// on the thread that created it, after every COM object created under it.
pub struct ComApartment {
    _not_send: PhantomData<*const ()>,
}

impl ComApartment {
    pub fn init_sta() -> ComResult<Self> {
        // S_OK and S_FALSE (already initialised as STA) both need a matching CoUninitialize;
        // RPC_E_CHANGED_MODE means this thread is an MTA and must not be uninitialised by us.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr.is_err() {
            return Err(ComError::new("CoInitializeEx", hr.0));
        }
        Ok(ComApartment { _not_send: PhantomData })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

/// How long a rejected call keeps being retried before COM reports the rejection.
const RETRY_CAP_MS: u32 = 3_000;
const RETRY_STEP_MS: u32 = 250;

/// IMessageFilter::RetryRejectedCall result: the delay in ms before the next retry,
/// or `u32::MAX` (-1) to cancel and surface RPC_E_CALL_REJECTED.
pub fn retry_delay(tick_ms: u32, reject_type: u32) -> u32 {
    // SERVERCALL_REJECTED = 1, SERVERCALL_RETRYLATER = 2.
    if matches!(reject_type, 1 | 2) && tick_ms < RETRY_CAP_MS {
        RETRY_STEP_MS
    } else {
        u32::MAX
    }
}

#[implement(IMessageFilter)]
struct MessageFilter;

impl IMessageFilter_Impl for MessageFilter_Impl {
    fn HandleInComingCall(&self, _calltype: u32, _caller: HTASK, _tick: u32, _info: *const INTERFACEINFO) -> u32 {
        0 // SERVERCALL_ISHANDLED
    }

    fn RetryRejectedCall(&self, _callee: HTASK, tick: u32, reject_type: u32) -> u32 {
        retry_delay(tick, reject_type)
    }

    fn MessagePending(&self, _callee: HTASK, _tick: u32, _pending_type: u32) -> u32 {
        2 // PENDINGMSG_WAITDEFPROCESS
    }
}

/// Registers a retry-on-busy message filter for the current STA thread and removes it
/// on drop. Registration is per apartment, so this must live on the worker thread.
pub struct MessageFilterGuard {
    previous: Option<IMessageFilter>,
    _not_send: PhantomData<*const ()>,
}

impl MessageFilterGuard {
    pub fn register(_apartment: &ComApartment) -> ComResult<Self> {
        use windows::Win32::Media::Audio::CoRegisterMessageFilter;
        let filter: IMessageFilter = MessageFilter.into();
        let mut previous = None;
        unsafe { CoRegisterMessageFilter(&filter, Some(&mut previous)) }
            .map_err(|e| ComError::from_windows("CoRegisterMessageFilter", &e))?;
        Ok(MessageFilterGuard { previous, _not_send: PhantomData })
    }
}

impl Drop for MessageFilterGuard {
    fn drop(&mut self) {
        use windows::Win32::Media::Audio::CoRegisterMessageFilter;
        unsafe {
            let _ = CoRegisterMessageFilter(self.previous.as_ref(), None);
        }
    }
}

// =============================================================================
// IDispatch wrapper
// =============================================================================

/// An automation object with a per-object DISPID cache. Dropping releases the object.
pub struct Dispatch {
    ptr: IDispatch,
    ids: HashMap<&'static str, i32>,
}

impl Dispatch {
    pub fn new(ptr: IDispatch) -> Self {
        Dispatch { ptr, ids: HashMap::new() }
    }

    /// Attach to an object that is already registered in the running object table.
    /// Never creates or launches anything.
    pub fn get_active(prog_id: &str) -> ComResult<Self> {
        let wide: Vec<u16> = prog_id.encode_utf16().chain(std::iter::once(0)).collect();
        let clsid = unsafe { CLSIDFromProgID(PCWSTR(wide.as_ptr())) }
            .map_err(|e| ComError::from_windows("CLSIDFromProgID", &e))?;
        let mut unknown = None;
        unsafe { GetActiveObject(&clsid, None, &mut unknown) }.map_err(|e| ComError::from_windows("GetActiveObject", &e))?;
        let unknown = unknown.ok_or_else(|| ComError::new("GetActiveObject", MK_E_UNAVAILABLE))?;
        let ptr: IDispatch = unknown.cast().map_err(|e| ComError::from_windows("QueryInterface(IDispatch)", &e))?;
        Ok(Dispatch::new(ptr))
    }

    fn dispid(&mut self, name: &'static str) -> ComResult<i32> {
        if let Some(id) = self.ids.get(name) {
            return Ok(*id);
        }
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let names = [PCWSTR(wide.as_ptr())];
        let mut id = 0i32;
        unsafe { self.ptr.GetIDsOfNames(&GUID::zeroed(), names.as_ptr(), 1, LOCALE_USER_DEFAULT, &mut id) }
            .map_err(|e| ComError::from_windows("GetIDsOfNames", &e))?;
        self.ids.insert(name, id);
        Ok(id)
    }

    fn invoke(&mut self, name: &'static str, flags: DISPATCH_FLAGS, args: Vec<VARIANT>) -> ComResult<VARIANT> {
        let id = self.dispid(name)?;
        // IDispatch takes arguments in reverse order.
        let mut rgvarg: Vec<VARIANT> = args.into_iter().rev().collect();
        let mut put_id = DISPID_PROPERTYPUT;
        let is_put = flags == DISPATCH_PROPERTYPUT;
        let params = DISPPARAMS {
            rgvarg: if rgvarg.is_empty() { std::ptr::null_mut() } else { rgvarg.as_mut_ptr() },
            rgdispidNamedArgs: if is_put { &mut put_id } else { std::ptr::null_mut() },
            cArgs: rgvarg.len() as u32,
            cNamedArgs: u32::from(is_put),
        };
        let mut result = VARIANT::new();
        let mut excep: EXCEPINFO = unsafe { std::mem::zeroed() };
        let mut arg_err = 0u32;
        let outcome = unsafe {
            self.ptr.Invoke(id, &GUID::zeroed(), LOCALE_USER_DEFAULT, flags, &params, Some(&mut result), Some(&mut excep), Some(&mut arg_err))
        };
        let description_len = excep.bstrDescription.len();
        let scode = excep.scode;
        unsafe {
            ManuallyDrop::drop(&mut excep.bstrSource);
            ManuallyDrop::drop(&mut excep.bstrDescription);
            ManuallyDrop::drop(&mut excep.bstrHelpFile);
        }
        match outcome {
            Ok(()) => Ok(result),
            Err(e) => {
                let mut err = ComError::from_windows("Invoke", &e);
                if err.hr == DISP_E_EXCEPTION {
                    err.scode = Some(scode);
                    err.description_len = description_len;
                }
                Err(err)
            }
        }
    }

    pub fn get(&mut self, name: &'static str) -> ComResult<VARIANT> {
        self.invoke(name, DISPATCH_PROPERTYGET, Vec::new())
    }

    pub fn put(&mut self, name: &'static str, value: VARIANT) -> ComResult<()> {
        self.invoke(name, DISPATCH_PROPERTYPUT, vec![value]).map(|_| ())
    }

    /// `args` are in natural (left-to-right) order.
    pub fn call(&mut self, name: &'static str, args: Vec<VARIANT>) -> ComResult<VARIANT> {
        self.invoke(name, DISPATCH_METHOD, args)
    }

    pub fn get_object(&mut self, name: &'static str) -> ComResult<Dispatch> {
        let v = self.get(name)?;
        variant_object(&v).ok_or_else(|| ComError::new(name, E_NOOBJECT))
    }

    pub fn call_object(&mut self, name: &'static str, args: Vec<VARIANT>) -> ComResult<Option<Dispatch>> {
        let v = self.call(name, args)?;
        Ok(variant_object(&v))
    }
}

/// Property returned no object where one was required.
pub const E_NOOBJECT: i32 = 0x8000_4003_u32 as i32; // E_POINTER

// =============================================================================
// VARIANT helpers (read-only views; the VARIANT keeps ownership and clears on drop)
// =============================================================================

fn vt(v: &VARIANT) -> u16 {
    unsafe { v.as_raw().Anonymous.Anonymous.vt }
}

pub fn variant_string(v: &VARIANT) -> Option<String> {
    if vt(v) != VT_BSTR.0 {
        return None;
    }
    let raw = unsafe { v.as_raw().Anonymous.Anonymous.Anonymous.bstrVal };
    // Borrow the BSTR owned by the VARIANT without freeing it.
    let bstr = ManuallyDrop::new(unsafe { BSTR::from_raw(raw) });
    Some(String::from_utf16_lossy(bstr.as_wide()))
}

pub fn variant_i32(v: &VARIANT) -> Option<i32> {
    let t = vt(v);
    let u = unsafe { v.as_raw().Anonymous.Anonymous.Anonymous };
    unsafe {
        match t {
            x if x == VT_I4.0 => Some(u.lVal),
            x if x == VT_I2.0 => Some(i32::from(u.iVal)),
            _ => None,
        }
    }
}

pub fn variant_bool(v: &VARIANT) -> Option<bool> {
    if vt(v) != VT_BOOL.0 {
        return None;
    }
    Some(unsafe { v.as_raw().Anonymous.Anonymous.Anonymous.boolVal } != 0)
}

/// Raw VT_DATE / VT_R8 value (days since 1899-12-30, machine-local time).
pub fn variant_date(v: &VARIANT) -> Option<f64> {
    let t = vt(v);
    if t != VT_DATE.0 && t != VT_R8.0 {
        return None;
    }
    Some(unsafe { v.as_raw().Anonymous.Anonymous.Anonymous.date })
}

/// Take a new reference to the object held by the VARIANT (the VARIANT stays valid).
pub fn variant_object(v: &VARIANT) -> Option<Dispatch> {
    let t = vt(v);
    if t != VT_DISPATCH.0 && t != VT_UNKNOWN.0 {
        return None;
    }
    let raw = unsafe { v.as_raw().Anonymous.Anonymous.Anonymous.pdispVal };
    if raw.is_null() {
        return None;
    }
    // Borrow the pointer owned by the VARIANT, then clone to add our own reference.
    let borrowed = ManuallyDrop::new(unsafe { IDispatch::from_raw(raw) });
    let owned: IDispatch = if t == VT_DISPATCH.0 {
        (*borrowed).clone()
    } else {
        (*borrowed).cast().ok()?
    };
    Some(Dispatch::new(owned))
}

pub fn variant_from_str(s: &str) -> VARIANT {
    VARIANT::from(s)
}

pub fn variant_from_i32(n: i32) -> VARIANT {
    VARIANT::from(n)
}

pub fn variant_from_bool(b: bool) -> VARIANT {
    VARIANT::from(b)
}

// =============================================================================
// DATE conversion
// =============================================================================

/// Days since 1899-12-30 -> civil date-time, rounded to the nearest second.
/// `None` for non-finite or out-of-range input.
pub fn date_to_naive(date: f64) -> Option<NaiveDateTime> {
    if !date.is_finite() || !(0.0..2_958_466.0).contains(&date) {
        return None;
    }
    let epoch = NaiveDate::from_ymd_opt(1899, 12, 30)?.and_hms_opt(0, 0, 0)?;
    epoch.checked_add_signed(Duration::seconds((date * 86_400.0).round() as i64))
}

/// Resolve a machine-local civil time to UTC through `lookup`.
/// Ambiguous (clocks go back): the earlier instant. Nonexistent (clocks go forward): moved
/// forward by one hour, i.e. read with the offset in force before the gap (02:30 becomes
/// 03:30 local). Deterministic for any zone.
pub fn resolve_local(naive: NaiveDateTime, lookup: impl Fn(&NaiveDateTime) -> LocalResult<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    match lookup(&naive) {
        LocalResult::Single(t) => Some(t),
        LocalResult::Ambiguous(first, second) => Some(first.min(second)),
        LocalResult::None => lookup(&naive.checked_add_signed(Duration::hours(1))?).earliest(),
    }
}

/// Outlook DATE values are in the machine's local time zone.
pub fn date_to_utc(date: f64) -> Option<DateTime<Utc>> {
    resolve_local(date_to_naive(date)?, |n| chrono::Local.from_local_datetime(n).map(|d| d.with_timezone(&Utc)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Timelike};

    /// A zone with a US-style 2 AM -> 3 AM spring-forward and 2 AM -> 1 AM fall-back,
    /// standard offset -5h, DST offset -4h, transitions on 2026-03-08 and 2026-11-01.
    fn us_eastern(n: &NaiveDateTime) -> LocalResult<DateTime<Utc>> {
        let std_off = FixedOffset::west_opt(5 * 3600).unwrap();
        let dst_off = FixedOffset::west_opt(4 * 3600).unwrap();
        let spring = NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let fall = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let t = n.time();
        let day = n.date();
        let hour = t.hour();
        let in_gap = day == spring && hour == 2;
        let ambiguous = day == fall && hour == 1;
        let is_dst = if day > spring && day < fall {
            true
        } else if day == spring {
            hour >= 3
        } else if day == fall {
            hour < 1
        } else {
            false
        };
        if in_gap {
            return LocalResult::None;
        }
        if ambiguous {
            let a = dst_off.from_local_datetime(n).unwrap().with_timezone(&Utc);
            let b = std_off.from_local_datetime(n).unwrap().with_timezone(&Utc);
            return LocalResult::Ambiguous(a, b);
        }
        let off = if is_dst { dst_off } else { std_off };
        LocalResult::Single(off.from_local_datetime(n).unwrap().with_timezone(&Utc))
    }

    #[test]
    fn date_epoch_and_known_values() {
        let n = date_to_naive(0.0).unwrap();
        assert_eq!(n.to_string(), "1899-12-30 00:00:00");
        // 2026-10-06 12:30:00 -> serial 46301.520833...
        let serial = 46_301.0 + (12.0 * 60.0 + 30.0) / 1440.0;
        assert_eq!(date_to_naive(serial).unwrap().to_string(), "2026-10-06 12:30:00");
    }

    #[test]
    fn date_rejects_garbage() {
        assert!(date_to_naive(f64::NAN).is_none());
        assert!(date_to_naive(f64::INFINITY).is_none());
        assert!(date_to_naive(-5.0).is_none());
        assert!(date_to_naive(3_000_000.0).is_none());
    }

    #[test]
    fn date_rounds_float_noise_to_the_second() {
        let serial = 46_301.0 + 9.0 / 24.0 + 1e-9;
        assert_eq!(date_to_naive(serial).unwrap().to_string(), "2026-10-06 09:00:00");
    }

    #[test]
    fn dst_ordinary_times_use_the_right_offset() {
        let winter = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap().and_hms_opt(9, 0, 0).unwrap();
        let summer = NaiveDate::from_ymd_opt(2026, 7, 15).unwrap().and_hms_opt(9, 0, 0).unwrap();
        assert_eq!(resolve_local(winter, us_eastern).unwrap().to_rfc3339(), "2026-01-15T14:00:00+00:00");
        assert_eq!(resolve_local(summer, us_eastern).unwrap().to_rfc3339(), "2026-07-15T13:00:00+00:00");
    }

    #[test]
    fn dst_ambiguous_hour_resolves_to_the_earlier_instant() {
        let n = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap().and_hms_opt(1, 30, 0).unwrap();
        // 01:30 EDT (UTC-4) = 05:30Z occurs before 01:30 EST (UTC-5) = 06:30Z.
        assert_eq!(resolve_local(n, us_eastern).unwrap().to_rfc3339(), "2026-11-01T05:30:00+00:00");
    }

    #[test]
    fn dst_gap_uses_the_offset_before_the_gap() {
        let n = NaiveDate::from_ymd_opt(2026, 3, 8).unwrap().and_hms_opt(2, 30, 0).unwrap();
        // 02:30 does not exist; with the pre-gap offset (UTC-5) it is 07:30Z (= 03:30 EDT).
        assert_eq!(resolve_local(n, us_eastern).unwrap().to_rfc3339(), "2026-03-08T07:30:00+00:00");
    }

    #[test]
    fn date_to_utc_round_trips_through_the_real_local_zone() {
        let serial = 46_301.0 + 9.0 / 24.0;
        let utc = date_to_utc(serial).unwrap();
        let back = utc.with_timezone(&chrono::Local).naive_local();
        assert_eq!(back, date_to_naive(serial).unwrap());
    }

    #[test]
    fn classifiers() {
        assert!(is_disconnected(0x8001_0108_u32 as i32));
        assert!(is_disconnected(0x8007_06BA_u32 as i32));
        assert!(is_disconnected(0x8004_01FD_u32 as i32));
        assert!(!is_disconnected(RPC_E_CALL_REJECTED));
        assert!(is_busy(RPC_E_CALL_REJECTED));
        assert!(is_busy(RPC_E_SERVERCALL_RETRYLATER));
        assert!(!is_busy(0x8001_0108_u32 as i32));
        assert!(is_blocked(E_ACCESSDENIED));
        assert!(is_blocked(0x800A_01B6_u32 as i32));
        assert!(!is_blocked(MK_E_UNAVAILABLE));
    }

    #[test]
    fn exception_scode_drives_classification() {
        let mut e = ComError::new("Invoke", DISP_E_EXCEPTION);
        assert!(!e.is_disconnected() && !e.is_blocked());
        e.scode = Some(E_ACCESSDENIED);
        assert!(e.is_blocked());
        e.scode = Some(0x8001_0108_u32 as i32);
        assert!(e.is_disconnected());
        assert!(e.to_string().contains("desc_len=0"));
    }

    #[test]
    fn retry_delay_is_capped() {
        assert_eq!(retry_delay(0, 2), RETRY_STEP_MS);
        assert_eq!(retry_delay(RETRY_CAP_MS - 1, 1), RETRY_STEP_MS);
        assert_eq!(retry_delay(RETRY_CAP_MS, 2), u32::MAX);
        assert_eq!(retry_delay(0, 0), u32::MAX);
    }

    /// A real in-process automation server (scrrun.dll) stands in for Outlook, which these
    /// tests must never touch. `None` when the runtime is not installed.
    fn scripting_object(prog_id: &str) -> Option<Dispatch> {
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
        let wide: Vec<u16> = prog_id.encode_utf16().chain(std::iter::once(0)).collect();
        let clsid = unsafe { CLSIDFromProgID(PCWSTR(wide.as_ptr())) }.ok()?;
        let ptr: IDispatch = unsafe { CoCreateInstance(&clsid, None, CLSCTX_INPROC_SERVER) }.ok()?;
        Some(Dispatch::new(ptr))
    }

    #[test]
    fn dispatch_against_a_real_automation_server() {
        let _apartment = ComApartment::init_sta().unwrap();
        let Some(mut dict) = scripting_object("Scripting.Dictionary") else { return };

        // PROPERTYPUT with its named DISPID_PROPERTYPUT argument.
        dict.put("CompareMode", variant_from_i32(1)).unwrap();
        assert_eq!(variant_i32(&dict.get("CompareMode").unwrap()), Some(1));

        // Arguments are passed in natural order: Add(key, item), then Exists(key).
        dict.call("Add", vec![variant_from_str("k"), variant_from_i32(7)]).unwrap();
        assert_eq!(variant_i32(&dict.get("Count").unwrap()), Some(1));
        assert_eq!(variant_bool(&dict.call("Exists", vec![variant_from_str("k")]).unwrap()), Some(true));
        assert_eq!(variant_bool(&dict.call("Exists", vec![variant_from_i32(7)]).unwrap()), Some(false));

        // A server-raised error surfaces as an automation exception: the scode drives the
        // classification and the description text is never kept.
        let err = dict.call("Add", vec![variant_from_str("k"), variant_from_i32(8)]).unwrap_err();
        assert_eq!(err.hr, DISP_E_EXCEPTION);
        assert_eq!(err.scode, Some(0x800A_01C9_u32 as i32), "{err}");
        assert!(err.is_blocked() && !err.is_busy() && !err.is_disconnected());

        // Unknown members fail cleanly and the object stays usable.
        assert!(dict.get("NoSuchMember").is_err());
        assert_eq!(variant_i32(&dict.get("Count").unwrap()), Some(1));
    }

    #[test]
    fn dispatch_returns_owned_child_objects() {
        let _apartment = ComApartment::init_sta().unwrap();
        let Some(mut fso) = scripting_object("Scripting.FileSystemObject") else { return };
        for _ in 0..50 {
            let mut drives = fso.get_object("Drives").unwrap();
            assert!(variant_i32(&drives.get("Count").unwrap()).is_some());
            assert!(fso.get("NoSuchMember").is_err());
        }
    }

    #[test]
    fn variant_string_round_trip_and_types() {
        let v = variant_from_str("héllo");
        assert_eq!(variant_string(&v).as_deref(), Some("héllo"));
        assert!(variant_i32(&v).is_none());
        let n = variant_from_i32(7);
        assert_eq!(variant_i32(&n), Some(7));
        let b = variant_from_bool(true);
        assert_eq!(variant_bool(&b), Some(true));
        assert!(variant_object(&v).is_none());
    }
}
