import { useEffect, useState, useRef } from "react";
import { motion } from "motion/react";
import { Pill } from "./components/Pill/Pill";
import { isTauriAvailable, tauriInvoke } from "./lib/tauri";
import { dlog } from "./lib/debugLog";

function App() {
  const [isFullscreen, setIsFullscreen] = useState(false);
  const fullscreenCheckRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const lastFullscreenState = useRef(false);
  // NOTE: screen-reader live regions live inside <Pill/> now, wired to the same
  // useScreenReader instance that calls announce(). App previously rendered them
  // against a separate instance that never announced, so nothing was ever spoken.

  // Position window on mount and handle display changes
  useEffect(() => {
    const positionWindow = () => tauriInvoke("position_window");
    positionWindow();

    // Listen for display/resolution changes
    const handleResize = () => {
      positionWindow();
    };

    // Reposition when window regains focus (handles monitor switches)
    const handleFocus = () => {
      positionWindow();
    };

    window.addEventListener("resize", handleResize);
    window.addEventListener("focus", handleFocus);

    return () => {
      window.removeEventListener("resize", handleResize);
      window.removeEventListener("focus", handleFocus);
    };
  }, []);

  // Fullscreen detection: poll so pill hides/shows when entering/leaving fullscreen
  useEffect(() => {
    if (!isTauriAvailable()) return;

    let isMounted = true;
    let isPending = false;
    const checkFullscreen = async () => {
      if (!isMounted || isPending) return; // Skip if previous check still in-flight
      isPending = true;

      try {
        const fullscreen = await tauriInvoke<boolean>("is_foreground_fullscreen");
        if (isMounted && fullscreen !== null && fullscreen !== lastFullscreenState.current) {
          dlog(
            "info",
            "app",
            fullscreen
              ? "fullscreen app detected — pill moved off-screen (y=-280), not clickable until it exits"
              : "fullscreen app exited — pill restored on-screen"
          );
          lastFullscreenState.current = fullscreen;
          setIsFullscreen(fullscreen);
          // While the pill is slid off-screen its (transparent) window would still
          // swallow clicks meant for the app underneath — let them pass through.
          tauriInvoke("set_click_through", { ignore: fullscreen }).catch(() => {});
        }
      } catch (error) {
        if (isMounted) {
          console.warn('Fullscreen check failed:', error);
        }
      } finally {
        isPending = false;
      }
    };

    // Initial check
    checkFullscreen();

    const POLL_MS = 1000; // 1s is responsive enough; avoids flooding the backend thread pool
    let intervalId: ReturnType<typeof setInterval> | null = null;

    const startPolling = () => {
      if (intervalId) clearInterval(intervalId);
      intervalId = setInterval(checkFullscreen, POLL_MS);
      fullscreenCheckRef.current = intervalId;
    };

    const stopPolling = () => {
      if (intervalId) {
        clearInterval(intervalId);
        intervalId = null;
        fullscreenCheckRef.current = null;
      }
    };

    // Pause polling when document is hidden (saves CPU when minimized)
    const handleVisibilityChange = () => {
      if (document.hidden) {
        stopPolling();
      } else {
        checkFullscreen(); // Immediate check when becoming visible
        startPolling();
      }
    };

    document.addEventListener("visibilitychange", handleVisibilityChange);
    startPolling();

    return () => {
      isMounted = false;
      stopPolling();
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, []);

  return (
    <>
      <motion.div
        className="w-full h-screen flex items-start justify-center pt-0"
        style={{ minHeight: "100vh", overflow: "visible" }}
        animate={{ y: isFullscreen ? -280 : 0 }}
        transition={{ type: "spring", stiffness: 320, damping: 30 }}
      >
        <Pill />
      </motion.div>
    </>
  );
}

export default App;
