import { Component, type ReactNode } from "react";
import { ERROR_CODES } from "../../lib/appInfo";
import { dlog } from "../../lib/debugLog";
import { describeError, stackFrames } from "../../lib/errors";
import { t } from "../../lib/i18n";
import { ErrorState } from "./ui/states";

interface TabBoundaryProps {
  /** Tab id, for the log only. Remount with a new key to reset after a failure. */
  tab: string;
  children: ReactNode;
}

/** Contains a render error to one tab: the tab shows an error state (Unavailable, code, Try again), the island keeps working. */
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
      <ErrorState
        title={t("island.unavailable")}
        code={ERROR_CODES.uiRender}
        // If the cause is still there the boundary simply fails again; the island never goes down with it.
        action={{ label: t("island.tryAgain"), onPress: () => this.setState({ failed: false }) }}
      />
    );
  }
}
