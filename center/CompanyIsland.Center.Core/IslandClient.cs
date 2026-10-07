using System.Collections.Concurrent;
using System.IO.Pipes;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace CompanyIsland.Center.Core;

public enum ConnectionState
{
    Disconnected,
    Connecting,
    Connected,
}

/// <summary>An event pushed by the island (<c>{"event": name, "payload": ...}</c>).</summary>
public readonly record struct IslandEvent(string Name, JsonElement Payload);

public static class IslandEventNames
{
    public const string SettingsChanged = "settings-changed";
    public const string NotesChanged = "notes-changed";
    public const string Navigate = "navigate";
}

public sealed class IslandClientOptions
{
    /// <summary>How long one connection attempt waits for the pipe to exist.</summary>
    public TimeSpan ConnectTimeout { get; init; } = TimeSpan.FromMilliseconds(500);

    /// <summary>Pause between a failed or dropped connection and the next attempt (about 3 s per cycle in all).</summary>
    public TimeSpan ReconnectDelay { get; init; } = TimeSpan.FromMilliseconds(2500);

    public TimeSpan HelloTimeout { get; init; } = TimeSpan.FromSeconds(5);

    public TimeSpan RequestTimeout { get; init; } = TimeSpan.FromSeconds(10);

    /// <summary>The protocol's line limit, each way. A longer incoming line closes the connection.</summary>
    public int MaxLineBytes { get; init; } = IslandProtocol.MaxLineBytes;
}

public static class IslandProtocol
{
    public const int Version = 1;
    public const int MaxLineBytes = 16 * 1024 * 1024;
    public const string ClientName = "center";
}

/// <summary>The island answered a request with <c>ok:false</c> (or the client could not complete it).</summary>
public class IslandException : Exception
{
    public IslandException(string code, string message) : base(message)
    {
        Code = code;
    }

    /// <summary>The leading code of the island's error ("APP-002"), or a client-side code, or empty.</summary>
    public string Code { get; }
}

public sealed class IslandDisconnectedException : IslandException
{
    public IslandDisconnectedException() : base("client-disconnected", "The island is not connected.")
    {
    }
}

public sealed class IslandTimeoutException : IslandException
{
    public IslandTimeoutException(string cmd) : base("client-timeout", $"The island did not answer {cmd} in time.")
    {
    }
}

/// <summary>
/// The Center's side of the island's named pipe: connects (and reconnects), says hello, correlates responses to requests
/// by id and raises the island's events. Never logs bodies. Events are raised on a pool thread, one at a time, in order.
/// </summary>
public sealed class IslandClient : IAsyncDisposable
{
    private readonly string _pipeName;
    private readonly IslandClientOptions _options;
    private readonly CancellationTokenSource _lifetime = new();
    private readonly SemaphoreSlim _nudge = new(0);
    private readonly object _gate = new();
    private Task? _loop;
    private volatile Connection? _connection;
    private ConnectionState _state = ConnectionState.Disconnected;
    private string? _appVersion;

    public IslandClient(string pipeName, IslandClientOptions? options = null)
    {
        _pipeName = pipeName;
        _options = options ?? new IslandClientOptions();
    }

    public ConnectionState State
    {
        get { lock (_gate) return _state; }
    }

    public bool IsConnected => State == ConnectionState.Connected;

    /// <summary>The island's version from the last successful hello.</summary>
    public string? AppVersion
    {
        get { lock (_gate) return _appVersion; }
    }

    /// <summary>The code or exception type of the last failed attempt (never message text from the island).</summary>
    public string? LastFailure { get; private set; }

    public event Action<ConnectionState>? StateChanged;

    public event Action<IslandEvent>? EventReceived;

    /// <summary>Starts the connect / reconnect loop. Calling it twice does nothing.</summary>
    public void Start()
    {
        lock (_gate)
        {
            _loop ??= Task.Run(RunAsync);
        }
    }

    /// <summary>Skips the current reconnect pause (the user just started the island).</summary>
    public void ReconnectNow()
    {
        if (_nudge.CurrentCount == 0)
        {
            _nudge.Release();
        }
    }

    public async ValueTask DisposeAsync()
    {
        _lifetime.Cancel();
        Connection? connection = _connection;
        connection?.Close();
        Task? loop;
        lock (_gate)
        {
            loop = _loop;
        }

        if (loop is not null)
        {
            try
            {
                await loop.ConfigureAwait(false);
            }
            catch
            {
                // The loop never throws; a failure here would only be a cancellation race.
            }
        }

        _lifetime.Dispose();
        _nudge.Dispose();
    }

    private void SetState(ConnectionState state)
    {
        lock (_gate)
        {
            if (_state == state)
            {
                return;
            }

            _state = state;
        }

        Raise(() => StateChanged?.Invoke(state));
    }

    private static void Raise(Action action)
    {
        try
        {
            action();
        }
        catch
        {
            // A subscriber's bug must not take the connection down.
        }
    }

    private async Task RunAsync()
    {
        CancellationToken ct = _lifetime.Token;
        while (!ct.IsCancellationRequested)
        {
            Connection? connection = null;
            try
            {
                SetState(ConnectionState.Connecting);
                var pipe = new NamedPipeClientStream(
                    ".", _pipeName, PipeDirection.InOut, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
                connection = new Connection(pipe, _options.MaxLineBytes, OnEvent);
                await pipe.ConnectAsync((int)_options.ConnectTimeout.TotalMilliseconds, ct).ConfigureAwait(false);

                Task readLoop = connection.RunReadLoopAsync();
                JsonElement hello = await connection.RequestAsync(
                    "hello",
                    new JsonObject { ["client"] = IslandProtocol.ClientName, ["protocol"] = IslandProtocol.Version },
                    _options.HelloTimeout,
                    ct).ConfigureAwait(false);

                if (hello.ValueKind != JsonValueKind.Object ||
                    !hello.TryGetProperty("protocol", out JsonElement protocol) ||
                    !protocol.TryGetInt32(out int version) ||
                    version != IslandProtocol.Version)
                {
                    throw new IslandException("APP-031", "unsupported protocol");
                }

                lock (_gate)
                {
                    _appVersion = hello.TryGetProperty("appVersion", out JsonElement v) && v.ValueKind == JsonValueKind.String
                        ? v.GetString()
                        : null;
                }

                LastFailure = null;
                _connection = connection;
                SetState(ConnectionState.Connected);
                await readLoop.ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                break;
            }
            catch (Exception e)
            {
                LastFailure = e is IslandException ie && ie.Code.Length > 0 ? ie.Code : e.GetType().Name;
            }
            finally
            {
                _connection = null;
                connection?.Close();
            }

            SetState(ConnectionState.Disconnected);
            try
            {
                await _nudge.WaitAsync(_options.ReconnectDelay, ct).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                break;
            }
        }

        SetState(ConnectionState.Disconnected);
    }

    private void OnEvent(IslandEvent e) => Raise(() => EventReceived?.Invoke(e));

    /// <summary>Sends one request and returns the response's <c>result</c>. Throws <see cref="IslandException"/> on <c>ok:false</c>.</summary>
    public Task<JsonElement> RequestAsync(string cmd, JsonObject? args = null, TimeSpan? timeout = null, CancellationToken ct = default)
    {
        Connection? connection = _connection;
        return connection is null
            ? Task.FromException<JsonElement>(new IslandDisconnectedException()) // a faulted task, never a synchronous throw
            : connection.RequestAsync(cmd, args, timeout ?? _options.RequestTimeout, ct);
    }

    public async Task<IslandSettings> GetSettingsAsync(CancellationToken ct = default)
    {
        JsonElement result = await RequestAsync("getSettings", ct: ct).ConfigureAwait(false);
        return ReadSettings(result);
    }

    public async Task<IslandSettings> UpdateSettingsAsync(SettingsPatch patch, CancellationToken ct = default)
    {
        JsonElement result = await RequestAsync("updateSettings", new JsonObject { ["patch"] = patch.ToJson() }, ct: ct)
            .ConfigureAwait(false);
        return ReadSettings(result);
    }

    public async Task<IReadOnlyList<Note>> NotesLoadAsync(CancellationToken ct = default)
    {
        JsonElement result = await RequestAsync("notesLoad", ct: ct).ConfigureAwait(false);
        return ReadNotes(result);
    }

    public async Task<IReadOnlyList<Note>> NotesSaveAsync(IEnumerable<Note> notes, CancellationToken ct = default)
    {
        JsonNode? list = JsonSerializer.SerializeToNode(notes.ToArray(), CenterJson.Default.NoteArray);
        JsonElement result = await RequestAsync("notesSave", new JsonObject { ["notes"] = list }, ct: ct).ConfigureAwait(false);
        return ReadNotes(result);
    }

    public async Task<IReadOnlyList<MonitorInfo>> GetMonitorsAsync(CancellationToken ct = default)
    {
        JsonElement result = await RequestAsync("getMonitors", ct: ct).ConfigureAwait(false);
        return result.Deserialize(CenterJson.Default.MonitorInfoArray) ?? [];
    }

    public async Task<NotificationAccess> GetNotificationStatusAsync(CancellationToken ct = default)
    {
        JsonElement result = await RequestAsync("getNotificationStatus", ct: ct).ConfigureAwait(false);
        return NotificationAccessParser.Parse(result.ValueKind == JsonValueKind.String ? result.GetString() : null);
    }

    /// <summary>Only ever from an explicit click in the Center.</summary>
    public async Task<NotificationAccess> RequestNotificationAccessAsync(CancellationToken ct = default)
    {
        JsonElement result = await RequestAsync("requestNotificationAccess", timeout: TimeSpan.FromSeconds(60), ct: ct)
            .ConfigureAwait(false);
        return NotificationAccessParser.Parse(result.ValueKind == JsonValueKind.String ? result.GetString() : null);
    }

    public Task OpenLogDirAsync(CancellationToken ct = default) => RequestAsync("openLogDir", ct: ct);

    /// <summary>
    /// Shows the island expanded on <paramref name="tab"/> (an <see cref="IslandTabs"/> value). The tab is required, and the
    /// request carries <c>show: true</c> so an island that is already open on that tab stays open instead of toggling shut.
    /// </summary>
    public Task ShowIslandAsync(string tab, CancellationToken ct = default)
    {
        ArgumentException.ThrowIfNullOrEmpty(tab);
        return RequestAsync("showIsland", new JsonObject { ["tab"] = tab, ["show"] = true }, ct: ct);
    }

    /// <summary>Writes a line to the island's log as scope <c>center</c>. Codes only: never user content.</summary>
    public Task LogAsync(string level, string message, CancellationToken ct = default) =>
        RequestAsync("log", new JsonObject { ["level"] = level, ["message"] = message }, ct: ct);

    public static IslandSettings ReadSettings(JsonElement element) =>
        (element.Deserialize(CenterJson.Default.IslandSettings) ?? throw new IslandException("client-protocol", "empty settings"))
        .Normalized();

    public static IReadOnlyList<Note> ReadNotes(JsonElement element) =>
        element.ValueKind == JsonValueKind.Array ? element.Deserialize(CenterJson.Default.NoteArray) ?? [] : [];

    /// <summary>One live pipe connection: line framing, request correlation and the read loop.</summary>
    private sealed class Connection
    {
        private readonly NamedPipeClientStream _pipe;
        private readonly int _maxLine;
        private readonly Action<IslandEvent> _onEvent;
        private readonly SemaphoreSlim _writeLock = new(1, 1);
        private readonly ConcurrentDictionary<long, TaskCompletionSource<JsonElement>> _pending = new();
        private long _nextId;
        private int _closed;

        public Connection(NamedPipeClientStream pipe, int maxLine, Action<IslandEvent> onEvent)
        {
            _pipe = pipe;
            _maxLine = maxLine;
            _onEvent = onEvent;
        }

        public async Task RunReadLoopAsync()
        {
            try
            {
                var reader = new LineReader(_pipe, _maxLine);
                while (true)
                {
                    ReadOnlyMemory<byte>? line = await reader.ReadLineAsync().ConfigureAwait(false);
                    if (line is null)
                    {
                        break;
                    }

                    Dispatch(line.Value);
                }
            }
            catch
            {
                // EOF, a reset pipe, a line over the limit: all end the connection the same way.
            }
            finally
            {
                Close();
            }
        }

        private void Dispatch(ReadOnlyMemory<byte> line)
        {
            if (line.IsEmpty)
            {
                return;
            }

            JsonDocument doc;
            try
            {
                doc = JsonDocument.Parse(line, new JsonDocumentOptions { MaxDepth = 64 });
            }
            catch (JsonException)
            {
                return;
            }

            using (doc)
            {
                JsonElement root = doc.RootElement;
                if (root.ValueKind != JsonValueKind.Object)
                {
                    return;
                }

                if (root.TryGetProperty("event", out JsonElement name) && name.ValueKind == JsonValueKind.String)
                {
                    JsonElement payload = root.TryGetProperty("payload", out JsonElement p) ? p.Clone() : default;
                    _onEvent(new IslandEvent(name.GetString()!, payload));
                    return;
                }

                // The island answers a line it cannot parse with "id": null; TryGetInt64 throws (not false) on a non-number.
                if (!root.TryGetProperty("id", out JsonElement idElement) ||
                    idElement.ValueKind != JsonValueKind.Number ||
                    !idElement.TryGetInt64(out long id))
                {
                    return;
                }

                if (!_pending.TryRemove(id, out var tcs))
                {
                    return;
                }

                bool ok = root.TryGetProperty("ok", out JsonElement okElement) && okElement.ValueKind == JsonValueKind.True;
                if (ok)
                {
                    tcs.TrySetResult(root.TryGetProperty("result", out JsonElement r) ? r.Clone() : default);
                }
                else
                {
                    string error = root.TryGetProperty("error", out JsonElement e) && e.ValueKind == JsonValueKind.String
                        ? e.GetString() ?? ""
                        : "";
                    tcs.TrySetException(ParseError(error));
                }
            }
        }

        public async Task<JsonElement> RequestAsync(string cmd, JsonObject? args, TimeSpan timeout, CancellationToken ct)
        {
            if (Volatile.Read(ref _closed) != 0)
            {
                throw new IslandDisconnectedException();
            }

            long id = Interlocked.Increment(ref _nextId);
            byte[] line = BuildRequest(id, cmd, args);
            if (line.Length > _maxLine)
            {
                throw new IslandException("client-too-large", "The request is larger than the protocol allows.");
            }

            var tcs = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
            _pending[id] = tcs;
            try
            {
                using var cts = CancellationTokenSource.CreateLinkedTokenSource(ct);
                cts.CancelAfter(timeout);
                try
                {
                    await _writeLock.WaitAsync(cts.Token).ConfigureAwait(false);
                    try
                    {
                        await _pipe.WriteAsync(line, cts.Token).ConfigureAwait(false);
                        await _pipe.FlushAsync(cts.Token).ConfigureAwait(false);
                    }
                    finally
                    {
                        _writeLock.Release();
                    }

                    // A response that raced the cancellation wins; otherwise waiting ends with the token.
                    return await tcs.Task.WaitAsync(cts.Token).ConfigureAwait(false);
                }
                catch (OperationCanceledException) when (!ct.IsCancellationRequested)
                {
                    throw new IslandTimeoutException(cmd);
                }
                catch (IOException)
                {
                    throw new IslandDisconnectedException();
                }
                catch (ObjectDisposedException)
                {
                    throw new IslandDisconnectedException();
                }
            }
            finally
            {
                _pending.TryRemove(id, out _);
            }
        }

        private static byte[] BuildRequest(long id, string cmd, JsonObject? args)
        {
            using var buffer = new MemoryStream();
            using (var writer = new Utf8JsonWriter(buffer))
            {
                writer.WriteStartObject();
                writer.WriteNumber("id", id);
                writer.WriteString("cmd", cmd);
                if (args is not null)
                {
                    writer.WritePropertyName("args");
                    args.WriteTo(writer);
                }

                writer.WriteEndObject();
            }

            buffer.WriteByte((byte)'\n');
            return buffer.ToArray();
        }

        public void Close()
        {
            if (Interlocked.Exchange(ref _closed, 1) != 0)
            {
                return;
            }

            foreach (var (_, tcs) in _pending)
            {
                tcs.TrySetException(new IslandDisconnectedException());
            }

            _pending.Clear();
            try
            {
                _pipe.Dispose();
            }
            catch
            {
                // Already gone.
            }
        }
    }

    /// <summary>"APP-002: cannot write notes" becomes code "APP-002", message "cannot write notes".</summary>
    internal static IslandException ParseError(string error)
    {
        int colon = error.IndexOf(':');
        if (colon > 0 && colon <= 16)
        {
            string code = error[..colon];
            if (code.All(c => c is (>= 'A' and <= 'Z') or (>= '0' and <= '9') or '-'))
            {
                return new IslandException(code, error[(colon + 1)..].Trim());
            }
        }

        return new IslandException("", error);
    }
}

/// <summary>Splits a stream into <c>\n</c>-terminated lines of at most <c>maxLine</c> bytes without buffering unbounded data.</summary>
internal sealed class LineReader
{
    private readonly Stream _stream;
    private readonly int _maxLine;
    private readonly byte[] _buffer = new byte[16 * 1024];
    private int _start;
    private int _end;
    private readonly MemoryStream _partial = new();

    public LineReader(Stream stream, int maxLine)
    {
        _stream = stream;
        _maxLine = maxLine;
    }

    /// <summary>The next line without its terminator, or null at end of stream. Throws <see cref="InvalidDataException"/> past the limit.</summary>
    public async ValueTask<ReadOnlyMemory<byte>?> ReadLineAsync(CancellationToken ct = default)
    {
        while (true)
        {
            int index = Array.IndexOf(_buffer, (byte)'\n', _start, _end - _start);
            if (index >= 0)
            {
                int length = index - _start;
                byte[] line;
                if (_partial.Length == 0)
                {
                    if (length > _maxLine)
                    {
                        throw new InvalidDataException("line too long");
                    }

                    line = _buffer.AsSpan(_start, length).ToArray();
                }
                else
                {
                    if (_partial.Length + length > _maxLine)
                    {
                        throw new InvalidDataException("line too long");
                    }

                    _partial.Write(_buffer, _start, length);
                    line = _partial.ToArray();
                    _partial.SetLength(0);
                }

                _start = index + 1;
                return line;
            }

            // No terminator yet: keep what we have (bounded) and read more.
            int pending = _end - _start;
            if (pending > 0)
            {
                if (_partial.Length + pending > _maxLine)
                {
                    throw new InvalidDataException("line too long");
                }

                _partial.Write(_buffer, _start, pending);
            }

            _start = _end = 0;
            int read = await _stream.ReadAsync(_buffer.AsMemory(), ct).ConfigureAwait(false);
            if (read == 0)
            {
                return null;
            }

            _end = read;
        }
    }

    public static string Utf8(ReadOnlyMemory<byte> bytes) => Encoding.UTF8.GetString(bytes.Span);
}
