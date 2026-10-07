// The dev server's handling of tour.html's meta CSP, read through the project's own vite config
// (resolved against the directory the tests run from, the repository root).
import { loadConfigFromFile, type Plugin } from "vite";
import { describe, expect, it } from "vitest";
import tourHtml from "../../tour.html?raw";

describe("the dev server and tour.html", () => {
  it("drops the meta CSP only when serving (the inline refresh script would be blocked), the build keeps it", async () => {
    const loaded = await loadConfigFromFile({ command: "serve", mode: "development" }, "vite.config.ts");
    const plugin = (loaded?.config.plugins ?? []).flat().find((p): p is Plugin => !!p && (p as Plugin).name === "tour-dev-csp");
    expect(plugin).toBeDefined();
    expect(plugin!.apply).toBe("serve");

    expect(tourHtml).toContain("Content-Security-Policy");
    const hook = plugin!.transformIndexHtml as { order: string; handler: (html: string) => string };
    const served = hook.handler(tourHtml);
    expect(served).not.toContain("Content-Security-Policy");
    expect(served).toContain("/src/tour/main.tsx");
    expect(served).toContain("<title>");
  });
});
