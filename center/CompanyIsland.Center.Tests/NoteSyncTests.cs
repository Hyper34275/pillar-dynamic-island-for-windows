using CompanyIsland.Center.Core;

namespace CompanyIsland.Center.Tests;

public class NoteSyncTests
{
    private static Note N(string id, string text = "t", bool pinned = false, long at = 1000) =>
        new() { Id = id, Text = text, CreatedAt = at, UpdatedAt = at, Pinned = pinned };

    private static NoteSync Make(Func<IReadOnlyList<Note>, Task<IReadOnlyList<Note>>> save) => new(save, () => 5000);

    private static Task<IReadOnlyList<Note>> Echo(IReadOnlyList<Note> notes) => Task.FromResult(notes);

    [Fact]
    public async Task A_change_before_the_notes_were_loaded_is_refused_and_nothing_is_sent()
    {
        int saves = 0;
        NoteSync sync = Make(n => { saves++; return Echo(n); });

        bool changeRan = false;
        bool ok = await sync.ChangeAsync(list =>
        {
            changeRan = true;
            list.Add(N("new"));
            return list;
        });

        Assert.False(ok);
        Assert.False(changeRan);
        Assert.Equal(0, saves);
        Assert.False(sync.Loaded);
        Assert.Empty(sync.Notes);
    }

    [Fact]
    public async Task After_the_load_a_change_saves_the_full_list_not_just_the_new_note()
    {
        IReadOnlyList<Note>? sent = null;
        NoteSync sync = Make(n => { sent = n; return Echo(n); });
        sync.Receive([N("a", at: 1), N("b", at: 2)]);

        bool ok = await sync.ChangeAsync(list =>
        {
            list.Add(N("c", at: 3));
            return list;
        });

        Assert.True(ok);
        Assert.NotNull(sent);
        Assert.Equal(["a", "b", "c"], sent!.Select(n => n.Id).OrderBy(x => x));
        Assert.Equal(3, sync.Notes.Count);
    }

    [Fact]
    public async Task A_failed_load_leaves_changes_off_until_a_later_load_works()
    {
        int saves = 0;
        NoteSync sync = Make(n => { saves++; return Echo(n); });

        // First load failed: nothing was received, so every attempt is refused.
        Assert.False(await sync.ChangeAsync(l => { l.Add(N("x")); return l; }));
        Assert.Equal(0, saves);

        sync.Receive([N("a")]);
        Assert.True(sync.Loaded);
        Assert.True(await sync.ChangeAsync(l => { l.Add(N("x")); return l; }));
        Assert.Equal(1, saves);
    }

    [Fact]
    public async Task Reset_turns_changes_off_again_for_the_next_connection()
    {
        NoteSync sync = Make(Echo);
        sync.Receive([N("a")]);
        sync.Reset();

        Assert.False(sync.Loaded);
        Assert.False(await sync.ChangeAsync(l => { l.Clear(); return l; }));
        Assert.Single(sync.Notes); // what was on screen stays
    }

    [Fact]
    public async Task A_change_queued_behind_a_save_is_refused_if_the_connection_was_reset_meanwhile()
    {
        var release = new TaskCompletionSource();
        int saves = 0;
        NoteSync sync = Make(async n =>
        {
            saves++;
            await release.Task;
            return n;
        });
        sync.Receive([N("a")]);

        Task<bool> first = sync.ChangeAsync(l => { l.Add(N("b")); return l; });
        Task<bool> second = sync.ChangeAsync(l => { l.Add(N("c")); return l; });
        sync.Reset();
        release.SetResult();

        Assert.True(await first);
        Assert.False(await second);
        Assert.Equal(1, saves);
    }

    [Fact]
    public async Task A_refused_save_puts_the_confirmed_list_back_and_reports_the_failure()
    {
        NoteSync sync = Make(_ => throw new IslandException("APP-020", "no"));
        sync.Receive([N("a")]);
        string? failedOp = null;
        sync.Failed += (op, _) => failedOp = op;

        bool ok = await sync.ChangeAsync(l => { l.Add(N("b")); return l; });

        Assert.False(ok);
        Assert.Equal("notesSave", failedOp);
        Assert.Equal(["a"], sync.Notes.Select(n => n.Id));
    }

    [Fact]
    public async Task Null_from_the_change_means_nothing_to_do()
    {
        int saves = 0;
        NoteSync sync = Make(n => { saves++; return Echo(n); });
        sync.Receive([N("a")]);

        Assert.True(await sync.ChangeAsync(_ => null));
        Assert.Equal(0, saves);
    }

    [Fact]
    public void Receive_raises_Changed_once_when_loading_and_shows_the_list()
    {
        NoteSync sync = Make(Echo);
        int changed = 0;
        sync.Changed += () => changed++;

        sync.Receive([N("a", at: 1), N("b", pinned: true, at: 0)]);

        Assert.Equal(1, changed);
        Assert.Equal(["b", "a"], sync.Notes.Select(n => n.Id)); // pinned first
    }

    [Fact]
    public async Task LoadRetry_keeps_trying_until_an_attempt_succeeds()
    {
        int attempts = 0;
        bool done = await LoadRetry.RunAsync(
            () => Task.FromResult(++attempts == 3),
            () => true,
            _ => TimeSpan.Zero);

        Assert.True(done);
        Assert.Equal(3, attempts);
    }

    [Fact]
    public async Task LoadRetry_stops_when_it_is_no_longer_wanted()
    {
        int attempts = 0;
        bool wanted = true;
        bool done = await LoadRetry.RunAsync(
            () => { attempts++; wanted = false; return Task.FromResult(false); },
            () => wanted,
            _ => TimeSpan.Zero);

        Assert.False(done);
        Assert.Equal(1, attempts);
    }

    [Fact]
    public async Task LoadRetry_does_not_even_try_when_it_stopped_being_wanted_during_the_pause()
    {
        int attempts = 0;
        bool done = await LoadRetry.RunAsync(() => { attempts++; return Task.FromResult(true); }, () => false, _ => TimeSpan.Zero);

        Assert.False(done);
        Assert.Equal(0, attempts);
    }

    [Theory]
    [InlineData(0, 2)]
    [InlineData(1, 4)]
    [InlineData(2, 8)]
    [InlineData(3, 15)]
    [InlineData(50, 15)]
    public void LoadRetry_pauses_grow_and_are_capped(int attempt, int seconds) =>
        Assert.Equal(TimeSpan.FromSeconds(seconds), LoadRetry.DefaultDelay(attempt));

    [Theory]
    [InlineData(600, 1.0, 1080, 600)]
    [InlineData(600, 2.0, 1040, 1040)] // 1200 px would not fit a 1040 px work area
    [InlineData(600, 2.5, 1380, 1380)]
    [InlineData(840, 2.0, 3000, 1680)]
    [InlineData(600, 1.25, 0, 750)] // unknown work area: design minimum
    [InlineData(600, 0.0, 1000, 600)] // nonsense scale: treated as 1
    public void Window_minimum_never_exceeds_the_work_area(double dip, double scale, int work, int expected) =>
        Assert.Equal(expected, WindowSizing.MinimumPixels(dip, scale, work));
}
