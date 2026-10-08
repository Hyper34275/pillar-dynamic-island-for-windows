using System.Text.Json.Nodes;
using System.Text.Json.Serialization;

namespace CompanyIsland.Center.Core;

/// <summary>
/// Source-generated serialisation for everything that crosses the pipe (trim-safe, no reflection).
/// camelCase on the wire; unknown fields are ignored.
/// </summary>
[JsonSourceGenerationOptions(
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    PropertyNameCaseInsensitive = true,
    DefaultIgnoreCondition = JsonIgnoreCondition.Never,
    WriteIndented = false)]
[JsonSerializable(typeof(Note))]
[JsonSerializable(typeof(Note[]))]
[JsonSerializable(typeof(IslandSettings))]
[JsonSerializable(typeof(MonitorInfo))]
[JsonSerializable(typeof(MonitorInfo[]))]
[JsonSerializable(typeof(AssistantCard))]
[JsonSerializable(typeof(AssistantItem))]
[JsonSerializable(typeof(Choice))]
[JsonSerializable(typeof(ResultGroup))]
[JsonSerializable(typeof(MailboxRef))]
[JsonSerializable(typeof(SearchResults))]
[JsonSerializable(typeof(List<AssistantItem>))]
[JsonSerializable(typeof(List<Choice>))]
[JsonSerializable(typeof(List<ResultGroup>))]
[JsonSerializable(typeof(List<string>))]
[JsonSerializable(typeof(List<SearchResults>))]
[JsonSerializable(typeof(SearchResults[]))]
[JsonSerializable(typeof(string))]
[JsonSerializable(typeof(JsonObject))]
public sealed partial class CenterJson : JsonSerializerContext
{
}

/// <summary>Builds the <c>patch</c> argument of <c>updateSettings</c>. Only the fields that were set are sent.</summary>
public sealed class SettingsPatch
{
    private readonly JsonObject _values = new();

    public bool IsEmpty => _values.Count == 0;

    public SettingsPatch LaunchWithWindows(bool value) => Put("launchWithWindows", JsonValue.Create(value));
    public SettingsPatch HideInFullscreen(bool value) => Put("hideInFullscreen", JsonValue.Create(value));
    public SettingsPatch MeetingReminderEnabled(bool value) => Put("meetingReminderEnabled", JsonValue.Create(value));
    public SettingsPatch ReminderMinutes(int value) => Put("reminderMinutes", JsonValue.Create(value));
    public SettingsPatch NotificationsEnabled(bool value) => Put("notificationsEnabled", JsonValue.Create(value));
    public SettingsPatch MeetingSilencePrompt(bool value) => Put("meetingSilencePrompt", JsonValue.Create(value));
    public SettingsPatch MeetingInvitesEnabled(bool value) => Put("meetingInvitesEnabled", JsonValue.Create(value));
    public SettingsPatch DebugLogging(bool value) => Put("debugLogging", JsonValue.Create(value));
    public SettingsPatch OnboardingDone(bool value) => Put("onboardingDone", JsonValue.Create(value));
    public SettingsPatch AiSearchEnabled(bool value) => Put("aiSearchEnabled", JsonValue.Create(value));
    public SettingsPatch AiSearchButton(bool value) => Put("aiSearchButton", JsonValue.Create(value));
    public SettingsPatch AiSearchHotkey(bool value) => Put("aiSearchHotkey", JsonValue.Create(value));
    public SettingsPatch IslandDisplay(string value) => Put("islandDisplay", JsonValue.Create(IslandDisplays.Normalize(value)));

    /// <summary>A present-but-null monitor id selects the primary display, so null is sent as JSON null.</summary>
    public SettingsPatch MonitorId(string? value) => Put("monitorId", value is null ? null : JsonValue.Create(value));

    private SettingsPatch Put(string key, JsonNode? value)
    {
        _values[key] = value;
        return this;
    }

    /// <summary>A copy of the patch as a JSON object (a node can only have one parent, so never hand out the inner one).</summary>
    public JsonObject ToJson() => (JsonObject)_values.DeepClone();

    /// <summary>The settings as they will look once the island has applied this patch (for optimistic UI).</summary>
    public IslandSettings ApplyTo(IslandSettings current)
    {
        var next = current;
        foreach (var (key, node) in _values)
        {
            switch (key)
            {
                case "launchWithWindows": next = next with { LaunchWithWindows = node!.GetValue<bool>() }; break;
                case "hideInFullscreen": next = next with { HideInFullscreen = node!.GetValue<bool>() }; break;
                case "meetingReminderEnabled": next = next with { MeetingReminderEnabled = node!.GetValue<bool>() }; break;
                case "reminderMinutes": next = next with { ReminderMinutes = node!.GetValue<int>() }; break;
                case "notificationsEnabled": next = next with { NotificationsEnabled = node!.GetValue<bool>() }; break;
                case "meetingSilencePrompt": next = next with { MeetingSilencePrompt = node!.GetValue<bool>() }; break;
                case "meetingInvitesEnabled": next = next with { MeetingInvitesEnabled = node!.GetValue<bool>() }; break;
                case "debugLogging": next = next with { DebugLogging = node!.GetValue<bool>() }; break;
                case "onboardingDone": next = next with { OnboardingDone = node!.GetValue<bool>() }; break;
                case "aiSearchEnabled": next = next with { AiSearchEnabled = node!.GetValue<bool>() }; break;
                case "aiSearchButton": next = next with { AiSearchButton = node!.GetValue<bool>() }; break;
                case "aiSearchHotkey": next = next with { AiSearchHotkey = node!.GetValue<bool>() }; break;
                case "islandDisplay": next = next with { IslandDisplay = node!.GetValue<string>() }; break;
                case "monitorId": next = next with { MonitorId = node?.GetValue<string>() }; break;
            }
        }
        return next;
    }
}
