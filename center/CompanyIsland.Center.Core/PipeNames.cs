using System.Diagnostics;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;

namespace CompanyIsland.Center.Core;

/// <summary>
/// The island's pipe: <c>\\.\pipe\CompanyIsland.Center.{sessionId}.{sidhash}</c>. .NET wants the name without the
/// <c>\\.\pipe\</c> prefix. sidhash = first 16 lowercase hex chars of SHA-256 over the UTF-8 SID string.
/// </summary>
public static class PipeNames
{
    public const string Prefix = "CompanyIsland.Center";

    public static string Build(int sessionId, string sid)
    {
        byte[] hash = SHA256.HashData(Encoding.UTF8.GetBytes(sid));
        string hex = Convert.ToHexString(hash.AsSpan(0, 8)).ToLowerInvariant();
        return $"{Prefix}.{sessionId}.{hex}";
    }

    /// <summary>The pipe name for the current Windows user and session.</summary>
    public static string ForCurrentUser()
    {
        using var identity = WindowsIdentity.GetCurrent();
        string sid = identity.User?.Value ?? throw new InvalidOperationException("The current user has no SID.");
        using var process = Process.GetCurrentProcess();
        return Build(process.SessionId, sid);
    }
}
