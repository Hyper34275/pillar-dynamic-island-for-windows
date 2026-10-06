import React, { useEffect } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";
import { CrashBoundary } from "./components/CrashBoundary";
import { useCrashRecovery } from "./hooks/useCrashRecovery";
import { APP_VERSION } from "./lib/appInfo";
import { dlog, installDebugLogging } from "./lib/debugLog";
import { describeError, stackFrames } from "./lib/errors";
import { ipc } from "./lib/ipc";
import { applyDocumentLocale, setFixedLocale, setFormatLocale } from "./lib/i18n";

function AppWithRecovery() {
  const { reportCrash } = useCrashRecovery({
    enableAutoRecovery: true,
    crashThreshold: 3,
    timeWindow: 60_000,
  });

  // Uncaught script errors are recorded in the crash history but don't reload the window
  // (only a render crash does). Promise rejections are handled in installDebugLogging:
  // logged and swallowed, never filed as crashes.
  useEffect(() => {
    const onWindowError = (event: ErrorEvent) => {
      if (!event.error) return; // e.g. "ResizeObserver loop" notifications carry no error
      reportCrash(event.error, { severity: "moderate", component: "window", action: "error" });
    };
    window.addEventListener("error", onWindowError);
    return () => window.removeEventListener("error", onWindowError);
  }, [reportCrash]);

  return (
    <CrashBoundary
      onError={(error, errorInfo) => {
        dlog(
          "error",
          "react",
          `render crash: ${describeError(error)}\n${stackFrames(error)}\ncomponentStack:${errorInfo.componentStack ?? " (none)"}`
        );
        reportCrash(error, {
          severity: "critical",
          component: "ReactTree",
          action: errorInfo.componentStack ? "render_with_stack" : "render",
        });
      }}
    >
      <App />
    </CrashBoundary>
  );
}

// The island is always Hebrew, whatever language Windows runs in: strings, weekday and month
// names, "tomorrow", "in 5 min". Number order and the 12/24-hour clock keep following the
// Windows regional format.
setFixedLocale("he");
applyDocumentLocale();
installDebugLogging();
dlog("info", "app", `app boot — v${APP_VERSION} dpr=${window.devicePixelRatio} win=${window.innerWidth}x${window.innerHeight}`);

// The date locale must be known before the first render: the compact island's width depends on it.
void ipc
  .getFormatLocale()
  .then(setFormatLocale)
  .finally(() => {
    ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
      <React.StrictMode>
        <AppWithRecovery />
      </React.StrictMode>
    );
  });
