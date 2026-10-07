using CompanyIsland.Center.Core;
using CompanyIsland.Center.Services;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CompanyIsland.Center.Pages;

/// <summary>
/// Every island setting. A change is sent at once (optimistic: the control already shows the new value); if the island
/// refuses, the model restores the old settings, this page redraws from them and shows an error bar.
/// Live: a <c>settings-changed</c> event redraws the page too.
/// </summary>
public sealed partial class SettingsPage : Page, ICenterPage
{
    private static readonly int[] ReminderChoices = [5, 10, 15, 30];

    private readonly CenterModel _model;
    private bool _updating;
    private bool _connected;

    public SettingsPage(CenterModel model)
    {
        _model = model;
        InitializeComponent();
        _model.SettingsChanged += Render;
        _model.MonitorsChanged += Render;
        _model.AccessChanged += Render;
        Render();
    }

    public void Show(CenterPage page) => Render();

    public void Hide()
    {
    }

    public void OnConnectionChanged(bool connected)
    {
        _connected = connected;
        if (connected)
        {
            ErrorBar.IsOpen = false;
        }

        Render();
    }

    /// <summary>Redraws every control from the model without raising change events.</summary>
    private void Render()
    {
        IslandSettings? settings = _model.Settings;
        RowsPanel.IsEnabled = _connected && settings is not null;
        if (settings is null)
        {
            return;
        }

        _updating = true;
        try
        {
            LaunchToggle.IsOn = settings.LaunchWithWindows;
            FullscreenToggle.IsOn = settings.HideInFullscreen;
            RemindersToggle.IsOn = settings.MeetingReminderEnabled;
            InvitesToggle.IsOn = settings.MeetingInvitesEnabled;
            SilenceToggle.IsOn = settings.MeetingSilencePrompt;
            NotificationsToggle.IsOn = settings.NotificationsEnabled;
            MinutesRow.IsEnabled = settings.MeetingReminderEnabled;

            RenderMinutes(settings.ReminderMinutes);
            RenderMonitors(settings.MonitorId);
            RenderDisplay(settings.IslandDisplay);
            RenderAccess(settings.NotificationsEnabled);
        }
        finally
        {
            _updating = false;
        }
    }

    private void RenderMinutes(int current)
    {
        // The usual choices, plus the current value when it is something else (an older file or a hand edit).
        var choices = ReminderChoices.Contains(current) ? ReminderChoices.ToList() : [.. ReminderChoices, current];
        choices.Sort();
        bool same = MinutesCombo.Items.Count == choices.Count &&
                    choices.Select((n, i) => (MinutesCombo.Items[i] as ComboBoxItem)?.Tag is int tag && tag == n).All(b => b);
        if (!same)
        {
            MinutesCombo.Items.Clear();
            foreach (int minutes in choices)
            {
                MinutesCombo.Items.Add(new ComboBoxItem { Content = Strings.Minutes(minutes), Tag = minutes });
            }
        }

        MinutesCombo.SelectedIndex = choices.IndexOf(current);
    }

    private void RenderMonitors(string? selectedId)
    {
        IReadOnlyList<MonitorInfo> monitors = _model.Monitors;
        MonitorRow.Visibility = monitors.Count > 1 ? Visibility.Visible : Visibility.Collapsed;
        if (monitors.Count <= 1)
        {
            return;
        }

        string effective = selectedId ?? monitors.FirstOrDefault(m => m.IsPrimaryDisplay)?.Id ?? monitors[0].Id;
        bool same = MonitorCombo.Items.Count == monitors.Count &&
                    monitors.Select((m, i) => (MonitorCombo.Items[i] as ComboBoxItem)?.Tag is string id && id == m.Id).All(b => b);
        if (!same)
        {
            // Rebuilt only when the list of displays changed, never while the drop-down is reporting a choice.
            MonitorCombo.Items.Clear();
            for (int i = 0; i < monitors.Count; i++)
            {
                MonitorCombo.Items.Add(new ComboBoxItem { Content = MonitorLabel(monitors[i], i), Tag = monitors[i].Id });
            }
        }

        MonitorCombo.SelectedIndex = monitors.Select((m, i) => (m, i)).FirstOrDefault(t => t.m.Id == effective).i;
    }

    private static string MonitorLabel(MonitorInfo monitor, int index)
    {
        string name = string.IsNullOrWhiteSpace(monitor.Name)
            ? (monitor.IsPrimaryDisplay ? Strings.MonitorPrimary : Strings.MonitorN(index + 1))
            : monitor.Name;
        return monitor.Width > 0 && monitor.Height > 0 ? $"{name} · {monitor.Width}×{monitor.Height}" : name;
    }

    private void RenderDisplay(string display)
    {
        RadioButton button = IslandDisplays.Normalize(display) switch
        {
            IslandDisplays.Clock => DisplayClockButton,
            IslandDisplays.Date => DisplayDateButton,
            _ => DisplayFullButton,
        };
        DisplayChoice.SelectedItem = button;
    }

    /// <summary>The access row: only when notifications are on and Windows has not said yes (the button only when it has not said anything).</summary>
    private void RenderAccess(bool notificationsEnabled)
    {
        NotificationAccess access = _model.Access;
        bool show = notificationsEnabled && access is not (NotificationAccess.Allowed or NotificationAccess.Unknown);
        AccessRow.Visibility = show ? Visibility.Visible : Visibility.Collapsed;
        AccessRow.Description = Strings.NotificationStatus(access);
        AccessButton.Visibility = access == NotificationAccess.Unspecified ? Visibility.Visible : Visibility.Collapsed;
    }

    private async Task ApplyAsync(SettingsPatch patch)
    {
        // Controls also raise their change events when they first load or are redrawn; only a real change is sent.
        if (_updating || patch.IsEmpty || _model.Settings is not { } current || patch.ApplyTo(current) == current)
        {
            return;
        }

        ErrorBar.IsOpen = false;
        bool ok = await _model.UpdateSettingsAsync(patch);
        if (!ok)
        {
            ErrorBar.Message = Strings.SaveSettingsFailed;
            ErrorBar.IsOpen = true;
        }
    }

    private void OnToggleChanged(object sender, RoutedEventArgs e)
    {
        if (_updating || sender is not ToggleSwitch toggle)
        {
            return;
        }

        bool on = toggle.IsOn;
        var patch = new SettingsPatch();
        if (ReferenceEquals(toggle, LaunchToggle)) patch.LaunchWithWindows(on);
        else if (ReferenceEquals(toggle, FullscreenToggle)) patch.HideInFullscreen(on);
        else if (ReferenceEquals(toggle, RemindersToggle)) patch.MeetingReminderEnabled(on);
        else if (ReferenceEquals(toggle, InvitesToggle)) patch.MeetingInvitesEnabled(on);
        else if (ReferenceEquals(toggle, SilenceToggle)) patch.MeetingSilencePrompt(on);
        else if (ReferenceEquals(toggle, NotificationsToggle)) patch.NotificationsEnabled(on);
        _ = ApplyAsync(patch);
    }

    private void OnMinutesChanged(object sender, SelectionChangedEventArgs e)
    {
        if (!_updating && MinutesCombo.SelectedItem is ComboBoxItem { Tag: int minutes })
        {
            _ = ApplyAsync(new SettingsPatch().ReminderMinutes(minutes));
        }
    }

    private void OnMonitorChanged(object sender, SelectionChangedEventArgs e)
    {
        if (!_updating && MonitorCombo.SelectedItem is ComboBoxItem { Tag: string id })
        {
            _ = ApplyAsync(new SettingsPatch().MonitorId(id));
        }
    }

    private void OnDisplayChanged(object sender, SelectionChangedEventArgs e)
    {
        if (!_updating && DisplayChoice.SelectedItem is RadioButton { Tag: string value })
        {
            _ = ApplyAsync(new SettingsPatch().IslandDisplay(value));
        }
    }

    private async void OnAccessClick(object sender, RoutedEventArgs e)
    {
        AccessButton.IsEnabled = false;
        bool ok = await _model.RequestNotificationAccessAsync();
        AccessButton.IsEnabled = true;
        if (!ok)
        {
            ErrorBar.Message = Strings.NotificationAccessFailed;
            ErrorBar.IsOpen = true;
        }
    }

    private async void OnOpenLogDirClick(object sender, RoutedEventArgs e)
    {
        if (!await _model.OpenLogDirAsync())
        {
            ErrorBar.Message = Strings.LoadFailed;
            ErrorBar.IsOpen = true;
        }
    }

    private async void OnShowDiagnosticsClick(object sender, RoutedEventArgs e)
    {
        if (!await _model.ShowIslandAsync(IslandTabs.Settings))
        {
            ErrorBar.Message = Strings.LoadFailed;
            ErrorBar.IsOpen = true;
        }
    }
}
