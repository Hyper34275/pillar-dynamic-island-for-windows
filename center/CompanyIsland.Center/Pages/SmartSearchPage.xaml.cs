using CompanyIsland.Center.Core;
using CompanyIsland.Center.Services;
using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.System;
using Windows.UI;

namespace CompanyIsland.Center.Pages;

/// <summary>
/// The Smart Search chat. The conversation comes from the island (<c>searchHistory</c>, memory only there), is redrawn on
/// <c>search-ready</c>, and every action (ask, choose a mailbox, search longer, open a result) is a pipe command that
/// carries only the typed text or opaque ids. Nothing typed or found is logged or stored here.
/// </summary>
public sealed partial class SmartSearchPage : Page, ICenterPage
{
    private readonly CenterModel _model;
    private readonly Dictionary<string, FrameworkElement> _anchors = new();
    private IReadOnlyList<SearchResults> _history = [];
    private bool _connected;
    private bool _busy;
    private SearchPending? _pending;
    private bool _renderedInteractive;
    private string? _focusQuery;
    private int _reloadVersion;

    public SmartSearchPage(CenterModel model)
    {
        _model = model;
        InitializeComponent();
        _model.SearchReady += OnSearchReady;
        _model.SettingsChanged += OnSettingsChanged;
        BuildExamples();
        Render();
        UpdateInputState();
    }

    public void Show(CenterPage page)
    {
        _focusQuery = page.QueryId;
        ErrorBar.IsOpen = false;
        _ = ReloadAsync(scrollToFocus: true);
        Input.Focus(FocusState.Programmatic);
    }

    public void Hide()
    {
    }

    public void OnConnectionChanged(bool connected)
    {
        _connected = connected;
        UpdateInputState();
        RefreshButtons();
        if (connected)
        {
            ErrorBar.IsOpen = false;
            // A cold start on search:<id> asked for that turn before the pipe was up.
            _ = ReloadAsync(scrollToFocus: _focusQuery is not null);
        }
    }

    private void OnSettingsChanged()
    {
        UpdateInputState();
        RefreshButtons();
    }

    private bool SearchEnabled => _model.Settings?.AiSearchEnabled ?? true;

    /// <summary>
    /// The choice, extend and remember controls are drawn enabled or not; redraw them (keeping the scroll position) when
    /// whether the page can talk to the island changes, so a dead button never looks alive.
    /// </summary>
    private void RefreshButtons()
    {
        if (_renderedInteractive == (_connected && SearchEnabled))
        {
            return;
        }

        double offset = Scroller.VerticalOffset;
        Render();
        Conversation.UpdateLayout();
        Scroller.ChangeView(null, offset, null, disableAnimation: true);
    }

    private void UpdateInputState()
    {
        bool enabled = _connected && SearchEnabled;
        bool wasEnabled = Input.IsEnabled;
        Input.IsEnabled = enabled && !_busy;
        if (!wasEnabled && Input.IsEnabled && IsLoaded)
        {
            // A disabled box cannot take focus, so it comes back when the box does (first connect, end of a search).
            Input.Focus(FocusState.Programmatic);
        }

        SendButton.IsEnabled = enabled && !_busy && !string.IsNullOrWhiteSpace(Input.Text);
        OffBar.IsOpen = _connected && !SearchEnabled;
        ExamplesPanel.IsHitTestVisible = enabled && !_busy;
    }

    // ----- data -----

    private void OnSearchReady(string? queryId)
    {
        // Any origin: the search bar and the island ask questions too.
        _ = ReloadAsync(scrollToFocus: false, bottomQuery: queryId);
    }

    private async Task ReloadAsync(bool scrollToFocus, string? bottomQuery = null)
    {
        if (!_connected)
        {
            Render();
            return;
        }

        int version = ++_reloadVersion;
        try
        {
            IReadOnlyList<SearchResults> history = await _model.Client.SearchHistoryAsync();
            if (version != _reloadVersion)
            {
                return;
            }

            _history = history;
        }
        catch (Exception ex)
        {
            _model.Report("searchHistory", ex);
            if (version == _reloadVersion)
            {
                ShowError(Strings.SearchHistoryFailed);
            }

            return;
        }

        Render();
        string? focus = scrollToFocus ? _focusQuery : null;
        if (scrollToFocus)
        {
            _focusQuery = null; // once: a later reload or reconnect must not jump back to it
        }

        if (SearchFormat.IndexOfQuery(_history, focus) >= 0)
        {
            ScrollToQuery(focus!);
        }
        else
        {
            ScrollToEnd();
        }
    }

    private void ShowError(string message)
    {
        ErrorBar.Message = message;
        ErrorBar.IsOpen = true;
    }

    /// <summary>The user-facing text for a failed search command, by the island's error code only.</summary>
    private static string ErrorText(Exception ex, string fallback) => ex is IslandException { Code: "APP-041" }
        ? Strings.SearchExpired
        : ex is IslandException { Code: "APP-040" }
            ? Strings.SearchOffTitle
            : fallback;

    // ----- actions -----

    private async Task AskAsync(SearchPendingKind kind, string text, string? queryId, Func<Task> run, string op)
    {
        if (_busy)
        {
            return;
        }

        if (!_connected)
        {
            ShowError(Strings.SearchFailed);
            return;
        }

        _busy = true;
        _pending = new SearchPending(kind, text, queryId, SearchFormat.LastQueryId(_history));
        if (kind == SearchPendingKind.Submit)
        {
            _focusQuery = null; // the user moves on; the conversation goes to the end
        }

        ErrorBar.IsOpen = false;
        UpdateInputState();
        Render();
        ScrollToEnd();
        try
        {
            await run();
        }
        catch (Exception ex)
        {
            _model.Report(op, ex);
            ShowError(ErrorText(ex, Strings.SearchFailed));
        }
        finally
        {
            _busy = false;
            _pending = null;
            UpdateInputState();
        }

        await ReloadAsync(scrollToFocus: false);
    }

    private Task SubmitAsync(string text)
    {
        text = text.Trim();
        if (text.Length == 0)
        {
            return Task.CompletedTask;
        }

        Input.Text = "";
        return AskAsync(SearchPendingKind.Submit, text, null, () => _model.Client.SearchSubmitAsync(text), "searchSubmit");
    }

    private Task ChooseAsync(AssistantCard card, Choice choice, bool remember) =>
        AskAsync(SearchPendingKind.Choose, card.Query, card.QueryId,
            () => _model.Client.SearchChooseAsync(card.QueryId, choice.Id, remember), "searchChoose");

    private Task ExtendAsync(AssistantCard card) =>
        AskAsync(SearchPendingKind.Extend, card.Query, card.QueryId, () => _model.Client.SearchExtendAsync(card.QueryId), "searchExtend");

    private async Task OpenAsync(AssistantCard card, AssistantItem item)
    {
        try
        {
            await _model.Client.SearchOpenAsync(card.QueryId, item.Id);
        }
        catch (Exception ex)
        {
            _model.Report("searchOpen", ex);
            ShowError(ex is IslandException { Code: "APP-041" } ? Strings.SearchExpired : Strings.SearchOpenFailed);
        }
    }

    private void OnSendClick(object sender, RoutedEventArgs e) => _ = SubmitAsync(Input.Text);

    private void OnInputChanged(object sender, TextChangedEventArgs e) => UpdateInputState();

    private void OnInputKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == VirtualKey.Enter && SendButton.IsEnabled)
        {
            e.Handled = true;
            _ = SubmitAsync(Input.Text);
        }
    }

    // ----- drawing -----

    private void BuildExamples()
    {
        foreach (string example in SearchFormat.ExampleQuestions)
        {
            var button = new Button
            {
                Content = new TextBlock { Text = example, TextWrapping = TextWrapping.Wrap },
                HorizontalAlignment = HorizontalAlignment.Stretch,
                HorizontalContentAlignment = HorizontalAlignment.Left,
            };
            AutomationProperties.SetName(button, example);
            button.Click += (_, _) => _ = SubmitAsync(example);
            ExamplesPanel.Children.Add(button);
        }
    }

    private T Res<T>(string key) where T : class => (T)Resources[key];

    private void Render()
    {
        _anchors.Clear();
        Conversation.Children.Clear();
        long now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        _renderedInteractive = _connected && SearchEnabled;
        SearchPending? pending = _busy ? _pending : null;
        int pendingTurn = pending?.TurnIndex(_history) ?? -1;
        for (int i = 0; i < _history.Count; i++)
        {
            SearchResults results = _history[i];
            bool last = i == _history.Count - 1;
            var turn = new StackPanel { Spacing = 8 };
            turn.Children.Add(UserBubble(results.Card.Query));
            turn.Children.Add(AnswerCard(results, interactive: last && !_busy && _renderedInteractive, now));
            if (i == pendingTurn)
            {
                // Choosing or searching longer works on this turn: the spinner goes under it, no second bubble.
                turn.Children.Add(ProcessingCard());
            }

            Conversation.Children.Add(turn);
            if (results.Card.QueryId.Length > 0)
            {
                _anchors[results.Card.QueryId] = turn;
            }
        }

        // The question that is still being worked on (unless the history already has it).
        if (pending is not null && pending.NeedsOwnTurn(_history))
        {
            Conversation.Children.Add(UserBubble(pending.Text));
            Conversation.Children.Add(ProcessingCard());
        }

        EmptyState.Visibility = _history.Count == 0 && !_busy ? Visibility.Visible : Visibility.Collapsed;
    }

    private Border UserBubble(string query)
    {
        var text = new TextBlock { Text = query, TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true };
        AutomationProperties.SetName(text, Strings.SearchYouSaid + ": " + query);
        return new Border { Style = Res<Style>("UserBubbleStyle"), Child = text };
    }

    private Border ProcessingCard()
    {
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
        row.Children.Add(new ProgressRing { IsActive = true, Width = 20, Height = 20 });
        row.Children.Add(new TextBlock { Text = Strings.SearchWorking, VerticalAlignment = VerticalAlignment.Center });
        AutomationProperties.SetName(row, Strings.SearchWorking);
        return new Border { Style = Res<Style>("AnswerCardStyle"), Child = row };
    }

    private Border AnswerCard(SearchResults results, bool interactive, long now)
    {
        AssistantCard card = results.Card;
        var body = new StackPanel { Spacing = 10 };
        AutomationProperties.SetName(body, Strings.SearchAnswer + ": " + card.Title);

        if (card.Title.Length > 0)
        {
            body.Children.Add(new TextBlock
            {
                Text = card.Title,
                Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"],
                TextWrapping = TextWrapping.Wrap,
                IsTextSelectionEnabled = true,
            });
        }

        if (card.Summary.Length > 0)
        {
            body.Children.Add(new TextBlock { Text = card.Summary, Style = Res<Style>("SecondaryTextStyle"), IsTextSelectionEnabled = true });
        }

        if (SearchFormat.NeedsAnswer(card))
        {
            body.Children.Add(ChoicesView(card, interactive));
        }

        var groups = SearchFormat.EffectiveGroups(results);
        int shown = 0;
        foreach (ResultGroup group in groups)
        {
            body.Children.Add(GroupView(card, group, interactive, now));
            shown += group.Items.Count;
        }

        if (card.Phase != CardPhases.Choices && card.Phase != CardPhases.Processing && groups.Count == 0 && card.Phase != CardPhases.Error &&
            card.Total == 0 && card.Items.Count == 0)
        {
            body.Children.Add(new TextBlock { Text = Strings.SearchNoResults, Style = Res<Style>("SecondaryTextStyle") });
        }

        string more = SearchFormat.MoreInfo(card, shown);
        if (more.Length > 0)
        {
            body.Children.Add(new TextBlock { Text = more, Style = Res<Style>("SecondaryCaptionStyle") });
        }

        if (card.Partial)
        {
            body.Children.Add(new TextBlock { Text = Strings.SearchPartial, Style = Res<Style>("SecondaryCaptionStyle"), TextWrapping = TextWrapping.Wrap });
        }

        if (card.CanExtend)
        {
            var extend = new Button { Content = Strings.SearchExtend, IsEnabled = interactive, HorizontalAlignment = HorizontalAlignment.Left };
            AutomationProperties.SetName(extend, Strings.SearchExtend);
            extend.Click += (_, _) => _ = ExtendAsync(card);
            body.Children.Add(extend);
        }

        return new Border { Style = Res<Style>("AnswerCardStyle"), Child = body };
    }

    private UIElement ChoicesView(AssistantCard card, bool interactive)
    {
        var panel = new StackPanel { Spacing = 8 };
        if (!string.IsNullOrWhiteSpace(card.Question))
        {
            panel.Children.Add(new TextBlock { Text = card.Question, TextWrapping = TextWrapping.Wrap });
        }

        CheckBox? remember = null;
        if (card.Choices.Any(SearchFormat.IsMailboxChoice))
        {
            remember = new CheckBox { Content = Strings.SearchRemember, IsEnabled = interactive };
            AutomationProperties.SetName(remember, Strings.SearchRemember);
        }

        foreach (Choice choice in card.Choices)
        {
            var button = new Button
            {
                Content = new TextBlock { Text = choice.Label, TextWrapping = TextWrapping.Wrap },
                HorizontalAlignment = HorizontalAlignment.Stretch,
                HorizontalContentAlignment = HorizontalAlignment.Left,
                IsEnabled = interactive,
            };
            if (choice.Preferred)
            {
                button.Style = (Style)Application.Current.Resources["AccentButtonStyle"];
            }

            AutomationProperties.SetName(button, choice.Label);
            bool mailbox = SearchFormat.IsMailboxChoice(choice);
            button.Click += (_, _) => _ = ChooseAsync(card, choice, mailbox && remember?.IsChecked == true);
            panel.Children.Add(button);
        }

        if (remember is not null)
        {
            panel.Children.Add(remember);
        }

        return panel;
    }

    private UIElement GroupView(AssistantCard card, ResultGroup group, bool interactive, long now)
    {
        var panel = new StackPanel { Spacing = 6 };
        var header = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, Margin = new Thickness(0, 4, 0, 0) };
        header.Children.Add(new FontIcon { Glyph = SearchFormat.GroupGlyph(group.Kind), Style = Res<Style>("GroupGlyphStyle") });
        var title = new TextBlock
        {
            Text = SearchFormat.GroupTitle(group),
            Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"],
        };
        AutomationProperties.SetHeadingLevel(title, AutomationHeadingLevel.Level3);
        header.Children.Add(title);
        header.Children.Add(new TextBlock { Text = group.Items.Count.ToString(), Style = Res<Style>("SecondaryCaptionStyle"), VerticalAlignment = VerticalAlignment.Center });
        panel.Children.Add(header);

        if (group.ErrorCode is not null)
        {
            panel.Children.Add(new TextBlock { Text = Strings.SearchGroupFailed, Style = Res<Style>("SecondaryCaptionStyle") });
        }

        foreach (AssistantItem item in group.Items)
        {
            panel.Children.Add(ItemCard(card, group, item, interactive, now));
        }

        return panel;
    }

    private UIElement ItemCard(AssistantCard card, ResultGroup group, AssistantItem item, bool interactive, long now)
    {
        // The group header already says the mailbox, so a mail card does not repeat it.
        string detail = SearchFormat.ItemDetail(item, includeSource: group.Kind != "mail");
        string time = SearchFormat.ItemTime(item, now);

        var grid = new Grid { ColumnSpacing = 12 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        var bar = new Rectangle { Style = Res<Style>("AccentBarStyle") };
        if (!IsHighContrast() && SearchFormat.TryParseAccent(item.Accent, out byte r, out byte g, out byte b))
        {
            bar.Fill = new SolidColorBrush(Color.FromArgb(255, r, g, b));
        }

        grid.Children.Add(bar);

        var text = new StackPanel { Spacing = 2 };
        Grid.SetColumn(text, 1);
        var titleRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        if (item.Unread)
        {
            var dot = new Ellipse { Style = Res<Style>("UnreadDotStyle") };
            ToolTipService.SetToolTip(dot, Strings.SearchUnread);
            titleRow.Children.Add(dot);
        }

        titleRow.Children.Add(new TextBlock
        {
            Text = item.Title,
            TextTrimming = TextTrimming.CharacterEllipsis,
            FontWeight = item.Unread ? FontWeights.SemiBold : FontWeights.Normal,
        });
        text.Children.Add(titleRow);
        if (detail.Length > 0)
        {
            text.Children.Add(new TextBlock { Text = detail, Style = Res<Style>("SecondaryCaptionStyle") });
        }

        grid.Children.Add(text);

        var side = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10, VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(side, 2);
        if (time.Length > 0)
        {
            side.Children.Add(new TextBlock { Text = time, Style = Res<Style>("SecondaryCaptionStyle"), VerticalAlignment = VerticalAlignment.Center });
        }

        if (item.Openable)
        {
            var open = new Button { Content = Strings.SearchOpen, IsEnabled = _connected };
            AutomationProperties.SetName(open, Strings.SearchOpen + ": " + item.Title);
            open.Click += (_, _) => _ = OpenAsync(card, item);
            side.Children.Add(open);
        }

        grid.Children.Add(side);

        AutomationProperties.SetName(grid, string.Join(", ", new[] { item.Unread ? Strings.SearchUnread : "", item.Title, detail, time }.Where(p => p.Length > 0)));
        return new Border { Style = Res<Style>("ResultCardStyle"), Child = grid };
    }

    private static bool IsHighContrast()
    {
        try
        {
            return new Windows.UI.ViewManagement.AccessibilitySettings().HighContrast;
        }
        catch
        {
            return false;
        }
    }

    // ----- scrolling -----

    private void ScrollToEnd()
    {
        Conversation.UpdateLayout();
        Scroller.ChangeView(null, Scroller.ScrollableHeight, null, disableAnimation: true);
    }

    private void ScrollToQuery(string queryId)
    {
        if (!_anchors.TryGetValue(queryId, out FrameworkElement? anchor))
        {
            ScrollToEnd();
            return;
        }

        Conversation.UpdateLayout();
        anchor.StartBringIntoView(new BringIntoViewOptions { AnimationDesired = false, VerticalAlignmentRatio = 0 });
    }
}
