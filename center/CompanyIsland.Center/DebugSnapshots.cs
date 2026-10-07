#if DEBUG
using CompanyIsland.Center.Core;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media.Imaging;
using System.Runtime.InteropServices.WindowsRuntime;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace CompanyIsland.Center;

/// <summary>
/// Debug builds only: when COMPANYISLAND_CENTER_SNAPSHOTS is set (comma separated page names, optionally "page+action"),
/// renders the window content to PNG files in COMPANYISLAND_CENTER_SNAPSHOT_DIR and exits. Used to look at the UI on a
/// machine where the window cannot be captured from the screen. Never compiled into Release.
/// </summary>
internal static class DebugSnapshots
{
    public static void StartIfRequested(MainWindow window)
    {
        string? spec = Environment.GetEnvironmentVariable("COMPANYISLAND_CENTER_SNAPSHOTS");
        string? dir = Environment.GetEnvironmentVariable("COMPANYISLAND_CENTER_SNAPSHOT_DIR");
        if (string.IsNullOrWhiteSpace(spec) || string.IsNullOrWhiteSpace(dir))
        {
            return;
        }

        _ = RunGuardedAsync(window, spec, dir);
    }

    private static async Task RunGuardedAsync(MainWindow window, string spec, string dir)
    {
        try
        {
            await RunAsync(window, spec, dir);
        }
        catch (Exception ex)
        {
            File.WriteAllText(Path.Combine(dir, "snapshot-error.txt"), ex.ToString());
            Application.Current.Exit();
        }
    }

    private static async Task RunAsync(MainWindow window, string spec, string dir)
    {
        Directory.CreateDirectory(dir);
        await Task.Delay(3500); // connect, load, first layout
        foreach (string item in spec.Split(',', StringSplitOptions.RemoveEmptyEntries))
        {
            string[] parts = item.Split('+', 2);
            if (CenterPage.TryParse(parts[0], out CenterPage page))
            {
                window.NavigateTo(page, bringToFront: false);
            }

            await Task.Delay(700);
            if (parts.Length > 1)
            {
                window.DebugCommand(parts[0], parts[1]);
                await Task.Delay(900);
            }

            await Task.Delay(500);
            string name = item.Replace(':', '_').Replace('+', '_');
            await SaveAsync(window.RootElement, Path.Combine(dir, "center-" + name + ".png"));
            object? focused = Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(window.RootElement.XamlRoot);
            File.AppendAllText(Path.Combine(dir, "focus.txt"), name + ": " + (focused is FrameworkElement fe ? fe.GetType().Name + " " + fe.Name : focused?.GetType().Name ?? "none") + Environment.NewLine);
        }

        Application.Current.Exit();
    }

    private static async Task SaveAsync(FrameworkElement element, string path)
    {
        var bitmap = new RenderTargetBitmap();
        await bitmap.RenderAsync(element);
        IBuffer pixels = await bitmap.GetPixelsAsync();
        using var stream = new InMemoryRandomAccessStream();
        BitmapEncoder encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, stream);
        encoder.SetPixelData(
            BitmapPixelFormat.Bgra8, BitmapAlphaMode.Premultiplied,
            (uint)bitmap.PixelWidth, (uint)bitmap.PixelHeight, 96, 96, pixels.ToArray());
        await encoder.FlushAsync();
        stream.Seek(0);
        byte[] bytes = new byte[stream.Size];
        await stream.ReadAsync(bytes.AsBuffer(), (uint)stream.Size, InputStreamOptions.None);
        await File.WriteAllBytesAsync(path, bytes);
    }
}
#endif
