namespace CompanyIsland.Center.Core;

/// <summary>
/// The per-user data folder the island and the Center share: <c>%LOCALAPPDATA%\Yuval</c>. The island (src-tauri/src/paths.rs)
/// moves an older <c>CompanyIsland</c> (or <c>PILLAR</c>) folder there once, before it opens any file, and it is the island
/// that starts the Center, so normally the new folder is already there when the Center runs. A Center started on its own
/// before that migration must not create <c>Yuval</c> itself (an existing folder stops the island from migrating), so it
/// keeps using the older folder until the island has moved it.
/// </summary>
public static class DataFolder
{
    public const string Name = "Yuval";

    /// <summary>Names of the folder in earlier versions, newest first (the same list as <c>LEGACY_APP_DIRS</c> in paths.rs).</summary>
    public static readonly string[] LegacyNames = { "CompanyIsland", "PILLAR" };

    /// <summary>The folder the Center reads and writes under <paramref name="localAppData"/>. Nothing is created here.</summary>
    public static string Resolve(string localAppData)
    {
        string current = Path.Combine(localAppData, Name);
        // An empty "Yuval" is what the island treats as absent too: it removes it and migrates.
        if (Directory.Exists(current) && !IsEmpty(current))
        {
            return current;
        }

        foreach (string legacy in LegacyNames)
        {
            string path = Path.Combine(localAppData, legacy);
            if (Directory.Exists(path))
            {
                return path;
            }
        }

        return current;
    }

    /// <summary><see cref="Resolve"/> on the real <c>%LOCALAPPDATA%</c>.</summary>
    public static string Resolve() => Resolve(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData));

    private static bool IsEmpty(string directory)
    {
        try
        {
            using IEnumerator<string> entries = Directory.EnumerateFileSystemEntries(directory).GetEnumerator();
            return !entries.MoveNext();
        }
        catch (IOException)
        {
            return false;
        }
        catch (UnauthorizedAccessException)
        {
            return false;
        }
    }
}
