using System.Diagnostics;
using System.Text.Json;
using CompanyIsland.Center.Core;
using Microsoft.UI.Dispatching;

namespace CompanyIsland.Center.Services;

/// <summary>
/// The Center's view of the island: connection state, settings, notes, monitors and notification access, kept in sync
/// through <see cref="IslandClient"/>. All events are raised on the UI thread. Edits are optimistic: the UI changes first,
/// and a failed save puts the last confirmed state back and returns false so the page can say so.
/// Errors are reported to the island's log by code only, never with note text.
/// </summary>
public sealed class CenterModel : IDisposable
{
    private readonly DispatcherQueue _ui;
    private readonly SemaphoreSlim _settingsGate = new(1, 1);
    private readonly NoteSync _notes;
    private IslandSettings? _confirmedSettings;
    private int _loadVersion;

    public CenterModel(DispatcherQueue ui)
    {
        _ui = ui;
        Client = new IslandClient(PipeNames.ForCurrentUser());
        _notes = new NoteSync(notes => Client.NotesSaveAsync(notes));
        _notes.Changed += () => NotesChanged?.Invoke();
        _notes.Failed += Report;
        Client.StateChanged += state => _ui.TryEnqueue(() => OnState(state));
        Client.EventReceived += e => _ui.TryEnqueue(() => OnEvent(e));
    }

    public IslandClient Client { get; }

    public bool Connected { get; private set; }

    public IslandSettings? Settings { get; private set; }

    /// <summary>Canonical order (pinned first, newest first).</summary>
    public IReadOnlyList<Note> Notes => _notes.Notes;

    /// <summary>The island's notes were received on this connection. Until then notes cannot be changed (a save would replace them all).</summary>
    public bool NotesLoaded => _notes.Loaded;

    public IReadOnlyList<MonitorInfo> Monitors { get; private set; } = [];

    public NotificationAccess Access { get; private set; } = NotificationAccess.Unknown;

    public string? IslandVersion => Client.AppVersion;

    public event Action? ConnectionChanged;
    public event Action? SettingsChanged;
    public event Action? NotesChanged;
    public event Action? MonitorsChanged;
    public event Action? AccessChanged;

    /// <summary>The initial load after a (re)connect failed; the string is a short Hebrew message.</summary>
    public event Action<string>? LoadFailed;

    public event Action<CenterPage>? NavigateRequested;

    /// <summary>A search finished anywhere (search bar, island or this Center). The payload is the query id, or null if it was malformed.</summary>
    public event Action<string?>? SearchReady;

    public void Start() => Client.Start();

    public void Dispose()
    {
        _ = Client.DisposeAsync().AsTask();
    }

    private void OnState(ConnectionState state)
    {
        bool connected = state == ConnectionState.Connected;
        if (connected == Connected)
        {
            return;
        }

        Connected = connected;
        // Each connection starts without notes: editing stays off until this connection's load has delivered them.
        _notes.Reset();
        ConnectionChanged?.Invoke();
        if (connected)
        {
            _ = LoadAllAsync();
        }
    }

    private void OnEvent(IslandEvent e)
    {
        try
        {
            switch (e.Name)
            {
                case IslandEventNames.SettingsChanged:
                    _confirmedSettings = IslandClient.ReadSettings(e.Payload);
                    Settings = _confirmedSettings;
                    SettingsChanged?.Invoke();
                    break;
                case IslandEventNames.NotesChanged:
                    _notes.Receive(IslandClient.ReadNotes(e.Payload));
                    break;
                case IslandEventNames.SearchReady:
                    SearchReady?.Invoke(IslandClient.ReadSearchReady(e.Payload));
                    break;
                case IslandEventNames.Navigate:
                    if (e.Payload.ValueKind == JsonValueKind.Object &&
                        e.Payload.TryGetProperty("page", out JsonElement page) &&
                        page.ValueKind == JsonValueKind.String &&
                        CenterPage.TryParse(page.GetString(), out CenterPage target))
                    {
                        NavigateRequested?.Invoke(target);
                    }

                    break;
            }
        }
        catch (Exception ex)
        {
            Report("event", ex);
        }
    }

    /// <summary>Loads everything the pages show. Each part fails on its own so one bad answer does not blank the rest.</summary>
    private async Task LoadAllAsync()
    {
        int version = ++_loadVersion;
        bool failed = false;

        async Task<T?> Part<T>(string op, Func<Task<T>> load) where T : class
        {
            try
            {
                return await load();
            }
            catch (Exception ex)
            {
                failed = true;
                Report(op, ex);
                return null;
            }
        }

        Task<IslandSettings?> settingsTask = Part("getSettings", () => Client.GetSettingsAsync());
        Task<IReadOnlyList<Note>?> notesTask = Part("notesLoad", () => Client.NotesLoadAsync());
        Task<IReadOnlyList<MonitorInfo>?> monitorsTask = Part("getMonitors", () => Client.GetMonitorsAsync());
        Task<string?> accessTask = Part<string>("getNotificationStatus", async () => (await Client.GetNotificationStatusAsync()).ToString());
        await Task.WhenAll(settingsTask, notesTask, monitorsTask, accessTask);
        if (version != _loadVersion || !Connected)
        {
            return;
        }

        if (await settingsTask is { } settings)
        {
            _confirmedSettings = settings;
            Settings = settings;
            SettingsChanged?.Invoke();
        }

        if (await notesTask is { } notes)
        {
            _notes.Receive(notes);
        }
        else if (!NotesLoaded)
        {
            _ = RetryNotesLoadAsync(version);
        }

        if (await monitorsTask is { } monitors)
        {
            Monitors = monitors;
            MonitorsChanged?.Invoke();
        }

        if (await accessTask is { } access && Enum.TryParse(access, out NotificationAccess parsed))
        {
            Access = parsed;
            AccessChanged?.Invoke();
        }

        if (failed)
        {
            LoadFailed?.Invoke(Strings.LoadFailed);
        }
    }

    /// <summary>The first notes load failed: keep asking (with growing pauses) while this connection lasts, so notes become editable once it works.</summary>
    private Task RetryNotesLoadAsync(int version) => LoadRetry.RunAsync(
        async () =>
        {
            try
            {
                IReadOnlyList<Note> notes = await Client.NotesLoadAsync();
                if (version == _loadVersion && Connected)
                {
                    _notes.Receive(notes);
                }

                return true;
            }
            catch (Exception ex)
            {
                Report("notesLoad", ex);
                return false;
            }
        },
        () => version == _loadVersion && Connected && !NotesLoaded);

    /// <summary>Applies the patch at once, then asks the island. False (and the old settings back) when the island refused or is gone.</summary>
    public async Task<bool> UpdateSettingsAsync(SettingsPatch patch)
    {
        if (Settings is null)
        {
            return false;
        }

        await _settingsGate.WaitAsync();
        try
        {
            IslandSettings baseline = _confirmedSettings ?? Settings;
            Settings = patch.ApplyTo(Settings);
            SettingsChanged?.Invoke();
            try
            {
                IslandSettings confirmed = await Client.UpdateSettingsAsync(patch);
                _confirmedSettings = confirmed;
                Settings = confirmed;
                SettingsChanged?.Invoke();
                return true;
            }
            catch (Exception ex)
            {
                Report("updateSettings", ex);
                Settings = baseline;
                SettingsChanged?.Invoke();
                return false;
            }
        }
        finally
        {
            _settingsGate.Release();
        }
    }

    /// <summary>
    /// Changes the note list (see <see cref="NoteSync.ChangeAsync"/>): optimistic, reverted when the island refuses, and
    /// refused outright (false) until the island's notes have been received on this connection.
    /// </summary>
    public Task<bool> ChangeNotesAsync(Func<List<Note>, List<Note>?> change) => _notes.ChangeAsync(change);

    /// <summary>The click on "אשר גישה": asks Windows through the island and shows the answer.</summary>
    public async Task<bool> RequestNotificationAccessAsync()
    {
        try
        {
            Access = await Client.RequestNotificationAccessAsync();
            AccessChanged?.Invoke();
            return true;
        }
        catch (Exception ex)
        {
            Report("requestNotificationAccess", ex);
            return false;
        }
    }

    public async Task<bool> OpenLogDirAsync()
    {
        try
        {
            await Client.OpenLogDirAsync();
            return true;
        }
        catch (Exception ex)
        {
            Report("openLogDir", ex);
            return false;
        }
    }

    public async Task<bool> ShowIslandAsync(string tab)
    {
        try
        {
            await Client.ShowIslandAsync(tab);
            return true;
        }
        catch (Exception ex)
        {
            Report("showIsland", ex);
            return false;
        }
    }

    /// <summary>Starts <c>..\CompanyIsland.exe</c> (the Center lives in <c>&lt;install dir&gt;\center</c>). False when it is not there.</summary>
    public bool TryStartIsland()
    {
        try
        {
            string path = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "CompanyIsland.exe"));
            if (!File.Exists(path))
            {
                return false;
            }

            using Process? process = Process.Start(new ProcessStartInfo(path) { UseShellExecute = false });
            Client.ReconnectNow();
            _ = Task.Delay(2000).ContinueWith(_ => Client.ReconnectNow(), TaskScheduler.Default);
            return process is not null;
        }
        catch
        {
            return false;
        }
    }

    /// <summary>Tells the island's log what failed: the operation and an error code, never any content.</summary>
    public void Report(string operation, Exception ex)
    {
        if (!Connected)
        {
            return;
        }

        string code = ex is IslandException { Code.Length: > 0 } ie ? ie.Code : ex.GetType().Name;
        // Fire and forget: a lost log line is not worth an error of its own.
        _ = Client.LogAsync("warn", $"{operation} failed ({code})").ContinueWith(t => _ = t.Exception, TaskScheduler.Default);
    }
}
