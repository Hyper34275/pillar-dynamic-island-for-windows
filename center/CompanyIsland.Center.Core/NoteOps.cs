using System.Globalization;
using System.Security.Cryptography;
using System.Text;

namespace CompanyIsland.Center.Core;

public enum NoteTextProblem
{
    None,
    Empty,
    TooLong,
}

/// <summary>
/// Client-side note rules, mirroring the island (Rust is authoritative; the Center pre-validates so the user gets
/// feedback before a save is refused): ids, text limits, canonical order, search and id generation.
/// </summary>
public static class NoteOps
{
    public const int MaxNotes = 500;
    public const int MaxTextChars = 10_000;
    public const int MaxIdChars = 64;

    /// <summary>16 lowercase hex characters from a CSPRNG (8 random bytes).</summary>
    public static string NewId() => Convert.ToHexString(RandomNumberGenerator.GetBytes(8)).ToLowerInvariant();

    /// <summary>1..64 characters of <c>[A-Za-z0-9_-]</c>.</summary>
    public static bool IsValidId(string? id)
    {
        if (string.IsNullOrEmpty(id) || id.Length > MaxIdChars)
        {
            return false;
        }

        foreach (char c in id)
        {
            bool ok = c is (>= 'a' and <= 'z') or (>= 'A' and <= 'Z') or (>= '0' and <= '9') or '_' or '-';
            if (!ok)
            {
                return false;
            }
        }

        return true;
    }

    /// <summary>Length in Unicode scalar values (how the island counts), not UTF-16 units.</summary>
    public static int CountChars(string text)
    {
        int count = 0;
        foreach (Rune _ in text.EnumerateRunes())
        {
            count++;
        }

        return count;
    }

    public static NoteTextProblem CheckText(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return NoteTextProblem.Empty;
        }

        return CountChars(text) > MaxTextChars ? NoteTextProblem.TooLong : NoteTextProblem.None;
    }

    /// <summary>Cuts the text to the limit at a scalar boundary (never inside a surrogate pair).</summary>
    public static string Truncate(string text, int maxChars = MaxTextChars)
    {
        int count = 0;
        int index = 0;
        foreach (Rune rune in text.EnumerateRunes())
        {
            if (count == maxChars)
            {
                return text[..index];
            }

            count++;
            index += rune.Utf16SequenceLength;
        }

        return text;
    }

    /// <summary>Pinned first, then updated (newest first), then id ascending. The order the island returns everywhere.</summary>
    public static List<Note> Sort(IEnumerable<Note> notes)
    {
        var list = notes.ToList();
        list.Sort(CompareCanonical);
        return list;
    }

    public static int CompareCanonical(Note a, Note b)
    {
        if (a.Pinned != b.Pinned)
        {
            return a.Pinned ? -1 : 1;
        }

        int byUpdated = b.UpdatedAt.CompareTo(a.UpdatedAt);
        return byUpdated != 0 ? byUpdated : string.CompareOrdinal(a.Id, b.Id);
    }

    /// <summary>
    /// What the island does with a note list before saving it: drops invalid ids and empty texts, truncates, fixes times,
    /// removes duplicate ids (the larger updatedAt wins), keeps the 500 newest, and returns the canonical order.
    /// </summary>
    public static List<Note> Sanitize(IEnumerable<Note> notes, long nowMs)
    {
        var byId = new Dictionary<string, Note>(StringComparer.Ordinal);
        foreach (Note raw in notes)
        {
            if (!IsValidId(raw.Id) || string.IsNullOrWhiteSpace(raw.Text))
            {
                continue;
            }

            long created = raw.CreatedAt > 0 ? raw.CreatedAt : nowMs;
            long updated = raw.UpdatedAt > 0 ? raw.UpdatedAt : nowMs;
            if (updated < created)
            {
                updated = created;
            }

            Note clean = raw with { Text = Truncate(raw.Text), CreatedAt = created, UpdatedAt = updated };
            if (!byId.TryGetValue(clean.Id, out Note? existing) || clean.UpdatedAt > existing.UpdatedAt)
            {
                byId[clean.Id] = clean;
            }
        }

        var kept = byId.Values.ToList();
        if (kept.Count > MaxNotes)
        {
            kept = kept
                .OrderByDescending(n => n.UpdatedAt)
                .ThenBy(n => n.Id, StringComparer.Ordinal)
                .Take(MaxNotes)
                .ToList();
        }

        kept.Sort(CompareCanonical);
        return kept;
    }

    /// <summary>
    /// Case-insensitive search: every whitespace-separated word of the query must occur in the note's text.
    /// An empty query matches everything. Order is kept.
    /// </summary>
    public static List<Note> Search(IEnumerable<Note> notes, string? query)
    {
        string[] words = (query ?? "").Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (words.Length == 0)
        {
            return notes.ToList();
        }

        CompareInfo compare = CultureInfo.InvariantCulture.CompareInfo;
        return notes
            .Where(n => words.All(w => compare.IndexOf(n.Text, w, CompareOptions.IgnoreCase) >= 0))
            .ToList();
    }

    public static long NowMs() => DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();

    /// <summary>A new note with a fresh id, stamped now.</summary>
    public static Note Create(string text, long nowMs, bool pinned = false) => new()
    {
        Id = NewId(),
        Text = text,
        CreatedAt = nowMs,
        UpdatedAt = nowMs,
        Pinned = pinned,
    };
}
