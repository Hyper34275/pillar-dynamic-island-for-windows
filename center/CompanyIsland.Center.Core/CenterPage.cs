namespace CompanyIsland.Center.Core;

public enum CenterPageKind
{
    Welcome,
    Tour,
    Settings,
    Notes,
    NotesNew,
    Note,
}

/// <summary>A page the Center can show: <c>welcome | tour | settings | notes | notes-new | note:&lt;id&gt;</c>.</summary>
public readonly record struct CenterPage(CenterPageKind Kind, string? NoteId = null)
{
    public static readonly CenterPage Welcome = new(CenterPageKind.Welcome);

    public string ToWire() => Kind switch
    {
        CenterPageKind.Tour => "tour",
        CenterPageKind.Settings => "settings",
        CenterPageKind.Notes => "notes",
        CenterPageKind.NotesNew => "notes-new",
        CenterPageKind.Note => "note:" + NoteId,
        _ => "welcome",
    };

    /// <summary>The page behind a wire value, or false when it is not a valid <c>CenterPage</c> (note ids follow the note id rules).</summary>
    public static bool TryParse(string? value, out CenterPage page)
    {
        page = Welcome;
        if (value is null)
        {
            return false;
        }

        switch (value)
        {
            case "welcome":
                return true;
            case "tour":
                page = new CenterPage(CenterPageKind.Tour);
                return true;
            case "settings":
                page = new CenterPage(CenterPageKind.Settings);
                return true;
            case "notes":
                page = new CenterPage(CenterPageKind.Notes);
                return true;
            case "notes-new":
                page = new CenterPage(CenterPageKind.NotesNew);
                return true;
        }

        const string notePrefix = "note:";
        if (value.StartsWith(notePrefix, StringComparison.Ordinal))
        {
            string id = value[notePrefix.Length..];
            if (NoteOps.IsValidId(id))
            {
                page = new CenterPage(CenterPageKind.Note, id);
                return true;
            }
        }

        return false;
    }

    /// <summary>
    /// The page named by <c>--page &lt;page&gt;</c> (or <c>--page=&lt;page&gt;</c>) among the arguments. Missing, repeated badly
    /// or invalid values fall back to the welcome page; the first <c>--page</c> wins.
    /// </summary>
    public static CenterPage FromArgs(IEnumerable<string> args)
    {
        string[] list = args.ToArray();
        for (int i = 0; i < list.Length; i++)
        {
            string arg = list[i];
            string? value = null;
            if (arg == "--page")
            {
                value = i + 1 < list.Length ? list[i + 1] : null;
            }
            else if (arg.StartsWith("--page=", StringComparison.Ordinal))
            {
                value = arg["--page=".Length..];
            }
            else
            {
                continue;
            }

            return TryParse(value, out CenterPage page) ? page : Welcome;
        }

        return Welcome;
    }

    /// <summary>Same as <see cref="FromArgs"/> for a raw command-line string (what a redirected activation carries).</summary>
    public static CenterPage FromCommandLine(string? commandLine) => FromArgs(CommandLine.Split(commandLine));
}

/// <summary>Windows-style command line splitting: whitespace separates, double quotes group, backslash-quote is a quote.</summary>
public static class CommandLine
{
    public static List<string> Split(string? commandLine)
    {
        var args = new List<string>();
        if (string.IsNullOrWhiteSpace(commandLine))
        {
            return args;
        }

        var current = new System.Text.StringBuilder();
        bool inQuotes = false;
        bool hasToken = false;
        for (int i = 0; i < commandLine.Length; i++)
        {
            char c = commandLine[i];
            if (c == '\\' && i + 1 < commandLine.Length && commandLine[i + 1] == '"')
            {
                current.Append('"');
                hasToken = true;
                i++;
            }
            else if (c == '"')
            {
                inQuotes = !inQuotes;
                hasToken = true;
            }
            else if (char.IsWhiteSpace(c) && !inQuotes)
            {
                if (hasToken)
                {
                    args.Add(current.ToString());
                    current.Clear();
                    hasToken = false;
                }
            }
            else
            {
                current.Append(c);
                hasToken = true;
            }
        }

        if (hasToken)
        {
            args.Add(current.ToString());
        }

        return args;
    }
}
