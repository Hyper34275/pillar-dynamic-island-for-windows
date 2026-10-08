using CompanyIsland.Center.Core;

namespace CompanyIsland.Center.Tests;

public sealed class DataFolderTests : IDisposable
{
    private readonly string _base = Path.Combine(Path.GetTempPath(), "yuval-datafolder-" + Guid.NewGuid().ToString("N"));

    public DataFolderTests() => Directory.CreateDirectory(_base);

    public void Dispose()
    {
        try { Directory.Delete(_base, recursive: true); } catch (IOException) { }
    }

    private string Make(string name, string? file = null)
    {
        string dir = Path.Combine(_base, name);
        Directory.CreateDirectory(dir);
        if (file is not null)
        {
            File.WriteAllText(Path.Combine(dir, file), "x");
        }

        return dir;
    }

    [Fact]
    public void The_data_folder_is_called_Yuval()
    {
        Assert.Equal("Yuval", DataFolder.Name);
        Assert.Equal(new[] { "CompanyIsland", "PILLAR" }, DataFolder.LegacyNames);
    }

    [Fact]
    public void The_new_folder_wins_when_it_has_content()
    {
        string yuval = Make("Yuval", "settings.json");
        Make("CompanyIsland", "settings.json");
        Assert.Equal(yuval, DataFolder.Resolve(_base));
    }

    [Fact]
    public void A_fresh_install_uses_the_new_name_without_creating_it()
    {
        Assert.Equal(Path.Combine(_base, "Yuval"), DataFolder.Resolve(_base));
        Assert.False(Directory.Exists(Path.Combine(_base, "Yuval")), "an existing Yuval folder would stop the island from migrating");
    }

    [Fact]
    public void Before_the_island_migrated_the_old_folder_is_used_and_Yuval_is_not_created()
    {
        string old = Make("CompanyIsland", "settings.json");
        Assert.Equal(old, DataFolder.Resolve(_base));
        Assert.False(Directory.Exists(Path.Combine(_base, "Yuval")));
    }

    [Fact]
    public void An_empty_Yuval_folder_counts_as_not_there_like_in_the_island()
    {
        Make("Yuval");
        string old = Make("CompanyIsland", "settings.json");
        Assert.Equal(old, DataFolder.Resolve(_base));
    }

    [Fact]
    public void The_PILLAR_folder_is_the_last_resort_and_CompanyIsland_comes_first()
    {
        string pillar = Make("PILLAR", "settings.json");
        Assert.Equal(pillar, DataFolder.Resolve(_base));
        string company = Make("CompanyIsland", "settings.json");
        Assert.Equal(company, DataFolder.Resolve(_base));
    }

    [Fact]
    public void The_real_folder_is_under_the_local_app_data_folder()
    {
        string local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        Assert.StartsWith(local, DataFolder.Resolve(), StringComparison.OrdinalIgnoreCase);
    }
}
