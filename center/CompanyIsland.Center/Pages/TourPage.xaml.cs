using System.Text.Json;
using CompanyIsland.Center.Core;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.Web.WebView2.Core;

namespace CompanyIsland.Center.Pages;

/// <summary>
/// The guided tour: <c>web\tour.html</c> (built by Vite from the island's own components with mock data) inside a locked-down
/// WebView2: its own user data folder, the island's browser arguments, a virtual host, no dev tools / context menu /
/// status bar / zoom / accelerator keys, and no navigation away from the tour's origin and no new windows.
/// The page may only ask the host to go to welcome, settings or notes, or to say it is done.
/// </summary>
public sealed partial class TourPage : Page, ICenterPage
{
    private const string HostName = "tour.companyisland.invalid";
    private static readonly string Origin = "https://" + HostName;
    private static readonly string TourUrl = Origin + "/tour.html";

    // Identical to the island's own WebView2 arguments (src-tauri tauri.conf.json).
    private const string BrowserArguments = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-background-networking";

    private bool _starting;
    private bool _ready;
    private bool _failed;
    private bool _everShown;

    public TourPage()
    {
        InitializeComponent();
    }

    public void Show(CenterPage page)
    {
        if (_failed)
        {
            return;
        }

        if (_ready)
        {
            // Every visit starts from the first step.
            if (_everShown)
            {
                Web.CoreWebView2.Reload();
            }

            _everShown = true;
            return;
        }

        _everShown = true;
        if (!_starting)
        {
            _starting = true;
            _ = StartAsync();
        }
    }

    public void Hide()
    {
    }

    public void OnConnectionChanged(bool connected)
    {
        // The tour is a mock; it needs no island.
    }

    private static string WebFolder()
    {
#if DEBUG
        string? overridePath = Environment.GetEnvironmentVariable("COMPANYISLAND_TOUR_DIR");
        if (!string.IsNullOrWhiteSpace(overridePath) && Path.IsPathRooted(overridePath))
        {
            return overridePath;
        }
#endif
        return Path.Combine(AppContext.BaseDirectory, "web");
    }

    private void ShowProblem(string title, string message)
    {
        _failed = true;
        ProblemBar.Title = title;
        ProblemBar.Message = message;
        ProblemBar.IsOpen = true;
        Web.Visibility = Visibility.Collapsed;
    }

    private async Task StartAsync()
    {
        string webFolder = WebFolder();
        if (!File.Exists(Path.Combine(webFolder, "tour.html")))
        {
            ShowProblem(Strings.TourMissingTitle, Strings.TourMissingBody);
            App.Model?.Report("tour", new FileNotFoundException());
            return;
        }

        try
        {
            string userData = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "CompanyIsland", "EBWebView-Center");
            var options = new CoreWebView2EnvironmentOptions { AdditionalBrowserArguments = BrowserArguments };
            CoreWebView2Environment environment = await CoreWebView2Environment.CreateWithOptionsAsync(null, userData, options);
            await Web.EnsureCoreWebView2Async(environment);

            CoreWebView2 core = Web.CoreWebView2;
            CoreWebView2Settings settings = core.Settings;
            settings.AreDevToolsEnabled = false;
            settings.AreDefaultContextMenusEnabled = false;
            settings.IsStatusBarEnabled = false;
            settings.IsZoomControlEnabled = false;
            settings.AreBrowserAcceleratorKeysEnabled = false;
            settings.AreDefaultScriptDialogsEnabled = false;
            settings.IsGeneralAutofillEnabled = false;
            settings.IsPasswordAutosaveEnabled = false;
            settings.IsPinchZoomEnabled = false;
            settings.IsSwipeNavigationEnabled = false;
            settings.IsBuiltInErrorPageEnabled = false;

            core.SetVirtualHostNameToFolderMapping(HostName, webFolder, CoreWebView2HostResourceAccessKind.Allow);
            core.NavigationStarting += OnNavigationStarting;
            core.NewWindowRequested += (_, e) => e.Handled = true; // never a second window
            core.DownloadStarting += (_, e) => e.Cancel = true;
            core.PermissionRequested += (_, e) => e.State = CoreWebView2PermissionState.Deny;
            core.WebMessageReceived += OnWebMessage;

            Web.DefaultBackgroundColor = Colors.Black;
            Web.Visibility = Visibility.Visible;
            _ready = true;
            core.Navigate(TourUrl);
        }
        catch (Exception ex)
        {
            ShowProblem(Strings.TourMissingTitle, Strings.TourWebViewFailed);
            App.Model?.Report("tour", ex);
        }
    }

    /// <summary>Only the tour's own origin may load; anything else is cancelled.</summary>
    private static void OnNavigationStarting(CoreWebView2 sender, CoreWebView2NavigationStartingEventArgs e)
    {
        if (!Uri.TryCreate(e.Uri, UriKind.Absolute, out Uri? uri) ||
            !string.Equals(uri.Scheme, "https", StringComparison.OrdinalIgnoreCase) ||
            !string.Equals(uri.Host, HostName, StringComparison.OrdinalIgnoreCase))
        {
            e.Cancel = true;
        }
    }

    /// <summary>The tour asks the host to move: <c>{"type":"navigate","page":"settings"}</c> or <c>{"type":"done"}</c>.</summary>
    private void OnWebMessage(CoreWebView2 sender, CoreWebView2WebMessageReceivedEventArgs e)
    {
        try
        {
            if (!Uri.TryCreate(e.Source, UriKind.Absolute, out Uri? source) ||
                !string.Equals(source.Host, HostName, StringComparison.OrdinalIgnoreCase))
            {
                return;
            }

            using JsonDocument doc = JsonDocument.Parse(e.WebMessageAsJson);
            JsonElement root = doc.RootElement;
            if (root.ValueKind != JsonValueKind.Object || !root.TryGetProperty("type", out JsonElement type))
            {
                return;
            }

            switch (type.GetString())
            {
                case "navigate":
                    if (root.TryGetProperty("page", out JsonElement page) &&
                        page.ValueKind == JsonValueKind.String &&
                        CenterPage.TryParse(page.GetString(), out CenterPage target) &&
                        target.Kind is CenterPageKind.Welcome or CenterPageKind.Settings or CenterPageKind.Notes)
                    {
                        App.Shell?.NavigateTo(target, bringToFront: false);
                    }

                    break;
                case "done":
                    App.Shell?.NavigateTo(CenterPage.Welcome, bringToFront: false);
                    break;
            }
        }
        catch (Exception ex)
        {
            App.Model?.Report("tour-message", ex);
        }
    }
}
