// The tour's only way out: messages to the Yuval Center's WebView2 (the C# side listens to
// WebMessageReceived). In a plain browser there is no host and every call is a no-op. The tour
// never talks to the island's backend: no Tauri IPC, no links, nothing that could open Outlook.

export type TourHostMessage = { type: "navigate"; page: "settings" | "welcome" | "notes" } | { type: "done" };

declare global {
  interface Window {
    chrome?: { webview?: { postMessage: (message: unknown) => void } };
  }
}

export function postToHost(message: TourHostMessage): void {
  try {
    window.chrome?.webview?.postMessage(message);
  } catch {
    // The host went away; the tour has nothing else to do about it.
  }
}
