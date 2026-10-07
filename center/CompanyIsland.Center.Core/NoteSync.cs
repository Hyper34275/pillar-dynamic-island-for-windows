namespace CompanyIsland.Center.Core;

/// <summary>
/// The Center's copy of the notes and the rules for changing them, kept free of UI types so it can be tested.
/// Nothing may be saved until the island's list has been received once (<see cref="Loaded"/>): a save always replaces the
/// island's whole list, so saving from an empty, never-loaded copy would delete every existing note.
/// Used from one thread (the UI thread); events are raised on it.
/// </summary>
public sealed class NoteSync
{
    private readonly Func<IReadOnlyList<Note>, Task<IReadOnlyList<Note>>> _save;
    private readonly Func<long> _nowMs;
    private readonly SemaphoreSlim _gate = new(1, 1);
    private List<Note> _confirmed = [];
    private int _inFlight;

    public NoteSync(Func<IReadOnlyList<Note>, Task<IReadOnlyList<Note>>> save, Func<long>? nowMs = null)
    {
        _save = save;
        _nowMs = nowMs ?? NoteOps.NowMs;
    }

    /// <summary>What to show: canonical order; includes a change that is still being saved.</summary>
    public IReadOnlyList<Note> Notes { get; private set; } = [];

    /// <summary>The island's list has been received since the last <see cref="Reset"/>. Until then every change is refused.</summary>
    public bool Loaded { get; private set; }

    /// <summary>Raised when <see cref="Notes"/> or <see cref="Loaded"/> changed.</summary>
    public event Action? Changed;

    /// <summary>A save failed (operation name and the exception, for the log).</summary>
    public event Action<string, Exception>? Failed;

    /// <summary>The connection was lost or is starting over: the list on screen stays, but changing it is off until the next load.</summary>
    public void Reset()
    {
        if (!Loaded)
        {
            return;
        }

        Loaded = false;
        Changed?.Invoke();
    }

    /// <summary>The island's complete list (an answer to notesLoad, or a notes-changed event).</summary>
    public void Receive(IReadOnlyList<Note> notes)
    {
        _confirmed = NoteOps.Sort(notes);
        bool wasLoaded = Loaded;
        Loaded = true;
        if (_inFlight == 0)
        {
            Notes = _confirmed;
            Changed?.Invoke();
        }
        else if (!wasLoaded)
        {
            Changed?.Invoke();
        }
    }

    /// <summary>
    /// Changes the note list: <paramref name="change"/> gets the latest confirmed list (a copy) and returns the new one, or
    /// null for "nothing to do". The new list shows at once; if the island refuses it the confirmed list comes back and the
    /// result is false. False too, without asking the island, when the list was never received. Saves run one at a time.
    /// </summary>
    public async Task<bool> ChangeAsync(Func<List<Note>, List<Note>?> change)
    {
        if (!Loaded)
        {
            return false;
        }

        Interlocked.Increment(ref _inFlight);
        await _gate.WaitAsync();
        try
        {
            if (!Loaded)
            {
                // The connection dropped while this change waited for its turn.
                return false;
            }

            List<Note>? next = change([.. _confirmed]);
            if (next is null)
            {
                return true;
            }

            Notes = NoteOps.Sanitize(next, _nowMs());
            Changed?.Invoke();
            try
            {
                IReadOnlyList<Note> saved = await _save(next);
                _confirmed = NoteOps.Sort(saved);
                Notes = _confirmed;
                Changed?.Invoke();
                return true;
            }
            catch (Exception ex)
            {
                Failed?.Invoke("notesSave", ex);
                Notes = _confirmed;
                Changed?.Invoke();
                return false;
            }
        }
        finally
        {
            _gate.Release();
            if (Interlocked.Decrement(ref _inFlight) == 0 && !ReferenceEquals(Notes, _confirmed))
            {
                // The island changed the notes while we were saving; show its latest list now that nothing is pending.
                Notes = _confirmed;
                Changed?.Invoke();
            }
        }
    }
}

/// <summary>Retries a failed load with growing pauses until it works or is no longer wanted.</summary>
public static class LoadRetry
{
    /// <summary>2 s, 4 s, 8 s, then 15 s between attempts.</summary>
    public static TimeSpan DefaultDelay(int attempt) => TimeSpan.FromSeconds(Math.Min(2 << Math.Min(attempt, 3), 15));

    /// <summary>
    /// Waits, then runs <paramref name="attempt"/> (true = done) again and again while <paramref name="wanted"/> stays true.
    /// Returns true when an attempt succeeded.
    /// </summary>
    public static async Task<bool> RunAsync(Func<Task<bool>> attempt, Func<bool> wanted, Func<int, TimeSpan>? delay = null)
    {
        delay ??= DefaultDelay;
        for (int i = 0; ; i++)
        {
            await Task.Delay(delay(i)).ConfigureAwait(true);
            if (!wanted())
            {
                return false;
            }

            if (await attempt().ConfigureAwait(true))
            {
                return true;
            }

            if (!wanted())
            {
                return false;
            }
        }
    }
}
