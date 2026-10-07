using System.Text.Json;
using CompanyIsland.Center.Core;

namespace CompanyIsland.Center.Tests;

public class IslandClientTests
{
    private static readonly IslandClientOptions Fast = new()
    {
        ConnectTimeout = TimeSpan.FromMilliseconds(300),
        ReconnectDelay = TimeSpan.FromMilliseconds(50),
        HelloTimeout = TimeSpan.FromSeconds(2),
        RequestTimeout = TimeSpan.FromSeconds(3),
    };

    private static async Task WaitUntil(Func<bool> condition, int timeoutMs = 5000)
    {
        DateTime end = DateTime.UtcNow.AddMilliseconds(timeoutMs);
        while (!condition())
        {
            if (DateTime.UtcNow > end)
            {
                throw new TimeoutException("condition not met in time");
            }

            await Task.Delay(10);
        }
    }

    private static async Task<(FakeIsland island, IslandClient client)> ConnectedAsync(
        FakeIsland.Handler? handler = null, IslandClientOptions? options = null)
    {
        var island = new FakeIsland(handler);
        island.Start();
        var client = new IslandClient(island.PipeName, options ?? Fast);
        client.Start();
        await WaitUntil(() => client.IsConnected);
        return (island, client);
    }

    [Fact]
    public async Task Hello_is_the_first_request_and_reports_the_version()
    {
        var (island, client) = await ConnectedAsync();
        await using (island)
        await using (client)
        {
            Assert.True(island.Requests.TryPeek(out var first));
            Assert.Equal("hello", first.Cmd);
            using JsonDocument args = JsonDocument.Parse(first.Args);
            Assert.Equal("center", args.RootElement.GetProperty("client").GetString());
            Assert.Equal(1, args.RootElement.GetProperty("protocol").GetInt32());
            Assert.Equal("1.0.4", client.AppVersion);
        }
    }

    [Fact]
    public async Task Request_response_returns_typed_settings_and_ignores_unknown_fields()
    {
        var (island, client) = await ConnectedAsync();
        await using (island)
        await using (client)
        {
            IslandSettings settings = await client.GetSettingsAsync();
            Assert.False(settings.LaunchWithWindows);
            Assert.Equal(10, settings.ReminderMinutes);
            Assert.Equal(IslandDisplays.Clock, settings.IslandDisplay);
            Assert.Null(settings.MonitorId);
            Assert.True(settings.MeetingInvitesEnabled); // missing field keeps its default
        }
    }

    [Fact]
    public async Task Unknown_island_display_is_normalised_to_full()
    {
        var (island, client) = await ConnectedAsync((s, cmd, a) =>
            cmd == "getSettings" ? """{"islandDisplay":"sparkly"}""" : FakeIsland.DefaultHandler(s, cmd, a));
        await using (island)
        await using (client)
        {
            Assert.Equal(IslandDisplays.Full, (await client.GetSettingsAsync()).IslandDisplay);
        }
    }

    [Fact]
    public async Task Update_settings_sends_only_the_patch_and_a_null_monitor_as_json_null()
    {
        var (island, client) = await ConnectedAsync((s, cmd, a) =>
            cmd == "updateSettings" ? """{"islandDisplay":"date","monitorId":null}""" : FakeIsland.DefaultHandler(s, cmd, a));
        await using (island)
        await using (client)
        {
            IslandSettings result = await client.UpdateSettingsAsync(new SettingsPatch().IslandDisplay("date").MonitorId(null));
            Assert.Equal("date", result.IslandDisplay);

            var request = island.Requests.Single(r => r.Cmd == "updateSettings");
            using JsonDocument doc = JsonDocument.Parse(request.Args);
            JsonElement patch = doc.RootElement.GetProperty("patch");
            Assert.Equal("date", patch.GetProperty("islandDisplay").GetString());
            Assert.Equal(JsonValueKind.Null, patch.GetProperty("monitorId").ValueKind);
            Assert.False(patch.TryGetProperty("launchWithWindows", out _));
        }
    }

    [Fact]
    public async Task Notes_save_sends_camel_case_notes_and_reads_the_answer()
    {
        var (island, client) = await ConnectedAsync((s, cmd, a) => cmd == "notesSave"
            ? """[{"id":"abc","text":"שלום","createdAt":5,"updatedAt":6,"pinned":true,"extra":1}]"""
            : FakeIsland.DefaultHandler(s, cmd, a));
        await using (island)
        await using (client)
        {
            var note = new Note { Id = "abc", Text = "שלום", CreatedAt = 5, UpdatedAt = 6, Pinned = true };
            IReadOnlyList<Note> saved = await client.NotesSaveAsync([note]);
            Assert.Equal(note, Assert.Single(saved));

            var request = island.Requests.Single(r => r.Cmd == "notesSave");
            using JsonDocument doc = JsonDocument.Parse(request.Args);
            JsonElement sent = doc.RootElement.GetProperty("notes")[0];
            Assert.Equal("abc", sent.GetProperty("id").GetString());
            Assert.Equal("שלום", sent.GetProperty("text").GetString());
            Assert.Equal(5, sent.GetProperty("createdAt").GetInt64());
            Assert.True(sent.GetProperty("pinned").GetBoolean());
        }
    }

    [Fact]
    public async Task Monitors_and_notification_status_parse()
    {
        var (island, client) = await ConnectedAsync((s, cmd, a) => cmd switch
        {
            "getMonitors" => """[{"id":"m1","name":"Dell","primary":true,"isPrimary":true,"width":1920,"height":1080,"scale":1.25}]""",
            "getNotificationStatus" => "\"unspecified\"",
            _ => FakeIsland.DefaultHandler(s, cmd, a),
        });
        await using (island)
        await using (client)
        {
            MonitorInfo monitor = Assert.Single(await client.GetMonitorsAsync());
            Assert.Equal("m1", monitor.Id);
            Assert.True(monitor.IsPrimaryDisplay);
            Assert.Equal(1.25, monitor.Scale);
            Assert.Equal(NotificationAccess.Unspecified, await client.GetNotificationStatusAsync());
        }
    }

    [Fact]
    public async Task Concurrent_requests_are_correlated_by_id_even_when_answered_out_of_order()
    {
        var gate = new TaskCompletionSource();
        var (island, client) = await ConnectedAsync((s, cmd, a) =>
        {
            if (cmd == "getNotificationStatus")
            {
                gate.Task.Wait(2000); // answered after the second request
                return "\"allowed\"";
            }

            if (cmd == "getMonitors")
            {
                gate.TrySetResult();
                return "[]";
            }

            return FakeIsland.DefaultHandler(s, cmd, a);
        });
        await using (island)
        await using (client)
        {
            Task<NotificationAccess> slow = client.GetNotificationStatusAsync();
            Task<IReadOnlyList<MonitorInfo>> fast = client.GetMonitorsAsync();
            Assert.Empty(await fast);
            Assert.Equal(NotificationAccess.Allowed, await slow);
        }
    }

    [Fact]
    public async Task Error_response_becomes_an_island_exception_with_its_code()
    {
        var (island, client) = await ConnectedAsync((s, cmd, a) =>
            cmd == "notesSave" ? throw new FakeError("APP-002: cannot write notes") : FakeIsland.DefaultHandler(s, cmd, a));
        await using (island)
        await using (client)
        {
            IslandException e = await Assert.ThrowsAsync<IslandException>(() => client.NotesSaveAsync([]));
            Assert.Equal("APP-002", e.Code);
            Assert.Equal("cannot write notes", e.Message);
        }
    }

    [Fact]
    public async Task A_request_the_island_never_answers_times_out()
    {
        var (island, client) = await ConnectedAsync((s, cmd, a) => cmd == "openLogDir" ? null : FakeIsland.DefaultHandler(s, cmd, a));
        await using (island)
        await using (client)
        {
            await Assert.ThrowsAsync<IslandTimeoutException>(
                () => client.RequestAsync("openLogDir", timeout: TimeSpan.FromMilliseconds(150)));
            // The connection is still usable afterwards.
            Assert.True(client.IsConnected);
            Assert.NotNull(await client.GetSettingsAsync());
        }
    }

    [Fact]
    public async Task Requests_fail_fast_while_disconnected()
    {
        await using var client = new IslandClient("CompanyIsland.Center.test.nobody." + Guid.NewGuid().ToString("N"), Fast);
        await Assert.ThrowsAsync<IslandDisconnectedException>(() => client.GetSettingsAsync());
    }

    [Fact]
    public async Task A_request_while_disconnected_returns_a_faulted_task_instead_of_throwing()
    {
        await using var client = new IslandClient("CompanyIsland.Center.test.nobody." + Guid.NewGuid().ToString("N"), Fast);
        Task<JsonElement> task = client.RequestAsync("getSettings");
        Assert.True(task.IsFaulted);
        await Assert.ThrowsAsync<IslandDisconnectedException>(() => task);
    }

    [Fact]
    public async Task Cancelling_the_token_cancels_the_request()
    {
        var (island, client) = await ConnectedAsync((s, cmd, a) => cmd == "openLogDir" ? null : FakeIsland.DefaultHandler(s, cmd, a));
        await using (island)
        await using (client)
        {
            using var cts = new CancellationTokenSource(100);
            await Assert.ThrowsAnyAsync<OperationCanceledException>(() => client.RequestAsync("openLogDir", ct: cts.Token));
        }
    }

    [Fact]
    public async Task Events_are_raised_in_order_with_their_payload()
    {
        var (island, client) = await ConnectedAsync();
        await using (island)
        await using (client)
        {
            var seen = new List<(string, string)>();
            client.EventReceived += e =>
            {
                lock (seen)
                {
                    seen.Add((e.Name, e.Payload.GetRawText()));
                }
            };

            var session = island.Sessions.First();
            await session.PushEventAsync("settings-changed", """{"islandDisplay":"date","future":true}""");
            await session.PushEventAsync("notes-changed", """[{"id":"n1","text":"x","createdAt":1,"updatedAt":1,"pinned":false}]""");
            await session.PushEventAsync("navigate", """{"page":"notes-new"}""");
            await WaitUntil(() => { lock (seen) { return seen.Count == 3; } });

            Assert.Equal(["settings-changed", "notes-changed", "navigate"], seen.Select(s => s.Item1));
            Assert.Equal(
                "date",
                IslandClient.ReadSettings(JsonDocument.Parse(seen[0].Item2).RootElement).IslandDisplay);
            Assert.Equal("n1", IslandClient.ReadNotes(JsonDocument.Parse(seen[1].Item2).RootElement).Single().Id);
            Assert.True(CenterPage.TryParse(
                JsonDocument.Parse(seen[2].Item2).RootElement.GetProperty("page").GetString(), out CenterPage page));
            Assert.Equal(CenterPageKind.NotesNew, page.Kind);
        }
    }

    [Fact]
    public async Task A_subscriber_that_throws_does_not_break_the_connection()
    {
        var (island, client) = await ConnectedAsync();
        await using (island)
        await using (client)
        {
            int calls = 0;
            client.EventReceived += _ =>
            {
                calls++;
                throw new InvalidOperationException("boom");
            };
            var session = island.Sessions.First();
            await session.PushEventAsync("notes-changed", "[]");
            await session.PushEventAsync("notes-changed", "[]");
            await WaitUntil(() => calls == 2);
            Assert.True(client.IsConnected);
        }
    }

    [Fact]
    public async Task Responses_split_across_writes_and_packed_into_one_write_are_framed_correctly()
    {
        var (island, client) = await ConnectedAsync((s, cmd, a) => cmd == "getSettings" ? null : FakeIsland.DefaultHandler(s, cmd, a));
        await using (island)
        await using (client)
        {
            var seen = new List<string>();
            client.EventReceived += e => { lock (seen) { seen.Add(e.Name); } };
            var session = island.Sessions.First();

            // Two events in a single write, then one event dribbled out a few bytes at a time.
            await session.SendRawAsync("{\"event\":\"a\",\"payload\":1}\n{\"event\":\"b\",\"payload\":2}\n");
            byte[] third = System.Text.Encoding.UTF8.GetBytes("{\"event\":\"c\",\"payload\":\"שלום\"}\n");
            for (int i = 0; i < third.Length; i += 3)
            {
                await session.SendBytesAsync(third[i..Math.Min(third.Length, i + 3)]);
                await Task.Delay(2);
            }

            await WaitUntil(() => { lock (seen) { return seen.Count == 3; } });
            Assert.Equal(["a", "b", "c"], seen);
        }
    }

    [Fact]
    public async Task An_oversize_line_closes_the_connection_and_the_client_reconnects()
    {
        var options = new IslandClientOptions
        {
            ConnectTimeout = Fast.ConnectTimeout,
            ReconnectDelay = Fast.ReconnectDelay,
            HelloTimeout = Fast.HelloTimeout,
            RequestTimeout = Fast.RequestTimeout,
            MaxLineBytes = 1024,
        };
        var (island, client) = await ConnectedAsync(options: options);
        await using (island)
        await using (client)
        {
            var states = new List<ConnectionState>();
            client.StateChanged += s => { lock (states) { states.Add(s); } };

            var first = island.Sessions.First();
            await first.SendRawAsync("{\"event\":\"big\",\"payload\":\"" + new string('x', 5000) + "\"}\n");

            await WaitUntil(() => island.Sessions.Count >= 2 && client.IsConnected);
            lock (states)
            {
                Assert.Contains(ConnectionState.Disconnected, states);
            }
        }
    }

    [Fact]
    public async Task A_line_of_exactly_the_limit_is_accepted()
    {
        const int limit = 512;
        var options = new IslandClientOptions
        {
            ConnectTimeout = Fast.ConnectTimeout,
            ReconnectDelay = Fast.ReconnectDelay,
            MaxLineBytes = limit,
        };
        var (island, client) = await ConnectedAsync(options: options);
        await using (island)
        await using (client)
        {
            string prefix = "{\"event\":\"ok\",\"payload\":\"";
            const string suffix = "\"}";
            string line = prefix + new string('y', limit - prefix.Length - suffix.Length) + suffix;
            Assert.Equal(limit, System.Text.Encoding.UTF8.GetByteCount(line));
            int calls = 0;
            client.EventReceived += _ => Interlocked.Increment(ref calls);
            await island.Sessions.First().SendRawAsync(line + "\n");
            await WaitUntil(() => calls == 1);
            Assert.True(client.IsConnected);
        }
    }

    [Fact]
    public async Task An_oversize_request_is_refused_before_it_is_sent()
    {
        var options = new IslandClientOptions
        {
            ConnectTimeout = Fast.ConnectTimeout,
            ReconnectDelay = Fast.ReconnectDelay,
            MaxLineBytes = 2048,
        };
        var (island, client) = await ConnectedAsync(options: options);
        await using (island)
        await using (client)
        {
            var big = new Note { Id = "a", Text = new string('z', 5000), CreatedAt = 1, UpdatedAt = 1 };
            IslandException e = await Assert.ThrowsAsync<IslandException>(() => client.NotesSaveAsync([big]));
            Assert.Equal("client-too-large", e.Code);
            Assert.DoesNotContain(island.Requests, r => r.Cmd == "notesSave");
            Assert.True(client.IsConnected);
        }
    }

    [Fact]
    public async Task Pending_requests_fail_when_the_island_goes_away_and_the_client_reconnects()
    {
        var (island, client) = await ConnectedAsync((s, cmd, a) => cmd == "openLogDir" ? null : FakeIsland.DefaultHandler(s, cmd, a));
        await using (island)
        await using (client)
        {
            Task pending = client.RequestAsync("openLogDir");
            await WaitUntil(() => island.Requests.Any(r => r.Cmd == "openLogDir"));
            island.Sessions.First().Close();

            await Assert.ThrowsAsync<IslandDisconnectedException>(() => pending);
            await WaitUntil(() => island.Sessions.Count >= 2 && client.IsConnected);
            Assert.Equal(2, island.Requests.Count(r => r.Cmd == "hello")); // a fresh hello on the new connection
            Assert.NotNull(await client.GetSettingsAsync());
        }
    }

    [Fact]
    public async Task The_client_connects_when_the_island_starts_later()
    {
        await using var island = new FakeIsland();
        await using var client = new IslandClient(island.PipeName, Fast);
        client.Start();
        await Task.Delay(200);
        Assert.False(client.IsConnected);
        Assert.Equal(ConnectionState.Disconnected, client.State == ConnectionState.Connecting ? ConnectionState.Disconnected : client.State);

        island.Start();
        await WaitUntil(() => client.IsConnected);
    }

    [Fact]
    public async Task An_unsupported_protocol_is_not_treated_as_connected()
    {
        await using var island = new FakeIsland((s, cmd, a) =>
            cmd == "hello" ? """{"protocol":2,"appVersion":"9.9.9"}""" : FakeIsland.DefaultHandler(s, cmd, a));
        island.Start();
        await using var client = new IslandClient(island.PipeName, Fast);
        client.Start();
        await WaitUntil(() => client.LastFailure == "APP-031");
        Assert.False(client.IsConnected);
    }

    [Fact]
    public async Task A_hello_the_island_refuses_keeps_the_client_disconnected_and_retrying()
    {
        await using var island = new FakeIsland((s, cmd, a) =>
            cmd == "hello" ? throw new FakeError("APP-031: unsupported protocol") : FakeIsland.DefaultHandler(s, cmd, a));
        island.Start();
        await using var client = new IslandClient(island.PipeName, Fast);
        client.Start();
        await WaitUntil(() => island.Requests.Count(r => r.Cmd == "hello") >= 2);
        Assert.False(client.IsConnected);
        Assert.Equal("APP-031", client.LastFailure);
    }

    [Fact]
    public async Task Garbage_lines_are_ignored()
    {
        var (island, client) = await ConnectedAsync();
        await using (island)
        await using (client)
        {
            await island.Sessions.First().SendRawAsync("not json\n\n[1,2]\n{\"id\":99999,\"ok\":true}\n");
            Assert.NotNull(await client.GetSettingsAsync());
            Assert.True(client.IsConnected);
        }
    }

    [Theory]
    [InlineData("""{"id":null,"ok":false,"error":"APP-031: bad request"}""")]
    [InlineData("""{"id":"7","ok":true}""")]
    [InlineData("""{"id":true,"ok":true}""")]
    [InlineData("""{"id":[1],"ok":true}""")]
    [InlineData("""{"id":{"a":1},"ok":true}""")]
    [InlineData("""{"id":1.5,"ok":true}""")]
    [InlineData("""{"ok":false,"error":"no id at all"}""")]
    public async Task Reply_without_a_usable_id_is_ignored_and_the_connection_survives(string line)
    {
        var (island, client) = await ConnectedAsync();
        await using (island)
        await using (client)
        {
            int sessions = island.Sessions.Count;
            await island.Sessions.First().SendRawAsync(line + "\n");
            // A normal request on the same connection still works, so the id-less line did not end it.
            Assert.NotNull(await client.GetSettingsAsync());
            Assert.True(client.IsConnected);
            Assert.Equal(sessions, island.Sessions.Count);
        }
    }

    [Fact]
    public async Task Id_null_error_does_not_fail_a_request_that_is_waiting()
    {
        var (island, client) = await ConnectedAsync((s, cmd, a) =>
        {
            if (cmd == "getSettings")
            {
                // The island answers an unparsable line with id null; a real answer follows on the same connection.
                _ = s.SendRawAsync("""{"id":null,"ok":false,"error":"APP-031: invalid request"}""" + "\n");
            }

            return FakeIsland.DefaultHandler(s, cmd, a);
        });
        await using (island)
        await using (client)
        {
            IslandSettings settings = await client.GetSettingsAsync();
            Assert.Equal(10, settings.ReminderMinutes);
            Assert.True(client.IsConnected);
            Assert.Single(island.Sessions);
        }
    }

    [Fact]
    public async Task Show_island_always_sends_the_tab_and_an_explicit_show_intent()
    {
        var (island, client) = await ConnectedAsync();
        await using (island)
        await using (client)
        {
            await client.ShowIslandAsync(IslandTabs.Settings);

            var request = island.Requests.Single(r => r.Cmd == "showIsland");
            using JsonDocument doc = JsonDocument.Parse(request.Args);
            Assert.Equal("settings", doc.RootElement.GetProperty("tab").GetString());
            Assert.True(doc.RootElement.GetProperty("show").GetBoolean());
        }
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    public async Task Show_island_refuses_a_missing_tab(string? tab)
    {
        var (island, client) = await ConnectedAsync();
        await using (island)
        await using (client)
        {
            await Assert.ThrowsAnyAsync<ArgumentException>(() => client.ShowIslandAsync(tab!));
            Assert.DoesNotContain(island.Requests, r => r.Cmd == "showIsland");
        }
    }

    [Fact]
    public async Task Dispose_stops_the_reconnect_loop()
    {
        var island = new FakeIsland();
        island.Start();
        var client = new IslandClient(island.PipeName, Fast);
        client.Start();
        await WaitUntil(() => client.IsConnected);
        await client.DisposeAsync();
        int before = island.Sessions.Count;
        await Task.Delay(300);
        Assert.Equal(before, island.Sessions.Count);
        await island.DisposeAsync();
    }

    [Fact]
    public async Task ReconnectNow_skips_the_pause()
    {
        var slow = new IslandClientOptions
        {
            ConnectTimeout = TimeSpan.FromMilliseconds(100),
            ReconnectDelay = TimeSpan.FromMinutes(5),
        };
        await using var island = new FakeIsland();
        await using var client = new IslandClient(island.PipeName, slow);
        client.Start();
        await Task.Delay(400); // first attempt failed, now in the 5 minute pause
        island.Start();
        client.ReconnectNow();
        await WaitUntil(() => client.IsConnected);
    }
}
