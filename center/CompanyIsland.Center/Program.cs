using System.Runtime.InteropServices;
using CompanyIsland.Center.Core;
using Microsoft.UI.Dispatching;
using Microsoft.Windows.AppLifecycle;
using Windows.ApplicationModel.Activation;

namespace CompanyIsland.Center;

/// <summary>
/// Our own entry point (DISABLE_XAML_GENERATED_MAIN): one Center per user session. A second launch
/// (<c>--page settings</c>) hands its arguments to the running instance and exits.
/// </summary>
public static class Program
{
    private const string InstanceKey = "CompanyIsland.Center.SingleInstance";

    [STAThread]
    public static int Main(string[] args)
    {
        WinRT.ComWrappersSupport.InitializeComWrappers();

        if (RedirectToRunningInstance())
        {
            return 0;
        }

        Microsoft.UI.Xaml.Application.Start(_ =>
        {
            var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
            SynchronizationContext.SetSynchronizationContext(context);
            var app = new App();
        });
        return 0;
    }

    /// <summary>True when another instance already runs: this launch's arguments went to it and this process should exit.</summary>
    private static bool RedirectToRunningInstance()
    {
        AppActivationArguments activation = AppInstance.GetCurrent().GetActivatedEventArgs();
        AppInstance keyInstance = AppInstance.FindOrRegisterForKey(InstanceKey);
        if (keyInstance.IsCurrent)
        {
            keyInstance.Activated += OnRedirectedActivation;
            return false;
        }

        // The island granted this process the right to take the foreground; pass it on to the instance we redirect to.
        NativeMethods.AllowSetForegroundWindow((uint)keyInstance.ProcessId);

        // Redirecting must not block the STA thread (the call needs to pump COM messages while it waits).
        IntPtr done = NativeMethods.CreateEvent(IntPtr.Zero, true, false, null);
        Task.Run(() =>
        {
            try
            {
                keyInstance.RedirectActivationToAsync(activation).AsTask().Wait();
            }
            catch
            {
                // The running instance may be closing; nothing useful to do from here.
            }
            finally
            {
                NativeMethods.SetEvent(done);
            }
        });

        var handles = new[] { done };
        NativeMethods.CoWaitForMultipleObjects(0, 0xFFFFFFFF, (uint)handles.Length, handles, out _);
        NativeMethods.CloseHandle(done);
        return true;
    }

    /// <summary>Runs on a pool thread when a later launch redirects its arguments here.</summary>
    private static void OnRedirectedActivation(object? sender, AppActivationArguments activation)
    {
        string? commandLine = null;
        if (activation.Kind == ExtendedActivationKind.Launch && activation.Data is ILaunchActivatedEventArgs launch)
        {
            commandLine = launch.Arguments;
        }

        App.OnLaunchRedirected(CenterPage.FromCommandLine(commandLine));
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
