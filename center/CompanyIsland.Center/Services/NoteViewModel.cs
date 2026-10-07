using System.ComponentModel;
using System.Runtime.CompilerServices;
using CompanyIsland.Center.Core;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;

namespace CompanyIsland.Center.Services;

/// <summary>One note card: the note plus the card's own state (inline edit, copy feedback, highlight).</summary>
public sealed class NoteViewModel : INotifyPropertyChanged
{
    private static readonly Brush NormalBorder = new SolidColorBrush(Windows.UI.Color.FromArgb(0x1A, 0xFF, 0xFF, 0xFF));
    private static readonly Brush EditingBorder = new SolidColorBrush(Windows.UI.Color.FromArgb(0x59, 0xFF, 0xFF, 0xFF));
    private static readonly Brush HighlightBorder = new SolidColorBrush(Colors.White);
    private static Brush? _systemBorder;

    private Note _note;
    private string _timeText = "";
    private bool _isEditing;
    private string _editText = "";
    private bool _copied;
    private bool _highlighted;
    private int _maxLines = 6;

    public NoteViewModel(Note note, long nowMs)
    {
        _note = note;
        _timeText = Strings.RelativeTime(note.UpdatedAt, nowMs);
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    public Note Note => _note;

    public string Id => _note.Id;

    public string Text => _note.Text;

    public bool Pinned => _note.Pinned;

    public string TimeText
    {
        get => _timeText;
        private set => Set(ref _timeText, value);
    }

    public bool IsEditing
    {
        get => _isEditing;
        private set
        {
            if (Set(ref _isEditing, value))
            {
                Raise(nameof(IsViewing));
                Raise(nameof(CardBorder));
                Raise(nameof(CanSaveEdit));
            }
        }
    }

    public bool IsViewing => !_isEditing;

    public string EditText
    {
        get => _editText;
        set
        {
            if (Set(ref _editText, value))
            {
                Raise(nameof(CanSaveEdit));
            }
        }
    }

    /// <summary>Save is available for a non-empty edit.</summary>
    public bool CanSaveEdit => _isEditing && !string.IsNullOrWhiteSpace(_editText);

    public int MaxLines
    {
        get => _maxLines;
        set => Set(ref _maxLines, value);
    }

    public string PinGlyph => Pinned ? "" : "";

    public string PinTip => Pinned ? Strings.UnpinNote : Strings.PinNote;

    public string CopyGlyph => _copied ? "" : "";

    public string CopyTip => _copied ? Strings.CopiedNote : Strings.CopyNote;

    public Brush CardBorder => HighContrastBorder() ?? (_highlighted ? HighlightBorder : _isEditing ? EditingBorder : NormalBorder);

    /// <summary>In a high-contrast theme every card border is the system's text colour (white-on-white would vanish); otherwise null.</summary>
    private static Brush? HighContrastBorder()
    {
        try
        {
            if (!new Windows.UI.ViewManagement.AccessibilitySettings().HighContrast)
            {
                return null;
            }

            Windows.UI.Color text = new Windows.UI.ViewManagement.UISettings().GetColorValue(Windows.UI.ViewManagement.UIColorType.Foreground);
            if (_systemBorder is not SolidColorBrush { } cached || cached.Color != text)
            {
                _systemBorder = new SolidColorBrush(text);
            }

            return _systemBorder;
        }
        catch
        {
            return null;
        }
    }

    /// <summary>A new version of the same note arrived. A card being edited keeps what the user is typing.</summary>
    public void Update(Note note, long nowMs)
    {
        if (note == _note)
        {
            TimeText = Strings.RelativeTime(note.UpdatedAt, nowMs);
            return;
        }

        bool pinChanged = note.Pinned != _note.Pinned;
        _note = note;
        Raise(nameof(Note));
        Raise(nameof(Text));
        if (pinChanged)
        {
            Raise(nameof(Pinned));
            Raise(nameof(PinGlyph));
            Raise(nameof(PinTip));
        }

        TimeText = Strings.RelativeTime(note.UpdatedAt, nowMs);
    }

    public void RefreshTime(long nowMs) => TimeText = Strings.RelativeTime(_note.UpdatedAt, nowMs);

    /// <summary>Starts editing. A card that is already being edited keeps what the user typed (a second "open this note" must not wipe it).</summary>
    public void BeginEdit()
    {
        if (_isEditing)
        {
            return;
        }

        EditText = _note.Text;
        IsEditing = true;
    }

    public void EndEdit() => IsEditing = false;

    /// <summary>Shows a check mark and "הועתק" for a moment.</summary>
    public async void FlashCopied()
    {
        _copied = true;
        Raise(nameof(CopyGlyph));
        Raise(nameof(CopyTip));
        await Task.Delay(1500);
        _copied = false;
        Raise(nameof(CopyGlyph));
        Raise(nameof(CopyTip));
    }

    /// <summary>A bright border for a moment: "this is the note you asked for".</summary>
    public async void FlashHighlight()
    {
        _highlighted = true;
        Raise(nameof(CardBorder));
        await Task.Delay(1600);
        _highlighted = false;
        Raise(nameof(CardBorder));
    }

    private bool Set<T>(ref T field, T value, [CallerMemberName] string? name = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value))
        {
            return false;
        }

        field = value;
        Raise(name);
        return true;
    }

    private void Raise(string? name) => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}
