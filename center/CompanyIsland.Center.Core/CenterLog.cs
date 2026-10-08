using System.Text;

namespace CompanyIsland.Center.Core;

/// <summary>
/// A tiny append-only file log that works before anything else does (no pipe, no WinUI): one line per event, UTF-8,
/// rotated to <c>center.log.1</c> at 512 KB. It never throws. Only stages, codes and exception TYPES go in, never message
/// content: callers pass fixed words, and <see cref="Describe"/> leaves exception messages out (they can hold paths or text).
/// </summary>
public sealed class FileLog
{
    public const long DefaultMaxBytes = 512 * 1024;

    private readonly object _gate = new();
    private readonly string _path;
    private readonly long _maxBytes;
    private readonly Func<DateTime> _now;

    public FileLog(string path, long maxBytes = DefaultMaxBytes, Func<DateTime>? now = null)
    {
        _path = path;
        _maxBytes = maxBytes;
        _now = now ?? (() => DateTime.Now);
    }

    public string Path => _path;

    public void Write(string level, string message)
    {
        try
        {
            string line = $"{_now():yyyy-MM-dd HH:mm:ss.fff} [{level}] [{Environment.ProcessId}] {message.ReplaceLineEndings(" | ")}{Environment.NewLine}";
            byte[] bytes = new UTF8Encoding(false).GetBytes(line);
            lock (_gate)
            {
                // Several writers (a redirected second launch, or two FileLogs on one path) append to the
                // same file. Each FileStream seeks to the end only when it opens, so two writers in
                // between would write at the same offset and one line would be lost: a named mutex
                // (session-local, per file) serialises open+write across processes.
                using var mutex = new Mutex(false, MutexName(_path));
                bool owned = false;
                try
                {
                    try { owned = mutex.WaitOne(500); } catch (AbandonedMutexException) { owned = true; }
                    Directory.CreateDirectory(System.IO.Path.GetDirectoryName(_path)!);
                    Rotate(bytes.Length);
                    for (int attempt = 0; ; attempt++)
                    {
                        try
                        {
                            using var stream = new FileStream(_path, FileMode.Append, FileAccess.Write, FileShare.ReadWrite | FileShare.Delete);
                            stream.Write(bytes, 0, bytes.Length);
                            break;
                        }
                        catch (IOException) when (attempt < 3)
                        {
                            Thread.Sleep(15);
                        }
                    }
                }
                finally
                {
                    if (owned) mutex.ReleaseMutex();
                }
            }
        }
        catch
        {
            // Logging must never take the app down.
        }
    }

    /// <summary>Local (this session) mutex name for a log file; the path is hashed (no user name in object names).</summary>
    private static string MutexName(string path)
    {
        byte[] hash = System.Security.Cryptography.SHA256.HashData(Encoding.UTF8.GetBytes(path.ToUpperInvariant()));
        return @"Local\CompanyIsland.CenterLog." + Convert.ToHexString(hash, 0, 8);
    }

    private void Rotate(int incoming)
    {
        try
        {
            var info = new FileInfo(_path);
            if (info.Exists && info.Length + incoming > _maxBytes)
            {
                File.Move(_path, _path + ".1", overwrite: true);
            }
        }
        catch
        {
            // Another process rotated at the same moment, or the file is locked: append to what is there.
        }
    }

    /// <summary>
    /// Exception type, HResult, and the frames of the stack trace (method names only), chained through inner exceptions.
    /// No message text.
    /// </summary>
    public static string Describe(Exception exception)
    {
        var text = new StringBuilder();
        int depth = 0;
        for (Exception? e = exception; e is not null && depth < 4; e = e.InnerException, depth++)
        {
            if (depth > 0)
            {
                text.Append(" <- ");
            }

            text.Append(e.GetType().FullName).Append(" HResult=0x").Append(e.HResult.ToString("X8"));
            if (depth == 0 && e.StackTrace is { } trace)
            {
                int frames = 0;
                foreach (string raw in trace.Split('\n'))
                {
                    string frame = raw.Trim();
                    if (frame.Length == 0 || frames >= 20)
                    {
                        continue;
                    }

                    // "at Ns.Type.Method(args) in C:\path\file.cs:line 12": keep the method, drop the file path.
                    int where = frame.IndexOf(" in ", StringComparison.Ordinal);
                    text.Append(" | ").Append(where > 0 ? frame[..where] : frame);
                    frames++;
                }
            }
        }

        return text.ToString();
    }
}

/// <summary>The Center's process-wide log: <c>%LOCALAPPDATA%\Yuval\logs\center.log</c> (see <see cref="DataFolder"/>).</summary>
public static class CenterLog
{
    private static readonly Lazy<FileLog> Log = new(() =>
    {
        string root = DataFolder.Resolve();
        return new FileLog(System.IO.Path.Combine(root, "logs", "center.log"));
    });

    public static string Path => Log.Value.Path;

    public static void Info(string message) => Log.Value.Write("INFO", message);

    public static void Warn(string message) => Log.Value.Write("WARN", message);

    public static void Error(string stage, Exception exception) =>
        Log.Value.Write("ERROR", $"{stage}: {FileLog.Describe(exception)}");
}
