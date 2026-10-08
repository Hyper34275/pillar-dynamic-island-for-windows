using System.Globalization;

namespace CompanyIsland.Center.Core;

/// <summary>
/// Every user-visible string of the Center, in Hebrew, in one place. Settings labels reuse the island's own wording
/// (src/lib/i18n.ts, the <c>he</c> table, <c>settings.*</c> and <c>notif.*</c> keys) so both apps say the same thing.
/// Static members so XAML can bind them with <c>{x:Bind s:Strings.Name}</c>.
/// </summary>
public static class Strings
{
    // Shell
    public static string AppTitle => "מרכז האי";
    public static string CenterStuck => "מרכז האי כבר פתוח אבל לא מגיב. סגרו את CompanyIsland.Center.exe במנהל המשימות ונסו שוב.";
    public static string NavWelcome => "ברוכים הבאים";
    public static string NavNotes => "פתקים";
    public static string NavSettings => "הגדרות";
    public static string NavTour => "סיור במערכת";
    public static string NavSearch => "חיפוש חכם";

    // Smart search (everything local: see docs/AI_SEARCH.md)
    public static string SearchTitle => "חיפוש חכם";
    public static string SearchSubtitle => "שאל על היומן, המיילים, הקבצים והפתקים. הכול מתבצע על המחשב הזה.";
    public static string SearchInputPlaceholder => "מה לחפש? למשל: מה יש לי ביומן מחר?";
    public static string SearchSend => "שלח";
    public static string SearchWorking => "מחפש...";
    public static string SearchEmptyHeading => "נסה לשאול";
    public static IReadOnlyList<string> SearchExamples() =>
    [
        "מה יש לי ביומן מחר?",
        "מה יש לאיציק ביומן מחר?",
        "תמצא את המייל עם המילה תקציב",
        "תמצא קובץ בשם דוח חודשי",
    ];
    public static string SearchExtend => "חפש עוד 10 שניות";
    public static string SearchOpen => "פתח";
    public static string SearchRemember => "זכור את הבחירה";
    public static string SearchUnread => "לא נקרא";
    public static string SearchPartial => "החיפוש לא הושלם. מוצגות התוצאות שנמצאו עד כה.";
    public static string SearchNoResults => "לא נמצאו תוצאות.";
    public static string SearchGroupFailed => "המקור הזה לא ענה";
    public static string SearchGroupCalendar => "יומן";
    public static string SearchGroupMail => "מיילים";
    public static string SearchGroupNotes => "פתקים";
    public static string SearchGroupFiles => "קבצים";
    public static string SearchGroupApps => "אפליקציות";
    public static string SearchGroupCalc => "חישוב";
    public static string SearchGroupOther => "מידע";
    public static string SearchToday => "היום";
    public static string SearchTomorrow => "מחר";
    public static string SearchYesterday => "אתמול";
    public static string SearchFailed => "החיפוש נכשל. נסה שוב.";
    public static string SearchOffTitle => "החיפוש החכם כבוי";
    public static string SearchOffMessage => "אפשר להפעיל אותו בהגדרות, בסעיף \"חיפוש חכם\".";
    public static string SearchOpenFailed => "לא ניתן לפתוח את הפריט";
    public static string SearchExpired => "התוצאות האלה כבר לא זמינות. שאל שוב.";
    public static string SearchHistoryFailed => "לא ניתן לטעון את השיחה";
    public static string SearchShownOfTotal(int shown, int total) => $"מוצגות {shown} מתוך {total} תוצאות";
    public static string SearchYouSaid => "שאלת";
    public static string SearchAnswer => "תשובה";

    // Settings: smart search section
    public static string SettingsSearchHeading => "חיפוש חכם";
    public static string SettingsSearchEnabled => "הפעל חיפוש חכם";
    public static string SettingsSearchEnabledDescription => "שאל שאלות בעברית או באנגלית על היומן, המיילים, הקבצים והפתקים.";
    public static string SettingsSearchButton => "כפתור AI בפס החיפוש";
    public static string SettingsSearchButtonDescription => "מציג כפתור חיפוש חכם בפס החיפוש של שורת המשימות.";
    public static string SettingsSearchHotkey => "קיצור מקלדת Ctrl+Alt+Space";
    public static string SettingsSearchHotkeyDescription => "פותח את שורת החיפוש החכם מכל מקום.";
    public static string SettingsSearchPrivacy => "הכול מקומי: השאלות והתוצאות לא נשמרות ולא נשלחות לשום שירות";
    public static string VersionFooter(string version) => $"גרסה {version}";

    public static string DisconnectedTitle => "האי לא פועל כרגע";
    public static string DisconnectedMessage => "מרכז האי שומר הגדרות ופתקים דרך האי הדינמי. הפעל את האי והמרכז יתחבר אליו אוטומטית.";
    public static string StartIsland => "הפעל את האי";
    public static string StartIslandFailed => "לא ניתן להפעיל את האי. בדוק שהאפליקציה מותקנת.";
    public static string LoadFailed => "לא ניתן לטעון את הנתונים מהאי";
    public static string Close => "סגור";

    // Welcome
    public static string WelcomeTitle => "ברוכים הבאים לאי הדינמי";
    public static string WelcomeIntro =>
        "האי הדינמי הוא סרגל קטן בראש המסך שמראה את התאריך והשעה, את הפגישה הבאה מ-Outlook ואת ההתראות של Windows, " +
        "ונפתח לתצוגה מלאה כשמעבירים עליו את העכבר. מרכז האי הוא המקום להגדרות, לפתקים ולסיור קצר שמסביר הכול.";
    public static string StartTour => "התחל סיור";
    public static string ToSettings => "להגדרות";
    public static string Finish => "סיום";
    public static string FeaturesHeading => "מה האי יודע לעשות";

    public static string FeatureClockTitle => "תאריך, שעה וסטטוס פגישה";
    public static string FeatureClockText => "האי הסגור מציג תאריך ושעה, ובפגישה קרובה גם \"בעוד X דק׳\" או \"בפגישה עד\".";
    public static string FeatureReminderTitle => "תזכורות לפגישות";
    public static string FeatureReminderText => "לפני פגישה מופיעה תזכורת עם הכפתור \"הצטרף\" ואפשרות לדחות ב-5 דקות.";
    public static string FeatureInviteTitle => "זימונים";
    public static string FeatureInviteText => "זימון חדש מגיע עם הכפתורים \"אשר\", \"אולי\" ו\"דחה\", בלי לפתוח את Outlook.";
    public static string FeatureNotificationsTitle => "התראות Windows";
    public static string FeatureNotificationsText => "התראות Windows מופיעות באי ונשמרות בלשונית ההתראות עד הפעלה מחדש.";
    public static string FeatureQuietTitle => "שקט בזמן פגישה";
    public static string FeatureQuietText => "כשפגישה מתחילה אפשר לבחור באי: \"צלצול\" או \"שקט\" עד סוף הפגישה.";
    public static string FeatureCalendarTitle => "יומן";
    public static string FeatureCalendarText => "מעבר בין ימים וציר זמן של הפגישות לאורך היום.";
    public static string FeatureNotesTitle => "פתקים";
    public static string FeatureNotesText => "כתוב, חפש והצמד פתקים. הם נשארים רק במחשב הזה.";
    public static string FeatureAboutTitle => "אודות";
    public static string FeatureAboutText => "העתקה בלחיצה של כתובת ה-IP ושם המחשב, למי שצריך לספר לתמיכה.";

    // Settings (labels shared with the island's Settings tab)
    public static string SettingsTitle => "הגדרות";
    public static string SettingsSubtitle => "כל שינוי נשמר מיד ומופעל באי.";
    public static string SectionGeneral => "כללי";
    public static string SectionMeetings => "פגישות";
    public static string SectionNotifications => "התראות";
    public static string SectionAppearance => "מראה האי";
    public static string SectionTools => "כלים";

    public static string LaunchWithWindows => "הפעלה עם Windows";
    public static string HideInFullscreen => "הסתר באפליקציות במסך מלא";
    public static string HideInFullscreenHint => "האי לא מופיע מעל משחקים, מצגות וסרטונים במסך מלא.";
    public static string MeetingReminders => "תזכורות לפגישות";
    public static string ReminderMinutes => "הזכר לי לפני";
    public static string Minutes(int n) => $"{n} דק׳";
    public static string CalendarPrefetch => "הורדת לו״ז מראש";
    public static string CalendarPrefetchHint => "היומן שלך וכל היומנים המשותפים, N ימים קדימה, בלי קשר להגדרות המטמון של Outlook. נשמר בזיכרון בלבד.";
    public static string PrefetchDays(int n) => n == 0 ? "כבוי" : $"{n} ימים";
    public static string Notifications => "הצג התראות";
    public static string NotificationsHint => "התראות Windows מוצגות באי.";
    public static string MeetingInvites => "זימונים לפגישות";
    public static string MeetingSilence => "הצע שקט בתחילת פגישה";
    public static string Monitor => "תצוגה";
    public static string MonitorHint => "המסך שבו האי מוצג.";
    public static string MonitorPrimary => "ראשית";
    public static string MonitorN(int n) => $"תצוגה {n}";
    public static string SaveSettingsFailed => "לא ניתן לשמור את ההגדרות";

    public static string IslandDisplay => "מה מוצג באי הסגור";
    public static string IslandDisplayHint => "הבחירה משפיעה על האי הסגור בלבד.";
    public static string DisplayFull => "שעה, תאריך ויום";
    public static string DisplayClock => "שעה בלבד";
    public static string DisplayDate => "תאריך ויום";

    public static string NotificationAccessTitle => "גישה להתראות Windows";
    public static string NotificationAllow => "אשר גישה";
    public static string NotificationAccessFailed => "לא ניתן לבקש גישה להתראות";

    public static string OpenLogDir => "פתח תיקיית יומנים";
    public static string OpenLogDirHint => "היומנים אינם כוללים תוכן של פגישות, התראות או פתקים.";
    public static string ShowDiagnostics => "הצג את האבחון באי";
    public static string ShowDiagnosticsHint => "פותח את האי בלשונית ההגדרות, שם מופיעים פרטי המחשב והאבחון.";
    public static string Open => "פתח";

    public static string NotificationStatus(NotificationAccess access) => access switch
    {
        NotificationAccess.Allowed => "מותר",
        NotificationAccess.Denied => "חסום בהגדרות Windows",
        NotificationAccess.Unspecified => "טרם אושר",
        NotificationAccess.Unsupported => "לא נתמך",
        NotificationAccess.Policy => "מושבת על ידי מדיניות",
        NotificationAccess.Error => "לא זמין",
        _ => "לא ידוע",
    };

    // Notes
    public static string NotesTitle => "פתקים";
    public static string NotesSubtitle => "הפתקים נשמרים רק במחשב הזה.";
    public static string NotesSearchPlaceholder => "חיפוש בפתקים";
    public static string NotesSearchName => "חיפוש בפתקים";
    public static string NewNotePlaceholder => "כתוב פתק חדש…";
    public static string NewNoteName => "פתק חדש";
    public static string NoteEditorHint => "Ctrl+Enter לשמירה";
    public static string SaveNote => "שמור";
    public static string SaveNoteHint => "שמור (Ctrl+Enter)";
    public static string Cancel => "ביטול";
    public static string CancelHint => "ביטול (Esc)";
    public static string PinNote => "הצמד";
    public static string UnpinNote => "בטל הצמדה";
    public static string CopyNote => "העתק";
    public static string CopiedNote => "הועתק";
    public static string EditNote => "ערוך";
    public static string DeleteNote => "מחק";
    public static string DeleteConfirmTitle => "למחוק את הפתק?";
    public static string DeleteConfirmBody => "אי אפשר לשחזר פתק שנמחק.";
    public static string PinnedBadge => "מוצמד";
    public static string ListViewName => "תצוגת רשימה";
    public static string GridViewName => "תצוגת רשת";
    public static string NotesEmptyTitle => "אין עדיין פתקים";
    public static string NotesEmptyBody => "הפתקים נשמרים רק במחשב הזה, בלי ענן ובלי חשבון. כתוב את הפתק הראשון שלך למעלה.";
    public static string NotesNoResults => "לא נמצאו פתקים";
    public static string NotesNoResultsBody => "נסה מילות חיפוש אחרות.";
    public static string NoteSaveFailed => "לא ניתן לשמור את הפתק";
    public static string NoteDeleteFailed => "לא ניתן למחוק את הפתק";
    public static string NoteTooLong(int max) => $"פתק יכול להכיל עד {max:N0} תווים.";
    public static string NoteLimit(int max) => $"הגעת למספר הפתקים המרבי ({max}). מחק פתק כדי להוסיף חדש.";
    public static string CharCount(int count, int max) => $"{count:N0} / {max:N0}";
    public static string NoteNotFound => "הפתק לא נמצא. ייתכן שנמחק.";
    public static string CopyFailed => "ההעתקה נכשלה";

    // Tour
    public static string TourMissingTitle => "הסיור אינו זמין";
    public static string TourMissingBody => "קבצי הסיור לא נמצאו בתיקיית ההתקנה. התקן מחדש את CompanyIsland.";
    public static string TourWebViewFailed => "לא ניתן להציג את הסיור. רכיב WebView2 אינו זמין במחשב הזה.";

    /// <summary>Hebrew relative time for a note ("עכשיו", "לפני 5 דקות", "אתמול", then a date).</summary>
    public static string RelativeTime(long thenMs, long nowMs, TimeZoneInfo? zone = null)
    {
        TimeSpan age = TimeSpan.FromMilliseconds(Math.Max(0, nowMs - thenMs));
        if (age < TimeSpan.FromSeconds(45))
        {
            return "עכשיו";
        }

        if (age < TimeSpan.FromMinutes(60))
        {
            int minutes = Math.Max(1, (int)Math.Round(age.TotalMinutes));
            return minutes switch
            {
                1 => "לפני דקה",
                2 => "לפני שתי דקות",
                60 => "לפני שעה",
                _ => $"לפני {minutes} דקות",
            };
        }

        if (age < TimeSpan.FromHours(24))
        {
            int hours = (int)age.TotalHours;
            return hours switch
            {
                1 => "לפני שעה",
                2 => "לפני שעתיים",
                _ => $"לפני {hours} שעות",
            };
        }

        zone ??= TimeZoneInfo.Local;
        DateTime then = TimeZoneInfo.ConvertTimeFromUtc(DateTimeOffset.FromUnixTimeMilliseconds(thenMs).UtcDateTime, zone);
        DateTime now = TimeZoneInfo.ConvertTimeFromUtc(DateTimeOffset.FromUnixTimeMilliseconds(nowMs).UtcDateTime, zone);
        int days = (now.Date - then.Date).Days;
        if (days <= 1)
        {
            return "אתמול";
        }

        if (days < 7)
        {
            return days == 2 ? "לפני יומיים" : $"לפני {days} ימים";
        }

        return then.ToString("d.M.yyyy", CultureInfo.InvariantCulture);
    }
}
