using System.Text.Json;
using System.Text.RegularExpressions;
using CompanyIsland.Center.Core;

namespace CompanyIsland.Center.Tests;

public class PipeNameTests
{
    [Fact]
    public void Name_is_session_and_16_hex_chars_of_the_sha256_of_the_sid()
    {
        // sha256("S-1-5-21-1004336348-1177238915-682003330-1001") starts with c683d27c80f2d3b9
        Assert.Equal(
            "CompanyIsland.Center.2.c683d27c80f2d3b9",
            PipeNames.Build(2, "S-1-5-21-1004336348-1177238915-682003330-1001"));
    }

    [Fact]
    public void Different_users_and_sessions_get_different_names()
    {
        Assert.NotEqual(PipeNames.Build(1, "S-1-5-21-1"), PipeNames.Build(1, "S-1-5-21-2"));
        Assert.NotEqual(PipeNames.Build(1, "S-1-5-21-1"), PipeNames.Build(2, "S-1-5-21-1"));
    }

    [Fact]
    public void Current_user_name_has_the_documented_shape()
    {
        Assert.Matches(@"^CompanyIsland\.Center\.\d+\.[0-9a-f]{16}$", PipeNames.ForCurrentUser());
    }
}

public class CenterPageTests
{
    [Theory]
    [InlineData("welcome", CenterPageKind.Welcome)]
    [InlineData("tour", CenterPageKind.Tour)]
    [InlineData("settings", CenterPageKind.Settings)]
    [InlineData("notes", CenterPageKind.Notes)]
    [InlineData("notes-new", CenterPageKind.NotesNew)]
    public void Plain_pages_parse(string wire, CenterPageKind kind)
    {
        Assert.True(CenterPage.TryParse(wire, out CenterPage page));
        Assert.Equal(kind, page.Kind);
        Assert.Equal(wire, page.ToWire());
    }

    [Fact]
    public void Note_pages_carry_a_valid_id()
    {
        Assert.True(CenterPage.TryParse("note:ab12_CD-9", out CenterPage page));
        Assert.Equal(CenterPageKind.Note, page.Kind);
        Assert.Equal("ab12_CD-9", page.NoteId);
        Assert.Equal("note:ab12_CD-9", page.ToWire());
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("Welcome")]
    [InlineData("settings ")]
    [InlineData("notes/new")]
    [InlineData("note:")]
    [InlineData("note:a b")]
    [InlineData("note:../x")]
    [InlineData("note:אבג")]
    [InlineData("note:a:b")]
    [InlineData("tour?x=1")]
    public void Invalid_pages_are_rejected(string? wire)
    {
        Assert.False(CenterPage.TryParse(wire, out CenterPage page));
        Assert.Equal(CenterPageKind.Welcome, page.Kind);
    }

    [Fact]
    public void Note_id_length_is_limited_to_64()
    {
        Assert.True(CenterPage.TryParse("note:" + new string('a', 64), out _));
        Assert.False(CenterPage.TryParse("note:" + new string('a', 65), out _));
    }

    [Theory]
    [InlineData("--page settings", CenterPageKind.Settings)]
    [InlineData("--page=notes-new", CenterPageKind.NotesNew)]
    [InlineData("--other 1 --page tour", CenterPageKind.Tour)]
    [InlineData("--page", CenterPageKind.Welcome)]
    [InlineData("--page bogus", CenterPageKind.Welcome)]
    [InlineData("", CenterPageKind.Welcome)]
    [InlineData("--page settings --page tour", CenterPageKind.Settings)]
    public void Arguments_pick_the_page(string commandLine, CenterPageKind expected)
    {
        Assert.Equal(expected, CenterPage.FromCommandLine(commandLine).Kind);
        Assert.Equal(expected, CenterPage.FromArgs(CommandLine.Split(commandLine)).Kind);
    }

    [Fact]
    public void A_redirected_command_line_with_the_exe_path_and_quotes_still_parses()
    {
        var page = CenterPage.FromCommandLine("\"C:\\Program Files\\CompanyIsland\\center\\CompanyIsland.Center.exe\" --page \"note:abc123\"");
        Assert.Equal(CenterPageKind.Note, page.Kind);
        Assert.Equal("abc123", page.NoteId);
    }

    [Fact]
    public void Command_line_splitting_follows_windows_rules()
    {
        Assert.Equal(["a", "b c", "d\"e", ""], CommandLine.Split("a \"b c\" d\\\"e \"\""));
        Assert.Empty(CommandLine.Split("   "));
        Assert.Empty(CommandLine.Split(null));
    }
}

public class NoteOpsTests
{
    private static Note N(string id, long created, long updated, bool pinned = false, string text = "t") =>
        new() { Id = id, Text = text, CreatedAt = created, UpdatedAt = updated, Pinned = pinned };

    [Fact]
    public void New_ids_are_16_lowercase_hex_and_unique()
    {
        var ids = Enumerable.Range(0, 500).Select(_ => NoteOps.NewId()).ToList();
        Assert.All(ids, id => Assert.Matches("^[0-9a-f]{16}$", id));
        Assert.Equal(ids.Count, ids.Distinct().Count());
        Assert.All(ids, id => Assert.True(NoteOps.IsValidId(id)));
    }

    [Theory]
    [InlineData("a", true)]
    [InlineData("A_b-9", true)]
    [InlineData("", false)]
    [InlineData(null, false)]
    [InlineData("a b", false)]
    [InlineData("a/b", false)]
    [InlineData("é", false)]
    public void Id_rules(string? id, bool valid) => Assert.Equal(valid, NoteOps.IsValidId(id));

    [Fact]
    public void Id_limit_is_64_chars()
    {
        Assert.True(NoteOps.IsValidId(new string('x', 64)));
        Assert.False(NoteOps.IsValidId(new string('x', 65)));
    }

    [Fact]
    public void Canonical_order_is_pinned_then_newest_updated_then_id()
    {
        var sorted = NoteOps.Sort([
            N("b", 1, 100),
            N("a", 1, 100),
            N("c", 1, 300),
            N("p-old", 1, 10, pinned: true),
            N("p-new", 1, 50, pinned: true),
        ]);
        Assert.Equal(["p-new", "p-old", "c", "a", "b"], sorted.Select(n => n.Id));
    }

    [Fact]
    public void Search_is_case_insensitive_and_needs_every_word()
    {
        var notes = new[]
        {
            N("1", 1, 1, text: "Buy MILK and bread"),
            N("2", 1, 1, text: "milk only"),
            N("3", 1, 1, text: "שלום עולם"),
        };
        Assert.Equal(["1", "2"], NoteOps.Search(notes, "milk").Select(n => n.Id));
        Assert.Equal(["1"], NoteOps.Search(notes, "  milk   BREAD ").Select(n => n.Id));
        Assert.Equal(["3"], NoteOps.Search(notes, "עולם").Select(n => n.Id));
        Assert.Empty(NoteOps.Search(notes, "nothing"));
        Assert.Equal(3, NoteOps.Search(notes, "   ").Count);
        Assert.Equal(3, NoteOps.Search(notes, null).Count);
    }

    [Fact]
    public void Text_validation()
    {
        Assert.Equal(NoteTextProblem.Empty, NoteOps.CheckText(null));
        Assert.Equal(NoteTextProblem.Empty, NoteOps.CheckText(" \r\n\t "));
        Assert.Equal(NoteTextProblem.None, NoteOps.CheckText("hi"));
        Assert.Equal(NoteTextProblem.None, NoteOps.CheckText(new string('a', 10_000)));
        Assert.Equal(NoteTextProblem.TooLong, NoteOps.CheckText(new string('a', 10_001)));
    }

    [Fact]
    public void Characters_are_counted_as_unicode_scalars_not_utf16_units()
    {
        string emoji = string.Concat(Enumerable.Repeat("😀", 10_000)); // 20,000 UTF-16 units, 10,000 scalars
        Assert.Equal(10_000, NoteOps.CountChars(emoji));
        Assert.Equal(NoteTextProblem.None, NoteOps.CheckText(emoji));
        Assert.Equal(NoteTextProblem.TooLong, NoteOps.CheckText(emoji + "x"));
    }

    [Fact]
    public void Truncate_never_splits_a_surrogate_pair()
    {
        string text = "ab😀cd";
        Assert.Equal("ab", NoteOps.Truncate(text, 2));
        Assert.Equal("ab😀", NoteOps.Truncate(text, 3));
        Assert.Equal(text, NoteOps.Truncate(text, 5));
        Assert.Equal(text, NoteOps.Truncate(text, 99));
    }

    [Fact]
    public void Sanitize_applies_the_islands_rules()
    {
        const long now = 1_000_000;
        var result = NoteOps.Sanitize(
        [
            N("ok", 10, 20, text: "keep"),
            N("bad id", 10, 20),                       // invalid id: dropped
            N("blank", 10, 20, text: "  \n "),         // empty text: dropped
            N("zero", 0, 0),                           // times <= 0 become now
            N("swap", 50, 40),                         // updatedAt < createdAt => createdAt
            N("dup", 1, 100, text: "old"),
            N("dup", 1, 200, text: "new"),             // the larger updatedAt wins
            N("long", 1, 1, text: new string('q', 10_050)),
        ], now);

        Assert.Equal(new[] { "zero", "dup", "ok", "swap", "long" }.Order(), result.Select(n => n.Id).Order());
        Assert.Equal("new", result.Single(n => n.Id == "dup").Text);
        Assert.Equal(now, result.Single(n => n.Id == "zero").CreatedAt);
        Assert.Equal(now, result.Single(n => n.Id == "zero").UpdatedAt);
        Assert.Equal(50, result.Single(n => n.Id == "swap").UpdatedAt);
        Assert.Equal(10_000, result.Single(n => n.Id == "long").Text.Length);
        Assert.Equal(result.Select(n => n.Id), NoteOps.Sort(result).Select(n => n.Id)); // canonical order
    }

    [Fact]
    public void Sanitize_keeps_the_500_newest()
    {
        var many = Enumerable.Range(1, 520).Select(i => N("n" + i, 1, i)).ToList();
        var result = NoteOps.Sanitize(many, 5);
        Assert.Equal(500, result.Count);
        Assert.DoesNotContain(result, n => n.Id == "n1");
        Assert.DoesNotContain(result, n => n.Id == "n20");
        Assert.Contains(result, n => n.Id == "n21");
        Assert.Equal("n520", result[0].Id);
    }

    [Fact]
    public void Create_stamps_a_fresh_note()
    {
        Note note = NoteOps.Create("hello", 123, pinned: true);
        Assert.True(NoteOps.IsValidId(note.Id));
        Assert.Equal(123, note.CreatedAt);
        Assert.Equal(123, note.UpdatedAt);
        Assert.True(note.Pinned);
    }
}

public class SettingsModelTests
{
    [Fact]
    public void Patch_applies_to_settings_for_optimistic_updates()
    {
        var current = new IslandSettings { MonitorId = "m2", ReminderMinutes = 30 };
        var next = new SettingsPatch().ReminderMinutes(10).IslandDisplay("date").MonitorId(null).LaunchWithWindows(false).ApplyTo(current);
        Assert.Equal(10, next.ReminderMinutes);
        Assert.Equal("date", next.IslandDisplay);
        Assert.Null(next.MonitorId);
        Assert.False(next.LaunchWithWindows);
        Assert.True(next.HideInFullscreen);
    }

    [Fact]
    public void CalendarPrefetchDays_defaults_patches_and_round_trips()
    {
        Assert.Equal(7, new IslandSettings().CalendarPrefetchDays);
        var patch = new SettingsPatch().CalendarPrefetchDays(14);
        Assert.Equal(14, patch.ToJson()["calendarPrefetchDays"]!.GetValue<int>());
        Assert.Equal(14, patch.ApplyTo(new IslandSettings()).CalendarPrefetchDays);
        Assert.Equal(0, new SettingsPatch().CalendarPrefetchDays(0).ApplyTo(new IslandSettings()).CalendarPrefetchDays);
        string json = JsonSerializer.Serialize(new IslandSettings { CalendarPrefetchDays = 30 }, CenterJson.Default.IslandSettings);
        Assert.Contains("\"calendarPrefetchDays\":30", json);
        Assert.Equal(30, JsonSerializer.Deserialize(json, CenterJson.Default.IslandSettings)!.CalendarPrefetchDays);
        Assert.Equal(7, JsonSerializer.Deserialize("{}", CenterJson.Default.IslandSettings)!.CalendarPrefetchDays);
    }

    [Fact]
    public void Patch_serialises_only_what_was_set()
    {
        var patch = new SettingsPatch().NotificationsEnabled(false);
        Assert.Equal("""{"notificationsEnabled":false}""", patch.ToJson().ToJsonString());
        Assert.False(patch.IsEmpty);
        Assert.True(new SettingsPatch().IsEmpty);
    }

    [Fact]
    public void Island_display_values_are_normalised()
    {
        Assert.Equal("full", IslandDisplays.Normalize(null));
        Assert.Equal("full", IslandDisplays.Normalize("x"));
        Assert.Equal("clock", IslandDisplays.Normalize("clock"));
        Assert.Equal("date", IslandDisplays.Normalize("date"));
        Assert.Equal("full", new SettingsPatch().IslandDisplay("nope").ToJson()["islandDisplay"]!.GetValue<string>());
    }

    [Fact]
    public void Settings_round_trip_through_the_source_generated_context()
    {
        var settings = new IslandSettings { MonitorId = "abc", OnboardingDone = true, IslandDisplay = "clock" };
        string json = JsonSerializer.Serialize(settings, CenterJson.Default.IslandSettings);
        Assert.Contains("\"islandDisplay\":\"clock\"", json);
        Assert.Contains("\"onboardingDone\":true", json);
        Assert.Equal(settings, JsonSerializer.Deserialize(json, CenterJson.Default.IslandSettings));
    }

    [Theory]
    [InlineData("allowed", NotificationAccess.Allowed)]
    [InlineData("denied", NotificationAccess.Denied)]
    [InlineData("unspecified", NotificationAccess.Unspecified)]
    [InlineData("unsupported", NotificationAccess.Unsupported)]
    [InlineData("policy", NotificationAccess.Policy)]
    [InlineData("error", NotificationAccess.Error)]
    [InlineData("whatever", NotificationAccess.Unknown)]
    [InlineData(null, NotificationAccess.Unknown)]
    public void Notification_status_parses(string? wire, NotificationAccess expected) =>
        Assert.Equal(expected, NotificationAccessParser.Parse(wire));
}

public class StringsTests
{
    private static readonly TimeZoneInfo Utc = TimeZoneInfo.Utc;
    private static readonly long Noon = new DateTimeOffset(2026, 10, 6, 12, 0, 0, TimeSpan.Zero).ToUnixTimeMilliseconds();

    private static string Ago(TimeSpan span) => Strings.RelativeTime(Noon - (long)span.TotalMilliseconds, Noon, Utc);

    [Fact]
    public void Relative_times_in_hebrew()
    {
        Assert.Equal("עכשיו", Ago(TimeSpan.FromSeconds(10)));
        Assert.Equal("לפני דקה", Ago(TimeSpan.FromMinutes(1)));
        Assert.Equal("לפני 5 דקות", Ago(TimeSpan.FromMinutes(5)));
        Assert.Equal("לפני שעה", Ago(TimeSpan.FromMinutes(70)));
        Assert.Equal("לפני שעתיים", Ago(TimeSpan.FromMinutes(130)));
        Assert.Equal("לפני 5 שעות", Ago(TimeSpan.FromHours(5)));
        Assert.Equal("לפני 20 שעות", Ago(TimeSpan.FromHours(20)));
        Assert.Equal("אתמול", Ago(TimeSpan.FromHours(30)));
        Assert.Equal("לפני יומיים", Ago(TimeSpan.FromDays(2)));
        Assert.Equal("לפני 4 ימים", Ago(TimeSpan.FromDays(4)));
        Assert.Equal("29.9.2026", Ago(TimeSpan.FromDays(7)));
    }

    [Fact]
    public void A_time_in_the_future_reads_as_now() =>
        Assert.Equal("עכשיו", Strings.RelativeTime(Noon + 60_000, Noon, Utc));

    [Fact]
    public void Settings_labels_match_the_islands_wording()
    {
        Assert.Equal("הפעלה עם Windows", Strings.LaunchWithWindows);
        Assert.Equal("הצע שקט בתחילת פגישה", Strings.MeetingSilence);
        Assert.Equal("5 דק׳", Strings.Minutes(5));
        Assert.Equal("שעה, תאריך ויום", Strings.DisplayFull);
        Assert.Equal("שעה בלבד", Strings.DisplayClock);
        Assert.Equal("תאריך ויום", Strings.DisplayDate);
    }

    [Fact]
    public void Every_visible_string_property_has_text_and_no_placeholder_leftovers()
    {
        foreach (var property in typeof(Strings).GetProperties(System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Static))
        {
            string? value = (string?)property.GetValue(null);
            Assert.False(string.IsNullOrWhiteSpace(value), property.Name);
            Assert.DoesNotMatch(new Regex(@"\{\d\}"), value!);
        }
    }
}

public class LineReaderTests
{
    [Fact]
    public async Task Reads_lines_across_small_chunks_and_ends_with_null()
    {
        using var stream = new MemoryStream(System.Text.Encoding.UTF8.GetBytes("ab\ncd\n\nlast"));
        var reader = new LineReader(stream, 100);
        Assert.Equal("ab", LineReader.Utf8((await reader.ReadLineAsync())!.Value));
        Assert.Equal("cd", LineReader.Utf8((await reader.ReadLineAsync())!.Value));
        Assert.Equal("", LineReader.Utf8((await reader.ReadLineAsync())!.Value));
        Assert.Null(await reader.ReadLineAsync()); // an unterminated tail is not a line
    }

    [Fact]
    public async Task A_line_over_the_limit_throws()
    {
        using var stream = new MemoryStream(System.Text.Encoding.UTF8.GetBytes(new string('a', 20_000) + "\n"));
        var reader = new LineReader(stream, 10_000);
        await Assert.ThrowsAsync<InvalidDataException>(async () => await reader.ReadLineAsync());
    }
}
