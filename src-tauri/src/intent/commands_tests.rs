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
    // looking for mail, files and meetings with words that are also engines, topics or verbs of a command
    "new mail from Dan",
    "new email from Dana",
    "new mail today",
    "new email about the budget",
    "new mail",
    "create email rule",
    "find net income report",
    "search web design brief",
    "look up web traffic report",
    "find online order confirmation",
    "search internet bill",
    "check internet bill",
    "find maps of Israel",
    "תחפש בנט",
    "תחפש מייל מנפתלי בנט",
    "תחפש קובץ ברשת",
    "תמצא לי מסמך ברשת",
    "תרגום החוזה",
    "translation of the contract",
    "google sent me a file",
    "גוגל שלחו לי מייל",
    "google calendar invite",
    "google drive file budget",
    "איך מגיעים לפגישה",
    "איך מגיעים לפגישה של מחר",
    "how do I get to my meeting",
    "נווט לפגישה",
    "navigate to my meeting",
    "תחפש את תחזית המכירות",
    "תמצא את הדוח על המניות",
    "search stock options policy",
    "תחפש חשבונית בדולר",
    "find exchange rate report",
    "מה יש לי עם מכבי מחר",
    "do I have a meeting about the dollar",
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
    // ("שלום" and "hello" are small talk now: see TALK below)
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
fn truncated_and_reversed_phrasings_never_panic_and_never_run_anything() {
    let mut texts: Vec<&str> = Vec::new();
    texts.extend(SEARCH.iter().map(|c| c.0));
    texts.extend(TRANSLATE.iter().map(|c| c.0));
    texts.extend(SITES.iter().map(|c| c.0));
    texts.extend(SETTINGS.iter().map(|c| c.0));
    texts.extend(NOTES.iter().map(|c| c.0));
    texts.extend(COMPOSE.iter().map(|c| c.0));
    for t in texts {
        let chars: Vec<char> = t.chars().collect();
        for cut in 0..=chars.len() {
            let head: String = chars[..cut].iter().collect();
            let tail: String = chars[cut..].iter().collect();
            for s in [head, tail] {
                let i = run(&s);
                // whatever it is, it is never an execution of a sensitive capability
                assert!(!matches!(i.decision, Decision::Execute { cap } if crate::intent::sensitivity(cap) != Sensitivity::Read), "{s:?}");
            }
        }
        let words: Vec<&str> = t.split_whitespace().collect();
        let reversed = words.iter().rev().copied().collect::<Vec<_>>().join(" ");
        let _ = run(&reversed);
    }
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

// ---- the same command in every grammatical form -------------------------------------------------

/// Masculine, feminine, plural, infinitive and imperative verbs, with and without the polite lead-ins,
/// "לי" and "בבקשה": every combination means the same command.
#[test]
fn every_verb_form_and_lead_in_gives_the_same_command() {
    let leads = ["", "בבקשה ", "תוכל ", "תוכלי ", "תוכלו ", "אפשר ", "אתה יכול ", "את יכולה ", "אני רוצה ", "בבקשה תוכל ", "היי, "];
    let tails = ["", " בבקשה", " please"];
    let mut errors = Vec::new();
    let mut check_same = |verbs: &[&str], rest: &str, cap: CapId, probe: &dyn Fn(&Interpretation) -> String, want: &str| {
        for verb in verbs {
            for lead in leads {
                for tail in tails {
                    let text = format!("{lead}{verb} {rest}{tail}");
                    let i = run(&text);
                    if i.decision != (Decision::Confirm { cap }) || probe(&i) != want {
                        errors.push(format!("{text:?}: {:?} / {}", i.decision, probe(&i)));
                    }
                }
            }
        }
    };
    let query = |i: &Interpretation| format!("{}|{}", i.slots.engine.clone().unwrap_or_default(), i.slots.query.clone().unwrap_or_default());
    check_same(&["תחפש", "חפש", "תחפשי", "חפשי", "לחפש", "תחפשו", "חפשו"], "בגוגל חתולים חמודים", caps::WEB_SEARCH, &query, "google|חתולים חמודים");
    check_same(&["תחפש", "חפש", "תחפשי", "חפשי", "לחפש"], "לי בגוגל חתולים חמודים", caps::WEB_SEARCH, &query, "google|חתולים חמודים");
    check_same(&["תחפש", "חפש", "תחפשי", "חפשי", "לחפש"], "ביוטיוב חתולים חמודים", caps::WEB_SEARCH, &query, "youtube|חתולים חמודים");
    check_same(&["תגגל", "גגל", "תגגלי", "גגלי", "לגגל"], "חתולים חמודים", caps::WEB_SEARCH, &query, "google|חתולים חמודים");
    let site = |i: &Interpretation| i.slots.site.clone().unwrap_or_default();
    for rest in ["ynet", "את ynet", "לי את ynet", "את האתר של ynet"] {
        check_same(&["פתח", "תפתח", "פתחי", "תפתחי", "לפתוח", "תפתחו", "פתחו"], rest, caps::WEB_OPEN, &site, "ynet");
    }
    let setting = |i: &Interpretation| i.slots.setting.clone().unwrap_or_default();
    for rest in ["הגדרות wifi", "את הגדרות ה-wifi", "לי את הגדרות הווייפיי"] {
        check_same(&["פתח", "תפתח", "פתחי", "תפתחי", "לפתוח"], rest, caps::SYSTEM_OPEN_SETTINGS, &setting, "wifi");
    }
    let folder = |i: &Interpretation| i.slots.folder.clone().unwrap_or_default();
    for rest in ["הורדות", "את ההורדות", "לי את תיקיית ההורדות"] {
        check_same(&["פתח", "תפתח", "פתחי", "תפתחי", "לפתוח", "תפתחו"], rest, caps::FOLDERS_OPEN, &folder, "downloads");
    }
    let note = |i: &Interpretation| i.slots.query.clone().unwrap_or_default();
    for verb_phrase in ["תרשום פתק:", "רשום פתק:", "תרשמי פתק:", "רשמי פתק:", "לרשום פתק:", "תרשום לי פתק:", "תוסיף פתק:", "הוסיפי פתק:", "תיצור פתק:", "צרי פתק חדש:"] {
        check_same(&[verb_phrase], "לקנות חלב", caps::NOTES_CREATE, &note, "לקנות חלב");
    }
    let mail = |i: &Interpretation| format!("{}|{}", i.slots.mail_to.clone().unwrap_or_default(), i.slots.mail_subject.clone().unwrap_or_default());
    for verb in ["תכתוב", "כתוב", "תכתבי", "כתבי", "לכתוב", "תנסח", "תכין", "הכן", "תיצור"] {
        check_same(&[verb], "מייל לדני בנושא תקציב", caps::MAIL_COMPOSE, &mail, "דני|תקציב");
    }
    let none = |_: &Interpretation| String::new();
    check_same(&["תנעל", "נעל", "תנעלי", "נעלי", "לנעול"], "את המחשב", caps::SYSTEM_LOCK, &none, "");
    finish(errors);
}

#[test]
fn english_verb_forms_and_lead_ins_give_the_same_command() {
    let leads = ["", "please ", "can you ", "could you ", "would you please ", "i want to ", "i'd like to ", "hey, ", "just ", "can you please "];
    let mut errors = Vec::new();
    let cases: Vec<(&str, CapId, String)> = vec![
        ("search google for cats", caps::WEB_SEARCH, "google|cats".into()),
        ("google cats", caps::WEB_SEARCH, "google|cats".into()),
        ("search youtube for cats", caps::WEB_SEARCH, "youtube|cats".into()),
        ("look up cats on wikipedia", caps::WEB_SEARCH, "wikipedia|cats".into()),
        ("navigate to Haifa", caps::WEB_SEARCH, "maps|Haifa".into()),
        ("open ynet", caps::WEB_OPEN, "ynet".into()),
        ("go to ynet", caps::WEB_OPEN, "ynet".into()),
        ("launch ynet", caps::WEB_OPEN, "ynet".into()),
        ("open wifi settings", caps::SYSTEM_OPEN_SETTINGS, "wifi".into()),
        ("open the downloads folder", caps::FOLDERS_OPEN, "downloads".into()),
        ("write a note: buy milk", caps::NOTES_CREATE, "buy milk".into()),
        ("write an email to Dana about the budget", caps::MAIL_COMPOSE, "Dana|the budget".into()),
        ("lock the computer", caps::SYSTEM_LOCK, String::new()),
    ];
    for (base, cap, want) in &cases {
        for lead in leads {
            for tail in ["", " please"] {
                let text = format!("{lead}{base}{tail}");
                let i = run(&text);
                let got = match *cap {
                    caps::WEB_SEARCH => format!("{}|{}", i.slots.engine.clone().unwrap_or_default(), i.slots.query.clone().unwrap_or_default()),
                    caps::WEB_OPEN => i.slots.site.clone().unwrap_or_default(),
                    caps::SYSTEM_OPEN_SETTINGS => i.slots.setting.clone().unwrap_or_default(),
                    caps::FOLDERS_OPEN => i.slots.folder.clone().unwrap_or_default(),
                    caps::NOTES_CREATE => i.slots.query.clone().unwrap_or_default(),
                    caps::MAIL_COMPOSE => format!("{}|{}", i.slots.mail_to.clone().unwrap_or_default(), i.slots.mail_subject.clone().unwrap_or_default()),
                    _ => String::new(),
                };
                if i.decision != (Decision::Confirm { cap: *cap }) || got != *want {
                    errors.push(format!("{text:?}: {:?} / {got} (wanted {want})", i.decision));
                }
            }
        }
    }
    finish(errors);
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

// ---- review fixes ---------------------------------------------------------------------------------

#[test]
fn symbols_between_words_stay_in_the_query_the_note_and_the_subject() {
    for (text, query) in [
        ("תחפש בגוגל 2 + 2", "2 + 2"),
        ("תחפש בגוגל AT & T", "AT & T"),
        ("google usd / ils", "usd / ils"),
        ("תחפש בגוגל x = 5", "x = 5"),
        ("תחפש בגוגל פיצה ?", "פיצה ?"),
        ("תתרגם 2 + 2", "2 + 2"),
        ("תחפש ביוטיוב AC / DC", "AC / DC"),
    ] {
        assert_eq!(run(text).slots.query.as_deref(), Some(query), "{text}");
    }
    assert_eq!(run("תרשום פתק: 5 + 3 = 8").slots.query.as_deref(), Some("5 + 3 = 8"));
    assert_eq!(run("תרשום פתק: לקנות חלב & לחם").slots.query.as_deref(), Some("לקנות חלב & לחם"));
    assert_eq!(run("תכתוב מייל לדני בנושא תקציב - טיוטה").slots.mail_subject.as_deref(), Some("תקציב - טיוטה"));
    // and the address carries them encoded
    let i = run("תחפש בגוגל 2 + 2");
    let url = sys::search_url("google", i.slots.query.as_deref().unwrap(), None, None).unwrap();
    assert_eq!(url, "https://www.google.com/search?q=2%20%2B%202");
    // a lone symbol is still not a word of the grammar: it does not break a command
    assert_eq!(run("תפתח את ynet !").slots.site.as_deref(), Some("ynet"));
    assert_eq!(run("תחפש בגוגל - חתולים").slots.query.as_deref(), Some("חתולים"));
}

fn remembered(first: &str) -> (Ctx, DateTime<Local>) {
    let mut ctx = Ctx::default();
    let i = interpret(first, &ctx, now(), &Known::default());
    ctx.remember(&i, now().timestamp_millis());
    (ctx, now() + chrono::Duration::seconds(8))
}

#[test]
fn the_answer_to_a_question_completes_the_command() {
    for (ask, answer, cap, engine, query) in [
        ("תחפש בגוגל", "חתולים חמודים", caps::WEB_SEARCH, Some("google"), "חתולים חמודים"),
        ("תחפש ביוטיוב", "שירים של עומר אדם", caps::WEB_SEARCH, Some("youtube"), "שירים של עומר אדם"),
        ("search google", "cats", caps::WEB_SEARCH, Some("google"), "cats"),
        ("תרשום פתק", "לקנות חלב", caps::NOTES_CREATE, None, "לקנות חלב"),
        ("take a note", "buy milk", caps::NOTES_CREATE, None, "buy milk"),
        ("תתרגם", "שלום", caps::WEB_SEARCH, Some("translate"), "שלום"),
    ] {
        let (ctx, later) = remembered(ask);
        let i = interpret(answer, &ctx, later, &Known::default());
        assert_eq!(i.decision, Decision::Confirm { cap }, "{ask} -> {answer}");
        assert_eq!(i.slots.query.as_deref(), Some(query), "{ask} -> {answer}");
        assert_eq!(i.slots.engine.as_deref(), engine, "{ask} -> {answer}");
        assert!(i.follow_up);
    }
    // the translation keeps deciding its target language from the answer
    let (ctx, later) = remembered("תתרגם");
    assert_eq!(interpret("שלום", &ctx, later, &Known::default()).slots.lang_to.as_deref(), Some("en"));
    let (ctx, later) = remembered("translate");
    assert_eq!(interpret("hello", &ctx, later, &Known::default()).slots.lang_to.as_deref(), Some("iw"));
    // a language named in the question stays
    let (ctx, later) = remembered("תתרגם לרוסית");
    assert_eq!(interpret("תודה", &ctx, later, &Known::default()).slots.lang_to.as_deref(), Some("ru"));
}

#[test]
fn something_else_typed_after_a_question_is_not_its_answer() {
    let (ctx, later) = remembered("תחפש בגוגל");
    // another command, a question and a request of the normal engine all go their own way
    assert_eq!(interpret("תפתח את ynet", &ctx, later, &Known::default()).decision, Decision::Confirm { cap: caps::WEB_OPEN });
    assert_eq!(interpret("מה יש לי מחר", &ctx, later, &Known::default()).decision, Decision::Execute { cap: caps::CALENDAR_LIST_EVENTS });
    assert!(detect("מה יש לי מחר", &ctx, later, &Known::default()).is_none());
    assert!(detect("תחפש מייל מדני", &ctx, later, &Known::default()).is_none());
    assert!(detect("???", &ctx, later, &Known::default()).is_none());
    // two minutes later nobody is waiting for an answer any more
    let (ctx, _) = remembered("תחפש בגוגל");
    let late = now() + chrono::Duration::seconds(300);
    assert!(detect("חתולים", &ctx, late, &Known::default()).is_none());
    // and a question of another kind is not answered by a search
    let (ctx, later) = remembered("מה יש לי מחר");
    assert!(detect("חתולים", &ctx, later, &Known::default()).is_none());
}

#[test]
fn a_new_mail_needs_a_recipient_or_a_subject_not_a_search_phrase() {
    for t in ["new email to Dan", "write a new email", "start mail", "create a mail about the budget", "compose an email"] {
        assert_eq!(run(t).decision, Decision::Confirm { cap: caps::MAIL_COMPOSE }, "{t}");
    }
    for t in ["new mail from Dan", "new mail today", "create email rule"] {
        assert_ne!(run(t).decision, Decision::Confirm { cap: caps::MAIL_COMPOSE }, "{t}");
    }
}

#[test]
fn generic_engine_words_search_the_web_only_when_the_sentence_says_so() {
    for (t, q) in [
        ("search online for used cars", "used cars"),
        ("search the web for pizza recipes", "pizza recipes"),
        ("search the net about cats", "cats"),
        ("תחפש ברשת קובץ של חתולים", "קובץ של חתולים"),
        ("תחפש מתכון לעוגה באינטרנט", "מתכון לעוגה"),
    ] {
        let i = run(t);
        assert_eq!((i.decision, i.slots.query.as_deref()), (Decision::Confirm { cap: caps::WEB_SEARCH }, Some(q)), "{t}");
    }
    for t in ["search web design brief", "find online order confirmation", "search internet bill", "תחפש קובץ ברשת", "תחפש בנט"] {
        assert_ne!(run(t).decision, Decision::Confirm { cap: caps::WEB_SEARCH }, "{t}");
    }
}

#[test]
fn web_topics_are_for_questions_not_for_finding_documents() {
    assert_eq!(web_topic_kind("מה שער הדולר"), Some(Fallback::Currency));
    assert_eq!(web_topic_kind("what's the weather in london"), Some(Fallback::Weather));
    for t in ["מה יש לי עם מכבי מחר", "פגישה על מניות מחר", "do I have a meeting about football", "מייל על שער הדולר"] {
        assert_eq!(web_topic_kind(t), None, "{t}");
    }
    // a request to find something is a search of this PC, whatever it says
    for t in ["תחפש את תחזית המכירות", "תמצא את הדוח על המניות", "find the exchange rate report"] {
        assert_ne!(run(t).decision, Decision::NoMatch, "{t}");
    }
}

// ---- talking to the assistant: help, greetings, thanks, "who are you" ------------------------------

const HELP: CapId = caps::ASSISTANT_HELP;
const HELLO: CapId = caps::ASSISTANT_HELLO;
const THANKS: CapId = caps::ASSISTANT_THANKS;
const ABOUT: CapId = caps::ASSISTANT_ABOUT;

/// (text, capability): sentences that are entirely small talk or a question about the assistant.
const TALK: &[(&str, CapId)] = &[
    // ---- help, Hebrew (masculine and feminine; with and without "?", "בבקשה", "לי") ----
    ("מה אתה יודע לעשות", HELP),
    ("מה אתה יודע לעשות?", HELP),
    ("מה את יודעת לעשות", HELP),
    ("מה את יודעת לעשות?", HELP),
    ("מה אתה יכול לעשות", HELP),
    ("מה את יכולה לעשות", HELP),
    ("מה אתה יכול לעשות לי", HELP),
    ("מה אתה יודע לעשות בשבילי", HELP),
    ("מה אתה יודע לעשות בבקשה", HELP),
    ("בבקשה, מה אתה יכול לעשות?", HELP),
    ("מה אתה מסוגל לעשות", HELP),
    ("מה את מסוגלת לעשות", HELP),
    ("מה אתה יודע", HELP),
    ("מה אתה עושה", HELP),
    ("מה אפשר לעשות", HELP),
    ("מה אפשר לעשות איתך?", HELP),
    ("מה אפשר לשאול", HELP),
    ("מה אפשר לשאול אותך?", HELP),
    ("מה אפשר לבקש ממך", HELP),
    ("מה אני יכול לשאול", HELP),
    ("מה אני יכולה לשאול אותך", HELP),
    ("מה אני יכולה לבקש ממך?", HELP),
    ("במה אתה יכול לעזור", HELP),
    ("במה את יכולה לעזור?", HELP),
    ("במה אתה יכול לעזור לי", HELP),
    ("איך אתה יכול לעזור לי?", HELP),
    ("איך משתמשים בך", HELP),
    ("איך משתמשים בך?", HELP),
    ("איך משתמשים בזה", HELP),
    ("איך אני משתמש בך", HELP),
    ("איך אני משתמשת בך?", HELP),
    ("איך להשתמש בך", HELP),
    ("איך אפשר להשתמש בזה", HELP),
    ("איך זה עובד", HELP),
    ("איך אתה עובד?", HELP),
    ("עזרה", HELP),
    ("עזרה בבקשה", HELP),
    ("עזור לי", HELP),
    ("תעזור לי", HELP),
    ("תעזרי לי בבקשה", HELP),
    ("אני צריך עזרה", HELP),
    ("אני צריכה עזרה", HELP),
    ("מה הפקודות", HELP),
    ("רשימת פקודות", HELP),
    ("אילו פקודות אתה מכיר", HELP),
    ("הוראות שימוש", HELP),
    ("היי, מה אתה יודע לעשות?", HELP),
    ("שלום מה אתה יכול לעשות", HELP),
    ("תגיד, מה אתה יודע לעשות בכלל?", HELP),
    ("היי יובל מה אתה יכול לעשות", HELP),
    ("יובל, מה אתה יודע לעשות?", HELP),
    // ---- help, English ----
    ("help", HELP),
    ("Help", HELP),
    ("HELP!", HELP),
    ("help me", HELP),
    ("help please", HELP),
    ("please help", HELP),
    ("can you help me", HELP),
    ("what can you do", HELP),
    ("What can you do?", HELP),
    ("what can you do for me", HELP),
    ("what do you do", HELP),
    ("what do you know", HELP),
    ("what are you able to do", HELP),
    ("what can I ask", HELP),
    ("What can I ask you?", HELP),
    ("what can I say", HELP),
    ("how do I use this", HELP),
    ("How do I use this?", HELP),
    ("how do I use you", HELP),
    ("how does this work", HELP),
    ("show me what you can do", HELP),
    ("tell me what you can do", HELP),
    ("hi, what can you do?", HELP),
    ("hey Yuval, what can you do?", HELP),
    // ---- greetings ----
    ("שלום", HELLO),
    ("שלום!", HELLO),
    ("שלום שלום", HELLO),
    ("היי", HELLO),
    ("היי!", HELLO),
    ("הי", HELLO),
    ("הי!", HELLO),
    ("אהלן", HELLO),
    ("הלו", HELLO),
    ("בוקר טוב", HELLO),
    ("בוקר טוב!", HELLO),
    ("ערב טוב", HELLO),
    ("צהריים טובים", HELLO),
    ("לילה טוב", HELLO),
    ("שלום יובל", HELLO),
    ("היי יובל", HELLO),
    ("hi", HELLO),
    ("Hi!", HELLO),
    ("hello", HELLO),
    ("Hello!", HELLO),
    ("hey", HELLO),
    ("hi there", HELLO),
    ("good morning", HELLO),
    ("good evening", HELLO),
    ("hello Yuval", HELLO),
    ("hey Yuval", HELLO),
    // ---- thanks ----
    ("תודה", THANKS),
    ("תודה!", THANKS),
    ("תודה רבה", THANKS),
    ("תודה רבה לך", THANKS),
    ("תודה על העזרה", THANKS),
    ("מעולה תודה", THANKS),
    ("תודה יובל", THANKS),
    ("אלף תודות", THANKS),
    ("thanks", THANKS),
    ("Thanks!", THANKS),
    ("thank you", THANKS),
    ("thank you so much", THANKS),
    ("thanks a lot", THANKS),
    ("thx", THANKS),
    ("thanks Yuval", THANKS),
    ("thanks for the help", THANKS),
    ("ok thanks", THANKS),
    // ---- who are you ----
    ("מי אתה", ABOUT),
    ("מי אתה?", ABOUT),
    ("מי את", ABOUT),
    ("מי אתה בעצם", ABOUT),
    ("מה אתה", ABOUT),
    ("ספר לי על עצמך", ABOUT),
    ("תספרי לי על עצמך", ABOUT),
    ("תציג את עצמך", ABOUT),
    ("מה שמך", ABOUT),
    ("מה השם שלך", ABOUT),
    ("איך קוראים לך", ABOUT),
    ("who are you", ABOUT),
    ("Who are you?", ABOUT),
    ("what are you", ABOUT),
    ("what's your name", ABOUT),
    ("tell me about yourself", ABOUT),
    ("introduce yourself", ABOUT),
];

fn is_talk(i: &Interpretation) -> bool {
    matches!(i.decision, Decision::Execute { cap } if caps::is_talk(cap))
}

#[test]
fn help_and_small_talk_are_understood() {
    let mut errors = Vec::new();
    for (text, cap) in TALK {
        let i = run(text);
        if i.decision != (Decision::Execute { cap: *cap }) {
            errors.push(format!("{text:?}: decision {:?} (wanted Execute {})", i.decision, cap.as_str()));
            continue;
        }
        // read-only, nothing extracted, in the language it was typed in
        if i.slots != Slots::default() || i.follow_up || i.lang != detect_lang(text) || i.ranked != vec![(*cap, 1.0)] || i.confidence < 0.9 {
            errors.push(format!("{text:?}: {i:?}"));
        }
        assert_eq!(crate::intent::sensitivity(*cap), Sensitivity::Read, "{text}");
    }
    finish(errors);
}

#[test]
fn the_corpus_of_talk_phrasings_is_large_enough() {
    let count = |cap: CapId| TALK.iter().filter(|(_, c)| *c == cap).count();
    assert!(count(HELP) >= 40, "help phrasings: {}", count(HELP));
    assert!(count(HELLO) >= 20, "greetings: {}", count(HELLO));
    assert!(count(THANKS) >= 12, "thanks: {}", count(THANKS));
    assert!(count(ABOUT) >= 12, "who-are-you: {}", count(ABOUT));
    let hebrew = TALK.iter().filter(|(t, _)| detect_lang(t) == Lang::He).count();
    assert!(hebrew >= 80 && TALK.len() - hebrew >= 40, "Hebrew {hebrew} of {}", TALK.len());
}

#[test]
fn a_question_mark_a_please_or_a_greeting_does_not_change_the_answer() {
    let mut errors = Vec::new();
    for (text, cap) in TALK {
        let he = detect_lang(text) == Lang::He;
        let polite = if he { format!("{text} בבקשה") } else { format!("{text} please") };
        for variant in [format!("{text}?"), format!("{text}!"), format!(" {text} "), format!("{text}."), polite, format!("{text}\u{200F}")] {
            let i = run(&variant);
            if i.decision != (Decision::Execute { cap: *cap }) {
                errors.push(format!("{variant:?}: {:?} (wanted {})", i.decision, cap.as_str()));
            }
        }
        if *cap != HELLO {
            let greeting = if he { format!("היי, {text}") } else { format!("hi, {text}") };
            let i = run(&greeting);
            if i.decision != (Decision::Execute { cap: *cap }) {
                errors.push(format!("{greeting:?}: {:?} (wanted {})", i.decision, cap.as_str()));
            }
        }
    }
    finish(errors);
}

/// Texts that look like small talk but ask for something: they keep the normal engine's decision.
const NOT_TALK: &[&str] = &[
    "מה יש לי היום",
    "מה יש לי",
    "מה יש לי מחר",
    "תעזור לי למצוא את הקובץ של התקציב",
    "תעזור לי לחפש מייל מדני",
    "שלום מדני",
    "שלום, מה יש לי מחר",
    "היי מה יש לי מחר",
    "help desk ticket",
    "help desk",
    "help me find the budget file",
    "help with excel",
    "help.txt",
    "עזרה עם המייל",
    "עזרה בהגדרות",
    "תודה על המייל",
    "תודה רבה על המייל",
    "המייל של תודה",
    "thanks for the file",
    "thank you email from Dana",
    "מה אתה יודע על התקציב",
    "מה יודע דני",
    "מה אפשר לעשות בקובץ",
    "מה אתה מחפש",
    "מי זה דני",
    "מי אתה חושב שאתה",
    "אתה יודע לעשות משהו?",
    "what do you do tomorrow",
    "what can you do about the budget",
    "who are you calling",
    "who is Dana",
    "how do I use excel",
    "how do I get to the airport",
    "hello world",
    "hello.txt",
    "hi-fi",
    "good morning email",
    "המייל האחרון מדני",
    "תחפש בגוגל עזרה",
    "search google for help",
    "תרשום פתק: תודה",
    "תכתוב מייל לדני תודה",
    "open help",
    "פתח עזרה",
    "מה נשמע אצל דני",
    "יובל",
    "מיובל",
    "המייל מיובל",
    "תחפש את יובל",
    "מה יש ליובל מחר",
    "שלום אני רוצה לדעת מה יש לי מחר ביומן ומי שלח לי מייל",
];

#[test]
fn real_requests_are_not_taken_for_small_talk() {
    let mut errors = Vec::new();
    for text in NOT_TALK {
        let i = run(text);
        if is_talk(&i) {
            errors.push(format!("{text:?}: became {:?}", i.decision));
        }
        if let Some(d) = detect(text, &Ctx::default(), now(), &Known::default()) {
            if is_talk(&d) {
                errors.push(format!("{text:?}: detect gave {:?}", d.decision));
            }
        }
    }
    finish(errors);
}

#[test]
fn the_requests_next_to_small_talk_keep_the_decision_they_had() {
    let d = |t: &str| run(t).decision;
    assert_eq!(d("מה יש לי היום"), Decision::Execute { cap: caps::CALENDAR_LIST_EVENTS });
    assert_eq!(d("היי מה יש לי מחר"), Decision::Execute { cap: caps::CALENDAR_LIST_EVENTS });
    assert_eq!(d("תעזור לי למצוא את הקובץ של התקציב"), Decision::Execute { cap: caps::FILES_SEARCH });
    assert_eq!(d("help me find the budget file"), Decision::Execute { cap: caps::FILES_SEARCH });
    assert_eq!(d("המייל האחרון מדני"), Decision::Execute { cap: caps::EMAIL_SEARCH });
    // a greeting followed by a name is, for the engine, what it always was
    assert_eq!(d("שלום מדני"), Decision::NoMatch);
    assert_eq!(d("help desk ticket"), Decision::NoMatch);
    assert_eq!(d("תודה על המייל"), Decision::NoMatch);
    // the commands keep their words
    assert_eq!(d("תחפש בגוגל עזרה"), Decision::Confirm { cap: caps::WEB_SEARCH });
    assert_eq!(d("search google for help"), Decision::Confirm { cap: caps::WEB_SEARCH });
    assert_eq!(d("תרשום פתק: תודה"), Decision::Confirm { cap: caps::NOTES_CREATE });
    assert_eq!(d("תפתח את ynet"), Decision::Confirm { cap: caps::WEB_OPEN });
}

#[test]
fn what_can_you_do_is_not_a_question_about_a_date() {
    // the screenshot: "מה אתה יודע לעשות" used to ask "לאיזה תאריך התכוונת?"
    for t in ["מה אתה יודע לעשות", "מה אתה יכול לעשות", "מה את יודעת לעשות", "מה אפשר לעשות", "מה אתה עושה", "מה אתה יודע לעשות?"] {
        let i = interpret(t, &Ctx::default(), now(), &Known::default());
        assert_eq!(i.decision, Decision::Execute { cap: caps::ASSISTANT_HELP }, "{t}");
        assert!(!matches!(i.decision, Decision::Clarify { ask: AskKind::Date, .. }), "{t}");
        assert!(i.slots.time.is_none() && i.slots.terms.is_empty(), "{t}");
    }
    // the same words with a day are still a question about that day
    for t in ["מה לעשות היום", "מה אפשר לעשות מחר"] {
        assert_eq!(run(t).decision, Decision::Execute { cap: caps::CALENDAR_LIST_EVENTS }, "{t}");
    }
}

#[test]
fn the_answer_to_a_question_comes_before_small_talk() {
    // "שלום" after "מה לתרגם?" is the text to translate, "עזרה" after "מה לחפש?" is the search
    for (ask, answer, cap, query) in [
        ("תחפש בגוגל", "עזרה", caps::WEB_SEARCH, "עזרה"),
        ("תחפש בגוגל", "help", caps::WEB_SEARCH, "help"),
        ("תתרגם", "תודה", caps::WEB_SEARCH, "תודה"),
        ("תתרגם", "שלום", caps::WEB_SEARCH, "שלום"),
        ("תרשום פתק", "תודה", caps::NOTES_CREATE, "תודה"),
        ("take a note", "hello", caps::NOTES_CREATE, "hello"),
    ] {
        let (ctx, later) = remembered(ask);
        let i = interpret(answer, &ctx, later, &Known::default());
        assert_eq!(i.decision, Decision::Confirm { cap }, "{ask} -> {answer}");
        assert_eq!(i.slots.query.as_deref(), Some(query), "{ask} -> {answer}");
        assert!(i.follow_up);
    }
    // nobody is waiting for an answer any more: it is small talk again
    let (ctx, _) = remembered("תחפש בגוגל");
    let late = now() + chrono::Duration::seconds(300);
    assert_eq!(interpret("עזרה", &ctx, late, &Known::default()).decision, Decision::Execute { cap: HELP });
    // and a question that is not a command's does not turn small talk into an answer
    let (ctx, later) = remembered("מה יש לי מחר");
    assert_eq!(interpret("תודה", &ctx, later, &Known::default()).decision, Decision::Execute { cap: THANKS });
}

#[test]
fn braces_in_the_lexicon_expand_to_every_combination() {
    assert_eq!(expand("מה {אתה|את} {יודע|יודעת}"), vec!["מה אתה יודע", "מה אתה יודעת", "מה את יודע", "מה את יודעת"]);
    assert_eq!(expand("{אני|} צריך עזרה"), vec!["אני צריך עזרה", " צריך עזרה"]);
    assert_eq!(expand("help"), vec!["help"]);
    // an unclosed brace is left as it is, never a panic
    assert_eq!(expand("a {b|c"), vec!["a {b|c"]);
    assert_eq!(expand("}{"), vec!["}{"]);
    assert_eq!(expand(""), vec![""]);
}

#[test]
fn every_talk_phrase_is_reachable_unique_and_no_command() {
    let l = lex();
    let mut seen: HashMap<String, &str> = HashMap::new();
    for (list, cap) in [("talk_help", HELP), ("talk_about", ABOUT), ("talk_thanks", THANKS), ("talk_hello", HELLO)] {
        let phrases = &l.lists[list];
        assert!(phrases.len() >= 40, "{list}: {} phrases", phrases.len());
        for p in phrases {
            let joined = p.join(" ");
            assert!(p.len() <= MAX_TALK_WORDS, "{list}: {joined:?} is longer than a sentence of small talk may be");
            // one meaning per phrase
            if let Some(prev) = seen.insert(joined.clone(), list) {
                assert_eq!(prev, list, "{joined:?} is in both {prev} and {list}");
            }
            // the sentence reaches its capability...
            assert_eq!(l.talk(&joined), Some(cap), "{list}: {joined:?}");
            // ...and is never the grammar of a command
            assert!(l.parse(&joined).is_none(), "{list}: {joined:?} is also a command");
        }
    }
    // the filler words are not phrases of their own
    for w in ["לי", "בבקשה", "please", "אז"] {
        assert!(l.talk(w).is_none(), "{w}");
    }
}

#[test]
fn small_talk_is_whole_sentences_only() {
    let l = lex();
    // too long, empty, or only the name / a symbol: nothing
    assert!(l.talk("").is_none() && l.talk("   ").is_none() && l.talk("???").is_none() && l.talk("יובל").is_none() && l.talk("yuval").is_none());
    assert!(l.talk("שלום ".repeat(12).trim()).is_none());
    assert_eq!(l.talk("שלום"), Some(HELLO));
    // one extra word of substance and it is a request
    for t in ["שלום דנה", "שלום לכולם שלי", "עזרה ביומן", "תודה תקציב", "help budget", "thanks Dana", "hello Dana"] {
        assert!(l.talk(t).is_none(), "{t}");
    }
}

#[test]
fn talk_detection_is_fast_and_never_panics() {
    let start = std::time::Instant::now();
    for _ in 0..300 {
        for t in ["מה יש לי ביומן מחר", "תחפש את המייל האחרון מדני עם המילה תקציב", "שלום", "מה אתה יודע לעשות", "help desk ticket"] {
            let _ = detect(t, &Ctx::default(), now(), &Known::default());
        }
    }
    assert!(start.elapsed() < std::time::Duration::from_secs(3), "{:?}", start.elapsed());
    for t in ["\u{202E}שלום\u{0000}", "שלום\n\nתודה", "ש", "'", "\"\"", "{|}", "מה {אתה|את}", &"עזרה ".repeat(300)] {
        let _ = run(t);
    }
}
