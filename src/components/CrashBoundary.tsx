import { Component, type ErrorInfo, type ReactNode } from "react";
import { ERROR_CODES } from "../lib/appInfo";
import { t } from "../lib/i18n";

interface CrashBoundaryProps {
  children: ReactNode;
  onError?: (error: Error, errorInfo: ErrorInfo) => void;
}

interface CrashBoundaryState {
  hasError: boolean;
}

/** Last line of defence for the whole tree. Tab-level failures are contained by TabBoundary. */
export class CrashBoundary extends Component<CrashBoundaryProps, CrashBoundaryState> {
  state: CrashBoundaryState = {
    hasError: false,
  };

  static getDerivedStateFromError(): CrashBoundaryState {
    return { hasError: true };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    this.props.onError?.(error, errorInfo);
  }

  // Reset the error state so React re-mounts children. If the underlying bug persists,
  // getDerivedStateFromError flips hasError back to true.
  private handleTryAgain = () => {
    this.setState({ hasError: false });
  };

  render() {
    if (this.state.hasError) {
      // The window is only as big as the collapsed island, so the fallback is a small pill.
      return (
        <div className="w-full flex items-start justify-center">
          <button
            type="button"
            onClick={this.handleTryAgain}
            className="h-[34px] px-4 rounded-full bg-black text-white/80 text-[12px] font-semibold flex items-center gap-2"
            title={t("island.tryAgain")}
            dir="ltr"
          >
            <span dir="auto">{t("island.unavailable")}</span>
            <span className="text-white/40 tabular-nums">{ERROR_CODES.uiRender}</span>
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
