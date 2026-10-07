using CompanyIsland.Center.Core;
using CompanyIsland.Center.Services;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;

namespace CompanyIsland.Center;

public partial class App : Application
{
    private static DispatcherQueue? _ui;
    private static MainWindow? _window;
    private static CenterPage? _pendingPage;

    public App()
    {
        InitializeComponent();
        UnhandledException += (_, e) =>
        {
            // Once the window is up, a crash in a page must not take the Center down with it.
            e.Handled = _window is not null;
            Model?.Report("ui", e.Exception);
        };
    }

    public static CenterModel? Model { get; private set; }

    public static MainWindow? Shell => _window;

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        _ui = DispatcherQueue.GetForCurrentThread();
        Model = new CenterModel(_ui);
        _window = new MainWindow(Model);

        // Environment.GetCommandLineArgs: the first launch's own arguments ("--page notes-new").
        CenterPage page = _pendingPage ?? CenterPage.FromArgs(Environment.GetCommandLineArgs().Skip(1));
        _pendingPage = null;
        _window.NavigateTo(page, bringToFront: false);
        _window.Activate();
        Model.Start();
#if DEBUG
        DebugSnapshots.StartIfRequested(_window);
#endif
    }

    /// <summary>A later launch handed its arguments over (any thread): navigate and come forward.</summary>
    public static void OnLaunchRedirected(CenterPage page)
    {
        DispatcherQueue? ui = _ui;
        if (ui is null)
        {
            _pendingPage = page; // the first launch has not finished starting
            return;
        }

        ui.TryEnqueue(() => _window?.NavigateTo(page, bringToFront: true));
    }
}
