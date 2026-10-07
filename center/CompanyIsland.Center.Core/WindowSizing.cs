namespace CompanyIsland.Center.Core;

/// <summary>Window size arithmetic that does not depend on WinUI, so it can be tested.</summary>
public static class WindowSizing
{
    /// <summary>
    /// The minimum size, in physical pixels, for a window whose design minimum is <paramref name="minDip"/> device-independent
    /// pixels at <paramref name="scale"/> (1.0 = 96 dpi): the design minimum, but never more than the work area, because
    /// Windows enforces the minimum even when it does not fit and the window would then hang off the screen.
    /// An unknown work area (zero or less) leaves the design minimum alone.
    /// </summary>
    public static int MinimumPixels(double minDip, double scale, int workAreaPixels)
    {
        int wanted = (int)Math.Round(minDip * (scale > 0 ? scale : 1.0));
        return workAreaPixels > 0 ? Math.Min(wanted, workAreaPixels) : wanted;
    }
}
