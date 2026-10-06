import { PillShell } from "./components/Pill/PillShell";

// The native window is exactly the island (top-centred by the backend), so the page is just
// the island at its top-centre. Hiding it for fullscreen apps is done by the backend.
function App() {
  return (
    <div className="w-full h-screen flex items-start justify-center">
      <PillShell />
    </div>
  );
}

export default App;
