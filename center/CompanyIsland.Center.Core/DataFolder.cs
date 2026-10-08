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
        // A "Yuval" with no file in it (only empty folders) is what the island treats as absent too:
        // it removes the shell and migrates.
        if (Directory.Exists(current) && HasFiles(current))
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

    /// <summary>True when any file is anywhere below <paramref name="directory"/>; a folder that cannot be read counts as content.</summary>
    private static bool HasFiles(string directory)
    {
        try
        {
            using IEnumerator<string> files = Directory.EnumerateFiles(directory, "*", SearchOption.AllDirectories).GetEnumerator();
            return files.MoveNext();
        }
        catch (IOException)
        {
            return true;
        }
        catch (UnauthorizedAccessException)
        {
            return true;
        }
    }
}
