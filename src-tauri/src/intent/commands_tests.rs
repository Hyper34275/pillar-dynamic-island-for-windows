//! Tests of the explicit commands (`intent::commands`): Hebrew and English phrasings with the decision,
//! the capability and the extracted query / site / setting / folder, and the sentences that must NOT
//! become commands. Every table runs through `interpret` (the real entry point, so the early return in
//! `interpret` is covered too) with a fixed clock.

use super::*;
use crate::intent::{interpret, Ctx, Decision, Interpretation, Known};
use crate::local::system_actions as sys;
use chrono::{NaiveDateTime, TimeZone};

fn now() -> DateTime<Local> {
    let ndt = NaiveDateTime::parse_from_str("2026-10-08T09:00", "%Y-%m-%dT%H:%M").unwrap();
    Local.from_local_datetime(&ndt).earliest().unwrap()
}

fn run(text: &str) -> Interpretation {
    interpret(text, &Ctx::default(), now(), &Known::default())
}

/// What a case expects. Fields left `None` are not checked.
#[derive(Default, Clone)]
struct Want {
    engine: Option<&'static str>,
    query: Option<&'static str>,
    site: Option<&'static str>,
    url: Option<&'static str>,
    setting: Option<&'static str>,
    folder: Option<&'static str>,
    to: Option<&'static str>,
    subject: Option<&'static str>,
    lang_to: Option<&'static str>,
    lang_from: Option<&'static str>,
    /// The subject must be absent.
    no_subject: bool,
    no_to: bool,
}

fn check(text: &str, cap: CapId, want: &Want, errors: &mut Vec<String>) {
    let i = run(text);
    if i.decision != (Decision::Confirm { cap }) {
        errors.push(format!("{text:?}: decision {:?} (wanted Confirm {})", i.decision, cap.as_str()));
        return;
    }
    let s = &i.slots;
    let mut bad = |what: &str, got: Option<&str>, w: Option<&str>| {
        if let Some(w) = w {
            if got != Some(w) {
                errors.push(format!("{text:?}: {what} {got:?} (wanted {w:?})"));
            }
        }
    };
    bad("engine", s.engine.as_deref(), want.engine);
    bad("query", s.query.as_deref(), want.query);
    bad("site", s.site.as_deref(), want.site);
    bad("url", s.url.as_deref(), want.url);
    bad("setting", s.setting.as_deref(), want.setting);
    bad("folder", s.folder.as_deref(), want.folder);
    bad("to", s.mail_to.as_deref(), want.to);
    bad("subject", s.mail_subject.as_deref(), want.subject);
    bad("lang_to", s.lang_to.as_deref(), want.lang_to);
    bad("lang_from", s.lang_from.as_deref(), want.lang_from);
    if want.no_subject && s.mail_subject.is_some() {
        errors.push(format!("{text:?}: subject {:?} (wanted none)", s.mail_subject));
    }
    if want.no_to && s.mail_to.is_some() {
        errors.push(format!("{text:?}: to {:?} (wanted none)", s.mail_to));
    }
    assert!(i.lang == detect_lang(text));
    assert!(!i.follow_up);
}

fn finish(errors: Vec<String>) {
    assert!(errors.is_empty(), "{} mismatches:\n{}", errors.len(), errors.join("\n"));
}

// ---- tables -------------------------------------------------------------------------------------

/// (text, engine, query)
const SEARCH: &[(&str, &str, &str)] = &[
    // ---- Hebrew: Google ----
    ("תחפש בגוגל חתולים", "google", "חתולים"),
    ("חפש בגוגל חתולים", "google", "חתולים"),
    ("תחפשי בגוגל מתכון לעוגת שוקולד", "google", "מתכון לעוגת שוקולד"),
    ("חפשי בגוגל מתכון לעוגת שוקולד", "google", "מתכון לעוגת שוקולד"),
    ("לחפש בגוגל מתכון לעוגת שוקולד", "google", "מתכון לעוגת שוקולד"),
    ("תחפש לי בגוגל מזג אוויר בחיפה", "google", "מזג אוויר בחיפה"),
    ("בבקשה תחפש בגוגל מסעדות בתל אביב", "google", "מסעדות בתל אביב"),
    ("תוכל לחפש בגוגל איך מכינים פיצה", "google", "איך מכינים פיצה"),
    ("אפשר לחפש בגוגל מחיר זהב", "google", "מחיר זהב"),
    ("אתה יכול לחפש בגוגל חתולים", "google", "חתולים"),
    ("אני רוצה לחפש בגוגל חתולים", "google", "חתולים"),
    ("תגגל חתולים חמודים", "google", "חתולים חמודים"),
    ("גגל מחיר דולר", "google", "מחיר דולר"),
    ("תגגלי מתכון לשקשוקה", "google", "מתכון לשקשוקה"),
    ("גוגל מזג אוויר", "google", "מזג אוויר"),
    ("גוגל: מה זה בינה מלאכותית", "google", "מה זה בינה מלאכותית"),
    ("תחפש באינטרנט מתכון לפיצה", "google", "מתכון לפיצה"),
    ("תחפש ברשת חדשות היום", "google", "חדשות היום"),
    ("חפש באינטרנט איך לשנות צמיג", "google", "איך לשנות צמיג"),
    ("אפשר לחפש ברשת איך מכינים פיצה", "google", "איך מכינים פיצה"),
    ("תחפש מתכון לפיצה בגוגל", "google", "מתכון לפיצה"),
    ("חפש מתכון לפיצה בגוגל בבקשה", "google", "מתכון לפיצה"),
    ("בגוגל תחפש חתולים", "google", "חתולים"),
    ("תחפש בגוגל \"הצעת מחיר\"", "google", "הצעת מחיר"),
    ("תחפש בגוגל את המילה פרדוקס", "google", "המילה פרדוקס"),
    ("חיפוש בגוגל: נגן מוזיקה", "google", "נגן מוזיקה"),
    ("תבדוק בגוגל כמה זה 5 דולר בשקלים", "google", "כמה זה 5 דולר בשקלים"),
    ("תמצא לי בגוגל קורס אקסל", "google", "קורס אקסל"),
    ("תחפש ב-גוגל חתולים", "google", "חתולים"),
    ("תחפש בגוגל חתולים.", "google", "חתולים"),
    ("תחפש בגוגל שער הדולר היום?", "google", "שער הדולר היום?"),
    ("תחפש בגוגל מה השעה בניו יורק", "google", "מה השעה בניו יורק"),
    ("תחפש בגוגל Python tutorial", "google", "Python tutorial"),
    ("תחפש בגוגל מי זכה באירוויזיון", "google", "מי זכה באירוויזיון"),
    ("תפתח בגוגל חיפוש של חתולים", "google", "חיפוש של חתולים"),
    // ---- Hebrew: YouTube ----
    ("תחפש ביוטיוב שירים של עומר אדם", "youtube", "שירים של עומר אדם"),
    ("חפש ביוטיוב מוזיקה רגועה", "youtube", "מוזיקה רגועה"),
    ("שים ביוטיוב מוזיקה רגועה", "youtube", "מוזיקה רגועה"),
    ("תשים לי ביוטיוב שיר של שלמה ארצי", "youtube", "שיר של שלמה ארצי"),
    ("תנגן ביוטיוב מוזיקה קלאסית", "youtube", "מוזיקה קלאסית"),
    ("תפעיל ביוטיוב חדשות", "youtube", "חדשות"),
    ("תפתח ביוטיוב סרטונים של חתולים", "youtube", "סרטונים של חתולים"),
    ("תחפש סרטון על בישול ביוטיוב", "youtube", "סרטון על בישול"),
    ("תשמיע מוזיקה רגועה ביוטיוב", "youtube", "מוזיקה רגועה"),
    ("תחפש ביו טיוב הרצאות על היסטוריה", "youtube", "הרצאות על היסטוריה"),
    // ---- Hebrew: Wikipedia, Bing, Maps, Waze ----
    ("תחפש בויקיפדיה אלברט איינשטיין", "wikipedia", "אלברט איינשטיין"),
    ("חפש בוויקיפדיה על ירושלים", "wikipedia", "ירושלים"),
    ("תחפש ירושלים בויקיפדיה", "wikipedia", "ירושלים"),
    ("תחפש בבינג חתולים", "bing", "חתולים"),
    ("איפה נמצא הכותל", "maps", "הכותל"),
    ("איפה נמצאת תל אביב", "maps", "תל אביב"),
    ("איפה נמצאים הרי האלפים", "maps", "הרי האלפים"),
    ("איפה נמצא מגדל אייפל?", "maps", "מגדל אייפל?"),
    ("מפה של תל אביב", "maps", "תל אביב"),
    ("מפת ישראל", "maps", "ישראל"),
    ("תראה לי מפה של חיפה", "maps", "חיפה"),
    ("תנווט לתל אביב", "maps", "תל אביב"),
    ("נווט לבית החולים איכילוב", "maps", "בית החולים איכילוב"),
    ("תנווט אותי לחיפה", "maps", "חיפה"),
    ("תנווט ללונדון", "maps", "לונדון"),
    ("תנווט ל-אילת", "maps", "אילת"),
    ("הוראות הגעה לים המלח", "maps", "ים המלח"),
    ("איך מגיעים לאילת", "maps", "אילת"),
    ("איך מגיעים אל הכנרת", "maps", "הכנרת"),
    ("תחפש במפות מסעדות בחיפה", "maps", "מסעדות בחיפה"),
    ("תחפש בגוגל מפות פיצה", "maps", "פיצה"),
    ("תחפש פיצה בגוגל מפות", "maps", "פיצה"),
    ("תראה לי את חיפה במפה", "maps", "חיפה"),
    ("תנווט בווייז לתל אביב", "waze", "תל אביב"),
    ("תחפש בווייז קניון עזריאלי", "waze", "קניון עזריאלי"),
    // ---- English ----
    ("search google for cats", "google", "cats"),
    ("google cats", "google", "cats"),
    ("google how to tie a tie", "google", "how to tie a tie"),
    ("can you google cats", "google", "cats"),
    ("please search google for cats", "google", "cats"),
    ("search the web for pizza recipes", "google", "pizza recipes"),
    ("search for pizza recipes on google", "google", "pizza recipes"),
    ("look up pizza recipes on google", "google", "pizza recipes"),
    ("find me a pizza place on google", "google", "a pizza place"),
    ("search online for used cars", "google", "used cars"),
    ("search bing for cats", "bing", "cats"),
    ("search youtube for lofi music", "youtube", "lofi music"),
    ("search on youtube for cats", "youtube", "cats"),
    ("play lofi music on youtube", "youtube", "lofi music"),
    ("look up Albert Einstein on wikipedia", "wikipedia", "Albert Einstein"),
    ("search wikipedia for Albert Einstein", "wikipedia", "Albert Einstein"),
    ("show me pizza places on google maps", "maps", "pizza places"),
    ("navigate to Tel Aviv", "maps", "Tel Aviv"),
    ("directions to the Western Wall", "maps", "the Western Wall"),
    ("how do I get to Haifa", "maps", "Haifa"),
    ("where is the Western Wall", "maps", "the Western Wall"),
    ("where's the Eiffel Tower?", "maps", "the Eiffel Tower?"),
    ("map of Israel", "maps", "Israel"),
    ("i want to search youtube for cats", "youtube", "cats"),
    ("hey, search google for the news", "google", "the news"),
    ("search google for \"red panda\"", "google", "red panda"),
];

/// (text, query, target language, source language)
const TRANSLATE: &[(&str, &str, &str, Option<&str>)] = &[
    ("תתרגם שלום", "שלום", "en", None),
    ("תרגם שלום לאנגלית", "שלום", "en", None),
    ("תרגם לאנגלית שלום עולם", "שלום עולם", "en", None),
    ("תרגם לי hello", "hello", "iw", None),
    ("תרגמי thank you", "thank you", "iw", None),
    ("אפשר לתרגם את המילה apple", "המילה apple", "en", None),
    ("איך אומרים תפוח באנגלית", "תפוח", "en", None),
    ("איך אומרים באנגלית ספר", "ספר", "en", None),
    ("איך אומרים תודה ברוסית", "תודה", "ru", None),
    ("מה זה apple בעברית", "apple", "iw", None),
    ("תתרגם מעברית לרוסית תודה רבה", "תודה רבה", "ru", Some("iw")),
    ("תתרגם מאנגלית לעברית good morning", "good morning", "iw", Some("en")),
    ("תרגם לערבית בוקר טוב", "בוקר טוב", "ar", None),
    ("translate hello", "hello", "iw", None),
    ("translate שלום", "שלום", "en", None),
    ("translate hello to hebrew", "hello", "iw", None),
    ("translate hello into french", "hello", "fr", None),
    ("translate good morning to spanish", "good morning", "es", None),
    ("how do you say apple in hebrew", "apple", "iw", None),
    ("how to say thank you in german", "thank you", "de", None),
    ("what is dog in french", "dog", "fr", None),
    ("translate from english to hebrew good morning", "good morning", "iw", Some("en")),
    ("please translate this to english: שלום עולם", "this to english: שלום עולם", "en", None),
];

/// (text, site key)
const SITES: &[(&str, &str)] = &[
    ("תפתח את ynet", "ynet"),
    ("פתח את ynet", "ynet"),
    ("תפתח ynet", "ynet"),
    ("פתח את אתר ynet", "ynet"),
    ("תכנס ל-ynet", "ynet"),
    ("כנס לאתר ynet", "ynet"),
    ("תפתח לי את ynet בבקשה", "ynet"),
    ("אפשר לפתוח את ynet", "ynet"),
    ("פתח יוטיוב", "youtube"),
    ("תפתח את יוטיוב", "youtube"),
    ("פתח את YouTube", "youtube"),
    ("תפתח את וואלה", "walla"),
    ("תפתח את מאקו", "mako"),
    ("תפתח את הארץ", "haaretz"),
    ("פתח את כאן", "kan"),
    ("פתח את כלכליסט", "calcalist"),
    ("פתח את גלובס", "globes"),
    ("פתח את ישראל היום", "israelhayom"),
    ("פתח את מעריב", "maariv"),
    ("פתח את יד2", "yad2"),
    ("פתח פייסבוק", "facebook"),
    ("תפתח את האינסטגרם", "instagram"),
    ("תכנס ללינקדאין", "linkedin"),
    ("פתח לינקדאין", "linkedin"),
    ("תפתח את טוויטר", "x"),
    ("פתח את נטפליקס", "netflix"),
    ("פתח את הנטפליקס", "netflix"),
    ("כנס לגוגל דרייב", "gdrive"),
    ("תפתח את גוגל דרייב", "gdrive"),
    ("תפתח את ג'ימייל", "gmail"),
    ("פתח לי את הג'ימייל", "gmail"),
    ("תפתח את gmail", "gmail"),
    ("תפתח את יומן גוגל", "gcalendar"),
    ("תפתח את גוגל קלנדר", "gcalendar"),
    ("תפתח את גוגל מפות", "gmaps"),
    ("תפתח את גוגל", "google"),
    ("פתח גוגל", "google"),
    ("תפתח את וייז", "waze"),
    ("פתח את ויקיפדיה", "wikipedia"),
    ("פתח את הוויקיפדיה", "wikipedia"),
    ("תפתח וואטסאפ ווב", "whatsapp-web"),
    ("תפתח את האתר של וואטסאפ", "whatsapp-web"),
    ("תפתח את וואטסאפ אונליין", "whatsapp-web"),
    ("תפתח את ספוטיפיי ווב", "spotify-web"),
    ("פתח אאוטלוק ווב", "outlook-web"),
    ("פתח אאוטלוק אונליין", "outlook-web"),
    ("פתח אופיס אונליין", "office"),
    ("פתח את מיקרוסופט 365", "office"),
    ("פתח טימס ווב", "teams-web"),
    ("פתח צ'אט ג'יפיטי", "chatgpt"),
    ("תפתח את גיטהאב", "github"),
    ("תפתח את ביטוח לאומי", "btl"),
    ("תפתח את בנק לאומי", "leumi"),
    ("תפתח את בנק הפועלים", "hapoalim"),
    ("תפתח את בנק דיסקונט", "discount"),
    ("פתח את gov.il", "govil"),
    ("פתח את אמזון", "amazon"),
    ("open ynet", "ynet"),
    ("open gmail", "gmail"),
    ("open youtube", "youtube"),
    ("go to ynet", "ynet"),
    ("visit github", "github"),
    ("open google drive", "gdrive"),
    ("open google", "google"),
    ("open the facebook site", "facebook"),
    ("open whatsapp web", "whatsapp-web"),
    ("open the whatsapp website", "whatsapp-web"),
    ("open outlook on the web", "outlook-web"),
    ("can you open netflix", "netflix"),
    ("launch youtube please", "youtube"),
    ("browse to wikipedia", "wikipedia"),
    ("please open linkedin", "linkedin"),
];

/// (text, normalised address)
const URLS: &[(&str, &str)] = &[
    ("פתח את example.co.il", "https://example.co.il"),
    ("תפתח את example.com", "https://example.com"),
    ("תכנס ל-example.com/path", "https://example.com/path"),
    ("תפתח את האתר example.com", "https://example.com"),
    ("תפתח את https://www.example.com/a?q=1", "https://www.example.com/a?q=1"),
    ("פתח http://intranet.example.org", "http://intranet.example.org"),
    ("open example.co.il", "https://example.co.il"),
    ("open https://example.com/a?b=1", "https://example.com/a?b=1"),
    ("go to www.example.org", "https://www.example.org"),
];

/// (text, setting key)
const SETTINGS: &[(&str, &str)] = &[
    ("פתח הגדרות", "home"),
    ("תפתח את ההגדרות", "home"),
    ("תפתח את הגדרות Windows", "home"),
    ("פתח הגדרות wifi", "wifi"),
    ("תפתח את הגדרות ה-wifi", "wifi"),
    ("פתח רשת אלחוטית", "wifi"),
    ("תחבר אותי לווייפיי", "wifi"),
    ("פתח הגדרות רשת", "network"),
    ("פתח מצב טיסה", "airplane"),
    ("פתח הגדרות בלוטוס", "bluetooth"),
    ("תפתח בלוטוס", "bluetooth"),
    ("תדליק בלוטוס", "bluetooth"),
    ("הגדרות תצוגה", "display"),
    ("פתח הגדרות מסך", "display"),
    ("תפתח את הרזולוציה", "display"),
    ("תשנה את הרזולוציה", "display"),
    ("תפתח מסך נוסף", "project"),
    ("תפתח הקרנה", "project"),
    ("הגדרות הצליל", "sound"),
    ("תפתח את השמע", "sound"),
    ("פתח רמקולים", "sound"),
    ("תפתח מדפסות", "printers"),
    ("פתח את המדפסת", "printers"),
    ("תפתח עדכוני ווינדוס", "update"),
    ("פתח windows update", "update"),
    ("תפתח עדכונים", "update"),
    ("תפתח vpn", "vpn"),
    ("פתח הגדרות פרוקסי", "proxy"),
    ("תפתח את הסוללה", "battery"),
    ("פתח צריכת חשמל", "power"),
    ("תחליף רקע", "background"),
    ("תשנה את הרקע", "background"),
    ("פתח הגדרות רקע", "background"),
    ("תפתח ערכות נושא", "themes"),
    ("פתח צבעים", "colors"),
    ("תפתח תאריך ושעה", "datetime"),
    ("תשנה את השפה", "language"),
    ("פתח הגדרות שפה", "language"),
    ("תפתח את המקלדת", "keyboard"),
    ("פתח הגדרות עכבר", "mouse"),
    ("פתח את ההתראות", "notifications"),
    ("תפתח אפליקציות", "apps"),
    ("פתח אפליקציות ברירת מחדל", "defaultapps"),
    ("פתח אחסון", "storage"),
    ("תפתח פרטיות", "privacy"),
    ("פתח הגדרות מצלמה", "camera"),
    ("פתח הרשאות מיקרופון", "microphone"),
    ("פתח מיקרופון", "microphone"),
    ("תפתח את מרכז הפעולות", "notifications"),
    ("פתח את לוח הבקרה", "control-panel"),
    ("תפתח לוח בקרה", "control-panel"),
    ("פתח כלי חיתוך", "screenclip"),
    ("תפתח צילום מסך", "screenclip"),
    ("open settings", "home"),
    ("open wifi settings", "wifi"),
    ("open bluetooth settings", "bluetooth"),
    ("open display settings", "display"),
    ("open sound settings", "sound"),
    ("open windows update", "update"),
    ("open control panel", "control-panel"),
    ("open snipping tool", "screenclip"),
    ("turn on bluetooth", "bluetooth"),
    ("change the wallpaper", "background"),
    ("open privacy settings", "privacy"),
    ("show me the printers", "printers"),
    ("open vpn settings", "vpn"),
    ("open battery settings", "battery"),
];

/// (text, folder key)
const FOLDERS: &[(&str, &str)] = &[
    ("פתח הורדות", "downloads"),
    ("תפתח את ההורדות", "downloads"),
    ("פתח את תיקיית ההורדות", "downloads"),
    ("תראה לי את תיקיית ההורדות", "downloads"),
    ("תכנס להורדות", "downloads"),
    ("תפתח מסמכים", "documents"),
    ("פתח את המסמכים שלי", "documents"),
    ("תפתח את תיקיית המסמכים", "documents"),
    ("תפתח את שולחן העבודה", "desktop"),
    ("תפתח תמונות", "pictures"),
    ("פתח את תיקיית התמונות", "pictures"),
    ("תפתח מוזיקה", "music"),
    ("פתח סרטונים", "videos"),
    ("תפתח את סל המחזור", "recycle"),
    ("פתח את המחשב הזה", "thispc"),
    ("תפתח את המחשב שלי", "thispc"),
    ("תפתח OneDrive", "onedrive"),
    ("פתח וואנדרייב", "onedrive"),
    ("open downloads", "downloads"),
    ("open the downloads folder", "downloads"),
    ("could you open the downloads folder please", "downloads"),
    ("open my documents", "documents"),
    ("open desktop", "desktop"),
    ("open pictures", "pictures"),
    ("open recycle bin", "recycle"),
    ("open this pc", "thispc"),
    ("open onedrive", "onedrive"),
];

/// (text, note text)
const NOTES: &[(&str, &str)] = &[
    ("תרשום פתק: לקנות חלב", "לקנות חלב"),
    ("תרשום פתק לקנות חלב", "לקנות חלב"),
    ("תרשום לי פתק: להתקשר לדני", "להתקשר לדני"),
    ("תוסיף פתק לקנות לחם", "לקנות לחם"),
    ("תוסיף פתק חדש: לשלם חשבון חשמל", "לשלם חשבון חשמל"),
    ("פתק חדש: לקנות חלב", "לקנות חלב"),
    ("תיצור פתק לקנות ביצים", "לקנות ביצים"),
    ("תכתוב פתק: לא לשכוח את הפגישה", "לא לשכוח את הפגישה"),
    ("רשום פתק לקנות פירות", "לקנות פירות"),
    ("תשמור פתק: הסיסמה בדלת", "הסיסמה בדלת"),
    ("תוסיף הערה: לבדוק את החשבונית", "לבדוק את החשבונית"),
    ("תרשום פתק שאומר לקנות חלב", "לקנות חלב"),
    ("בבקשה תרשום פתק: לקנות חלב, לחם וביצים", "לקנות חלב, לחם וביצים"),
    ("take a note: buy milk", "buy milk"),
    ("note: buy milk", "buy milk"),
    ("add a note buy bread", "buy bread"),
    ("create a new note: call Dan", "call Dan"),
    ("new note call Dan", "call Dan"),
    ("write a note: pick up the kids", "pick up the kids"),
    ("make a note to self: renew the passport", "renew the passport"),
    ("please add a note: water the plants", "water the plants"),
];

/// (text, recipient, subject)
const COMPOSE: &[(&str, Option<&str>, Option<&str>)] = &[
    ("תכתוב מייל לדני בנושא תקציב", Some("דני"), Some("תקציב")),
    ("תכתוב מייל לדני", Some("דני"), None),
    ("תכתוב לדני מייל", Some("דני"), None),
    ("תכתוב מייל חדש לדנה", Some("דנה"), None),
    ("תכין מייל לדנה בנושא פגישה", Some("דנה"), Some("פגישה")),
    ("תכתוב לי מייל ליובל", Some("יובל"), None),
    ("תנסח מייל לדני בנושא הצעת מחיר", Some("דני"), Some("הצעת מחיר")),
    ("תכתוב מייל לדני כהן בנושא חופשה", Some("דני כהן"), Some("חופשה")),
    ("תכתוב מייל לדני ולדנה בנושא ישיבה", Some("דני; דנה"), Some("ישיבה")),
    ("תכתוב מייל ל-דני בנושא תקציב", Some("דני"), Some("תקציב")),
    ("תכתוב מייל אל דני בנושא תקציב", Some("דני"), Some("תקציב")),
    ("תכתוב מייל לדני על הפגישה של מחר", Some("דני"), Some("הפגישה של מחר")),
    ("תכתוב מייל לדני שאני מאחר", Some("דני"), Some("שאני מאחר")),
    ("תכתוב מייל ל-dana@example.com בנושא שלום", Some("dana@example.com"), Some("שלום")),
    ("תכתוב מייל לליאור בנושא סיכום", Some("ליאור"), Some("סיכום")),
    ("תפתח מייל חדש", None, None),
    ("תפתח מייל חדש לדני", Some("דני"), None),
    ("תכתוב מייל", None, None),
    ("אפשר לכתוב מייל לדני בנושא תקציב", Some("דני"), Some("תקציב")),
    ("write an email to Dana about the budget", Some("Dana"), Some("the budget")),
    ("compose an email to Dana", Some("Dana"), None),
    ("email Dana about the budget", Some("Dana"), Some("the budget")),
    ("draft an email to dana@example.com subject Hello", Some("dana@example.com"), Some("Hello")),
    ("new email to Dan", Some("Dan"), None),
    ("write a new email", None, None),
    ("can you write an email to Dan regarding the invoice", Some("Dan"), Some("the invoice")),
];

/// Texts that lock the PC.
const LOCK: &[&str] = &[
    "תנעל את המחשב",
    "נעל את המחשב",
    "תנעל את המסך",
    "תנעלי את המחשב",
    "נעילת מסך",
    "אפשר לנעול את המחשב",
    "תנעל את המחשב בבקשה",
    "תנעל",
    "lock",
    "lock the computer",
    "lock my pc",
    "lock screen",
    "lock workstation",
    "please lock the computer",
];

/// Texts a command verb is in but that are NOT commands of this file (they stay with the normal engine).
const NOT_COMMANDS: &[&str] = &[
    // calendar / mail questions that mention Google
    "מה יש לי ביומן",
    "המייל מגוגל",
    "פגישה עם גוגל מחר",
    "תמצא את המייל מגוגל",
    "תחפש את המייל מגוגל",
    "מה יש לאיציק ביומן מחר?",
    "מייל מגוגל מאתמול",
    "show me the mail from Google",
    "meeting with Google tomorrow",
    "find the email from google about the invoice",
    // local objects
    "תפתח את המייל האחרון",
    "פתח את הקובץ של החשבונית",
    "פתח את הפתק האחרון",
    "פתח את הפגישה של מחר",
    "תמצא את הקובץ מחר",
    "תחפש מסמך על תקציב",
    "הצג את הפתקים שלי",
    "find the email with the word budget",
    "show me my meetings",
    "search my mail for invoices",
    "open the file budget",
    // programs: the app launcher
    "פתח אקסל",
    "הפעל מחשבון",
    "open Word",
    "פתח וואטסאפ",
    "open whatsapp",
    "open outlook",
    "פתח אאוטלוק",
    "פתח מצלמה",
    "תפתח את הצייר",
    "launch notepad",
    "תריץ ספוטיפיי",
    // where is / maps on local things
    "איפה נמצא הקובץ של דני",
    "where is my meeting",
    "where is the budget file",
    "איפה נמצאת הפגישה של מחר",
    // not enough to be a command
    "גוגל",
    "google",
    "ynet",
    "יוטיוב",
    "lock the door",
    "נעל את הדלת",
    "תנעל את הדלת",
    "פתח רשת",
    "מה השעה",
    "שלום",
    "hello",
    "תפתח",
    "google drive",
    "גוגל מפות",
    "תרשום",
    "ציפור",
    "הגדרות של דני",
    // unsafe
    "תמחק את כל המיילים",
    "OPEN 'C:\\Windows\\system32\\cmd.exe'",
    "run setup.exe",
    "הפעל cmd",
    "פתח C:\\Windows\\notepad.exe",
    "open file:///C:/Windows/system32/cmd.exe",
    "פתח javascript:alert(1)",
    "open ms-settings:network",
    "open \\\\server\\share",
    "תפתח report.pdf",
    "פתח budget.xlsx",
    "open setup.exe",
    "שלח מייל לדנה שאני מאחר",
    "תשלח מייל ליובל",
    "send an email to Dana",
    "תבטל את הפגישה של מחר",
    "delete all notes",
    "תקבע לי פגישה מחר",
    "format c:",
    "download invoice pdf",
];

fn want() -> Want {
    Want::default()
}

// ---- tests --------------------------------------------------------------------------------------

#[test]
fn web_search_phrasings() {
    let mut errors = Vec::new();
    for (text, engine, query) in SEARCH {
        check(text, caps::WEB_SEARCH, &Want { engine: Some(engine), query: Some(query), ..want() }, &mut errors);
    }
    finish(errors);
}

#[test]
fn translate_phrasings() {
    let mut errors = Vec::new();
    for (text, query, to, from) in TRANSLATE {
        check(text, caps::WEB_SEARCH, &Want { engine: Some("translate"), query: Some(query), lang_to: Some(to), lang_from: *from, ..want() }, &mut errors);
    }
    finish(errors);
}

#[test]
fn website_phrasings() {
    let mut errors = Vec::new();
    for (text, site) in SITES {
        check(text, caps::WEB_OPEN, &Want { site: Some(site), ..want() }, &mut errors);
    }
    for (text, url) in URLS {
        check(text, caps::WEB_OPEN, &Want { url: Some(url), ..want() }, &mut errors);
    }
    finish(errors);
}

#[test]
fn settings_phrasings() {
    let mut errors = Vec::new();
    for (text, key) in SETTINGS {
        check(text, caps::SYSTEM_OPEN_SETTINGS, &Want { setting: Some(key), ..want() }, &mut errors);
    }
    finish(errors);
}

#[test]
fn folder_phrasings() {
    let mut errors = Vec::new();
    for (text, key) in FOLDERS {
        check(text, caps::FOLDERS_OPEN, &Want { folder: Some(key), ..want() }, &mut errors);
    }
    finish(errors);
}

#[test]
fn note_phrasings() {
    let mut errors = Vec::new();
    for (text, body) in NOTES {
        check(text, caps::NOTES_CREATE, &Want { query: Some(body), ..want() }, &mut errors);
    }
    finish(errors);
}

#[test]
fn compose_phrasings() {
    let mut errors = Vec::new();
    for (text, to, subject) in COMPOSE {
        let w = Want { to: *to, subject: *subject, no_to: to.is_none(), no_subject: subject.is_none(), ..want() };
        check(text, caps::MAIL_COMPOSE, &w, &mut errors);
    }
    finish(errors);
}

#[test]
fn lock_phrasings() {
    let mut errors = Vec::new();
    for text in LOCK {
        check(text, caps::SYSTEM_LOCK, &want(), &mut errors);
    }
    finish(errors);
}

#[test]
fn things_that_are_not_commands_stay_with_the_normal_engine() {
    let mut errors = Vec::new();
    for text in NOT_COMMANDS {
        if let Some(i) = detect(text, &Ctx::default(), now(), &Known::default()) {
            errors.push(format!("{text:?}: became {:?}", i.decision));
        }
    }
    finish(errors);
}

#[test]
fn the_corpus_of_phrasings_is_large_enough() {
    let he = |t: &&str| detect_lang(t) == Lang::He;
    let mut texts: Vec<&str> = Vec::new();
    texts.extend(SEARCH.iter().map(|c| c.0));
    texts.extend(TRANSLATE.iter().map(|c| c.0));
    texts.extend(SITES.iter().map(|c| c.0));
    texts.extend(URLS.iter().map(|c| c.0));
    texts.extend(SETTINGS.iter().map(|c| c.0));
    texts.extend(FOLDERS.iter().map(|c| c.0));
    texts.extend(NOTES.iter().map(|c| c.0));
    texts.extend(COMPOSE.iter().map(|c| c.0));
    texts.extend(LOCK.iter().copied());
    let hebrew = texts.iter().filter(|t| he(t)).count();
    let english = texts.len() - hebrew;
    assert!(hebrew >= 150, "Hebrew phrasings: {hebrew}");
    assert!(english >= 50, "English phrasings: {english}");
}

// ---- decisions, clarifications and slots in detail ----------------------------------------------

#[test]
fn every_command_is_a_confirm_and_never_an_execute() {
    for t in ["תחפש בגוגל חתולים", "תפתח את ynet", "פתח הגדרות wifi", "פתח הורדות", "תרשום פתק: x", "תכתוב מייל לדני", "תנעל את המחשב", "translate hello"] {
        let i = run(t);
        assert!(matches!(i.decision, Decision::Confirm { .. }), "{t}: {:?}", i.decision);
        if let Decision::Confirm { cap } = i.decision {
            assert_ne!(crate::intent::sensitivity(cap), Sensitivity::Read, "{t}");
        }
        assert!(i.confidence > 0.9);
    }
}

#[test]
fn a_missing_text_asks_for_it() {
    for (t, cap) in [
        ("תחפש בגוגל", caps::WEB_SEARCH),
        ("חפש ביוטיוב", caps::WEB_SEARCH),
        ("search google", caps::WEB_SEARCH),
        ("תרשום פתק", caps::NOTES_CREATE),
        ("תוסיף פתק חדש", caps::NOTES_CREATE),
        ("take a note", caps::NOTES_CREATE),
        ("תתרגם", caps::WEB_SEARCH),
        ("translate", caps::WEB_SEARCH),
    ] {
        let i = run(t);
        assert_eq!(i.decision, Decision::Clarify { ask: AskKind::Content, cap: Some(cap) }, "{t}");
    }
    // "open youtube" alone is the site, not a question
    assert_eq!(run("פתח ביוטיוב").decision, Decision::Confirm { cap: caps::WEB_OPEN });
}

#[test]
fn the_query_keeps_what_the_user_typed() {
    // case, punctuation inside, Hebrew with niqqud and a mix of both
    let i = run("תחפש בגוגל How To Tie A Tie, step-by-step");
    assert_eq!(i.slots.query.as_deref(), Some("How To Tie A Tie, step-by-step"));
    let i = run("תחפש בגוגל שָׁלוֹם עולם");
    assert_eq!(i.slots.query.as_deref(), Some("שלום עולם"), "niqqud is a mark and is removed by the normaliser");
    let i = run("תחפש   בגוגל    חתולים     חמודים");
    assert_eq!(i.slots.query.as_deref(), Some("חתולים חמודים"));
    let i = run("תחפש בגוגל \"חתולים\"");
    assert_eq!(i.slots.query.as_deref(), Some("חתולים"));
    let i = run("תחפש בגוגל 'a b'");
    assert_eq!(i.slots.query.as_deref(), Some("a b"));
}

#[test]
fn queries_become_safe_urls() {
    let i = run("תחפש בגוגל שלום & להתראות=1 #x");
    let url = sys::search_url(i.slots.engine.as_deref().unwrap(), i.slots.query.as_deref().unwrap(), None, None).unwrap();
    assert_eq!(url.matches('&').count(), 0);
    assert!(url.starts_with("https://www.google.com/search?q="));
    assert!(!url.contains(' ') && !url.contains('#'));
}

#[test]
fn a_site_with_a_search_word_is_still_a_site() {
    assert_eq!(run("פתח את גוגל מפות").slots.site.as_deref(), Some("gmaps"));
    assert_eq!(run("פתח את גוגל").slots.site.as_deref(), Some("google"));
    assert!(run("פתח את גוגל").slots.query.is_none());
}

#[test]
fn compose_never_carries_a_body_or_a_send_flag() {
    let i = run("תכתוב מייל לדני בנושא תקציב");
    assert_eq!(i.decision, Decision::Confirm { cap: caps::MAIL_COMPOSE });
    // the only slots that exist for it
    assert_eq!(i.slots.mail_to.as_deref(), Some("דני"));
    assert_eq!(i.slots.mail_subject.as_deref(), Some("תקציב"));
    assert!(i.slots.query.is_none() && i.slots.engine.is_none() && i.slots.url.is_none());
    // sending is not a command at all
    for t in ["שלח מייל לדני", "תשלח מייל לדני בנושא תקציב", "send an email to Dana about the budget", "send the email"] {
        assert!(detect(t, &Ctx::default(), now(), &Known::default()).is_none(), "{t}");
    }
}

#[test]
fn engines_and_sites_in_the_lexicon_exist_in_the_tables() {
    let l = lex();
    for (cat, map) in [("engines", true), ("sites", false)] {
        let c = l.cats.get(cat).unwrap();
        for key in c.map.values() {
            if map {
                assert!(sys::engine_name(key, true).is_some(), "engine {key} has no table entry");
            } else {
                assert!(sys::site_by_key(key).is_some(), "site {key} has no table entry");
            }
        }
    }
    for key in l.cats["web_sites"].map.values() {
        assert!(sys::site_by_key(key).is_some(), "{key}");
    }
    for key in l.cats["settings"].map.values().chain(l.cats["settings_weak"].map.values()) {
        assert!(sys::setting_by_key(key).is_some(), "setting {key} has no table entry");
    }
    for key in l.cats["folders"].map.values() {
        assert!(sys::folder_by_key(key).is_some(), "folder {key} has no table entry");
    }
    for key in l.cats["languages"].map.values() {
        assert!(sys::language_name(key, true).is_some(), "language {key}");
    }
}

#[test]
fn every_table_entry_can_be_asked_for() {
    let l = lex();
    let named = |cat: &str| -> HashSet<String> { l.cats[cat].map.values().cloned().collect() };
    let sites = named("sites");
    for s in sys::SITES {
        assert!(sites.contains(s.key), "site {} has no spoken name", s.key);
    }
    let mut settings = named("settings");
    settings.extend(named("settings_weak"));
    for s in sys::SETTINGS.iter().filter(|s| s.key != "home") {
        assert!(settings.contains(s.key), "setting {} has no spoken name", s.key);
    }
    let folders = named("folders");
    for f in sys::FOLDERS {
        assert!(folders.contains(f.key), "folder {} has no spoken name", f.key);
    }
    let engines = named("engines");
    for e in sys::ENGINES.iter().filter(|e| e.0 != "translate") {
        assert!(engines.contains(e.0), "engine {} has no spoken name", e.0);
    }
}

#[test]
fn no_spoken_name_points_at_two_things() {
    // the same words under two keys of one catalogue would make the answer depend on map order
    let v: serde_json::Value = serde_json::from_str(LEXICON).unwrap();
    for cat in ["engines", "sites", "web_sites", "settings", "settings_weak", "folders", "languages", "fallback"] {
        let mut seen: HashMap<String, String> = HashMap::new();
        for (key, forms) in v[cat].as_object().unwrap() {
            for f in forms.as_array().unwrap().iter().filter_map(|x| x.as_str()) {
                let folded = fold(f);
                if let Some(prev) = seen.insert(folded.clone(), key.clone()) {
                    assert_eq!(prev, *key, "{cat}: {folded:?} names both {prev} and {key}");
                }
            }
        }
    }
}

#[test]
fn the_lexicon_parses() {
    let v: serde_json::Value = serde_json::from_str(LEXICON).expect("commands.json is valid JSON");
    assert!(v.as_object().unwrap().len() >= 30);
    let l = lex();
    assert!(l.lists["v_search"].len() > 10 && l.cats["sites"].map.len() > 100);
}

#[test]
fn context_does_not_change_a_command() {
    // a follow-up context from a mail search must not swallow an explicit command
    let mut ctx = Ctx::default();
    let first = interpret("תחפש לי את המייל האחרון מיובל", &ctx, now(), &Known::default());
    ctx.remember(&first, now().timestamp_millis());
    let i = interpret("תחפש בגוגל חתולים", &ctx, now() + chrono::Duration::seconds(5), &Known::default());
    assert_eq!(i.decision, Decision::Confirm { cap: caps::WEB_SEARCH });
    assert!(!i.follow_up);
}

#[test]
fn odd_input_never_panics() {
    for t in [
        "",
        " ",
        "תחפש בגוגל",
        "בגוגל",
        "ב-",
        "-",
        ":",
        "תפתח את",
        "תפתח את ב",
        "\u{202E}תחפש בגוגל\u{202C} x",
        "תחפש בגוגל \u{0000}\u{FFFF}",
        "תנווט ל",
        "איפה נמצא",
        "where is",
        "תכתוב מייל ל",
        "email",
        "translate to",
        "תרגם לאנגלית",
        "how do you say in hebrew",
        "ל-",
        "'''",
        "\"\"\"",
        "https://",
        "תפתח https://",
    ] {
        let _ = run(t);
    }
    let long = "תחפש בגוגל ".to_string() + &"חתולים ".repeat(200);
    let i = run(&long);
    assert!(i.slots.query.as_deref().map_or(true, |q| q.chars().count() <= sys::MAX_QUERY_CHARS));
    let _ = run(&"פתח ".repeat(300));
}

#[test]
fn detection_is_fast() {
    let start = std::time::Instant::now();
    for _ in 0..200 {
        for t in ["תחפש בגוגל חתולים", "תפתח את ynet", "מה יש לי ביומן מחר", "פתח הגדרות wifi", "תכתוב מייל לדני בנושא תקציב"] {
            let _ = detect(t, &Ctx::default(), now(), &Known::default());
        }
    }
    // 1000 detections; the lexicon is built once
    assert!(start.elapsed() < std::time::Duration::from_secs(3), "{:?}", start.elapsed());
}

// ---- the fallback offer -------------------------------------------------------------------------

#[test]
fn fallback_kinds() {
    assert_eq!(fallback_kind("מה מזג האוויר"), Some(Fallback::Weather));
    assert_eq!(fallback_kind("מה מזג האוויר מחר בחיפה"), Some(Fallback::Weather));
    assert_eq!(fallback_kind("what's the weather in london"), Some(Fallback::Weather));
    assert_eq!(fallback_kind("יירד גשם מחר?"), Some(Fallback::Weather));
    assert_eq!(fallback_kind("מה שער הדולר"), Some(Fallback::Currency));
    assert_eq!(fallback_kind("שער היורו היום"), Some(Fallback::Currency));
    assert_eq!(fallback_kind("dollar to shekel exchange rate"), Some(Fallback::Currency));
    assert_eq!(fallback_kind("מה החדשות"), Some(Fallback::News));
    assert_eq!(fallback_kind("latest news"), Some(Fallback::News));
    assert_eq!(fallback_kind("תוצאות הכדורגל"), Some(Fallback::Sports));
    assert_eq!(fallback_kind("מי ניצח אתמול"), Some(Fallback::Sports));
    assert_eq!(fallback_kind("football scores"), Some(Fallback::Sports));
    assert_eq!(fallback_kind("כמה אנשים גרים בישראל"), Some(Fallback::Generic));
    assert_eq!(fallback_kind("hello"), Some(Fallback::Generic));
}

#[test]
fn web_topics_are_not_guessed_as_mail_or_file_searches() {
    for t in ["מה מזג האוויר", "מה שער הדולר", "שער היורו היום", "תוצאות הכדורגל", "מי ניצח אתמול", "מה החדשות", "what's the weather in london", "latest news", "football scores", "dollar to shekel exchange rate"] {
        assert_eq!(run(t).decision, Decision::NoMatch, "{t}");
    }
    // naming something on this PC keeps the question with the normal engine
    for t in ["מייל על שער הדולר", "תחפש מסמך על שער הדולר", "פגישה על ספורט מחר", "what do I have in my calendar about football"] {
        assert_ne!(run(t).decision, Decision::NoMatch, "{t}");
    }
}

#[test]
fn fallback_is_not_offered_for_what_a_web_search_cannot_answer() {
    for t in [
        "",
        "   ",
        "???",
        "12345",
        "תמחק את כל המיילים",
        "delete all notes",
        "OPEN 'C:\\Windows\\system32\\cmd.exe'",
        "file:///C:/x",
        "run setup.exe",
        "הפעל setup.exe",
        "שלח מייל לדנה שאני מאחר",
        "תבטל את הפגישה של מחר",
        "send the report to Dana",
        "תקבע לי פגישה מחר",
        &"א".repeat(300),
    ] {
        assert_eq!(fallback_kind(t), None, "{t:?}");
    }
}

// ---- apps: the existing launcher must be reached by every launch verb ---------------------------

#[test]
fn launch_verbs_reach_the_app_launcher() {
    let names_he = [
        "מחשבון",
        "פנקס רשימות",
        "צייר",
        "מנהל המשימות",
        "סייר הקבצים",
        "שורת הפקודה",
        "פאוורשל",
        "אאוטלוק",
        "וורד",
        "אקסל",
        "פאוורפוינט",
        "וואן נוט",
        "טימס",
        "כרום",
        "אדג'",
        "פיירפוקס",
        "זום",
        "וואטסאפ",
        "ספוטיפיי",
        "פתקיות",
        "מצלמה",
        "שעון",
        "שעון מעורר",
    ];
    let names_en = [
        "calculator",
        "notepad",
        "paint",
        "task manager",
        "file explorer",
        "outlook",
        "word",
        "excel",
        "powerpoint",
        "onenote",
        "teams",
        "chrome",
        "edge",
        "firefox",
        "zoom",
        "whatsapp",
        "spotify",
        "sticky notes",
        "camera",
        "clock",
    ];
    let verbs_he = ["פתח", "תפתח", "תריץ", "תפעיל", "הפעל", "תפתחי", "תפעילי"];
    let verbs_en = ["open", "launch", "start", "run"];
    let mut errors = Vec::new();
    for n in names_he {
        for v in verbs_he {
            for text in [format!("{v} {n}"), format!("{v} את {n}"), format!("{v} לי את {n}"), format!("{v} {n} בבקשה")] {
                let i = run(&text);
                if i.decision != (Decision::Confirm { cap: caps::APPS_LAUNCH }) {
                    errors.push(format!("{text:?}: {:?}", i.decision));
                }
            }
        }
    }
    for n in names_en {
        for v in verbs_en {
            let text = format!("{v} {n}");
            let i = run(&text);
            if i.decision != (Decision::Confirm { cap: caps::APPS_LAUNCH }) {
                errors.push(format!("{text:?}: {:?}", i.decision));
            }
        }
    }
    // with the Hebrew article
    for n in ["המחשבון", "הצייר", "הוורד", "האקסל", "הכרום", "הזום", "הוואטסאפ", "הספוטיפיי", "הפאוורפוינט", "הטימס"] {
        for v in ["פתח את", "תפתח את", "תריץ את", "תפעיל את", "הפעל את"] {
            let text = format!("{v} {n}");
            let i = run(&text);
            if i.decision != (Decision::Confirm { cap: caps::APPS_LAUNCH }) {
                errors.push(format!("{text:?}: {:?}", i.decision));
            }
        }
    }
    finish(errors);
}

#[test]
fn program_names_the_general_words_would_misread_keep_their_canonical_slot() {
    for (text, app) in [
        ("פתח את סייר הקבצים", "סייר הקבצים"),
        ("open file explorer", "סייר הקבצים"),
        ("תפתח פתקיות", "פתקיות"),
        ("open sticky notes", "פתקיות"),
        ("תפעיל את הזום", "זום"),
    ] {
        let i = run(text);
        assert_eq!(i.decision, Decision::Confirm { cap: caps::APPS_LAUNCH }, "{text}");
        assert_eq!(i.slots.app.as_deref(), Some(app), "{text}");
    }
    // the singular note and a file are still a note and a file
    assert_eq!(run("פתח את הפתק האחרון").decision, Decision::Confirm { cap: caps::NOTES_OPEN });
    assert_eq!(run("פתח את הקובץ של החשבונית").decision, Decision::Confirm { cap: caps::FILES_OPEN });
}
