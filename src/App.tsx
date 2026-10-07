import { PillShell } from "./components/Pill/PillShell";
import { stageSize } from "./components/Pill/usePillGeometry";
import { useIslandLimits } from "./lib/island/limits";

// The native window is the island's stage (top-centred on the monitor by the backend), so the page
// is just the island at the stage's top centre. It centres on the stage width it asked the backend
// for, not on the viewport: WebView2 can report a viewport a device pixel or two wider than the
// window (measured: 401.14 CSS px for a 700 px window at 175 %), which would put the island that
// much right of the monitor's centre. Hiding it for fullscreen apps is done by the backend.
function App() {
  const stage = stageSize(useIslandLimits());
  return (
    // Anchored at the window's physical origin (left: 0, not "start": the document is RTL and a
    // block would otherwise hug the right edge of the wider viewport).
    <div className="absolute top-0 h-screen flex items-start justify-center" style={{ left: 0, width: stage.width }}>
      <PillShell />
    </div>
  );
}

export default App;
