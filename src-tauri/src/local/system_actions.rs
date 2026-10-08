//! Windows-side actions behind the smart-search commands ("תחפש בגוגל ...", "תפתח את ynet", "פתח הגדרות
//! wifi", "פתח הורדות", "תנעל את המחשב", "תכתוב מייל לדני"). Everything here is split in two:
//!
//! - **pure** tables and builders (search URLs, address validation, the Windows settings table, the
//!   known sites and folders), unit-tested and used by `intent::commands` and `assistant::actions`;
//! - **effects** (`open_url`, `open_setting`, `open_folder`, `lock_workstation`, `compose_mail`) that run
//!   only after an explicit click on an item the assistant minted. They never take text: each takes a
//!   key of a table below or an address that passes [`normalize_web_url`] again.
//!
//! Safety rules, all enforced here and not in the callers:
//! - only `http` / `https` addresses are ever opened (never `file:`, `javascript:`, `ms-settings:` typed
//!   by the user, a path or a command line);
//! - settings pages come from [`SETTINGS`] only, folders from [`FOLDERS`] only;
//! - a new mail is only ever *displayed* in the user's own Outlook; this module has no code that sends
//!   mail (a test fails if one appears);
//! - nothing typed by the user (query, note text, recipient) is logged.

use super::{known_folder, shell_open};
use std::path::PathBuf;
use windows::core::HSTRING;
use windows::Win32::UI::Shell::{
    ShellExecuteW, FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_SkyDrive,
    FOLDERID_System, FOLDERID_Videos,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

// =============================================================================
// Text
// =============================================================================

/// Longest query that goes into a URL (characters).
pub const MAX_QUERY_CHARS: usize = 300;
/// Longest address that is opened.
pub const MAX_URL_LEN: usize = 2000;

pub fn has_hebrew(s: &str) -> bool {
    s.chars().any(|c| ('\u{05D0}'..='\u{05EA}').contains(&c))
}

/// A query ready for a URL: control characters dropped, whitespace collapsed, one pair of wrapping
/// quotes removed, at most [`MAX_QUERY_CHARS`] characters. `None` when nothing is left.
pub fn clean_query(raw: &str) -> Option<String> {
    let collapsed: String = raw.chars().map(|c| if c.is_control() || c.is_whitespace() { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s = collapsed.as_str();
    for (open, close) in [('"', '"'), ('\'', '\''), ('\u{201C}', '\u{201D}'), ('\u{05F4}', '\u{05F4}'), ('\u{00AB}', '\u{00BB}')] {
        if s.chars().count() >= 2 && s.starts_with(open) && s.ends_with(close) {
            s = s[open.len_utf8()..s.len() - close.len_utf8()].trim();
            break;
        }
    }
    let s: String = s.chars().take(MAX_QUERY_CHARS).collect();
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Percent-encoding of every byte except the RFC 3986 unreserved set (UTF-8, so Hebrew is safe).
pub fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// =============================================================================
// Web addresses
// =============================================================================

/// Top-level domains accepted for an address typed without a scheme ("example.co.il"). Deliberately
/// excludes the ones that are also file extensions (zip, mov, sh, py, rs, pl, md...), so "report.pdf" or
/// "setup.exe" is never taken for a website.
const TLDS: &[&str] = &[
    "com", "org", "net", "edu", "gov", "info", "biz", "io", "co", "ai", "app", "dev", "tv", "me", "us", "uk", "il", "de", "fr", "es", "it", "nl", "ru", "cn",
    "jp", "br", "in", "au", "ca", "eu", "ch", "at", "be", "se", "no", "dk", "fi", "gr", "tr", "ua", "mx", "xyz", "online", "site", "store", "shop", "tech",
    "news", "blog", "cloud", "live", "pro", "wiki",
];

fn valid_label(l: &str) -> bool {
    (1..=63).contains(&l.len()) && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') && !l.starts_with('-') && !l.ends_with('-')
}

fn is_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().map_or(false, |n| n <= 255))
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    s.get(..prefix.len()).filter(|p| p.eq_ignore_ascii_case(prefix)).map(|_| &s[prefix.len()..])
}

/// A typed address as a safe `http(s)` URL, or `None`.
///
/// - `https://` and `http://` addresses are kept; any other scheme (`file:`, `javascript:`, `ms-settings:`,
///   `mailto:`, a UNC or drive path...) is refused;
/// - without a scheme the text must be a domain whose last label is in [`TLDS`]; `https://` is added;
/// - no credentials (`user@host`), no spaces or control characters, ASCII host only, at most
///   [`MAX_URL_LEN`] characters; characters outside the URL alphabet in the path are percent-encoded.
pub fn normalize_web_url(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() || s.len() > MAX_URL_LEN || s.chars().any(|c| c.is_control() || c.is_whitespace() || matches!(c, '\\' | '"' | '<' | '>' | '`' | '{' | '}' | '|' | '^')) {
        return None;
    }
    let (scheme, rest, explicit) = if let Some(r) = strip_prefix_ci(s, "https://") {
        ("https", r, true)
    } else if let Some(r) = strip_prefix_ci(s, "http://") {
        ("http", r, true)
    } else {
        // "host:8080/x" has a dot before the colon; "javascript:..." or "ms-settings:..." does not.
        let before_colon = s.split(['/', '?', '#']).next().unwrap_or("");
        if let Some((head, _)) = before_colon.split_once(':') {
            if !head.contains('.') {
                return None;
            }
        }
        ("https", s, false)
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u32>().map_or(false, |n| (1..=65535).contains(&n)) => (h, Some(p)),
        Some(_) => return None,
        None => (authority, None),
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if !host.is_ascii() {
        return None;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || !labels.iter().all(|l| valid_label(l)) {
        return None;
    }
    let tld = labels[labels.len() - 1];
    let ok_host = is_ipv4(&host) && explicit || (tld.bytes().all(|b| b.is_ascii_alphabetic()) && if explicit { (2..=24).contains(&tld.len()) } else { TLDS.contains(&tld) });
    if !ok_host {
        return None;
    }
    let mut out = format!("{scheme}://{host}");
    if let Some(p) = port {
        out.push(':');
        out.push_str(p);
    }
    for ch in tail.chars() {
        if ch.is_ascii() {
            out.push(ch);
        } else {
            let mut buf = [0u8; 4];
            for b in ch.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    (out.len() <= MAX_URL_LEN).then_some(out)
}

/// The host of a URL that [`normalize_web_url`] produced ("www.google.com"), for display.
pub fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once(':').map_or(authority, |(h, _)| h);
    (!host.is_empty()).then(|| host.to_string())
}

// =============================================================================
// Search engines and translation languages
// =============================================================================

/// `(key, Hebrew name, English name)` of every search target of `web.search`.
#[cfg_attr(not(test), allow(dead_code))]
pub const ENGINES: &[(&str, &str, &str)] = &[
    ("google", "גוגל", "Google"),
    ("youtube", "יוטיוב", "YouTube"),
    ("wikipedia", "ויקיפדיה", "Wikipedia"),
    ("maps", "גוגל מפות", "Google Maps"),
    ("waze", "Waze", "Waze"),
    ("bing", "בינג", "Bing"),
    ("translate", "גוגל תרגום", "Google Translate"),
];

#[cfg_attr(not(test), allow(dead_code))]
pub fn engine_name(key: &str, hebrew: bool) -> Option<&'static str> {
    ENGINES.iter().find(|e| e.0 == key).map(|e| if hebrew { e.1 } else { e.2 })
}

/// `(Google Translate code, Hebrew name, English name)`. Hebrew is `iw`, the code Google Translate has
/// always accepted.
pub const LANGUAGES: &[(&str, &str, &str)] = &[
    ("en", "אנגלית", "English"),
    ("iw", "עברית", "Hebrew"),
    ("ar", "ערבית", "Arabic"),
    ("ru", "רוסית", "Russian"),
    ("fr", "צרפתית", "French"),
    ("es", "ספרדית", "Spanish"),
    ("de", "גרמנית", "German"),
    ("it", "איטלקית", "Italian"),
    ("pt", "פורטוגזית", "Portuguese"),
    ("tr", "טורקית", "Turkish"),
    ("uk", "אוקראינית", "Ukrainian"),
    ("ja", "יפנית", "Japanese"),
    ("zh-CN", "סינית", "Chinese"),
    ("yi", "יידיש", "Yiddish"),
    ("am", "אמהרית", "Amharic"),
];

pub fn language_name(code: &str, hebrew: bool) -> Option<&'static str> {
    LANGUAGES.iter().find(|l| l.0 == code).map(|l| if hebrew { l.1 } else { l.2 })
}

/// The target language when the user named none: English for Hebrew text, Hebrew for everything else.
pub fn default_translate_target(text: &str) -> &'static str {
    if has_hebrew(text) {
        "en"
    } else {
        "iw"
    }
}

/// The address of a search. `from` / `to` are language codes and only matter for `translate`.
pub fn search_url(engine: &str, query: &str, from: Option<&str>, to: Option<&str>) -> Option<String> {
    let q = clean_query(query)?;
    let e = encode_component(&q);
    let valid_lang = |c: &str| LANGUAGES.iter().any(|l| l.0 == c);
    let url = match engine {
        "google" => format!("https://www.google.com/search?q={e}"),
        "youtube" => format!("https://www.youtube.com/results?search_query={e}"),
        "wikipedia" => format!("https://{}.wikipedia.org/w/index.php?search={e}", if has_hebrew(&q) { "he" } else { "en" }),
        "maps" => format!("https://www.google.com/maps/search/?api=1&query={e}"),
        "waze" => format!("https://waze.com/ul?q={e}"),
        "bing" => format!("https://www.bing.com/search?q={e}"),
        "translate" => {
            let sl = from.filter(|c| valid_lang(c)).unwrap_or("auto");
            let tl = to.filter(|c| valid_lang(c)).unwrap_or_else(|| default_translate_target(&q));
            format!("https://translate.google.com/?sl={sl}&tl={tl}&text={e}")
        }
        _ => return None,
    };
    // A query of astral characters can encode to more than an address may hold: no search, not a cut one.
    (url.len() <= MAX_URL_LEN).then_some(url)
}

// =============================================================================
// Known websites
// =============================================================================

pub struct Site {
    pub key: &'static str,
    pub url: &'static str,
    pub he: &'static str,
    pub en: &'static str,
}

const fn site(key: &'static str, url: &'static str, he: &'static str, en: &'static str) -> Site {
    Site { key, url, he, en }
}

/// The built-in websites of `web.open`. The words that name them are in `intent/lexicon/commands.json`
/// (a test keeps both lists in step). Israeli sites are the ones whose address is certain.
pub const SITES: &[Site] = &[
    site("google", "https://www.google.com", "גוגל", "Google"),
    site("gmail", "https://mail.google.com", "Gmail", "Gmail"),
    site("gdrive", "https://drive.google.com", "Google Drive", "Google Drive"),
    site("gdocs", "https://docs.google.com", "Google Docs", "Google Docs"),
    site("gsheets", "https://docs.google.com/spreadsheets/", "Google Sheets", "Google Sheets"),
    site("gcalendar", "https://calendar.google.com", "יומן גוגל", "Google Calendar"),
    site("gmaps", "https://www.google.com/maps", "גוגל מפות", "Google Maps"),
    site("gtranslate", "https://translate.google.com", "גוגל תרגום", "Google Translate"),
    site("gphotos", "https://photos.google.com", "Google Photos", "Google Photos"),
    site("gmeet", "https://meet.google.com", "Google Meet", "Google Meet"),
    site("youtube", "https://www.youtube.com", "יוטיוב", "YouTube"),
    site("waze", "https://www.waze.com/live-map", "Waze", "Waze"),
    site("wikipedia", "https://www.wikipedia.org", "ויקיפדיה", "Wikipedia"),
    site("facebook", "https://www.facebook.com", "פייסבוק", "Facebook"),
    site("instagram", "https://www.instagram.com", "אינסטגרם", "Instagram"),
    site("linkedin", "https://www.linkedin.com", "לינקדאין", "LinkedIn"),
    site("x", "https://x.com", "X (טוויטר)", "X (Twitter)"),
    site("tiktok", "https://www.tiktok.com", "טיקטוק", "TikTok"),
    site("reddit", "https://www.reddit.com", "רדיט", "Reddit"),
    site("whatsapp-web", "https://web.whatsapp.com", "וואטסאפ ווב", "WhatsApp Web"),
    site("telegram-web", "https://web.telegram.org", "טלגרם ווב", "Telegram Web"),
    site("netflix", "https://www.netflix.com", "נטפליקס", "Netflix"),
    site("spotify-web", "https://open.spotify.com", "ספוטיפיי ווב", "Spotify Web"),
    site("outlook-web", "https://outlook.office.com/mail/", "Outlook ווב", "Outlook on the web"),
    site("office", "https://www.office.com", "Microsoft 365", "Microsoft 365"),
    site("teams-web", "https://teams.microsoft.com", "Teams ווב", "Teams on the web"),
    site("chatgpt", "https://chatgpt.com", "ChatGPT", "ChatGPT"),
    site("claude", "https://claude.ai", "Claude", "Claude"),
    site("github", "https://github.com", "GitHub", "GitHub"),
    site("stackoverflow", "https://stackoverflow.com", "Stack Overflow", "Stack Overflow"),
    site("bing", "https://www.bing.com", "בינג", "Bing"),
    site("amazon", "https://www.amazon.com", "אמזון", "Amazon"),
    site("ebay", "https://www.ebay.com", "איביי", "eBay"),
    site("ynet", "https://www.ynet.co.il", "ynet", "ynet"),
    site("walla", "https://www.walla.co.il", "וואלה", "Walla"),
    site("mako", "https://www.mako.co.il", "מאקו", "Mako"),
    site("haaretz", "https://www.haaretz.co.il", "הארץ", "Haaretz"),
    site("kan", "https://www.kan.org.il", "כאן", "Kan"),
    site("sport5", "https://www.sport5.co.il", "ספורט 5", "Sport5"),
    site("calcalist", "https://www.calcalist.co.il", "כלכליסט", "Calcalist"),
    site("globes", "https://www.globes.co.il", "גלובס", "Globes"),
    site("themarker", "https://www.themarker.com", "דה מרקר", "TheMarker"),
    site("israelhayom", "https://www.israelhayom.co.il", "ישראל היום", "Israel Hayom"),
    site("maariv", "https://www.maariv.co.il", "מעריב", "Maariv"),
    site("yad2", "https://www.yad2.co.il", "יד2", "Yad2"),
    site("zap", "https://www.zap.co.il", "זאפ", "Zap"),
    site("ksp", "https://ksp.co.il", "KSP", "KSP"),
    site("govil", "https://www.gov.il/he", "gov.il", "gov.il"),
    site("btl", "https://www.btl.gov.il", "ביטוח לאומי", "National Insurance (BTL)"),
    site("leumi", "https://www.leumi.co.il", "בנק לאומי", "Bank Leumi"),
    site("hapoalim", "https://www.bankhapoalim.co.il", "בנק הפועלים", "Bank Hapoalim"),
    site("discount", "https://www.discountbank.co.il", "בנק דיסקונט", "Discount Bank"),
    site("mizrahi", "https://www.mizrahi-tefahot.co.il", "בנק מזרחי טפחות", "Mizrahi Tefahot"),
];

pub fn site_by_key(key: &str) -> Option<&'static Site> {
    SITES.iter().find(|s| s.key == key)
}

// =============================================================================
// Windows settings
// =============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingKind {
    /// An `ms-settings:` page, opened with `ShellExecute`.
    Uri,
    /// The classic Control Panel (`control.exe` from the System folder).
    ControlPanel,
    /// The screen-snip overlay (`ms-screenclip:`), Windows 10 1809 and later.
    ScreenClip,
}

pub struct Setting {
    pub key: &'static str,
    pub kind: SettingKind,
    /// The `ms-settings:` URI (empty for the other kinds).
    pub uri: &'static str,
    /// The page exists on Windows 10 22H2 (19045). Checked against Microsoft's "Launch the Windows
    /// Settings app" reference (learn.microsoft.com/windows/apps/develop/launch/launch-settings); a page
    /// that is missing there, or deprecated on 1809+, is `false` and never offered.
    pub win10: bool,
    pub he: &'static str,
    pub en: &'static str,
}

const fn setting(key: &'static str, uri: &'static str, he: &'static str, en: &'static str) -> Setting {
    Setting { key, kind: SettingKind::Uri, uri, win10: true, he, en }
}

/// Windows settings pages. Each URI is one the Microsoft reference lists, valid on Windows 10 22H2 and
/// Windows 11; `home` (the bare `ms-settings:`) is also the fallback for an unknown key.
pub const SETTINGS: &[Setting] = &[
    setting("home", "ms-settings:", "הגדרות Windows", "Windows Settings"),
    setting("wifi", "ms-settings:network-wifi", "הגדרות Wi-Fi", "Wi-Fi settings"),
    setting("network", "ms-settings:network-status", "הגדרות רשת", "Network settings"),
    setting("ethernet", "ms-settings:network-ethernet", "הגדרות רשת קווית", "Ethernet settings"),
    setting("airplane", "ms-settings:network-airplanemode", "מצב טיסה", "Airplane mode"),
    setting("hotspot", "ms-settings:network-mobilehotspot", "נקודה חמה", "Mobile hotspot"),
    setting("vpn", "ms-settings:network-vpn", "הגדרות VPN", "VPN settings"),
    setting("proxy", "ms-settings:network-proxy", "הגדרות פרוקסי", "Proxy settings"),
    setting("bluetooth", "ms-settings:bluetooth", "הגדרות בלוטוס", "Bluetooth settings"),
    setting("display", "ms-settings:display", "הגדרות תצוגה", "Display settings"),
    setting("nightlight", "ms-settings:nightlight", "תאורת לילה", "Night light"),
    setting("project", "ms-settings:project", "הקרנה למסך נוסף", "Project to a second screen"),
    setting("sound", "ms-settings:sound", "הגדרות שמע", "Sound settings"),
    setting("printers", "ms-settings:printers", "מדפסות וסורקים", "Printers & scanners"),
    setting("mouse", "ms-settings:mousetouchpad", "הגדרות עכבר", "Mouse settings"),
    setting("keyboard", "ms-settings:typing", "הגדרות הקלדה", "Typing settings"),
    setting("update", "ms-settings:windowsupdate", "Windows Update", "Windows Update"),
    setting("battery", "ms-settings:batterysaver", "חיסכון בסוללה", "Battery saver"),
    setting("power", "ms-settings:powersleep", "צריכת חשמל ושינה", "Power & sleep"),
    setting("background", "ms-settings:personalization-background", "רקע שולחן העבודה", "Background"),
    setting("themes", "ms-settings:themes", "ערכות נושא", "Themes"),
    // `colors` is the older alias of personalization-colors and works on both Windows 10 and 11.
    setting("colors", "ms-settings:colors", "צבעים", "Colors"),
    setting("lockscreen", "ms-settings:lockscreen", "מסך הנעילה", "Lock screen"),
    setting("taskbar", "ms-settings:taskbar", "שורת המשימות", "Taskbar"),
    setting("datetime", "ms-settings:dateandtime", "תאריך ושעה", "Date & time"),
    setting("region", "ms-settings:regionformatting", "אזור ופורמט", "Region"),
    setting("language", "ms-settings:regionlanguage", "שפה", "Language"),
    setting("notifications", "ms-settings:notifications", "התראות ופעולות", "Notifications & actions"),
    setting("focus", "ms-settings:quiethours", "עזרת מיקוד", "Focus assist"),
    setting("apps", "ms-settings:appsfeatures", "אפליקציות ותכונות", "Apps & features"),
    setting("defaultapps", "ms-settings:defaultapps", "אפליקציות ברירת מחדל", "Default apps"),
    setting("startupapps", "ms-settings:startupapps", "אפליקציות הפעלה", "Startup apps"),
    setting("storage", "ms-settings:storagesense", "אחסון", "Storage"),
    setting("privacy", "ms-settings:privacy", "פרטיות", "Privacy"),
    setting("camera", "ms-settings:privacy-webcam", "פרטיות המצלמה", "Camera privacy"),
    setting("microphone", "ms-settings:privacy-microphone", "פרטיות המיקרופון", "Microphone privacy"),
    setting("location", "ms-settings:privacy-location", "פרטיות המיקום", "Location privacy"),
    setting("about", "ms-settings:about", "מידע על המחשב", "About your PC"),
    setting("accounts", "ms-settings:yourinfo", "הפרטים שלי", "Your info"),
    setting("signin", "ms-settings:signinoptions", "אפשרויות כניסה", "Sign-in options"),
    setting("security", "ms-settings:windowsdefender", "אבטחת Windows", "Windows Security"),
    setting("troubleshoot", "ms-settings:troubleshoot", "פתרון בעיות", "Troubleshoot"),
    setting("recovery", "ms-settings:recovery", "שחזור", "Recovery"),
    setting("clipboard", "ms-settings:clipboard", "לוח העתקה", "Clipboard"),
    setting("multitasking", "ms-settings:multitasking", "ריבוי משימות", "Multitasking"),
    setting("remotedesktop", "ms-settings:remotedesktop", "שולחן עבודה מרוחק", "Remote Desktop"),
    setting("usb", "ms-settings:usb", "הגדרות USB", "USB settings"),
    setting("speech", "ms-settings:speech", "דיבור", "Speech"),
    setting("accessibility", "ms-settings:easeofaccess-display", "נגישות", "Ease of access"),
    setting("magnifier", "ms-settings:easeofaccess-magnifier", "הגדלה", "Magnifier"),
    setting("narrator", "ms-settings:easeofaccess-narrator", "קורא מסך", "Narrator"),
    Setting { key: "control-panel", kind: SettingKind::ControlPanel, uri: "", win10: true, he: "לוח הבקרה", en: "Control Panel" },
    Setting { key: "screenclip", kind: SettingKind::ScreenClip, uri: "ms-screenclip:", win10: true, he: "כלי החיתוך", en: "Screen snip" },
];

/// The `ms-settings:` pages of the Microsoft reference that exist on Windows 10 22H2 and that the
/// table above relies on (a unit test checks every [`SETTINGS`] URI against this list).
#[cfg(test)]
pub const DOC_WIN10_URIS: &[&str] = &[
    "ms-settings:", "ms-settings:network-wifi", "ms-settings:network-status", "ms-settings:network-ethernet", "ms-settings:network-airplanemode",
    "ms-settings:network-mobilehotspot", "ms-settings:network-vpn", "ms-settings:network-proxy", "ms-settings:bluetooth", "ms-settings:display",
    "ms-settings:nightlight", "ms-settings:project", "ms-settings:sound", "ms-settings:printers", "ms-settings:mousetouchpad", "ms-settings:typing",
    "ms-settings:windowsupdate", "ms-settings:batterysaver", "ms-settings:powersleep", "ms-settings:personalization-background", "ms-settings:themes",
    "ms-settings:colors", "ms-settings:lockscreen", "ms-settings:taskbar", "ms-settings:dateandtime", "ms-settings:regionformatting",
    "ms-settings:regionlanguage", "ms-settings:notifications", "ms-settings:quiethours", "ms-settings:appsfeatures", "ms-settings:defaultapps",
    "ms-settings:startupapps", "ms-settings:storagesense", "ms-settings:privacy", "ms-settings:privacy-webcam", "ms-settings:privacy-microphone",
    "ms-settings:privacy-location", "ms-settings:about", "ms-settings:yourinfo", "ms-settings:signinoptions", "ms-settings:windowsdefender",
    "ms-settings:troubleshoot", "ms-settings:recovery", "ms-settings:clipboard", "ms-settings:multitasking", "ms-settings:remotedesktop", "ms-settings:usb",
    "ms-settings:speech", "ms-settings:easeofaccess-display", "ms-settings:easeofaccess-magnifier", "ms-settings:easeofaccess-narrator",
];

pub fn setting_by_key(key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.key == key)
}

/// The page to open for `key`: the table entry when it exists and is valid on Windows 10, otherwise the
/// Settings home page.
pub fn setting_or_home(key: &str) -> &'static Setting {
    setting_by_key(key).filter(|s| s.win10).unwrap_or(&SETTINGS[0])
}

// =============================================================================
// Folders
// =============================================================================

pub struct Folder {
    pub key: &'static str,
    pub he: &'static str,
    pub en: &'static str,
}

/// The folders of `folders.open`. Resolution is by the shell ([`resolve_folder`]), never by a path from text.
pub const FOLDERS: &[Folder] = &[
    Folder { key: "downloads", he: "הורדות", en: "Downloads" },
    Folder { key: "documents", he: "מסמכים", en: "Documents" },
    Folder { key: "desktop", he: "שולחן העבודה", en: "Desktop" },
    Folder { key: "pictures", he: "תמונות", en: "Pictures" },
    Folder { key: "music", he: "מוזיקה", en: "Music" },
    Folder { key: "videos", he: "סרטונים", en: "Videos" },
    Folder { key: "recycle", he: "סל המחזור", en: "Recycle Bin" },
    Folder { key: "thispc", he: "המחשב הזה", en: "This PC" },
    Folder { key: "onedrive", he: "OneDrive", en: "OneDrive" },
];

pub fn folder_by_key(key: &str) -> Option<&'static Folder> {
    FOLDERS.iter().find(|f| f.key == key)
}

#[derive(Clone, Debug, PartialEq)]
pub enum FolderTarget {
    /// A real directory.
    Path(PathBuf),
    /// A virtual shell folder, opened as `explorer.exe <name>`.
    Shell(&'static str),
}

/// The virtual folders (`shell:` names).
fn shell_name(key: &str) -> Option<&'static str> {
    match key {
        "recycle" => Some("shell:RecycleBinFolder"),
        "thispc" => Some("shell:MyComputerFolder"),
        _ => None,
    }
}

fn onedrive_path() -> Option<PathBuf> {
    // The OneDrive client sets these; the business account wins over the personal one, like the file search.
    for var in ["OneDriveCommercial", "OneDrive"] {
        if let Some(p) = std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute() && p.is_dir()) {
            return Some(p);
        }
    }
    known_folder(&FOLDERID_SkyDrive).filter(|p| p.is_dir())
}

/// Where a folder key points on this PC, or `None` (unknown key, or a folder that does not exist, such as
/// OneDrive on a PC without it).
pub fn resolve_folder(key: &str) -> Option<FolderTarget> {
    if let Some(name) = shell_name(key) {
        return Some(FolderTarget::Shell(name));
    }
    let dir = match key {
        "downloads" => known_folder(&FOLDERID_Downloads),
        "documents" => known_folder(&FOLDERID_Documents),
        "desktop" => known_folder(&FOLDERID_Desktop),
        "pictures" => known_folder(&FOLDERID_Pictures),
        "music" => known_folder(&FOLDERID_Music),
        "videos" => known_folder(&FOLDERID_Videos),
        "onedrive" => onedrive_path(),
        _ => None,
    }?;
    dir.is_dir().then_some(FolderTarget::Path(dir))
}

// =============================================================================
// Effects (explicit click only)
// =============================================================================

fn shell_execute(verb_target: &str, params: Option<&str>) -> Result<(), String> {
    let params = params.map(HSTRING::from);
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(verb_target), params.as_ref().map_or(windows::core::PCWSTR::null(), |p| windows::core::PCWSTR(p.as_ptr())), None, SW_SHOWNORMAL) };
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecute returned {}", r.0 as isize))
    }
}

/// Open an `http(s)` address in the default browser. The address is validated again here.
pub fn open_url(url: &str) -> Result<(), String> {
    let safe = normalize_web_url(url).filter(|u| u == url).ok_or_else(|| "APP-050: this address cannot be opened".to_string())?;
    shell_execute(&safe, None).map_err(|_| "APP-050: could not open the browser".to_string())
}

/// Open a settings page by key (unknown keys open the Settings home page).
pub fn open_setting(key: &str) -> Result<(), String> {
    let s = setting_or_home(key);
    let result = match s.kind {
        SettingKind::Uri | SettingKind::ScreenClip => shell_execute(s.uri, None),
        SettingKind::ControlPanel => {
            let control = known_folder(&FOLDERID_System).map(|d| d.join("control.exe")).filter(|p| p.is_file()).ok_or_else(|| "control.exe not found".to_string())?;
            shell_open(&control)
        }
    };
    result.map_err(|_| "APP-051: could not open the settings page".to_string())
}

/// Open a known folder in Explorer.
pub fn open_folder(key: &str) -> Result<(), String> {
    match resolve_folder(key) {
        Some(FolderTarget::Path(p)) => shell_open(&p).map_err(|_| "APP-052: could not open the folder".to_string()),
        Some(FolderTarget::Shell(name)) => {
            let explorer = crate::paths::explorer_exe();
            shell_execute(&explorer.to_string_lossy(), Some(name)).map_err(|_| "APP-052: could not open the folder".to_string())
        }
        None => Err("APP-052: the folder does not exist on this PC".into()),
    }
}

/// Lock the workstation (the same as Win+L). Only after the click on "נעל".
pub fn lock_workstation() -> Result<(), String> {
    unsafe { windows::Win32::System::Shutdown::LockWorkStation() }.map_err(|_| "APP-053: could not lock the PC".to_string())
}

fn mail_field(raw: &str, max: usize) -> String {
    raw.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

/// Open a NEW message window in the user's own running classic Outlook, with To and Subject filled in.
/// The message is only displayed: it is never saved, sent or given a body, and this module has no code
/// path that sends mail. Attach-only (an Outlook that is not running is not started).
pub fn compose_mail(to: Option<&str>, subject: Option<&str>) -> Result<(), String> {
    use crate::com::{self, ComError, Dispatch};
    use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
    let to = to.map(|t| mail_field(t, 200)).filter(|t| !t.is_empty());
    let subject = subject.map(|s| mail_field(s, 255)).filter(|s| !s.is_empty());
    crate::outlook::run_on_outlook("companyisland-mail-compose", 15, move |pid| {
        let mut app = Dispatch::get_active("Outlook.Application")?;
        // olMailItem = 0
        let mut item = app.call_object("CreateItem", vec![com::variant_from_i32(0)])?.ok_or_else(|| ComError::new("CreateItem", com::E_NOOBJECT))?;
        if let Some(to) = &to {
            item.put("To", com::variant_from_str(to))?;
        }
        if let Some(subject) = &subject {
            item.put("Subject", com::variant_from_str(subject))?;
        }
        if to.is_some() {
            // Best effort: turn the typed name into the address-book entry. A name that cannot be
            // resolved simply stays underlined in the window for the user to fix.
            if let Ok(mut recipients) = item.get_object("Recipients") {
                let _ = recipients.call("ResolveAll", Vec::new());
            }
        }
        // The click was on our (never-activated) window, so Windows lets this process hand the
        // foreground on to the user's own Outlook for this one window.
        unsafe {
            let _ = AllowSetForegroundWindow(pid);
        }
        item.call("Display", vec![com::variant_from_bool(false)])?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_is_utf8_percent_and_hebrew_safe() {
        assert_eq!(encode_component("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(encode_component("שלום"), "%D7%A9%D7%9C%D7%95%D7%9D");
        assert_eq!(encode_component("AZaz09-._~"), "AZaz09-._~");
        assert_eq!(encode_component("100%"), "100%25");
        assert_eq!(encode_component("a+b"), "a%2Bb");
    }

    #[test]
    fn queries_are_cleaned() {
        assert_eq!(clean_query("  חתולים   חמודים \n"), Some("חתולים חמודים".into()));
        assert_eq!(clean_query("\"מזג אוויר\""), Some("מזג אוויר".into()));
        assert_eq!(clean_query("'x'"), Some("x".into()));
        assert_eq!(clean_query("\u{201C}hello world\u{201D}"), Some("hello world".into()));
        assert_eq!(clean_query("a\u{0}b\tc"), Some("a b c".into()));
        assert_eq!(clean_query("   "), None);
        assert_eq!(clean_query("\"\""), None);
        assert_eq!(clean_query(&"ש".repeat(1000)).unwrap().chars().count(), MAX_QUERY_CHARS);
    }

    #[test]
    fn search_urls() {
        assert_eq!(search_url("google", "חתולים", None, None).unwrap(), "https://www.google.com/search?q=%D7%97%D7%AA%D7%95%D7%9C%D7%99%D7%9D");
        assert_eq!(search_url("google", "cats & dogs", None, None).unwrap(), "https://www.google.com/search?q=cats%20%26%20dogs");
        assert_eq!(search_url("youtube", "lofi", None, None).unwrap(), "https://www.youtube.com/results?search_query=lofi");
        assert_eq!(search_url("bing", "x", None, None).unwrap(), "https://www.bing.com/search?q=x");
        assert_eq!(search_url("waze", "תל אביב", None, None).unwrap(), "https://waze.com/ul?q=%D7%AA%D7%9C%20%D7%90%D7%91%D7%99%D7%91");
        assert_eq!(search_url("maps", "Eiffel Tower", None, None).unwrap(), "https://www.google.com/maps/search/?api=1&query=Eiffel%20Tower");
        assert!(search_url("wikipedia", "חתול", None, None).unwrap().starts_with("https://he.wikipedia.org/w/index.php?search="));
        assert!(search_url("wikipedia", "cat", None, None).unwrap().starts_with("https://en.wikipedia.org/w/index.php?search="));
        assert_eq!(search_url("nope", "x", None, None), None);
        assert_eq!(search_url("google", "  ", None, None), None);
    }

    #[test]
    fn translate_urls_pick_the_target_language() {
        assert_eq!(search_url("translate", "שלום", None, None).unwrap(), "https://translate.google.com/?sl=auto&tl=en&text=%D7%A9%D7%9C%D7%95%D7%9D");
        assert_eq!(search_url("translate", "hello", None, None).unwrap(), "https://translate.google.com/?sl=auto&tl=iw&text=hello");
        assert_eq!(search_url("translate", "hello", None, Some("ru")).unwrap(), "https://translate.google.com/?sl=auto&tl=ru&text=hello");
        assert_eq!(search_url("translate", "hello", Some("en"), Some("iw")).unwrap(), "https://translate.google.com/?sl=en&tl=iw&text=hello");
        // an unknown language code never reaches the URL
        assert_eq!(search_url("translate", "hello", Some("x&y"), Some("evil")).unwrap(), "https://translate.google.com/?sl=auto&tl=iw&text=hello");
    }

    #[test]
    fn an_address_never_outgrows_what_may_be_opened() {
        // the longest Hebrew query still fits ...
        let hebrew = "ש".repeat(MAX_QUERY_CHARS);
        let url = search_url("google", &hebrew, None, None).unwrap();
        assert!(url.len() <= MAX_URL_LEN);
        assert_eq!(normalize_web_url(&url).as_deref(), Some(url.as_str()), "what the card offers can be opened");
        // ... four-byte characters do not: no search at all rather than a cut one
        assert_eq!(search_url("google", &"😀".repeat(MAX_QUERY_CHARS), None, None), None);
    }

    #[test]
    fn a_query_cannot_break_out_of_its_parameter() {
        let u = search_url("google", "x&q=evil#frag?y=1 \"'<>", None, None).unwrap();
        assert_eq!(u.matches('?').count(), 1);
        assert_eq!(u.matches('&').count(), 0);
        assert!(!u.contains('#') && !u.contains(' ') && !u.contains('"') && !u.contains('<'));
    }

    #[test]
    fn typed_addresses() {
        assert_eq!(normalize_web_url("https://example.com/a?b=1"), Some("https://example.com/a?b=1".into()));
        assert_eq!(normalize_web_url("HTTP://Example.COM"), Some("http://example.com".into()));
        assert_eq!(normalize_web_url("example.co.il"), Some("https://example.co.il".into()));
        assert_eq!(normalize_web_url("www.ynet.co.il/home"), Some("https://www.ynet.co.il/home".into()));
        assert_eq!(normalize_web_url("example.com:8080/x"), Some("https://example.com:8080/x".into()));
        assert_eq!(normalize_web_url("http://192.168.1.1/admin"), Some("http://192.168.1.1/admin".into()));
        assert_eq!(normalize_web_url("https://example.com/שלום"), Some("https://example.com/%D7%A9%D7%9C%D7%95%D7%9D".into()));
        // never anything else than http(s)
        for bad in [
            "javascript:alert(1)",
            "file:///C:/Windows/system32/cmd.exe",
            "ms-settings:network",
            "mailto:a@b.com",
            "ftp://example.com",
            "\\\\server\\share",
            "C:\\Windows\\notepad.exe",
            "c:/windows/notepad.exe",
            "https://user:pw@example.com",
            "https://example.com/a b",
            "https://",
            "https://localhost/x",
            "https://exa mple.com",
            "https://-bad.com",
            "https://bad..com",
            "https://example.com:99999",
            "report.pdf",
            "setup.exe",
            "budget.xlsx",
            "archive.zip",
            "main.rs",
            "3.5",
            "10.0.0.1",
            "example",
            "",
            "   ",
            "https://exämple.com",
            "data:text/html,<script>",
        ] {
            assert_eq!(normalize_web_url(bad), None, "{bad}");
        }
        assert_eq!(normalize_web_url(&format!("https://example.com/{}", "a".repeat(MAX_URL_LEN))), None);
    }

    #[test]
    fn hosts_for_display() {
        assert_eq!(url_host("https://www.google.com/search?q=x"), Some("www.google.com".into()));
        assert_eq!(url_host("http://example.com:8080/a"), Some("example.com".into()));
        assert_eq!(url_host("nonsense"), None);
    }

    #[test]
    fn every_site_url_is_a_safe_https_address() {
        let mut keys = std::collections::HashSet::new();
        for s in SITES {
            assert!(keys.insert(s.key), "duplicate site {}", s.key);
            assert!(s.url.starts_with("https://"), "{}", s.key);
            assert_eq!(normalize_web_url(s.url).as_deref(), Some(s.url), "{}", s.key);
            assert!(url_host(s.url).is_some());
            assert!(!s.he.is_empty() && !s.en.is_empty());
        }
        for key in ["ynet", "walla", "mako", "haaretz", "youtube", "gmail", "gdrive", "gcalendar", "gmaps", "waze", "wikipedia", "facebook", "instagram", "linkedin", "x", "whatsapp-web", "netflix", "spotify-web", "outlook-web", "office", "teams-web", "chatgpt", "github"] {
            assert!(site_by_key(key).is_some(), "{key}");
        }
    }

    #[test]
    fn the_settings_table_is_valid_on_windows_10() {
        let mut keys = std::collections::HashSet::new();
        for s in SETTINGS {
            assert!(keys.insert(s.key), "duplicate setting {}", s.key);
            assert!(s.win10, "{} is not valid on Windows 10 22H2 and must not be offered", s.key);
            assert!(!s.he.is_empty() && !s.en.is_empty());
            match s.kind {
                SettingKind::Uri => {
                    assert!(s.uri.starts_with("ms-settings:"), "{}", s.key);
                    assert!(DOC_WIN10_URIS.contains(&s.uri), "{} ({}) is not in the Microsoft reference list", s.key, s.uri);
                    assert!(s.uri.bytes().all(|b| b.is_ascii_lowercase() || b == b':' || b == b'-'), "{}", s.uri);
                }
                SettingKind::ControlPanel => assert!(s.uri.is_empty()),
                SettingKind::ScreenClip => assert_eq!(s.uri, "ms-screenclip:"),
            }
        }
        // every page the task names has an entry
        for key in [
            "wifi", "bluetooth", "display", "sound", "printers", "update", "vpn", "proxy", "battery", "power", "background", "themes", "colors", "datetime", "language",
            "keyboard", "mouse", "notifications", "apps", "defaultapps", "storage", "privacy", "camera", "microphone", "project", "home", "control-panel",
        ] {
            assert!(setting_by_key(key).is_some(), "{key}");
        }
        assert_eq!(setting_by_key("wifi").unwrap().uri, "ms-settings:network-wifi");
        assert_eq!(setting_by_key("update").unwrap().uri, "ms-settings:windowsupdate");
        assert_eq!(setting_by_key("camera").unwrap().uri, "ms-settings:privacy-webcam");
        // an unknown key falls back to the Settings home page
        assert_eq!(setting_or_home("no-such-page").uri, "ms-settings:");
        assert_eq!(setting_or_home("").uri, "ms-settings:");
        assert_eq!(setting_or_home("wifi").key, "wifi");
    }

    #[test]
    fn folders_resolve_through_the_shell() {
        let mut keys = std::collections::HashSet::new();
        for f in FOLDERS {
            assert!(keys.insert(f.key));
        }
        assert_eq!(resolve_folder("recycle"), Some(FolderTarget::Shell("shell:RecycleBinFolder")));
        assert_eq!(resolve_folder("thispc"), Some(FolderTarget::Shell("shell:MyComputerFolder")));
        assert_eq!(resolve_folder("no-such-folder"), None);
        assert_eq!(resolve_folder("C:\\Windows"), None);
        assert_eq!(resolve_folder("../x"), None);
        for key in ["downloads", "documents", "desktop", "pictures", "music", "videos"] {
            // present on every normal profile; when the shell reports one it is an absolute directory
            if let Some(FolderTarget::Path(p)) = resolve_folder(key) {
                assert!(p.is_absolute() && p.is_dir(), "{key}");
            }
        }
        let downloads = resolve_folder("downloads").expect("the Downloads folder exists on a normal profile");
        assert!(matches!(downloads, FolderTarget::Path(_)));
        let documents = resolve_folder("documents").expect("Documents");
        assert_ne!(downloads, documents);
    }

    #[test]
    fn the_effects_refuse_what_the_tables_do_not_know() {
        assert!(open_url("file:///C:/Windows/system32/cmd.exe").unwrap_err().starts_with("APP-050"));
        assert!(open_url("javascript:alert(1)").unwrap_err().starts_with("APP-050"));
        assert!(open_url("example.com").unwrap_err().starts_with("APP-050"), "only an already-normalised address is opened");
        assert!(open_folder("C:\\Windows").unwrap_err().starts_with("APP-052"));
        assert!(open_folder("").unwrap_err().starts_with("APP-052"));
    }

    #[test]
    fn this_module_has_no_code_that_sends_mail() {
        // Only the code above the tests is looked at, and the forbidden names are built at run time.
        let src = include_str!("system_actions.rs");
        let code = src.split("mod tests {").next().unwrap();
        for forbidden in [format!("\"{}\"", "Send"), format!("{}AndSave", "Send"), format!("\"{}Mail\"", "Send"), format!("\"{}\"", "Submit")] {
            assert!(!code.contains(&forbidden), "{forbidden}");
        }
        // the one Outlook call that shows the window
        assert!(code.contains("\"Display\""));
    }
}
