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

    public void Show(CenterPage page)
    {
        Render();
        _ = LoadAiAsync();
    }

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
        if (connected)
        {
            _ = LoadAiAsync();
        }
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
            SearchEnabledToggle.IsOn = settings.AiSearchEnabled;
            SearchButtonToggle.IsOn = settings.AiSearchButton;
            SearchHotkeyToggle.IsOn = settings.AiSearchHotkey;
            // The button and the hotkey only mean something while smart search is on.
            SearchButtonRow.IsEnabled = settings.AiSearchEnabled;
            SearchHotkeyRow.IsEnabled = settings.AiSearchEnabled;
            MinutesRow.IsEnabled = settings.MeetingReminderEnabled;

            RenderMinutes(settings.ReminderMinutes);
            RenderPrefetch(settings.CalendarPrefetchDays);
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

    private static readonly int[] PrefetchChoices = [0, 3, 7, 14, 30];

    private void RenderPrefetch(int current)
    {
        // The listed choices, plus the stored value (shown as its number) when it is something else.
        var choices = PrefetchChoices.Contains(current) ? PrefetchChoices.ToList() : [.. PrefetchChoices, current];
        choices.Sort();
        bool same = PrefetchCombo.Items.Count == choices.Count &&
                    choices.Select((n, i) => (PrefetchCombo.Items[i] as ComboBoxItem)?.Tag is int tag && tag == n).All(b => b);
        if (!same)
        {
            PrefetchCombo.Items.Clear();
            foreach (int days in choices)
            {
                string label = PrefetchChoices.Contains(days) ? Strings.PrefetchDays(days) : days.ToString();
                PrefetchCombo.Items.Add(new ComboBoxItem { Content = label, Tag = days });
            }
        }

        PrefetchCombo.SelectedIndex = choices.IndexOf(current);
    }

    private void OnPrefetchChanged(object sender, SelectionChangedEventArgs e)
    {
        if (!_updating && PrefetchCombo.SelectedItem is ComboBoxItem { Tag: int days })
        {
            _ = ApplyAsync(new SettingsPatch().CalendarPrefetchDays(days));
        }
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
        else if (ReferenceEquals(toggle, SearchEnabledToggle)) patch.AiSearchEnabled(on);
        else if (ReferenceEquals(toggle, SearchButtonToggle)) patch.AiSearchButton(on);
        else if (ReferenceEquals(toggle, SearchHotkeyToggle)) patch.AiSearchHotkey(on);
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

    // ---- The user's AI server (docs/AI_API.md) ----

    private AiConfig? _ai;
    private bool _aiClearKey;

    private async Task LoadAiAsync()
    {
        if (!_connected)
        {
            return;
        }

        try
        {
            RenderAi(await _model.Client.AiGetConfigAsync());
        }
        catch (IslandException ex) when (ex.Code == "APP-031")
        {
            // an island from before the AI connection: the section has nothing to talk to
            AiSection.Visibility = Visibility.Collapsed;
        }
        catch (Exception ex)
        {
            _model.Report("aiGetConfig", ex);
        }
    }

    private void RenderAi(AiConfig config)
    {
        _ai = config;
        _aiClearKey = false;
        AiSection.Visibility = Visibility.Visible;
        AiEnabledToggle.Toggled -= OnAiEnabledToggled;
        AiEnabledToggle.IsOn = config.Enabled;
        AiEnabledToggle.Toggled += OnAiEnabledToggled;
        AiProviderCombo.SelectedIndex = Math.Max(0, Array.IndexOf(AiProviders.All, config.Provider));
        AiEndpointBox.Text = config.Endpoint;
        AiModelBox.Text = config.Model;
        AiKeyBox.Password = "";
        AiKeyBox.PlaceholderText = config.HasKey ? Strings.AiKeyPlaceholderSaved : Strings.AiKeyPlaceholderEmpty;
        AiKeyClearButton.IsEnabled = config.HasKey;
        AiModeCombo.SelectedIndex = config.Mode == AiModes.Always ? 1 : 0;
        AiShareToggle.IsOn = config.ShareResults;
        AiFileHintText.Text = Strings.AiFileHint(config.FilePath);
        AiEnabledRow.IsEnabled = !config.Locked;
        foreach (Control c in new Control[] { AiProviderCombo, AiEndpointBox, AiModelBox, AiKeyBox, AiKeyClearButton, AiModeCombo, AiShareToggle, AiSaveButton })
        {
            c.IsEnabled = !config.Locked && (c != AiKeyClearButton || config.HasKey);
        }

        SearchPrivacyRow.Title = config.Enabled ? Strings.SettingsSearchPrivacyAi : Strings.SettingsSearchPrivacy;
        if (config.Error is { Length: > 0 } error)
        {
            ShowAi(InfoBarSeverity.Error, Strings.AiFileError(error));
        }
        else if (config.Locked)
        {
            ShowAi(InfoBarSeverity.Informational, Strings.AiLocked);
        }
    }

    private void ShowAi(InfoBarSeverity severity, string message)
    {
        AiBar.Severity = severity;
        AiBar.Message = message;
        AiBar.IsOpen = true;
    }

    private AiConfigPatch FormPatch()
    {
        var patch = new AiConfigPatch()
            .Enabled(AiEnabledToggle.IsOn)
            .Provider((AiProviderCombo.SelectedItem as ComboBoxItem)?.Tag as string ?? AiProviders.OpenAi)
            .Endpoint(AiEndpointBox.Text)
            .Model(AiModelBox.Text)
            .Mode((AiModeCombo.SelectedItem as ComboBoxItem)?.Tag as string ?? AiModes.Fallback)
            .ShareResults(AiShareToggle.IsOn);
        if (AiKeyBox.Password.Trim().Length > 0)
        {
            patch.ApiKey(AiKeyBox.Password);
        }
        else if (_aiClearKey)
        {
            patch.ApiKey("");
        }

        return patch;
    }

    /// <summary>Saves <paramref name="patch"/>; true when the island took it.</summary>
    private async Task<bool> SaveAiAsync(AiConfigPatch patch, bool announce)
    {
        if (_ai is { Locked: true })
        {
            return true;
        }

        AiBar.IsOpen = false;
        try
        {
            RenderAi(await _model.Client.AiSetConfigAsync(patch));
            if (announce)
            {
                ShowAi(InfoBarSeverity.Success, Strings.AiSaved);
            }

            return true;
        }
        catch (Exception ex)
        {
            _model.Report("aiSetConfig", ex);
            ShowAi(InfoBarSeverity.Error, ex is IslandException { Message.Length: > 0 } ie ? ie.Message : Strings.AiSaveFailed);
            return false;
        }
    }

    private async void OnAiEnabledToggled(object sender, RoutedEventArgs e)
    {
        // like every other switch on this page, it applies at once
        await SaveAiAsync(new AiConfigPatch().Enabled(AiEnabledToggle.IsOn), announce: false);
    }

    private async void OnAiSaveClick(object sender, RoutedEventArgs e)
    {
        await SaveAiAsync(FormPatch(), announce: true);
    }

    private void OnAiKeyClearClick(object sender, RoutedEventArgs e)
    {
        _aiClearKey = true;
        AiKeyBox.Password = "";
        AiKeyBox.PlaceholderText = Strings.AiKeyPlaceholderEmpty;
        AiKeyClearButton.IsEnabled = false;
    }

    private async void OnAiTestClick(object sender, RoutedEventArgs e)
    {
        // the test uses what is on the form: save it first
        if (!await SaveAiAsync(FormPatch(), announce: false))
        {
            return;
        }

        AiTestButton.IsEnabled = false;
        AiProgress.IsActive = true;
        ShowAi(InfoBarSeverity.Informational, Strings.AiTesting);
        try
        {
            AiTestResult result = await _model.Client.AiTestAsync();
            string where = result.Url.Length > 0 ? Environment.NewLine + result.Url : "";
            ShowAi(result.Ok ? InfoBarSeverity.Success : InfoBarSeverity.Error,
                (result.Ok ? Strings.AiTestOk(result.Ms, result.Message) : Strings.AiTestFailed(result.Message)) + where);
        }
        catch (Exception ex)
        {
            _model.Report("aiTest", ex);
            ShowAi(InfoBarSeverity.Error, Strings.AiTestFailed(ex.Message));
        }
        finally
        {
            AiTestButton.IsEnabled = true;
            AiProgress.IsActive = false;
        }
    }

    private async void OnAiOpenFileClick(object sender, RoutedEventArgs e)
    {
        try
        {
            await _model.Client.AiOpenConfigFileAsync();
        }
        catch (Exception ex)
        {
            _model.Report("aiOpenConfigFile", ex);
            ShowAi(InfoBarSeverity.Error, ex.Message);
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
