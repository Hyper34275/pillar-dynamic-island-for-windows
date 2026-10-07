using CompanyIsland.Center.Core;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CompanyIsland.Center.Pages;

/// <summary>One feature card on the welcome page.</summary>
public sealed record FeatureItem(string Glyph, string Title, string Description);

public sealed partial class WelcomePage : Page, ICenterPage
{
    public WelcomePage()
    {
        InitializeComponent();
        // Set explicitly as well: the text must not depend on when x:Bind one-time bindings run.
        HeroIntro.Text = Strings.WelcomeIntro;
        MockStatusText.Text = "בעוד 5 דק׳";
        StartTourButton.Content = Strings.StartTour;
        SettingsButton.Content = Strings.ToSettings;
        FinishButton.Content = Strings.Finish;
    }

    // Segoe MDL2 Assets glyphs (also in Segoe Fluent Icons).
    public IReadOnlyList<FeatureItem> FeatureItems { get; } =
    [
        new("", Strings.FeatureClockTitle, Strings.FeatureClockText),
        new("", Strings.FeatureReminderTitle, Strings.FeatureReminderText),
        new("", Strings.FeatureInviteTitle, Strings.FeatureInviteText),
        new("", Strings.FeatureNotificationsTitle, Strings.FeatureNotificationsText),
        new("", Strings.FeatureQuietTitle, Strings.FeatureQuietText),
        new("", Strings.FeatureCalendarTitle, Strings.FeatureCalendarText),
        new("", Strings.FeatureNotesTitle, Strings.FeatureNotesText),
        new("", Strings.FeatureAboutTitle, Strings.FeatureAboutText),
    ];

    public void Show(CenterPage page)
    {
    }

    public void Hide()
    {
    }

    public void OnConnectionChanged(bool connected)
    {
        // The welcome page works without the island.
    }

    private void OnStartTourClick(object sender, RoutedEventArgs e) =>
        App.Shell?.NavigateTo(new CenterPage(CenterPageKind.Tour), bringToFront: false);

    private void OnSettingsClick(object sender, RoutedEventArgs e) =>
        App.Shell?.NavigateTo(new CenterPage(CenterPageKind.Settings), bringToFront: false);

    private void OnFinishClick(object sender, RoutedEventArgs e) => App.Shell?.CloseCenter();
}
