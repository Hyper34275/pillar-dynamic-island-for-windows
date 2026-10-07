using System.Collections.ObjectModel;
using CompanyIsland.Center.Core;
using CompanyIsland.Center.Services;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.ApplicationModel.DataTransfer;
using VirtualKey = Windows.System.VirtualKey;

namespace CompanyIsland.Center.Pages;

/// <summary>
/// Notes: an editor, search, a list or grid of cards (pinned first, then newest) with pin / copy / edit-in-place / delete.
/// All changes go through <see cref="CenterModel.ChangeNotesAsync"/>: optimistic, reverted (with an error bar) when the
/// island refuses. The list/grid choice is kept in memory only (an unpackaged app has no ApplicationData).
/// </summary>
public sealed partial class NotesPage : Page, ICenterPage
{
    private const int ListMaxLines = 6;
    private const int GridMaxLines = 5;

    private readonly CenterModel _model;
    private readonly Dictionary<string, NoteViewModel> _viewModels = [];
    private readonly DispatcherTimer _clock = new() { Interval = TimeSpan.FromSeconds(30) };
    private string _query = "";
    private bool _grid;
    private bool _connected;
    private bool _visible;
    private string? _pendingNoteId;
    private bool _focusEditorWhenShown;

    public NotesPage(CenterModel model)
    {
        _model = model;
        InitializeComponent();
        EditorMessage.Text = Strings.NoteEditorHint;
        _model.NotesChanged += OnNotesChanged;
        _clock.Tick += (_, _) => RefreshTimes();
        Render();
    }

    /// <summary>The cards on screen, in order. Edited in place so a card being edited keeps its state.</summary>
    public ObservableCollection<NoteViewModel> Items { get; } = [];

    public void Show(CenterPage page)
    {
        _visible = true;
        _clock.Start();
        RefreshTimes();
        switch (page.Kind)
        {
            case CenterPageKind.NotesNew:
                _focusEditorWhenShown = true;
                FocusEditorSoon();
                break;
            case CenterPageKind.Note:
                _pendingNoteId = page.NoteId;
                TryOpenPendingNote();
                break;
        }
    }

    public void Hide()
    {
        _visible = false;
        _clock.Stop();
    }

    public void OnConnectionChanged(bool connected)
    {
        _connected = connected;
        UpdateInteractive();
        if (connected)
        {
            ErrorBar.IsOpen = false; // an old failure says nothing about the new connection
        }

        Render();
    }

    /// <summary>
    /// The editor, the cards and their buttons work only while connected and once the island's notes were received:
    /// a save replaces the island's whole list, so saving before the load would delete the notes that were never shown.
    /// </summary>
    private void UpdateInteractive() => Interactive.IsEnabled = _connected && _model.NotesLoaded;

    private void OnNotesChanged()
    {
        UpdateInteractive();
        Render();
        TryOpenPendingNote();
        if (_focusEditorWhenShown && Interactive.IsEnabled)
        {
            FocusEditorSoon();
        }
    }

    // ---- Rendering ------------------------------------------------------------------------------------------

    /// <summary>Brings the cards in line with the model and the search without recreating the ones that stay.</summary>
    private void Render()
    {
        long now = NoteOps.NowMs();
        IReadOnlyList<Note> all = _model.Notes;
        HashSet<string> ids = all.Select(n => n.Id).ToHashSet(StringComparer.Ordinal);

        foreach (string gone in _viewModels.Keys.Where(id => !ids.Contains(id)).ToList())
        {
            _viewModels.Remove(gone);
        }

        foreach (Note note in all)
        {
            if (_viewModels.TryGetValue(note.Id, out NoteViewModel? existing))
            {
                existing.Update(note, now);
            }
            else
            {
                _viewModels[note.Id] = new NoteViewModel(note, now) { MaxLines = _grid ? GridMaxLines : ListMaxLines };
            }
        }

        // The search filters by text; a card being edited stays whatever the search says.
        HashSet<string> matching = NoteOps.Search(all, _query).Select(n => n.Id).ToHashSet(StringComparer.Ordinal);
        List<NoteViewModel> target = all
            .Where(n => matching.Contains(n.Id) || _viewModels[n.Id].IsEditing)
            .Select(n => _viewModels[n.Id])
            .ToList();

        Reconcile(target);

        bool searching = !string.IsNullOrWhiteSpace(_query);
        bool empty = target.Count == 0 && (_model.NotesLoaded || searching);
        EmptyState.Visibility = empty ? Visibility.Visible : Visibility.Collapsed;
        if (empty)
        {
            bool noNotesAtAll = all.Count == 0;
            EmptyIcon.Glyph = noNotesAtAll ? "" : "";
            EmptyTitle.Text = noNotesAtAll ? Strings.NotesEmptyTitle : Strings.NotesNoResults;
            EmptyBody.Text = noNotesAtAll ? Strings.NotesEmptyBody : Strings.NotesNoResultsBody;
        }
    }

    private void Reconcile(List<NoteViewModel> target)
    {
        HashSet<NoteViewModel> wanted = [.. target];
        for (int i = Items.Count - 1; i >= 0; i--)
        {
            if (!wanted.Contains(Items[i]))
            {
                Items.RemoveAt(i);
            }
        }

        for (int i = 0; i < target.Count; i++)
        {
            if (i < Items.Count && ReferenceEquals(Items[i], target[i]))
            {
                continue;
            }

            int current = Items.IndexOf(target[i]);
            if (current >= 0)
            {
                Items.Move(current, i);
            }
            else
            {
                Items.Insert(i, target[i]);
            }
        }
    }

    private void RefreshTimes()
    {
        long now = NoteOps.NowMs();
        foreach (NoteViewModel vm in _viewModels.Values)
        {
            vm.RefreshTime(now);
        }
    }

    // ---- Header: search and view ----------------------------------------------------------------------------

    private void OnSearchTextChanged(AutoSuggestBox sender, AutoSuggestBoxTextChangedEventArgs args)
    {
        _query = sender.Text ?? "";
        Render();
    }

    private void OnListToggleClick(object sender, RoutedEventArgs e) => SetGrid(false);

    private void OnGridToggleClick(object sender, RoutedEventArgs e) => SetGrid(true);

    private void SetGrid(bool grid)
    {
        _grid = grid;
        ListToggle.IsChecked = !grid;
        GridToggle.IsChecked = grid;
        NotesRepeater.Layout = grid
            ? new UniformGridLayout
            {
                MinItemWidth = 280,
                MinItemHeight = 176,
                MinRowSpacing = 12,
                MinColumnSpacing = 12,
                ItemsStretch = UniformGridLayoutItemsStretch.Fill,
            }
            : new StackLayout { Spacing = 12 };
        foreach (NoteViewModel vm in _viewModels.Values)
        {
            vm.MaxLines = grid ? GridMaxLines : ListMaxLines;
        }
    }

    // ---- New note -------------------------------------------------------------------------------------------

    private void OnEditorTextChanged(object sender, TextChangedEventArgs e)
    {
        string text = Editor.Text;
        SaveButton.IsEnabled = !string.IsNullOrWhiteSpace(text);
        int count = NoteOps.CountChars(text);
        bool near = count >= NoteOps.MaxTextChars - 1000;
        EditorCounter.Visibility = near ? Visibility.Visible : Visibility.Collapsed;
        EditorCounter.Text = Strings.CharCount(count, NoteOps.MaxTextChars);
    }

    private void OnEditorKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == VirtualKey.Enter && IsCtrlDown())
        {
            e.Handled = true;
            _ = SaveNewNoteAsync();
        }
    }

    private void OnSaveClick(object sender, RoutedEventArgs e) => _ = SaveNewNoteAsync();

    private async Task SaveNewNoteAsync()
    {
        string text = Editor.Text;
        NoteTextProblem problem = NoteOps.CheckText(text);
        if (problem == NoteTextProblem.Empty)
        {
            return;
        }

        if (problem == NoteTextProblem.TooLong)
        {
            ShowError(Strings.NoteTooLong(NoteOps.MaxTextChars));
            return;
        }

        bool limit = false;
        Note note = NoteOps.Create(text, NoteOps.NowMs());
        SaveButton.IsEnabled = false;
        bool ok = await _model.ChangeNotesAsync(list =>
        {
            if (list.Count >= NoteOps.MaxNotes)
            {
                limit = true;
                return null;
            }

            list.Add(note);
            return list;
        });

        if (limit)
        {
            ShowError(Strings.NoteLimit(NoteOps.MaxNotes));
            SaveButton.IsEnabled = true;
        }
        else if (ok)
        {
            ErrorBar.IsOpen = false;
            Editor.Text = "";
            Editor.Focus(FocusState.Programmatic);
        }
        else
        {
            ShowError(Strings.NoteSaveFailed);
            SaveButton.IsEnabled = !string.IsNullOrWhiteSpace(Editor.Text);
        }
    }

    private void FocusEditorSoon()
    {
        // The page becomes visible in the same turn; focus once layout has run.
        DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, () =>
        {
            // While the notes are still loading the editor is disabled and cannot take focus; OnNotesChanged asks again.
            if (_focusEditorWhenShown && _visible && Interactive.IsEnabled)
            {
                _focusEditorWhenShown = false;
                Scroller.ChangeView(null, 0, null, disableAnimation: true);
                Editor.Focus(FocusState.Programmatic);
            }
        });
    }

    // ---- Cards ----------------------------------------------------------------------------------------------

    private static NoteViewModel? ViewModelOf(object sender) => (sender as FrameworkElement)?.Tag as NoteViewModel;

    private void OnNoteDoubleTapped(object sender, DoubleTappedRoutedEventArgs e)
    {
        if (ViewModelOf(sender) is { } vm && _connected && _model.NotesLoaded)
        {
            BeginEdit(vm);
        }
    }

    private void OnEditClick(object sender, RoutedEventArgs e)
    {
        if (ViewModelOf(sender) is { } vm)
        {
            BeginEdit(vm);
        }
    }

    private void BeginEdit(NoteViewModel vm)
    {
        vm.BeginEdit();
        FocusCardEditor(vm);
    }

    /// <summary>Puts the cursor at the end of the card's text box once the card has switched to editing (and exists on screen).</summary>
    private void FocusCardEditor(NoteViewModel vm)
    {
        DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, () =>
        {
            int index = Items.IndexOf(vm);
            if (index >= 0 && vm.IsEditing && NotesRepeater.TryGetElement(index) is { } card && FindChild<TextBox>(card) is { } box)
            {
                box.Focus(FocusState.Programmatic);
                box.Select(box.Text.Length, 0);
            }
        });
    }

    private void OnCardEditorKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (ViewModelOf(sender) is not { } vm)
        {
            return;
        }

        if (e.Key == VirtualKey.Enter && IsCtrlDown())
        {
            e.Handled = true;
            _ = SaveEditAsync(vm);
        }
        else if (e.Key == VirtualKey.Escape)
        {
            e.Handled = true;
            vm.EndEdit();
            Render();
        }
    }

    private void OnSaveEditClick(object sender, RoutedEventArgs e)
    {
        if (ViewModelOf(sender) is { } vm)
        {
            _ = SaveEditAsync(vm);
        }
    }

    private void OnCancelEditClick(object sender, RoutedEventArgs e)
    {
        if (ViewModelOf(sender) is { } vm)
        {
            vm.EndEdit();
            Render();
        }
    }

    private async Task SaveEditAsync(NoteViewModel vm)
    {
        string text = vm.EditText;
        NoteTextProblem problem = NoteOps.CheckText(text);
        if (problem == NoteTextProblem.Empty)
        {
            return;
        }

        if (problem == NoteTextProblem.TooLong)
        {
            ShowError(Strings.NoteTooLong(NoteOps.MaxTextChars));
            return;
        }

        if (text == vm.Text)
        {
            vm.EndEdit();
            Render();
            return;
        }

        string id = vm.Id;
        long now = NoteOps.NowMs();
        bool ok = await _model.ChangeNotesAsync(list =>
        {
            int index = list.FindIndex(n => n.Id == id);
            if (index < 0)
            {
                return null; // deleted meanwhile
            }

            list[index] = list[index] with { Text = text, UpdatedAt = Math.Max(now, list[index].CreatedAt) };
            return list;
        });

        if (ok)
        {
            ErrorBar.IsOpen = false;
            vm.EndEdit();
            Render();
        }
        else
        {
            ShowError(Strings.NoteSaveFailed); // the card stays in edit mode with the text intact
        }
    }

    private async void OnPinClick(object sender, RoutedEventArgs e)
    {
        if (ViewModelOf(sender) is not { } vm)
        {
            return;
        }

        string id = vm.Id;
        bool pin = !vm.Pinned;
        bool ok = await _model.ChangeNotesAsync(list =>
        {
            int index = list.FindIndex(n => n.Id == id);
            if (index < 0)
            {
                return null;
            }

            list[index] = list[index] with { Pinned = pin };
            return list;
        });

        if (!ok)
        {
            ShowError(Strings.NoteSaveFailed);
        }
    }

    private void OnCopyClick(object sender, RoutedEventArgs e)
    {
        if (ViewModelOf(sender) is not { } vm)
        {
            return;
        }

        try
        {
            var package = new DataPackage();
            package.SetText(vm.Text);
            Clipboard.SetContent(package);
            Clipboard.Flush();
            vm.FlashCopied();
        }
        catch (Exception)
        {
            ShowError(Strings.CopyFailed);
        }
    }

    private void OnDeleteClick(object sender, RoutedEventArgs e)
    {
        if (ViewModelOf(sender) is not { } vm || sender is not FrameworkElement anchor)
        {
            return;
        }

        var flyout = new Flyout { FlyoutPresenterStyle = (Style)Resources["CenterFlyoutStyle"] };
        var confirm = new Button
        {
            Content = Strings.DeleteNote,
            Style = (Style)Application.Current.Resources["AccentButtonStyle"],
            HorizontalAlignment = HorizontalAlignment.Left,
            MinWidth = 88,
        };
        confirm.Click += async (_, _) =>
        {
            flyout.Hide();
            await DeleteAsync(vm);
        };
        flyout.Content = new StackPanel
        {
            Spacing = 12,
            MinWidth = 240,
            FlowDirection = FlowDirection.RightToLeft,
            Children =
            {
                new StackPanel
                {
                    Spacing = 4,
                    Children =
                    {
                        new TextBlock { Text = Strings.DeleteConfirmTitle, Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"] },
                        new TextBlock
                        {
                            Text = Strings.DeleteConfirmBody,
                            Style = (Style)Application.Current.Resources["CaptionTextBlockStyle"],
                            Foreground = (Brush)Application.Current.Resources["TextFillColorSecondaryBrush"],
                            TextWrapping = TextWrapping.Wrap,
                        },
                    },
                },
                confirm,
            },
        };
        flyout.ShowAt(anchor);
    }

    private async Task DeleteAsync(NoteViewModel vm)
    {
        string id = vm.Id;
        bool ok = await _model.ChangeNotesAsync(list => list.RemoveAll(n => n.Id == id) > 0 ? list : null);
        if (!ok)
        {
            ShowError(Strings.NoteDeleteFailed);
        }
    }

    // ---- note:<id> ------------------------------------------------------------------------------------------

    /// <summary>Opens the requested note for editing and scrolls to it, as soon as the notes are known.</summary>
    private void TryOpenPendingNote()
    {
        if (_pendingNoteId is not { } id || !_visible || !_model.NotesLoaded)
        {
            return;
        }

        _pendingNoteId = null;
        if (!_viewModels.TryGetValue(id, out NoteViewModel? vm))
        {
            ShowError(Strings.NoteNotFound);
            return;
        }

        if (!string.IsNullOrEmpty(_query))
        {
            SearchBox.Text = ""; // the note must be on screen
        }

        Render();
        if (_connected)
        {
            BeginEdit(vm); // no-op when the card is already being edited: the typed text stays
        }

        vm.FlashHighlight();
        DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, () =>
        {
            int index = Items.IndexOf(vm);
            if (index < 0)
            {
                return;
            }

            FrameworkElement card = (FrameworkElement)NotesRepeater.GetOrCreateElement(index);
            card.UpdateLayout();
            card.StartBringIntoView(new BringIntoViewOptions { AnimationDesired = true, VerticalAlignmentRatio = 0.3 });
            if (_connected)
            {
                FocusCardEditor(vm); // the card may only have been realised by the scroll
            }
        });
    }

#if DEBUG
    /// <summary>Debug builds only: puts the page in a state worth a screenshot.</summary>
    public void DebugCommand(string command)
    {
        switch (command)
        {
            case "grid": SetGrid(true); break;
            case "edit": if (Items.Count > 0) BeginEdit(Items[0]); break;
            case "search": SearchBox.Text = "שלום"; break;
            case "draft": Editor.Text = "טיוטה של פתק חדש שעוד לא נשמר"; break;
        }
    }
#endif

    // ---- helpers --------------------------------------------------------------------------------------------

    private void ShowError(string message)
    {
        ErrorBar.Message = message;
        ErrorBar.IsOpen = true;
    }

    private static bool IsCtrlDown() =>
        InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control).HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);

    private static T? FindChild<T>(DependencyObject root) where T : DependencyObject
    {
        int count = VisualTreeHelper.GetChildrenCount(root);
        for (int i = 0; i < count; i++)
        {
            DependencyObject child = VisualTreeHelper.GetChild(root, i);
            if (child is T match)
            {
                return match;
            }

            if (FindChild<T>(child) is { } nested)
            {
                return nested;
            }
        }

        return null;
    }
}
