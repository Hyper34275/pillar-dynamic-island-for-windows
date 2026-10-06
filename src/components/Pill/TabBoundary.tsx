import { Component, type ReactNode } from "react";
import { ERROR_CODES } from "../../lib/appInfo";
import { dlog } from "../../lib/debugLog";
import { describeError, stackFrames } from "../../lib/errors";
import { t } from "../../lib/i18n";

interface TabBoundaryProps {
  /** Tab id, for the log only. Remount with a new key to reset after a failure. */
  tab: string;
  children: ReactNode;
}

/** Contains a render error to one tab: the tab shows "Unavailable", the island keeps working. */
export class TabBoundary extends Component<TabBoundaryProps, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch(error: Error) {
    dlog("error", "react", `tab "${this.props.tab}" render failed [${ERROR_CODES.uiRender}] ${describeError(error)}\n${stackFrames(error)}`);
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <div dir="ltr" role="alert" className="flex-1 flex flex-col items-center justify-center gap-1 text-center">
        <span className="text-[14px] font-semibold text-white/80" dir="auto">
          {t("island.unavailable")}
        </span>
        <span className="text-[11px] font-medium text-white/40 tabular-nums">{ERROR_CODES.uiRender}</span>
      </div>
    );
  }
}
