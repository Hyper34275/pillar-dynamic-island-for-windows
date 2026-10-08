using System.Globalization;

namespace CompanyIsland.Center.Core;

// Wire shapes of smart search (docs/AI_SEARCH.md section 3), mirroring src-tauri/src/assistant/wire.rs.
// Plain set properties (the source generator resets absent init-only members) and string-typed enums, so an
// unknown phase or kind from a newer island never fails the whole payload. Registered in CenterJson (reflection is off).

/// <summary>One result line. <see cref="Id"/> is opaque and only meaningful together with the query id.</summary>
public sealed record AssistantItem
{
    public string Id { get; set; } = "";

    /// <summary>event | mail | note | file | app | calc | slot | info</summary>
    public string Kind { get; set; } = "";

    public string Title { get; set; } = "";
    public string? Subtitle { get; set; }

    /// <summary>Unix ms: event start, mail received, note or file modified.</summary>
    public long? Time { get; set; }

    /// <summary>Unix ms: event end.</summary>
    public long? EndTime { get; set; }

    /// <summary><c>#RRGGBB</c> (calendar colour).</summary>
    public string? Accent { get; set; }

    public bool Openable { get; set; }
    public bool Unread { get; set; }

    /// <summary>Mail: the mailbox. Event: the calendar. File: the folder.</summary>
    public string? Source { get; set; }
}

public sealed record Choice
{
    public string Id { get; set; } = "";
    public string Label { get; set; } = "";

    /// <summary>mailbox | allMailboxes | option</summary>
    public string Kind { get; set; } = "option";

    public bool Preferred { get; set; }
}

/// <summary>The card of one query (also what the island shows).</summary>
public sealed record AssistantCard
{
    public string QueryId { get; set; } = "";
    public string Query { get; set; } = "";

    /// <summary>processing | answer | choices | error</summary>
    public string Phase { get; set; } = "answer";

    public string Lang { get; set; } = "";
    public string Title { get; set; } = "";
    public string Summary { get; set; } = "";
    public string? Question { get; set; }
    public List<Choice> Choices { get; set; } = [];
    public List<AssistantItem> Items { get; set; } = [];
    public int Total { get; set; }
    public bool Partial { get; set; }
    public bool CanExtend { get; set; }
    public string? ErrorCode { get; set; }
    public List<string> Sources { get; set; } = [];
    public long CreatedAt { get; set; }
    public bool FollowUp { get; set; }
}

public sealed record MailboxRef
{
    public string Id { get; set; } = "";
    public string Name { get; set; } = "";
}

public sealed record ResultGroup
{
    /// <summary>calendar | mail | notes | files | apps | calc | availability</summary>
    public string Kind { get; set; } = "";

    public string Title { get; set; } = "";
    public MailboxRef? Mailbox { get; set; }
    public List<AssistantItem> Items { get; set; } = [];
    public bool Truncated { get; set; }
    public string? ErrorCode { get; set; }
}

/// <summary>Everything about one query: its card and the result groups.</summary>
public sealed record SearchResults
{
    public AssistantCard Card { get; set; } = new();
    public List<ResultGroup> Groups { get; set; } = [];
}

public static class CardPhases
{
    public const string Processing = "processing";
    public const string Answer = "answer";
    public const string Choices = "choices";
    public const string Error = "error";
}

public static class ChoiceKinds
{
    public const string Mailbox = "mailbox";
    public const string AllMailboxes = "allMailboxes";
}

/// <summary>Pure presentation helpers of the Smart Search page (kept out of the WinUI code so they can be tested).</summary>
public static class SearchFormat
{
    /// <summary>Example questions of the empty state. Clicking one sends it as typed.</summary>
    public static readonly IReadOnlyList<string> ExampleQuestions = Strings.SearchExamples();

    /// <summary>
    /// The groups to draw for a query: the groups the island sent, or, when it sent none, the card's items grouped
    /// by kind (so an answer that only filled <c>items</c> still shows). Empty groups are dropped, unless they carry an error.
    /// </summary>
    public static IReadOnlyList<ResultGroup> EffectiveGroups(SearchResults results)
    {
        var groups = results.Groups.Where(g => g.Items.Count > 0 || g.ErrorCode is not null).ToList();
        if (groups.Count > 0 || results.Card.Items.Count == 0)
        {
            return groups;
        }

        var byKind = new List<ResultGroup>();
        foreach (AssistantItem item in results.Card.Items)
        {
            string kind = GroupKindOf(item.Kind);
            ResultGroup? group = byKind.FirstOrDefault(g => g.Kind == kind);
            if (group is null)
            {
                group = new ResultGroup { Kind = kind, Title = "" };
                byKind.Add(group);
            }

            group.Items.Add(item);
        }

        return byKind;
    }

    /// <summary>The group kind an item kind belongs to.</summary>
    public static string GroupKindOf(string itemKind) => itemKind switch
    {
        "event" or "slot" => "calendar",
        "mail" => "mail",
        "note" => "notes",
        "file" => "files",
        "app" => "apps",
        "calc" => "calc",
        _ => "info",
    };

    /// <summary>The header of a group: the mailbox name for mail, the island's title when it sent one, else the source name.</summary>
    public static string GroupTitle(ResultGroup group)
    {
        if (group.Kind == "mail" && !string.IsNullOrWhiteSpace(group.Mailbox?.Name))
        {
            return group.Mailbox!.Name;
        }

        if (!string.IsNullOrWhiteSpace(group.Title))
        {
            return group.Title;
        }

        return group.Kind switch
        {
            "calendar" or "availability" => Strings.SearchGroupCalendar,
            "mail" => Strings.SearchGroupMail,
            "notes" => Strings.SearchGroupNotes,
            "files" => Strings.SearchGroupFiles,
            "apps" => Strings.SearchGroupApps,
            "calc" => Strings.SearchGroupCalc,
            _ => Strings.SearchGroupOther,
        };
    }

    /// <summary>Segoe Fluent glyph of a group kind.</summary>
    public static string GroupGlyph(string kind) => kind switch
    {
        "calendar" or "availability" => "",
        "mail" => "",
        "notes" => "",
        "files" => "",
        "apps" => "",
        "calc" => "",
        _ => "",
    };

    /// <summary>
    /// The time line of an item. Events: "09:00–10:30", with the day ahead of it when it is not today. Everything else
    /// (mail received, file modified): a relative time. Empty when the item has no time.
    /// </summary>
    public static string ItemTime(AssistantItem item, long nowMs, TimeZoneInfo? zone = null)
    {
        if (item.Time is not long time)
        {
            return "";
        }

        zone ??= TimeZoneInfo.Local;
        if (item.Kind is "event" or "slot")
        {
            DateTime start = ToLocal(time, zone);
            DateTime today = ToLocal(nowMs, zone).Date;
            string range = start.ToString("HH:mm", CultureInfo.InvariantCulture);
            if (item.EndTime is long end && end > time)
            {
                range += "–" + ToLocal(end, zone).ToString("HH:mm", CultureInfo.InvariantCulture);
            }

            return start.Date == today ? range : DayLabel(start.Date, today) + " " + range;
        }

        return Strings.RelativeTime(time, nowMs, zone);
    }

    /// <summary>"היום", "מחר", "אתמול", else d.M.</summary>
    public static string DayLabel(DateTime day, DateTime today)
    {
        int diff = (day.Date - today.Date).Days;
        return diff switch
        {
            0 => Strings.SearchToday,
            1 => Strings.SearchTomorrow,
            -1 => Strings.SearchYesterday,
            _ => day.ToString("d.M", CultureInfo.InvariantCulture),
        };
    }

    private static DateTime ToLocal(long unixMs, TimeZoneInfo zone) =>
        TimeZoneInfo.ConvertTimeFromUtc(DateTimeOffset.FromUnixTimeMilliseconds(unixMs).UtcDateTime, zone);

    /// <summary>The second line of a card: sender or place, then the calendar or folder, skipping empty parts.</summary>
    public static string ItemDetail(AssistantItem item, bool includeSource)
    {
        var parts = new List<string>(2);
        if (!string.IsNullOrWhiteSpace(item.Subtitle))
        {
            parts.Add(item.Subtitle!);
        }

        if (includeSource && !string.IsNullOrWhiteSpace(item.Source))
        {
            parts.Add(item.Source!);
        }

        return string.Join(" · ", parts);
    }

    /// <summary><c>#RRGGBB</c> to its channels, or null when the text is anything else.</summary>
    public static bool TryParseAccent(string? text, out byte r, out byte g, out byte b)
    {
        r = g = b = 0;
        if (text is null || text.Length != 7 || text[0] != '#')
        {
            return false;
        }

        return byte.TryParse(text.AsSpan(1, 2), NumberStyles.AllowHexSpecifier, CultureInfo.InvariantCulture, out r) &&
               byte.TryParse(text.AsSpan(3, 2), NumberStyles.AllowHexSpecifier, CultureInfo.InvariantCulture, out g) &&
               byte.TryParse(text.AsSpan(5, 2), NumberStyles.AllowHexSpecifier, CultureInfo.InvariantCulture, out b);
    }

    /// <summary>Choices that pick one mailbox (the "remember" checkbox applies only when one of these is clicked).</summary>
    public static bool IsMailboxChoice(Choice choice) => choice.Kind == ChoiceKinds.Mailbox;

    /// <summary>Whether the card asks something the user must answer.</summary>
    public static bool NeedsAnswer(AssistantCard card) => card.Phase == CardPhases.Choices && card.Choices.Count > 0;

    /// <summary>"נמצאו 12 תוצאות, מוצגות 5" when the card knows more than the groups hold, else empty.</summary>
    public static string MoreInfo(AssistantCard card, int shown) =>
        card.Total > shown && shown > 0 ? Strings.SearchShownOfTotal(shown, card.Total) : "";

    /// <summary>The index of the result with this query id, or -1 (the page scrolls to it on <c>search:&lt;id&gt;</c>).</summary>
    public static int IndexOfQuery(IReadOnlyList<SearchResults> history, string? queryId)
    {
        if (string.IsNullOrEmpty(queryId))
        {
            return -1;
        }

        for (int i = history.Count - 1; i >= 0; i--)
        {
            if (history[i].Card.QueryId == queryId)
            {
                return i;
            }
        }

        return -1;
    }

    /// <summary>The query id of the newest turn, or null when there is none (taken before a question is sent).</summary>
    public static string? LastQueryId(IReadOnlyList<SearchResults> history) =>
        history.Count == 0 ? null : history[^1].Card.QueryId;

    /// <summary>
    /// Whether a question sent from this page already shows in the history: the newest turn carries that text and is not the
    /// turn that was newest before sending. No clocks: a repeat of an old question is a new turn with a new id.
    /// </summary>
    public static bool SubmitInHistory(IReadOnlyList<SearchResults> history, string text, string? lastIdBefore) =>
        history.Count > 0 && history[^1].Card.Query == text && history[^1].Card.QueryId != lastIdBefore;
}

/// <summary>What the Smart Search page is waiting for: a new question, or an action on a turn that is already shown.</summary>
public enum SearchPendingKind
{
    Submit,
    Choose,
    Extend,
}

/// <summary>
/// One running search command. A new question needs a bubble and a spinner of its own; choosing or searching longer works on
/// an existing turn, so the spinner goes under that turn and nothing is added to the conversation.
/// </summary>
public sealed record SearchPending(SearchPendingKind Kind, string Text, string? QueryId, string? LastQueryIdBefore)
{
    /// <summary>The turn the spinner belongs to (Choose and Extend), when the history has it.</summary>
    public int TurnIndex(IReadOnlyList<SearchResults> history) =>
        Kind == SearchPendingKind.Submit ? -1 : SearchFormat.IndexOfQuery(history, QueryId);

    /// <summary>True when the conversation must get an extra bubble and spinner after the last turn.</summary>
    public bool NeedsOwnTurn(IReadOnlyList<SearchResults> history) => Kind switch
    {
        SearchPendingKind.Submit => !SearchFormat.SubmitInHistory(history, Text, LastQueryIdBefore),
        _ => TurnIndex(history) < 0,
    };
}
