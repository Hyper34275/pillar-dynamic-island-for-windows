using System.Text.Json;
using CompanyIsland.Center.Core;

namespace CompanyIsland.Center.Tests;

public class SearchPageTests
{
    [Fact]
    public void Search_page_parses_with_and_without_a_query_id()
    {
        Assert.True(CenterPage.TryParse("search", out CenterPage plain));
        Assert.Equal(CenterPageKind.Search, plain.Kind);
        Assert.Null(plain.QueryId);
        Assert.Equal("search", plain.ToWire());

        Assert.True(CenterPage.TryParse("search:q1-Ab_9", out CenterPage withId));
        Assert.Equal(CenterPageKind.Search, withId.Kind);
        Assert.Equal("q1-Ab_9", withId.QueryId);
        Assert.Equal("search:q1-Ab_9", withId.ToWire());
    }

    [Theory]
    [InlineData("search:")]
    [InlineData("search ")]
    [InlineData("Search")]
    [InlineData("search:a b")]
    [InlineData("search:a/b")]
    [InlineData("search:a:b")]
    [InlineData("search:אבג")]
    public void Invalid_search_pages_are_rejected(string wire)
    {
        Assert.False(CenterPage.TryParse(wire, out CenterPage page));
        Assert.Equal(CenterPageKind.Welcome, page.Kind);
    }

    [Fact]
    public void Query_id_length_is_limited_to_64()
    {
        Assert.True(CenterPage.TryParse("search:" + new string('a', 64), out _));
        Assert.False(CenterPage.TryParse("search:" + new string('a', 65), out _));
    }

    [Fact]
    public void Command_line_names_the_search_page()
    {
        Assert.Equal(new CenterPage(CenterPageKind.Search, QueryId: "q9"), CenterPage.FromArgs(["--page", "search:q9"]));
        Assert.Equal(new CenterPage(CenterPageKind.Search), CenterPage.FromArgs(["--page=search"]));
        Assert.Equal(CenterPage.Welcome, CenterPage.FromArgs(["--page", "search:bad id"]));
        Assert.Equal(new CenterPage(CenterPageKind.Search, QueryId: "q9"), CenterPage.FromCommandLine("--page search:q9"));
    }
}

public class SearchSettingsTests
{
    [Fact]
    public void Patch_sets_the_three_search_switches()
    {
        var patch = new SettingsPatch().AiSearchEnabled(false).AiSearchButton(false).AiSearchHotkey(false);
        IslandSettings next = patch.ApplyTo(new IslandSettings());
        Assert.False(next.AiSearchEnabled);
        Assert.False(next.AiSearchButton);
        Assert.False(next.AiSearchHotkey);

        var json = patch.ToJson();
        Assert.False(json["aiSearchEnabled"]!.GetValue<bool>());
        Assert.False(json["aiSearchButton"]!.GetValue<bool>());
        Assert.False(json["aiSearchHotkey"]!.GetValue<bool>());
    }

    [Fact]
    public void Search_switches_default_to_on_and_a_no_op_patch_changes_nothing()
    {
        var settings = new IslandSettings();
        Assert.True(settings.AiSearchEnabled && settings.AiSearchButton && settings.AiSearchHotkey);
        Assert.Equal(settings, new SettingsPatch().AiSearchEnabled(true).ApplyTo(settings));
    }

    [Fact]
    public void Settings_json_round_trip_reads_the_switches_and_old_files_get_the_defaults()
    {
        using JsonDocument off = JsonDocument.Parse("""{"aiSearchEnabled":false,"aiSearchButton":false,"aiSearchHotkey":false}""");
        IslandSettings read = IslandClient.ReadSettings(off.RootElement);
        Assert.False(read.AiSearchEnabled);
        Assert.False(read.AiSearchButton);
        Assert.False(read.AiSearchHotkey);

        using JsonDocument old = JsonDocument.Parse("""{"islandDisplay":"date"}""");
        IslandSettings defaults = IslandClient.ReadSettings(old.RootElement);
        Assert.True(defaults.AiSearchEnabled && defaults.AiSearchButton && defaults.AiSearchHotkey);
    }
}

public class SearchModelTests
{
    private const string CardJson = """
        {"queryId":"q1","query":"מה יש לאיציק ביומן מחר?","phase":"answer","lang":"he","title":"מחר יש לאיציק 2 פגישות",
         "summary":"09:00 · 14:00","question":null,
         "choices":[{"id":"c1","label":"תיבה","kind":"mailbox","preferred":true}],
         "items":[{"id":"i1","kind":"event","title":"סטטוס","subtitle":null,"time":1800000000000,"endTime":1800003600000,
                   "accent":"#40C8E0","openable":true,"unread":false,"source":"איציק","futureField":1}],
         "total":2,"partial":true,"canExtend":true,"errorCode":null,"sources":["calendar"],"createdAt":1800000000000,"followUp":false}
        """;

    [Fact]
    public void A_card_deserialises_with_every_field()
    {
        using JsonDocument doc = JsonDocument.Parse(CardJson);
        AssistantCard card = IslandClient.ReadCard(doc.RootElement);
        Assert.Equal("q1", card.QueryId);
        Assert.Equal(CardPhases.Answer, card.Phase);
        Assert.Equal(2, card.Total);
        Assert.True(card.Partial);
        Assert.True(card.CanExtend);
        Assert.Null(card.Question);
        Assert.Equal(["calendar"], card.Sources);
        Choice choice = Assert.Single(card.Choices);
        Assert.True(SearchFormat.IsMailboxChoice(choice));
        Assert.True(choice.Preferred);
        AssistantItem item = Assert.Single(card.Items);
        Assert.Equal("event", item.Kind);
        Assert.Equal(1800003600000, item.EndTime);
        Assert.True(item.Openable);
        Assert.Equal("איציק", item.Source);
    }

    [Fact]
    public void Results_and_history_deserialise_groups_with_mailboxes()
    {
        string json = $$"""
            [{"card":{{CardJson}},"groups":[
              {"kind":"mail","title":"","mailbox":{"id":"m1","name":"תיבה משותפת"},"items":[],"truncated":true,"errorCode":"MAIL-105"},
              {"kind":"calendar","title":"יומן","mailbox":null,"items":[],"truncated":false,"errorCode":null}]}]
            """;
        using JsonDocument doc = JsonDocument.Parse(json);
        SearchResults[] history = doc.RootElement.Deserialize(CenterJson.Default.SearchResultsArray)!;
        SearchResults one = Assert.Single(history);
        Assert.Equal("q1", one.Card.QueryId);
        Assert.Equal(2, one.Groups.Count);
        Assert.Equal("תיבה משותפת", one.Groups[0].Mailbox!.Name);
        Assert.True(one.Groups[0].Truncated);
        Assert.Equal("MAIL-105", one.Groups[0].ErrorCode);
    }

    [Fact]
    public void Unknown_phases_and_kinds_do_not_fail_the_payload()
    {
        using JsonDocument doc = JsonDocument.Parse("""{"queryId":"q2","phase":"someday","items":[{"id":"x","kind":"hologram","title":"t"}]}""");
        AssistantCard card = IslandClient.ReadCard(doc.RootElement);
        Assert.Equal("someday", card.Phase);
        Assert.Equal("hologram", card.Items[0].Kind);
        Assert.False(SearchFormat.NeedsAnswer(card));
    }

    [Fact]
    public void Search_ready_payload_needs_a_valid_query_id()
    {
        Assert.Equal("q1-A", IslandClient.ReadSearchReady(JsonDocument.Parse("""{"queryId":"q1-A"}""").RootElement));
        Assert.Null(IslandClient.ReadSearchReady(JsonDocument.Parse("""{"queryId":"a b"}""").RootElement));
        Assert.Null(IslandClient.ReadSearchReady(JsonDocument.Parse("""{"queryId":5}""").RootElement));
        Assert.Null(IslandClient.ReadSearchReady(JsonDocument.Parse("[]").RootElement));
        Assert.Null(IslandClient.ReadSearchReady(default));
    }
}

public class SearchFormatTests
{
    // 2027-01-15 12:00 UTC
    private static readonly long Noon = new DateTimeOffset(2027, 1, 15, 12, 0, 0, TimeSpan.Zero).ToUnixTimeMilliseconds();
    private static readonly TimeZoneInfo Utc = TimeZoneInfo.Utc;

    private static long At(int dayOffset, int hour, int minute = 0) =>
        new DateTimeOffset(2027, 1, 15 + dayOffset, hour, minute, 0, TimeSpan.Zero).ToUnixTimeMilliseconds();

    [Fact]
    public void Event_today_shows_a_time_range()
    {
        var item = new AssistantItem { Kind = "event", Time = At(0, 14), EndTime = At(0, 15, 30) };
        Assert.Equal("14:00–15:30", SearchFormat.ItemTime(item, Noon, Utc));
    }

    [Fact]
    public void Event_tomorrow_and_yesterday_carry_the_day()
    {
        Assert.Equal("מחר 09:00", SearchFormat.ItemTime(new AssistantItem { Kind = "event", Time = At(1, 9) }, Noon, Utc));
        Assert.Equal(
            "אתמול 09:00–10:00",
            SearchFormat.ItemTime(new AssistantItem { Kind = "event", Time = At(-1, 9), EndTime = At(-1, 10) }, Noon, Utc));
        Assert.Equal("20.1 08:00", SearchFormat.ItemTime(new AssistantItem { Kind = "event", Time = At(5, 8) }, Noon, Utc));
    }

    [Fact]
    public void Mail_and_files_show_a_relative_time_and_no_time_shows_nothing()
    {
        Assert.Equal("לפני שעתיים", SearchFormat.ItemTime(new AssistantItem { Kind = "mail", Time = At(0, 10) }, Noon, Utc));
        Assert.Equal("אתמול", SearchFormat.ItemTime(new AssistantItem { Kind = "file", Time = At(-1, 9) }, Noon, Utc));
        Assert.Equal("", SearchFormat.ItemTime(new AssistantItem { Kind = "note" }, Noon, Utc));
    }

    [Fact]
    public void Group_titles_prefer_the_mailbox_name()
    {
        Assert.Equal("יובל כהן", SearchFormat.GroupTitle(new ResultGroup { Kind = "mail", Title = "מיילים", Mailbox = new MailboxRef { Name = "יובל כהן" } }));
        Assert.Equal("מיילים", SearchFormat.GroupTitle(new ResultGroup { Kind = "mail", Title = "מיילים" }));
        Assert.Equal(Strings.SearchGroupFiles, SearchFormat.GroupTitle(new ResultGroup { Kind = "files" }));
        Assert.Equal(Strings.SearchGroupOther, SearchFormat.GroupTitle(new ResultGroup { Kind = "zzz" }));
    }

    [Fact]
    public void Empty_groups_are_dropped_unless_they_failed()
    {
        var results = new SearchResults
        {
            Groups =
            [
                new ResultGroup { Kind = "mail", Items = [new AssistantItem { Id = "a", Kind = "mail" }] },
                new ResultGroup { Kind = "files" },
                new ResultGroup { Kind = "notes", ErrorCode = "FILES-101" },
            ],
        };
        Assert.Equal(["mail", "notes"], SearchFormat.EffectiveGroups(results).Select(g => g.Kind));
    }

    [Fact]
    public void Items_without_groups_are_grouped_by_kind()
    {
        var results = new SearchResults
        {
            Card = new AssistantCard
            {
                Items =
                [
                    new AssistantItem { Id = "1", Kind = "event" },
                    new AssistantItem { Id = "2", Kind = "slot" },
                    new AssistantItem { Id = "3", Kind = "file" },
                ],
            },
        };
        var groups = SearchFormat.EffectiveGroups(results);
        Assert.Equal(["calendar", "files"], groups.Select(g => g.Kind));
        Assert.Equal(2, groups[0].Items.Count);
        Assert.Empty(SearchFormat.EffectiveGroups(new SearchResults()));
    }

    [Fact]
    public void Item_detail_joins_the_parts_that_exist()
    {
        var item = new AssistantItem { Subtitle = "דנה", Source = "תיבה" };
        Assert.Equal("דנה · תיבה", SearchFormat.ItemDetail(item, true));
        Assert.Equal("דנה", SearchFormat.ItemDetail(item, false));
        Assert.Equal("", SearchFormat.ItemDetail(new AssistantItem { Subtitle = " " }, true));
    }

    [Theory]
    [InlineData("#40C8E0", true, 0x40, 0xC8, 0xE0)]
    [InlineData("#00ff7f", true, 0x00, 0xFF, 0x7F)]
    [InlineData("40C8E0", false, 0, 0, 0)]
    [InlineData("#40C8E", false, 0, 0, 0)]
    [InlineData("#GG0000", false, 0, 0, 0)]
    [InlineData(null, false, 0, 0, 0)]
    public void Accent_parsing(string? text, bool ok, int r, int g, int b)
    {
        Assert.Equal(ok, SearchFormat.TryParseAccent(text, out byte rr, out byte gg, out byte bb));
        Assert.Equal((r, g, b), ((int)rr, (int)gg, (int)bb));
    }

    [Fact]
    public void Query_lookup_finds_the_latest_match()
    {
        var history = new List<SearchResults>
        {
            new() { Card = new AssistantCard { QueryId = "a" } },
            new() { Card = new AssistantCard { QueryId = "b" } },
        };
        Assert.Equal(1, SearchFormat.IndexOfQuery(history, "b"));
        Assert.Equal(-1, SearchFormat.IndexOfQuery(history, "zz"));
        Assert.Equal(-1, SearchFormat.IndexOfQuery(history, null));
    }

    [Fact]
    public void Choices_need_an_answer_only_in_the_choices_phase()
    {
        var choices = new List<Choice> { new() { Id = "x", Label = "y" } };
        Assert.True(SearchFormat.NeedsAnswer(new AssistantCard { Phase = CardPhases.Choices, Choices = choices }));
        Assert.False(SearchFormat.NeedsAnswer(new AssistantCard { Phase = CardPhases.Answer, Choices = choices }));
        Assert.False(SearchFormat.NeedsAnswer(new AssistantCard { Phase = CardPhases.Choices }));
    }

    [Fact]
    public void More_info_shows_only_when_the_card_has_more_than_the_groups()
    {
        Assert.Equal("מוצגות 3 מתוך 12 תוצאות", SearchFormat.MoreInfo(new AssistantCard { Total = 12 }, 3));
        Assert.Equal("", SearchFormat.MoreInfo(new AssistantCard { Total = 3 }, 3));
        Assert.Equal("", SearchFormat.MoreInfo(new AssistantCard { Total = 3 }, 0));
    }

    [Fact]
    public void Examples_and_strings_are_hebrew_and_not_empty()
    {
        Assert.True(SearchFormat.ExampleQuestions.Count >= 3);
        Assert.All(SearchFormat.ExampleQuestions, q => Assert.Matches("[֐-׿]", q));
        Assert.Matches("[֐-׿]", Strings.SettingsSearchPrivacy);
    }
}

public class SearchClientTests
{
    private static readonly IslandClientOptions Fast = new()
    {
        ConnectTimeout = TimeSpan.FromMilliseconds(300),
        ReconnectDelay = TimeSpan.FromMilliseconds(50),
        HelloTimeout = TimeSpan.FromSeconds(2),
        RequestTimeout = TimeSpan.FromMilliseconds(400),
    };

    private static async Task<(FakeIsland island, IslandClient client)> ConnectedAsync(FakeIsland.Handler handler)
    {
        var island = new FakeIsland(handler);
        island.Start();
        var client = new IslandClient(island.PipeName, Fast);
        client.Start();
        DateTime end = DateTime.UtcNow.AddSeconds(5);
        while (!client.IsConnected)
        {
            if (DateTime.UtcNow > end)
            {
                throw new TimeoutException();
            }

            await Task.Delay(10);
        }

        return (island, client);
    }

    private const string Card = """{"queryId":"q1","query":"x","phase":"answer","title":"t","summary":"","choices":[],"items":[],"total":0}""";

    [Fact]
    public async Task Submit_sends_the_text_and_waits_longer_than_the_normal_request_timeout()
    {
        var (island, client) = await ConnectedAsync((s, cmd, a) =>
        {
            if (cmd == "searchSubmit")
            {
                Thread.Sleep(800); // longer than RequestTimeout (400 ms) but within the 45 s search timeout
                return Card;
            }

            return FakeIsland.DefaultHandler(s, cmd, a);
        });
        await using (island)
        await using (client)
        {
            AssistantCard card = await client.SearchSubmitAsync("תמצא את המייל עם המילה תקציב");
            Assert.Equal("q1", card.QueryId);

            var request = island.Requests.Single(r => r.Cmd == "searchSubmit");
            using JsonDocument args = JsonDocument.Parse(request.Args);
            Assert.Equal("תמצא את המייל עם המילה תקציב", args.RootElement.GetProperty("text").GetString());
        }
    }

    [Fact]
    public async Task Choose_extend_open_results_and_history_send_the_documented_arguments()
    {
        var (island, client) = await ConnectedAsync((s, cmd, a) => cmd switch
        {
            "searchChoose" or "searchExtend" => Card,
            "searchOpen" => "null",
            "searchResults" => $$"""{"card":{{Card}},"groups":[]}""",
            "searchHistory" => $$"""[{"card":{{Card}},"groups":[]}]""",
            _ => FakeIsland.DefaultHandler(s, cmd, a),
        });
        await using (island)
        await using (client)
        {
            await client.SearchChooseAsync("q1", "all", true);
            await client.SearchExtendAsync("q1");
            await client.SearchOpenAsync("q1", "i7");
            Assert.Equal("q1", (await client.SearchResultsAsync("q1")).Card.QueryId);
            Assert.Single(await client.SearchHistoryAsync());

            static JsonElement Args(FakeIsland island, string cmd) =>
                JsonDocument.Parse(island.Requests.Single(r => r.Cmd == cmd).Args).RootElement.Clone();

            JsonElement choose = Args(island, "searchChoose");
            Assert.Equal("q1", choose.GetProperty("queryId").GetString());
            Assert.Equal("all", choose.GetProperty("optionId").GetString());
            Assert.True(choose.GetProperty("remember").GetBoolean());
            Assert.Equal("q1", Args(island, "searchExtend").GetProperty("queryId").GetString());
            JsonElement open = Args(island, "searchOpen");
            Assert.Equal("i7", open.GetProperty("itemId").GetString());
            Assert.Equal("q1", Args(island, "searchResults").GetProperty("queryId").GetString());
        }
    }

    [Theory]
    [InlineData("mb:0123456789abcdef")]
    [InlineData("cal:fedcba9876543210")]
    [InlineData("all")]
    public async Task Choose_sends_the_choice_ids_the_island_mints_unchanged(string optionId)
    {
        var (island, client) = await ConnectedAsync((s, cmd, a) => cmd == "searchChoose" ? Card : FakeIsland.DefaultHandler(s, cmd, a));
        await using (island)
        await using (client)
        {
            await client.SearchChooseAsync("q1", optionId, false);
            JsonElement args = JsonDocument.Parse(island.Requests.Single(r => r.Cmd == "searchChoose").Args).RootElement;
            Assert.Equal(optionId, args.GetProperty("optionId").GetString());
            // The shape the island's validator accepts: "all", or mb:/cal: plus [A-Za-z0-9_-]{1,64}.
            Assert.Matches("^(all|(mb|cal):[A-Za-z0-9_-]{1,64})$", args.GetProperty("optionId").GetString());
        }
    }

    private static SearchResults Turn(string id, string query) => new() { Card = new AssistantCard { QueryId = id, Query = query } };

    [Fact]
    public void Choosing_or_searching_longer_adds_no_second_bubble_to_the_turn_it_works_on()
    {
        // An old card (created long ago): no clock decides anything.
        var history = new List<SearchResults> { Turn("a", "mail from dana"), Turn("b", "files about tax") };
        foreach (SearchPendingKind kind in new[] { SearchPendingKind.Choose, SearchPendingKind.Extend })
        {
            var pending = new SearchPending(kind, "files about tax", "b", "b");
            Assert.Equal(1, pending.TurnIndex(history));
            Assert.False(pending.NeedsOwnTurn(history), kind.ToString());
        }

        // The turn is gone from the history (it expired): the question gets a turn of its own.
        var lost = new SearchPending(SearchPendingKind.Choose, "x", "zz", "b");
        Assert.Equal(-1, lost.TurnIndex(history));
        Assert.True(lost.NeedsOwnTurn(history));
    }

    [Fact]
    public void A_new_question_shows_until_a_new_turn_with_its_text_appears()
    {
        var before = new List<SearchResults> { Turn("a", "same question") };
        string? last = SearchFormat.LastQueryId(before);
        var pending = new SearchPending(SearchPendingKind.Submit, "same question", null, last);

        // Asked again right away: the old identical turn must not hide the new question.
        Assert.True(pending.NeedsOwnTurn(before));
        Assert.Equal(-1, pending.TurnIndex(before));

        // The island recorded it as a new turn.
        var after = new List<SearchResults> { Turn("a", "same question"), Turn("b", "same question") };
        Assert.False(pending.NeedsOwnTurn(after));

        // First question ever.
        var first = new SearchPending(SearchPendingKind.Submit, "hello", null, SearchFormat.LastQueryId(new List<SearchResults>()));
        Assert.Null(SearchFormat.LastQueryId(new List<SearchResults>()));
        Assert.True(first.NeedsOwnTurn(new List<SearchResults>()));
        Assert.False(first.NeedsOwnTurn(new List<SearchResults> { Turn("a", "hello") }));
    }

    [Fact]
    public async Task An_island_error_surfaces_with_its_code()
    {
        var (island, client) = await ConnectedAsync((s, cmd, a) =>
            cmd == "searchOpen" ? throw new FakeError("APP-041: search expired") : FakeIsland.DefaultHandler(s, cmd, a));
        await using (island)
        await using (client)
        {
            IslandException e = await Assert.ThrowsAsync<IslandException>(() => client.SearchOpenAsync("q1", "i1"));
            Assert.Equal("APP-041", e.Code);
        }
    }

    [Fact]
    public async Task A_blank_question_is_refused_before_it_is_sent()
    {
        var (island, client) = await ConnectedAsync(FakeIsland.DefaultHandler);
        await using (island)
        await using (client)
        {
            await Assert.ThrowsAnyAsync<ArgumentException>(() => client.SearchSubmitAsync("  "));
            Assert.DoesNotContain(island.Requests, r => r.Cmd == "searchSubmit");
        }
    }
}
