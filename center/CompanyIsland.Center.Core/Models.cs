using System.Text.Json.Serialization;

namespace CompanyIsland.Center.Core;

/// <summary>One note. Times are unix milliseconds. Mirrors the island's Note type.</summary>
public sealed record Note
{
    public string Id { get; set; } = "";
    public string Text { get; set; } = "";
    public long CreatedAt { get; set; }
    public long UpdatedAt { get; set; }
    public bool Pinned { get; set; }
}

/// <summary>What the collapsed island shows. Unknown wire values count as <see cref="Full"/>.</summary>
public static class IslandDisplays
{
    public const string Full = "full";
    public const string Clock = "clock";
    public const string Date = "date";

    public static string Normalize(string? value) => value is Clock or Date ? value : Full;
}

/// <summary>
/// The island's settings as the island sends them. Missing fields keep their defaults. (Plain setters, not <c>init</c>:
/// the source-generated deserialiser resets absent init-only properties to default(T) instead of keeping the initialiser.)
/// </summary>
public sealed record IslandSettings
{
    public int SchemaVersion { get; set; } = 1;
    public bool LaunchWithWindows { get; set; } = true;
    public bool HideInFullscreen { get; set; } = true;
    public bool MeetingReminderEnabled { get; set; } = true;
    public int ReminderMinutes { get; set; } = 30;
    public string? MonitorId { get; set; }
    public bool NotificationsEnabled { get; set; } = true;
    public bool MeetingSilencePrompt { get; set; } = true;
    public bool MeetingInvitesEnabled { get; set; } = true;
    public bool DebugLogging { get; set; }
    public bool OnboardingDone { get; set; }
    public string IslandDisplay { get; set; } = IslandDisplays.Full;
    public bool AiSearchEnabled { get; set; } = true;
    public bool AiSearchButton { get; set; } = true;
    public bool AiSearchHotkey { get; set; } = true;

    /// <summary>The same settings with <see cref="IslandDisplay"/> forced to a known value.</summary>
    public IslandSettings Normalized() => this with { IslandDisplay = IslandDisplays.Normalize(IslandDisplay) };
}

public sealed record MonitorInfo
{
    public string Id { get; set; } = "";
    public string Name { get; set; } = "";
    public bool Primary { get; set; }
    public bool IsPrimary { get; set; }
    public int Width { get; set; }
    public int Height { get; set; }
    public double Scale { get; set; } = 1;

    /// <summary>The wire format carries the flag twice (<c>primary</c> and <c>isPrimary</c>); either one counts.</summary>
    [JsonIgnore]
    public bool IsPrimaryDisplay => IsPrimary || Primary;
}

/// <summary>Windows notification access as the island reports it.</summary>
public enum NotificationAccess
{
    Unknown,
    Allowed,
    Denied,
    Unspecified,
    Unsupported,
    Policy,
    Error,
}

public static class NotificationAccessParser
{
    public static NotificationAccess Parse(string? wire) => wire switch
    {
        "allowed" => NotificationAccess.Allowed,
        "denied" => NotificationAccess.Denied,
        "unspecified" => NotificationAccess.Unspecified,
        "unsupported" => NotificationAccess.Unsupported,
        "policy" => NotificationAccess.Policy,
        "error" => NotificationAccess.Error,
        _ => NotificationAccess.Unknown,
    };
}

/// <summary>The tabs the island can be asked to show (<c>showIsland</c>).</summary>
public static class IslandTabs
{
    public const string Calendar = "calendar";
    public const string Notifications = "notifications";
    public const string Notes = "notes";
    public const string About = "about";
    public const string Settings = "settings";
}
