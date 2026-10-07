using System.Collections.Concurrent;
using System.IO.Pipes;
using System.Text;
using System.Text.Json;

namespace CompanyIsland.Center.Tests;

/// <summary>
/// An in-process stand-in for the island's pipe server, on a unique pipe name per instance.
/// It speaks the protocol's framing (one JSON object per line) and lets a test script the answers.
/// </summary>
internal sealed class FakeIsland : IAsyncDisposable
{
    public delegate string? Handler(Session session, string cmd, JsonElement args);

    private readonly CancellationTokenSource _cts = new();
    private readonly List<Task> _tasks = [];
    private Task? _accept;

    public string PipeName { get; } = "CompanyIsland.Center.test." + Guid.NewGuid().ToString("N");

    public ConcurrentQueue<(string Cmd, string Args)> Requests { get; } = new();

    public ConcurrentQueue<Session> Sessions { get; } = new();

    /// <summary>Answers a request with a raw JSON result, or throws <see cref="FakeError"/>. Returning null sends nothing (the request times out).</summary>
    public Handler OnRequest { get; set; }

    public FakeIsland(Handler? handler = null)
    {
        OnRequest = handler ?? DefaultHandler;
    }

    public static string? DefaultHandler(Session session, string cmd, JsonElement args) => cmd switch
    {
        "hello" => """{"protocol":1,"appVersion":"1.0.4"}""",
        "getSettings" => """{"schemaVersion":1,"launchWithWindows":false,"islandDisplay":"clock","reminderMinutes":10,"monitorId":null,"unknownField":42}""",
        "notesLoad" => "[]",
        _ => "null",
    };

    public void Start()
    {
        _accept = Task.Run(AcceptLoopAsync);
    }

    private async Task AcceptLoopAsync()
    {
        while (!_cts.IsCancellationRequested)
        {
            var server = new NamedPipeServerStream(
                PipeName, PipeDirection.InOut, NamedPipeServerStream.MaxAllowedServerInstances,
                PipeTransmissionMode.Byte, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
            try
            {
                await server.WaitForConnectionAsync(_cts.Token);
            }
            catch
            {
                server.Dispose();
                return;
            }

            var session = new Session(server, this);
            Sessions.Enqueue(session);
            lock (_tasks)
            {
                _tasks.Add(Task.Run(() => session.RunAsync(_cts.Token)));
            }
        }
    }

    public async ValueTask DisposeAsync()
    {
        _cts.Cancel();
        foreach (Session s in Sessions)
        {
            s.Close();
        }

        try
        {
            // Unblocks WaitForConnectionAsync that ignores cancellation on some runtimes.
            await using var poke = new NamedPipeClientStream(".", PipeName, PipeDirection.InOut, PipeOptions.Asynchronous);
            await poke.ConnectAsync(100);
        }
        catch
        {
        }

        if (_accept is not null)
        {
            await Task.WhenAny(_accept, Task.Delay(1000));
        }

        _cts.Dispose();
    }

    public sealed class Session
    {
        private readonly NamedPipeServerStream _pipe;
        private readonly FakeIsland _owner;
        private readonly SemaphoreSlim _write = new(1, 1);

        public Session(NamedPipeServerStream pipe, FakeIsland owner)
        {
            _pipe = pipe;
            _owner = owner;
        }

        public async Task RunAsync(CancellationToken ct)
        {
            var pending = new MemoryStream();
            var buffer = new byte[4096];
            try
            {
                while (!ct.IsCancellationRequested)
                {
                    int read = await _pipe.ReadAsync(buffer, ct);
                    if (read == 0)
                    {
                        return;
                    }

                    for (int i = 0; i < read; i++)
                    {
                        if (buffer[i] == (byte)'\n')
                        {
                            string line = Encoding.UTF8.GetString(pending.ToArray());
                            pending.SetLength(0);
                            _ = Task.Run(() => HandleAsync(line), ct);
                        }
                        else
                        {
                            pending.WriteByte(buffer[i]);
                        }
                    }
                }
            }
            catch
            {
            }
        }

        private async Task HandleAsync(string line)
        {
            using JsonDocument doc = JsonDocument.Parse(line);
            JsonElement root = doc.RootElement;
            long id = root.GetProperty("id").GetInt64();
            string cmd = root.GetProperty("cmd").GetString()!;
            JsonElement args = root.TryGetProperty("args", out JsonElement a) ? a.Clone() : default;
            _owner.Requests.Enqueue((cmd, args.ValueKind == JsonValueKind.Undefined ? "" : args.GetRawText()));
            try
            {
                string? result = _owner.OnRequest(this, cmd, args);
                if (result is null)
                {
                    return;
                }

                await SendRawAsync("{\"id\":" + id + ",\"ok\":true,\"result\":" + result + "}\n");
            }
            catch (FakeError e)
            {
                string error = JsonSerializer.Serialize(e.Message);
                await SendRawAsync("{\"id\":" + id + ",\"ok\":false,\"error\":" + error + "}\n");
            }
        }

        public Task PushEventAsync(string name, string payloadJson) =>
            SendRawAsync("{\"event\":\"" + name + "\",\"payload\":" + payloadJson + "}\n");

        public async Task SendRawAsync(string text) => await SendBytesAsync(Encoding.UTF8.GetBytes(text));

        public async Task SendBytesAsync(byte[] bytes)
        {
            await _write.WaitAsync();
            try
            {
                await _pipe.WriteAsync(bytes);
                await _pipe.FlushAsync();
            }
            catch
            {
            }
            finally
            {
                _write.Release();
            }
        }

        public void Close()
        {
            try
            {
                _pipe.Dispose();
            }
            catch
            {
            }
        }
    }
}

internal sealed class FakeError(string message) : Exception(message);
