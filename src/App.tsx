import { useEffect, useState } from "react";
import { motion } from "motion/react";
import { PillShell } from "./components/Pill/PillShell";
import { useIslandEvents } from "./hooks/useIslandEvents";
import { useSettings } from "./hooks/useSettings";
import { ipc } from "./lib/ipc";
import { dlog } from "./lib/debugLog";

function App() {
  const { settings } = useSettings();
  const [fullscreen, setFullscreen] = useState(false);
  useIslandEvents({ onFullscreenChanged: setFullscreen });

  // The island slides out of sight while a fullscreen app is in front (if the user wants that).
  const suspended = fullscreen && settings.hideInFullscreen;

  useEffect(() => {
    dlog("info", "app", suspended ? "fullscreen app in front — island hidden" : "island visible");
    // While slid off-screen the transparent window would still swallow clicks meant for
    // the app underneath — let them pass through. Otherwise it must receive them.
    void ipc.setClickThrough(suspended);
  }, [suspended]);

  return (
    <motion.div
      className="w-full h-screen flex items-start justify-center pt-0"
      style={{ minHeight: "100vh", overflow: "visible" }}
      animate={{ y: suspended ? -280 : 0 }}
      transition={{ type: "spring", stiffness: 320, damping: 30 }}
    >
      <PillShell suspended={suspended} />
    </motion.div>
  );
}

export default App;
