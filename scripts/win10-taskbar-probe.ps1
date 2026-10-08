<#
.SYNOPSIS
  READ-ONLY dump of the Windows taskbar for CompanyIsland's AI button anchor.

.DESCRIPTION
  Run once on a Windows 10 21H2 PC (a normal user session, no admin needed) with the taskbar search
  box shown ("Show search box"). It changes nothing: it only reads window classes and rectangles,
  one registry value, the taskbar position and DPI, and (optionally) the UI Automation tree of the
  taskbar. The report is a plain text file; send it back so the class names the app assumes
  (Shell_TrayWnd / TrayDummySearchControl) can be confirmed or corrected.

  No personal data is collected: window titles are NOT written (only classes, rectangles and
  styles). UI Automation names are written only for the elements whose name looks like the search
  box ("search", "חיפוש", "type here"); everything else is class/control type only.

.PARAMETER Out
  Report path (default: taskbar-probe.txt next to the script, or the desktop if that is read-only).

.PARAMETER SkipUia
  Skip the UI Automation section (it asks Explorer for its accessibility tree, which can take a few seconds).
#>
param(
    [string]$Out = (Join-Path $PSScriptRoot 'taskbar-probe.txt'),
    [switch]$SkipUia
)

$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class TbProbe {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct APPBARDATA {
        public uint cbSize; public IntPtr hWnd; public uint uCallbackMessage; public uint uEdge; public RECT rc; public IntPtr lParam;
    }
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, EnumProc cb, IntPtr lParam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder sb, int max);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] static extern IntPtr GetWindowLongPtr(IntPtr h, int idx);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("shell32.dll")] static extern UIntPtr SHAppBarMessage(uint msg, ref APPBARDATA data);

    public static IntPtr Find(string cls) { return FindWindow(cls, null); }

    public class Info {
        public IntPtr Hwnd; public IntPtr Parent; public string Class; public RECT Rect; public bool Visible;
        public long Style; public long ExStyle; public uint Pid; public uint Dpi;
    }

    static Info Describe(IntPtr h) {
        var sb = new StringBuilder(256);
        GetClassName(h, sb, sb.Capacity);
        RECT r; GetWindowRect(h, out r);
        uint pid; GetWindowThreadProcessId(h, out pid);
        uint dpi = 0; try { dpi = GetDpiForWindow(h); } catch { }
        return new Info {
            Hwnd = h, Parent = GetParent(h), Class = sb.ToString(), Rect = r, Visible = IsWindowVisible(h),
            Style = GetWindowLongPtr(h, -16).ToInt64(), ExStyle = GetWindowLongPtr(h, -20).ToInt64(), Pid = pid, Dpi = dpi
        };
    }

    public static Info Self(IntPtr h) { return Describe(h); }

    public static List<Info> Children(IntPtr parent) {
        var list = new List<Info>();
        EnumChildWindows(parent, delegate (IntPtr h, IntPtr l) { list.Add(Describe(h)); return true; }, IntPtr.Zero);
        return list;
    }

    // ABM_GETTASKBARPOS = 5, ABM_GETSTATE = 4
    public static string TaskbarPos() {
        var d = new APPBARDATA(); d.cbSize = (uint)Marshal.SizeOf(typeof(APPBARDATA));
        UIntPtr ok = SHAppBarMessage(5, ref d);
        string[] edges = { "left", "top", "right", "bottom" };
        string edge = d.uEdge < 4 ? edges[d.uEdge] : d.uEdge.ToString();
        return string.Format("ok={0} edge={1} rect=({2},{3})-({4},{5})", ok != UIntPtr.Zero, edge, d.rc.Left, d.rc.Top, d.rc.Right, d.rc.Bottom);
    }
    public static string TaskbarState() {
        var d = new APPBARDATA(); d.cbSize = (uint)Marshal.SizeOf(typeof(APPBARDATA));
        uint s = (uint)SHAppBarMessage(4, ref d);
        return string.Format("0x{0:X} autohide={1} alwaysOnTop={2}", s, (s & 1) != 0, (s & 2) != 0);
    }
}
'@

$lines = New-Object System.Collections.Generic.List[string]
function Add-Line([string]$s) { $script:lines.Add($s) }
function Fmt-Rect($r) { '({0},{1})-({2},{3}) {4}x{5}' -f $r.Left, $r.Top, $r.Right, $r.Bottom, ($r.Right - $r.Left), ($r.Bottom - $r.Top) }

$os = Get-CimInstance Win32_OperatingSystem
Add-Line "CompanyIsland taskbar probe (read-only)"
Add-Line ("Date: {0}" -f (Get-Date -Format 's'))
Add-Line ("OS: {0} build {1}" -f $os.Caption, $os.BuildNumber)
Add-Line ("Display scale (HKCU LogPixels): {0}" -f ((Get-ItemProperty 'HKCU:\Control Panel\Desktop' -ErrorAction SilentlyContinue).LogPixels))
Add-Line ''

Add-Line '== Registry (read) =='
$search = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Search' -ErrorAction SilentlyContinue
Add-Line ("SearchboxTaskbarMode = {0}   (0 hidden, 1 icon, 2 box; empty = value missing)" -f $search.SearchboxTaskbarMode)
$adv = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -ErrorAction SilentlyContinue
Add-Line ("ShowCortanaButton = {0}   TaskbarSmallIcons = {1}" -f $adv.ShowCortanaButton, $adv.TaskbarSmallIcons)
Add-Line ''

Add-Line '== Taskbar position (SHAppBarMessage) =='
Add-Line ('ABM_GETTASKBARPOS: ' + [TbProbe]::TaskbarPos())
Add-Line ('ABM_GETSTATE:      ' + [TbProbe]::TaskbarState())
Add-Line ''

Add-Line '== Monitors =='
Add-Type -AssemblyName System.Windows.Forms
foreach ($s in [System.Windows.Forms.Screen]::AllScreens) {
    Add-Line ("{0} primary={1} bounds={2} work={3}" -f $s.DeviceName, $s.Primary, $s.Bounds, $s.WorkingArea)
}
Add-Line ''

foreach ($cls in @('Shell_TrayWnd', 'Shell_SecondaryTrayWnd')) {
    $h = [TbProbe]::Find($cls)
    Add-Line ("== {0} ==" -f $cls)
    if ($h -eq [IntPtr]::Zero) { Add-Line '(not found)'; Add-Line ''; continue }
    $self = [TbProbe]::Self($h)
    Add-Line ("hwnd=0x{0:X} pid={1} dpi={2} visible={3} style=0x{4:X} exstyle=0x{5:X} rect={6}" -f $h.ToInt64(), $self.Pid, $self.Dpi, $self.Visible, $self.Style, $self.ExStyle, (Fmt-Rect $self.Rect))
    Add-Line 'children (EnumChildWindows, all depths):'
    foreach ($c in [TbProbe]::Children($h)) {
        $mark = if ($c.Class -match 'Search|Cortana|TrayButton|Dummy') { '  <== search-related?' } else { '' }
        Add-Line ("  hwnd=0x{0:X} parent=0x{1:X} class={2} visible={3} style=0x{4:X} exstyle=0x{5:X} rect={6}{7}" -f $c.Hwnd.ToInt64(), $c.Parent.ToInt64(), $c.Class, $c.Visible, $c.Style, $c.ExStyle, (Fmt-Rect $c.Rect), $mark)
    }
    Add-Line ''
}

Add-Line '== Assumed anchor =='
$h = [TbProbe]::Find('Shell_TrayWnd')
$dummy = $null
if ($h -ne [IntPtr]::Zero) { $dummy = [TbProbe]::Children($h) | Where-Object { $_.Class -eq 'TrayDummySearchControl' } | Select-Object -First 1 }
if ($dummy) {
    Add-Line ("TrayDummySearchControl found: visible={0} rect={1}" -f $dummy.Visible, (Fmt-Rect $dummy.Rect))
    Add-Line "CHECK: does this rectangle match the visible search box, or does it span the whole free taskbar area?"
} else {
    Add-Line 'TrayDummySearchControl NOT found as a descendant of Shell_TrayWnd.'
}
Add-Line ''

if (-not $SkipUia) {
    Add-Line '== UI Automation (taskbar, depth <= 4) =='
    try {
        Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
        $root = [System.Windows.Automation.AutomationElement]::RootElement
        $cond = New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::ClassNameProperty), 'Shell_TrayWnd'
        $tray = $root.FindFirst([System.Windows.Automation.TreeScope]::Children, $cond)
        if (-not $tray) { Add-Line '(Shell_TrayWnd not in the UIA tree)' }
        else {
            $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
            $count = 0
            function Walk($el, $depth) {
                if ($depth -gt 4 -or $script:count -gt 200) { return }
                $script:count++
                $c = $el.Current
                $name = ''
                if ($c.Name -match 'search|חיפוש|type here|cortana') { $name = ' name="' + $c.Name + '"' }
                $r = $c.BoundingRectangle
                Add-Line ('{0}{1} class={2} autoId={3}{4} rect=({5:N0},{6:N0},{7:N0},{8:N0})' -f ('  ' * $depth), $c.ControlType.ProgrammaticName, $c.ClassName, $c.AutomationId, $name, $r.Left, $r.Top, $r.Right, $r.Bottom)
                $child = $walker.GetFirstChild($el)
                while ($child) { Walk $child ($depth + 1); $child = $walker.GetNextSibling($child) }
            }
            Walk $tray 0
        }
    } catch {
        Add-Line ('UI Automation unavailable: ' + $_.Exception.Message)
    }
    Add-Line ''
}

Add-Line '== Foreground-window classes worth knowing =='
Add-Line 'Press Win+S, then run this script again from a second console is not possible (focus moves);'
Add-Line 'instead, with the search flyout open, run:  (Get-Process SearchApp,SearchUI,SearchHost -ErrorAction SilentlyContinue | Select Name,Id)'

$target = $Out
try {
    $lines | Out-File -FilePath $target -Encoding utf8
} catch {
    $target = Join-Path ([Environment]::GetFolderPath('Desktop')) 'taskbar-probe.txt'
    $lines | Out-File -FilePath $target -Encoding utf8
}
Write-Host "Report written to $target ($($lines.Count) lines). Nothing on the system was changed."
