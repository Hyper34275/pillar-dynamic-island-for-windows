using CompanyIsland.Center.Core;
using CompanyIsland.Center.Pages;
using CompanyIsland.Center.Services;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Windows.Graphics;
using Windows.UI.ViewManagement;
using WinRT.Interop;

namespace CompanyIsland.Center;

/// <summary>What the shell needs from a page: told when it becomes (or stops being) the visible one.</summary>
public interface ICenterPage
{
    /// <summary>The page was selected (again). <paramref name="page"/> carries a note id for <c>note:&lt;id&gt;</c> and the like.</summary>
    void Show(CenterPage page);

    void Hide();

    /// <summary>The connection to the island came or went. Pages that edit island data disable their inputs.</summary>
    void OnConnectionChanged(bool connected);
}

public sealed partial class MainWindow : Window
{
    // The navigation pane is 48 DIP wide (icons) and opens over the page, so the pages keep the width they had next to the old 232 DIP pane.
    private const double DesignWidth = 820;
    private const double DesignHeight = 700;
    private const double MinWidthDip = 660;
    private const double MinHeightDip = 600;

    /// <summary>Hover intent before the pane opens (a pointer crossing it on the way to the title bar does not open it) and the grace before it closes.</summary>
    private static readonly TimeSpan PaneOpenDelay = TimeSpan.FromMilliseconds(120);
    private static readonly TimeSpan PaneCloseDelay = TimeSpan.FromMilliseconds(250);

    private readonly CenterModel _model;
    private readonly Dictionary<CenterPageKind, UIElement> _pages = [];
    private readonly IntPtr _hwnd;
    private ICenterPage? _current;
    private CenterPageKind _currentKind = CenterPageKind.Welcome;
    private bool _syncingSelection;
    private bool _barArmed;
    private AccessibilitySettings? _accessibility;
    private readonly DispatcherTimer _paneTimer = new();
    private FrameworkElement? _pane;
    private bool _pointerOnPane;
    private bool _keyboardInPane;
    private long _lastKeyDown = long.MinValue / 2;
    private const long KeyFocusWindowMs = 1000;

    public MainWindow(CenterModel model)
    {
        _model = model;
        InitializeComponent();
        Title = Strings.AppTitle;
        _hwnd = WindowNative.GetWindowHandle(this);

        // x:Bind on a Window is evaluated once at load; set the few texts explicitly so they never depend on that.
        NavWelcome.Content = Strings.NavWelcome;
        NavNotes.Content = Strings.NavNotes;
        NavSearch.Content = Strings.NavSearch;
        NavSettings.Content = Strings.NavSettings;
        NavTour.Content = Strings.NavTour;
        VersionText.Text = Strings.VersionFooter(AppVersion());
        DisconnectedBar.Title = Strings.DisconnectedTitle;
        DisconnectedBar.Message = Strings.DisconnectedMessage;
        StartIslandButton.Content = Strings.StartIsland;

        ConfigureAppWindow();

        _model.ConnectionChanged += OnConnectionChanged;
        _model.NavigateRequested += page => NavigateTo(page, bringToFront: true);
        _model.LoadFailed += message => ShowNotice(message);

        // The warning waits a moment at start-up so a connection that takes 100 ms does not flash it.
        var grace = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(1200) };
        grace.Tick += (_, _) =>
        {
            grace.Stop();
            _barArmed = true;
            OnConnectionChanged();
        };
        grace.Start();
        OnConnectionChanged();

        Closed += (_, _) =>
        {
            CenterLog.Info("window closed");
            _model.Dispose();
            Application.Current.Exit();
        };
    }

    private static string AppVersion()
    {
        Version? version = typeof(MainWindow).Assembly.GetName().Version;
        return version is null ? "" : $"{version.Major}.{version.Minor}.{version.Build}";
    }

    /// <summary>Icon, size, minimum size, position and title bar colours. Nothing here needs more than Windows 10 1809.</summary>
    private void ConfigureAppWindow()
    {
        AppWindow appWindow = AppWindow;
        string icon = Path.Combine(AppContext.BaseDirectory, "Assets", "icon.ico");
        if (File.Exists(icon))
        {
            appWindow.SetIcon(icon);
        }

        double scale = NativeMethods.GetDpiForWindow(_hwnd) / 96.0;
        DisplayArea area = DisplayArea.GetFromWindowId(appWindow.Id, DisplayAreaFallback.Primary);
        RectInt32 work = area.WorkArea;
        int width = Math.Min((int)Math.Round(DesignWidth * scale), work.Width);
        int height = Math.Min((int)Math.Round(DesignHeight * scale), work.Height);
        appWindow.Resize(new SizeInt32(width, height));
        appWindow.Move(new PointInt32(work.X + (work.Width - width) / 2, work.Y + (work.Height - height) / 2));

        UpdateMinimumSize();
        appWindow.Changed += (_, args) =>
        {
            // Dragged to another monitor (other DPI, other work area): the minimum has to follow.
            if (args.DidPositionChange || args.DidSizeChange)
            {
                UpdateMinimumSize();
            }
        };

        try
        {
            _accessibility = new AccessibilitySettings();
            _accessibility.HighContrastChanged += (_, _) => DispatcherQueue.TryEnqueue(ApplyTitleBarColors);
        }
        catch
        {
            _accessibility = null;
        }

        ApplyTitleBarColors();
    }

    /// <summary>The design minimum in physical pixels, clamped to the work area of the monitor the window is on.</summary>
    private void UpdateMinimumSize()
    {
        if (AppWindow.Presenter is not OverlappedPresenter presenter)
        {
            return;
        }

        double scale = NativeMethods.GetDpiForWindow(_hwnd) / 96.0;
        RectInt32 work = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Nearest).WorkArea;
        int minWidth = WindowSizing.MinimumPixels(MinWidthDip, scale, work.Width);
        int minHeight = WindowSizing.MinimumPixels(MinHeightDip, scale, work.Height);
        if (presenter.PreferredMinimumWidth != minWidth)
        {
            presenter.PreferredMinimumWidth = minWidth;
        }

        if (presenter.PreferredMinimumHeight != minHeight)
        {
            presenter.PreferredMinimumHeight = minHeight;
        }
    }

    /// <summary>Black and white title bar, except in a high-contrast theme where the system's own colours must stay.</summary>
    private void ApplyTitleBarColors()
    {
        if (!AppWindowTitleBar.IsCustomizationSupported())
        {
            return;
        }

        AppWindowTitleBar bar = AppWindow.TitleBar;
        if (_accessibility?.HighContrast == true)
        {
            bar.ResetToDefault();
            return;
        }

        Windows.UI.Color black = Colors.Black;
        Windows.UI.Color white = Colors.White;
        Windows.UI.Color grey = Windows.UI.Color.FromArgb(0xFF, 0x99, 0x99, 0x99);
        Windows.UI.Color hover = Windows.UI.Color.FromArgb(0xFF, 0x26, 0x26, 0x26);
        Windows.UI.Color pressed = Windows.UI.Color.FromArgb(0xFF, 0x3A, 0x3A, 0x3A);
        bar.BackgroundColor = black;
        bar.ForegroundColor = white;
        bar.InactiveBackgroundColor = black;
        bar.InactiveForegroundColor = grey;
        bar.ButtonBackgroundColor = black;
        bar.ButtonForegroundColor = white;
        bar.ButtonInactiveBackgroundColor = black;
        bar.ButtonInactiveForegroundColor = grey;
        bar.ButtonHoverBackgroundColor = hover;
        bar.ButtonHoverForegroundColor = white;
        bar.ButtonPressedBackgroundColor = pressed;
        bar.ButtonPressedForegroundColor = white;
    }

    /// <summary>Shows a page. From a launch argument or pipe event; <paramref name="bringToFront"/> also brings the window forward.</summary>
    public void NavigateTo(CenterPage page, bool bringToFront)
    {
        _current?.Hide();
        _currentKind = KindOfPage(page.Kind);
        UIElement element = EnsurePage(_currentKind);
        foreach (UIElement other in _pages.Values)
        {
            other.Visibility = ReferenceEquals(other, element) ? Visibility.Visible : Visibility.Collapsed;
        }

        _current = (ICenterPage)element;
        SyncSelection();
        _current.OnConnectionChanged(_model.Connected);
        _current.Show(page);

        if (bringToFront)
        {
            BringToFront();
        }
    }

    /// <summary>Notes, notes-new and note:&lt;id&gt; are all the Notes page.</summary>
    private static CenterPageKind KindOfPage(CenterPageKind kind) =>
        kind is CenterPageKind.NotesNew or CenterPageKind.Note ? CenterPageKind.Notes : kind;

    private UIElement EnsurePage(CenterPageKind kind)
    {
        if (_pages.TryGetValue(kind, out UIElement? existing))
        {
            return existing;
        }

        UIElement created = kind switch
        {
            CenterPageKind.Settings => new SettingsPage(_model),
            CenterPageKind.Notes => new NotesPage(_model),
            CenterPageKind.Search => new SmartSearchPage(_model),
            CenterPageKind.Tour => new TourPage(),
            _ => new WelcomePage(),
        };
        created.Visibility = Visibility.Collapsed;
        _pages[kind] = created;
        PageHost.Children.Add(created);
        return created;
    }

    private void SyncSelection()
    {
        NavigationViewItem item = _currentKind switch
        {
            CenterPageKind.Notes => NavNotes,
            CenterPageKind.Search => NavSearch,
            CenterPageKind.Settings => NavSettings,
            CenterPageKind.Tour => NavTour,
            _ => NavWelcome,
        };
        if (!ReferenceEquals(Nav.SelectedItem, item))
        {
            _syncingSelection = true;
            Nav.SelectedItem = item;
            _syncingSelection = false;
        }
    }

    private void OnNavSelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (_syncingSelection || args.SelectedItem is not NavigationViewItem { Tag: string tag })
        {
            return;
        }

        if (CenterPage.TryParse(tag, out CenterPage page) && KindOfPage(page.Kind) != _currentKind)
        {
            NavigateTo(page, bringToFront: false);
        }
    }

    /// <summary>
    /// The pane shows icons only and opens over the page while the pointer is on it or keyboard focus is in it.
    /// Without the template part (another WinUI template) it simply stays compact; the icons have tooltips.
    /// </summary>
    private void OnNavLoaded(object sender, RoutedEventArgs e)
    {
        if (_pane is not null || FindNamed(Nav, "PaneContentGrid") is not { } pane)
        {
            return;
        }

        _pane = pane;
        _paneTimer.Tick += (_, _) =>
        {
            _paneTimer.Stop();
            Nav.IsPaneOpen = _pointerOnPane || _keyboardInPane;
        };
        pane.PointerEntered += (_, _) =>
        {
            _pointerOnPane = true;
            UpdatePane();
        };
        pane.PointerExited += (_, _) =>
        {
            _pointerOnPane = false;
            UpdatePane();
        };
        pane.PointerCanceled += (_, _) =>
        {
            _pointerOnPane = false;
            UpdatePane();
        };
        // Only focus moved by a key (Tab, arrows) opens it. WinUI also reports the focus it puts on the first item when the
        // window is activated as keyboard focus, and that must not open the pane.
        Root.AddHandler(UIElement.KeyDownEvent, new KeyEventHandler((_, _) => _lastKeyDown = Environment.TickCount64), handledEventsToo: true);
        pane.GotFocus += (_, args) =>
        {
            _keyboardInPane = args.OriginalSource is Control { FocusState: FocusState.Keyboard }
                && Environment.TickCount64 - _lastKeyDown < KeyFocusWindowMs;
            UpdatePane();
        };
        pane.LostFocus += (_, _) => DispatcherQueue.TryEnqueue(() =>
        {
            _keyboardInPane = FocusManager.GetFocusedElement(Root.XamlRoot) is DependencyObject focused && IsInside(focused, pane);
            UpdatePane();
        });
    }

    private void UpdatePane()
    {
        bool open = _pointerOnPane || _keyboardInPane;
        _paneTimer.Stop();
        if (Nav.IsPaneOpen != open)
        {
            _paneTimer.Interval = open ? PaneOpenDelay : PaneCloseDelay;
            _paneTimer.Start();
        }
    }

    // The version line does not fit the 48 DIP pane, so it shows only while the pane is open.
    private void OnPaneOpening(NavigationView sender, object args) => VersionText.Visibility = Visibility.Visible;

    private void OnPaneClosing(NavigationView sender, NavigationViewPaneClosingEventArgs args) => VersionText.Visibility = Visibility.Collapsed;

    private static FrameworkElement? FindNamed(DependencyObject root, string name)
    {
        for (int i = 0; i < Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChildrenCount(root); i++)
        {
            DependencyObject child = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChild(root, i);
            if (child is FrameworkElement { Name: var childName } element && childName == name)
            {
                return element;
            }

            if (FindNamed(child, name) is { } nested)
            {
                return nested;
            }
        }

        return null;
    }

    private static bool IsInside(DependencyObject element, DependencyObject container)
    {
        for (DependencyObject? node = element; node is not null; node = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(node))
        {
            if (ReferenceEquals(node, container))
            {
                return true;
            }
        }

        return false;
    }

    private void OnConnectionChanged()
    {
        bool connected = _model.Connected;
        DisconnectedBar.IsOpen = !connected && _barArmed;
        if (connected)
        {
            NoticeBar.IsOpen = false;
        }

        foreach (UIElement page in _pages.Values)
        {
            ((ICenterPage)page).OnConnectionChanged(connected);
        }
    }

    /// <summary>A short error line under the connection bar (closable).</summary>
    public void ShowNotice(string message)
    {
        NoticeBar.Message = message;
        NoticeBar.IsOpen = true;
    }

    private void OnStartIslandClick(object sender, RoutedEventArgs e)
    {
        if (!_model.TryStartIsland())
        {
            ShowNotice(Strings.StartIslandFailed);
        }
    }

    /// <summary>Restores a minimised window, shows it and takes the foreground (the island granted that right to this process).</summary>
    public void BringToFront()
    {
        AppWindow.Show();
        if (AppWindow.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Minimized } presenter)
        {
            presenter.Restore();
        }

        NativeMethods.SetForegroundWindow(_hwnd);
        Activate();
    }

#if DEBUG
    internal FrameworkElement RootElement => Root;

    internal void DebugCommand(string page, string command)
    {
        if (command == "pane")
        {
            // As if the pointer rested on the pane.
            _pointerOnPane = true;
            Nav.IsPaneOpen = true;
            return;
        }

        if (!_pages.TryGetValue(KindOfPage(CenterPage.TryParse(page, out CenterPage parsed) ? parsed.Kind : CenterPageKind.Welcome), out UIElement? element))
        {
            return;
        }

        if (command == "bottom")
        {
            if (FindScroller(element) is { } scroller)
            {
                scroller.ChangeView(null, double.MaxValue, null, disableAnimation: true);
            }
        }
        else if (element is NotesPage notes)
        {
            notes.DebugCommand(command);
        }
    }

    private static ScrollViewer? FindScroller(DependencyObject root)
    {
        for (int i = 0; i < Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChildrenCount(root); i++)
        {
            DependencyObject child = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChild(root, i);
            if (child is ScrollViewer sv)
            {
                return sv;
            }

            if (FindScroller(child) is { } nested)
            {
                return nested;
            }
        }

        return null;
    }
#endif

    /// <summary>The "סיום" button of the welcome page.</summary>
    public void CloseCenter() => Close();
}
