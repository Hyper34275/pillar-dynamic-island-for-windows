using System.Text;
using CompanyIsland.Center.Core;

namespace CompanyIsland.Center.Tests;

public sealed class FileLogTests : IDisposable
{
    private readonly string _dir = Path.Combine(Path.GetTempPath(), "ci-center-log-" + Guid.NewGuid().ToString("N"));

    public void Dispose()
    {
        try
        {
            Directory.Delete(_dir, recursive: true);
        }
        catch (IOException)
        {
        }
    }

    [Fact]
    public void Lines_are_appended_as_utf8_and_the_folder_is_created()
    {
        var log = new FileLog(Path.Combine(_dir, "logs", "center.log"), now: () => new DateTime(2026, 10, 8, 9, 5, 7, 42));
        log.Write("INFO", "מרכז first");
        log.Write("WARN", "second");

        string[] lines = File.ReadAllLines(log.Path, Encoding.UTF8);
        Assert.Equal(2, lines.Length);
        Assert.StartsWith("2026-10-08 09:05:07.042 [INFO] [", lines[0]);
        Assert.EndsWith("] מרכז first", lines[0]);
        Assert.Contains("[WARN]", lines[1]);
        Assert.False(File.ReadAllBytes(log.Path).Take(3).SequenceEqual(new byte[] { 0xEF, 0xBB, 0xBF }), "no BOM");
    }

    [Fact]
    public void A_message_stays_on_one_line()
    {
        var log = new FileLog(Path.Combine(_dir, "center.log"));
        log.Write("INFO", "a\r\nb\nc");
        Assert.Single(File.ReadAllLines(log.Path));
    }

    [Fact]
    public void The_file_rotates_to_dot_one_at_the_limit_and_keeps_only_one_old_file()
    {
        string path = Path.Combine(_dir, "center.log");
        var log = new FileLog(path, maxBytes: 400);
        for (int i = 0; i < 40; i++)
        {
            log.Write("INFO", "line " + i);
        }

        Assert.True(File.Exists(path + ".1"));
        Assert.False(File.Exists(path + ".2"));
        Assert.True(new FileInfo(path).Length <= 400);
        Assert.Contains("line 39", File.ReadAllText(path));
        Assert.DoesNotContain("line 0" + Environment.NewLine, File.ReadAllText(path));
    }

    [Fact]
    public void Writing_never_throws_even_when_the_path_is_unusable()
    {
        string blocker = Path.Combine(_dir, "file");
        Directory.CreateDirectory(_dir);
        File.WriteAllText(blocker, "x");
        var log = new FileLog(Path.Combine(blocker, "sub", "center.log")); // a folder cannot be created below a file
        log.Write("INFO", "ignored");
    }

    [Fact]
    public void Describe_has_the_type_and_hresult_and_frames_but_never_the_message()
    {
        Exception thrown;
        try
        {
            throw new InvalidOperationException("secret note text", new IOException("inner secret"));
        }
        catch (Exception e)
        {
            thrown = e;
        }

        string text = FileLog.Describe(thrown);
        Assert.Contains("System.InvalidOperationException HResult=0x", text);
        Assert.Contains("System.IO.IOException", text);
        Assert.Contains(nameof(Describe_has_the_type_and_hresult_and_frames_but_never_the_message), text);
        Assert.DoesNotContain("secret", text);
        Assert.DoesNotContain(".cs:line", text);
    }

    [Fact]
    public void Two_writers_can_append_to_the_same_file()
    {
        string path = Path.Combine(_dir, "center.log");
        var a = new FileLog(path);
        var b = new FileLog(path);
        Parallel.For(0, 50, i => (i % 2 == 0 ? a : b).Write("INFO", "n" + i));
        Assert.Equal(50, File.ReadAllLines(path).Length);
    }
}
