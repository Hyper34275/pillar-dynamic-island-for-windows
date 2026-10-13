using System.Text.Json;
using CompanyIsland.Center.Core;

namespace CompanyIsland.Center.Tests;

public class AiConfigTests
{
    [Fact]
    public void The_island_view_is_read_without_a_key_and_normalised()
    {
        using JsonDocument doc = JsonDocument.Parse(
            """{"enabled":true,"provider":"something","endpoint":"http://ai:8000/v1","model":"llama","mode":"weird","shareResults":false,"timeoutSeconds":30,"hasKey":true,"locked":false,"filePath":"C:\\x\\ai-config.json","error":null,"headers":{}}""");
        AiConfig c = IslandClient.ReadAiConfig(doc.RootElement);
        Assert.True(c.Enabled && c.HasKey && !c.ShareResults);
        Assert.Equal(AiProviders.OpenAi, c.Provider);
        Assert.Equal(AiModes.Fallback, c.Mode);
        Assert.Equal("http://ai:8000/v1", c.Endpoint);
        Assert.Null(c.Error);
    }

    [Fact]
    public void A_patch_sends_only_what_was_set()
    {
        var patch = new AiConfigPatch().Enabled(true).Provider("anthropic").Endpoint("  https://gw/v1  ").Mode("always");
        string json = patch.ToJson().ToJsonString();
        Assert.Equal("""{"enabled":true,"provider":"anthropic","endpoint":"https://gw/v1","mode":"always"}""", json);
        Assert.True(new AiConfigPatch().IsEmpty);
        Assert.Equal("""{"apiKey":""}""", new AiConfigPatch().ApiKey("").ToJson().ToJsonString());
    }

    [Fact]
    public void A_test_result_is_read()
    {
        using JsonDocument doc = JsonDocument.Parse("""{"ok":false,"ms":120,"message":"AI-004: the server refused the key (HTTP 401)","url":"https://gw/v1/messages"}""");
        AiTestResult r = doc.RootElement.Deserialize(CenterJson.Default.AiTestResult)!;
        Assert.False(r.Ok);
        Assert.StartsWith("AI-004", r.Message);
    }
}
