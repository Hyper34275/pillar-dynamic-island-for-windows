using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Markup;

namespace CompanyIsland.Center.Controls;

/// <summary>A card for one setting. Whatever is placed inside the tag is the control shown at the end of the row.</summary>
[ContentProperty(Name = nameof(ActionContent))]
public sealed partial class SettingRow : UserControl
{
    public static readonly DependencyProperty TitleProperty = DependencyProperty.Register(
        nameof(Title), typeof(string), typeof(SettingRow), new PropertyMetadata("", (d, _) => ((SettingRow)d).Refresh()));

    public static readonly DependencyProperty DescriptionProperty = DependencyProperty.Register(
        nameof(Description), typeof(string), typeof(SettingRow), new PropertyMetadata("", (d, _) => ((SettingRow)d).Refresh()));

    public static readonly DependencyProperty GlyphProperty = DependencyProperty.Register(
        nameof(Glyph), typeof(string), typeof(SettingRow), new PropertyMetadata("", (d, _) => ((SettingRow)d).Refresh()));

    public static readonly DependencyProperty ActionContentProperty = DependencyProperty.Register(
        nameof(ActionContent), typeof(object), typeof(SettingRow), new PropertyMetadata(null, (d, _) => ((SettingRow)d).Refresh()));

    public SettingRow()
    {
        InitializeComponent();
        IsEnabledChanged += (_, _) => Card.Opacity = IsEnabled ? 1 : 0.5;
        Refresh();
    }

    public string Title
    {
        get => (string)GetValue(TitleProperty);
        set => SetValue(TitleProperty, value);
    }

    public string Description
    {
        get => (string)GetValue(DescriptionProperty);
        set => SetValue(DescriptionProperty, value);
    }

    /// <summary>A Segoe Fluent Icons / Segoe MDL2 Assets code point (both fonts share these glyphs).</summary>
    public string Glyph
    {
        get => (string)GetValue(GlyphProperty);
        set => SetValue(GlyphProperty, value);
    }

    public object? ActionContent
    {
        get => GetValue(ActionContentProperty);
        set => SetValue(ActionContentProperty, value);
    }

    private void Refresh()
    {
        if (TitleText is null)
        {
            return; // a property was set before InitializeComponent ran
        }

        TitleText.Text = Title;
        DescriptionText.Text = Description;
        DescriptionText.Visibility = string.IsNullOrEmpty(Description) ? Visibility.Collapsed : Visibility.Visible;
        IconHost.Glyph = Glyph;
        IconHost.Visibility = string.IsNullOrEmpty(Glyph) ? Visibility.Collapsed : Visibility.Visible;
        ActionHost.Content = ActionContent;
    }
}
