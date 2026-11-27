import React from "react";
import ReactDOM from "react-dom/client";
import { hydratePrefs } from "@/lib/prefs";
import { AppRouter } from "./app/router";
import "./app/globals.css";

const rootElement = document.getElementById("root");
if (!rootElement) {
  throw new Error("root element not found");
}

const root = ReactDOM.createRoot(rootElement);

function render(): void {
  root.render(
    <React.StrictMode>
      <AppRouter />
    </React.StrictMode>,
  );
}

// Read remembered settings before first paint so tools render the user's
// values rather than flashing defaults and snapping. `hydratePrefs` never
// rejects — if IPC is unavailable the app simply runs without memory, so
// rendering is not gated on success.
void hydratePrefs().finally(render);
