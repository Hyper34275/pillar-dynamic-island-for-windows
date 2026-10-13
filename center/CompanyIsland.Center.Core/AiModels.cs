using System.Text.Json.Nodes;

namespace CompanyIsland.Center.Core;

/// <summary>
/// The AI connection's settings as the island sends them (<c>aiGetConfig</c>, docs/AI_API.md). The key itself never
/// reaches the Center: <see cref="HasKey"/> only says whether one is set. Plain setters (see <see cref="IslandSettings"/>).
/// </summary>
public sealed record AiConfig
{
    public bool Enabled { get; set; }
    public string Provider { get; set; } = AiProviders.OpenAi;
    public string Endpoint { get; set; } = "";
    public string Model { get; set; } = "";
    public string Mode { get; set; } = AiModes.Fallback;
    public bool ShareResults { get; set; } = true;
    public int TimeoutSeconds { get; set; } = 45;
    public bool HasKey { get; set; }
    /// <summary>An administrator's file is in force: shown, not editable.</summary>
    public bool Locked { get; set; }
    /// <summary>Where the user's <c>ai-config.json</c> is.</summary>
    public string FilePath { get; set; } = "";
    /// <summary>The files could not be read (code and reason), else null.</summary>
    public string? Error { get; set; }
}

/// <summary>The answer of <c>aiTest</c>.</summary>
public sealed record AiTestResult
{
    public bool Ok { get; set; }
    public long Ms { get; set; }
    public string Message { get; set; } = "";
    public string Url { get; set; } = "";
}

public static class AiProviders
{
    public const string OpenAi = "openai";
    public const string Azure = "azure";
    public const string Anthropic = "anthropic";

    public static readonly string[] All = [OpenAi, Azure, Anthropic];

    public static string Normalize(string? value) => value is Azure or Anthropic ? value : OpenAi;
}

public static class AiModes
{
    public const string Fallback = "fallback";
    public const string Always = "always";

    public static string Normalize(string? value) => value == Always ? Always : Fallback;
}

/// <summary>Builds the <c>patch</c> argument of <c>aiSetConfig</c>. Only the fields that were set are sent.</summary>
public sealed class AiConfigPatch
{
    private readonly JsonObject _values = new();

    public bool IsEmpty => _values.Count == 0;

    public AiConfigPatch Enabled(bool value) => Put("enabled", JsonValue.Create(value));
    public AiConfigPatch Provider(string value) => Put("provider", JsonValue.Create(AiProviders.Normalize(value)));
    public AiConfigPatch Endpoint(string value) => Put("endpoint", JsonValue.Create(value.Trim()));
    public AiConfigPatch Model(string value) => Put("model", JsonValue.Create(value.Trim()));
    public AiConfigPatch Mode(string value) => Put("mode", JsonValue.Create(AiModes.Normalize(value)));
    public AiConfigPatch ShareResults(bool value) => Put("shareResults", JsonValue.Create(value));

    /// <summary>A new key (stored encrypted for this Windows user by the island); an empty string removes the key.</summary>
    public AiConfigPatch ApiKey(string value) => Put("apiKey", JsonValue.Create(value.Trim()));

    public JsonObject ToJson() => (JsonObject)_values.DeepClone();

    private AiConfigPatch Put(string name, JsonNode? value)
    {
        _values[name] = value;
        return this;
    }
}
