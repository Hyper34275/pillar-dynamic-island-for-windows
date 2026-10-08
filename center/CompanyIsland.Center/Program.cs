using System.Diagnostics;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using CompanyIsland.Center.Core;
using Microsoft.UI.Dispatching;
using Microsoft.Windows.AppLifecycle;
using Windows.ApplicationModel.Activation;

namespace CompanyIsland.Center;

/// <summary>
/// Our own entry point (DISABLE_XAML_GENERATED_MAIN): one Center per user session. A second launch
/// (<c>--page settings</c>) hands its arguments to the running instance and exits.
/// Every stage is written to center.log (see <see cref="CenterLog"/>) before anything that can fail, so a Center that dies
/// or hangs at start-up leaves a trace even though it never reached the island's pipe.
/// </summary>
public static class Program
{
    private const string InstanceKey = "CompanyIsland.Center.SingleInstance";

    /// <summary>How long the redirect to the running instance may take before that instance is called stuck.</summary>
    private const uint RedirectTimeoutMs = 5000;

    private static readonly TimeSpan WindowWatchdog = TimeSpan.FromSeconds(10);
    private static Timer? _watchdog;
    private static volatile bool _windowShown;

    /// <summary>Called by the app when the main window has been activated.</summary>
    public static void MarkWindowShown() => _windowShown = true;

    [STAThread]
    public static int Main(string[] args)
    {
        // Nothing before this line may need WinUI or the Windows App SDK: a failure there must still be logged.
        CenterLog.Info($"process start version={VersionText()} os={Environment.OSVersion.Version} pid={Environment.ProcessId} page={PageKind(args)}");
        HookGlobalHandlers();
        try
        {
            return Run();
        }
        catch (Exception e)
        {
            CenterLog.Error("main", e);
            throw;
        }
    }

    // Its own method (never inlined): the WinUI types it uses are loaded when it is first compiled, which is inside Main's try.
    [MethodImpl(MethodImplOptions.NoInlining)]
    private static int Run()
    {
        CenterLog.Info("stage com-wrappers");
        WinRT.ComWrappersSupport.InitializeComWrappers();

        CenterLog.Info("stage single-instance");
        if (RedirectToRunningInstance())
        {
            CenterLog.Info("exit after redirect");
            return 0;
        }

        _watchdog = new Timer(_ =>
        {
            if (!_windowShown)
            {
                CenterLog.Warn($"still no window after {WindowWatchdog.TotalSeconds:0} s");
            }
        }, null, WindowWatchdog, Timeout.InfiniteTimeSpan);

        try
        {
            CenterLog.Info("stage application-start");
            Microsoft.UI.Xaml.Application.Start(_ =>
            {
                try
                {
                    var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
                    SynchronizationContext.SetSynchronizationContext(context);
                    CenterLog.Info("stage app-construct");
                    var app = new App();
                }
                catch (Exception e)
                {
                    CenterLog.Error("app-construct", e);
                    throw;
                }
            });
        }
        catch (Exception e)
        {
            CenterLog.Error("application-start", e);
            throw;
        }

        CenterLog.Info("application-start returned, exit");
        return 0;
    }

    private static string VersionText()
    {
        Version? version = typeof(Program).Assembly.GetName().Version;
        return version is null ? "?" : $"{version.Major}.{version.Minor}.{version.Build}";
    }

    /// <summary>The kind of the first page only (never a note or query id).</summary>
    private static string PageKind(string[] args)
    {
        try
        {
            return CenterPage.FromArgs(args).Kind.ToString();
        }
        catch
        {
            return "?";
        }
    }

    private static void HookGlobalHandlers()
    {
        AppDomain.CurrentDomain.UnhandledException += (_, e) =>
        {
            if (e.ExceptionObject is Exception exception)
            {
                CenterLog.Error(e.IsTerminating ? "unhandled (terminating)" : "unhandled", exception);
            }
            else
            {
                CenterLog.Warn("unhandled non-exception object");
            }
        };
        TaskScheduler.UnobservedTaskException += (_, e) => CenterLog.Error("unobserved task", e.Exception);
        AppDomain.CurrentDomain.ProcessExit += (_, _) => CenterLog.Info("process exit");
    }

    /// <summary>True when another instance already runs: this launch's arguments went to it and this process should exit.</summary>
    private static bool RedirectToRunningInstance()
    {
        AppActivationArguments activation = AppInstance.GetCurrent().GetActivatedEventArgs();
        AppInstance keyInstance = AppInstance.FindOrRegisterForKey(InstanceKey);
        if (keyInstance.IsCurrent)
        {
            CenterLog.Info("single-instance: registered, this is the first instance");
            keyInstance.Activated += OnRedirectedActivation;
            return false;
        }

        uint targetPid = keyInstance.ProcessId;
        CenterLog.Info($"single-instance: redirecting to pid {targetPid}");

        // The island granted this process the right to take the foreground; pass it on to the instance we redirect to.
        NativeMethods.AllowSetForegroundWindow(targetPid);

        // Redirecting must not block the STA thread (the call needs to pump COM messages while it waits).
        IntPtr done = NativeMethods.CreateEvent(IntPtr.Zero, true, false, null);
        string? failure = null;
        Task.Run(() =>
        {
            try
            {
                keyInstance.RedirectActivationToAsync(activation).AsTask().Wait();
            }
            catch (Exception e)
            {
                // The running instance may be closing.
                failure = FileLog.Describe(e);
            }
            finally
            {
                NativeMethods.SetEvent(done);
            }
        });

        uint wait = NativeMethods.CoWaitForMultipleObjects(0, RedirectTimeoutMs, 1, [done], out _);
        // The worker thread may still be running after a timeout: the handle is deliberately not closed then.
        bool completed = wait == 0;
        if (completed)
        {
            NativeMethods.CloseHandle(done);
        }

        TargetState target = InspectTarget(targetPid);
        if (completed && failure is null)
        {
            CenterLog.Info($"single-instance: redirected to pid {targetPid} (alive={target.Alive}, window={target.HasWindow})");
            return true;
        }

        if (!completed)
        {
            CenterLog.Warn($"single-instance: redirect to pid {targetPid} did not complete in {RedirectTimeoutMs / 1000} s (alive={target.Alive}, window={target.HasWindow})");
        }
        else
        {
            CenterLog.Warn($"single-instance: redirect to pid {targetPid} failed (alive={target.Alive}, window={target.HasWindow}): {failure}");
        }

        // A running Center that cannot take the request (or is gone) must not be a silent no-op; nothing is ever killed.
        if (!completed || target.Alive)
        {
            NativeMethods.ShowMessage(Strings.CenterStuck);
        }

        return true;
    }

    private readonly record struct TargetState(bool Alive, bool HasWindow);

    private static TargetState InspectTarget(uint pid)
    {
        try
        {
            using Process process = Process.GetProcessById((int)pid);
            if (process.HasExited)
            {
                return new TargetState(false, false);
            }

            return new TargetState(true, process.MainWindowHandle != IntPtr.Zero);
        }
        catch (Exception)
        {
            return new TargetState(false, false);
        }
    }

    /// <summary>Runs on a pool thread when a later launch redirects its arguments here.</summary>
    private static void OnRedirectedActivation(object? sender, AppActivationArguments activation)
    {
        string? commandLine = null;
        if (activation.Kind == ExtendedActivationKind.Launch && activation.Data is ILaunchActivatedEventArgs launch)
        {
            commandLine = launch.Arguments;
        }

        CenterPage page = CenterPage.FromCommandLine(commandLine);
        CenterLog.Info($"redirected launch received page={page.Kind}");
        App.OnLaunchRedirected(page);
    }
}

internal static partial class NativeMethods
{
    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool AllowSetForegroundWindow(uint processId);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool SetForegroundWindow(IntPtr hwnd);

    [LibraryImport("user32.dll")]
    public static partial uint GetDpiForWindow(IntPtr hwnd);

    [LibraryImport("user32.dll", EntryPoint = "MessageBoxW", StringMarshalling = StringMarshalling.Utf16)]
    private static partial int MessageBox(IntPtr hwnd, string text, string caption, uint type);

    /// <summary>A plain native message (right to left, in front): used when no window of ours can be shown.</summary>
    public static void ShowMessage(string text)
    {
        const uint iconWarning = 0x30, setForeground = 0x10000, topMost = 0x40000, right = 0x80000, rtlReading = 0x100000;
        try
        {
            MessageBox(IntPtr.Zero, text, Strings.AppTitle, iconWarning | setForeground | topMost | right | rtlReading);
        }
        catch (Exception e)
        {
            CenterLog.Error("message-box", e);
        }
    }

    [LibraryImport("kernel32.dll", EntryPoint = "CreateEventW", StringMarshalling = StringMarshalling.Utf16)]
    public static partial IntPtr CreateEvent(IntPtr attributes, [MarshalAs(UnmanagedType.Bool)] bool manualReset, [MarshalAs(UnmanagedType.Bool)] bool initialState, string? name);

    [LibraryImport("kernel32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool SetEvent(IntPtr handle);

    [LibraryImport("kernel32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool CloseHandle(IntPtr handle);

    [LibraryImport("ole32.dll")]
    public static partial uint CoWaitForMultipleObjects(uint flags, uint timeout, uint handleCount, IntPtr[] handles, out uint index);
}
