import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import AssistantPanel from "./AssistantPanel";
import SpeechOverlay from "./SpeechOverlay";

const overlay = new URLSearchParams(window.location.search).has("overlay");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {new URLSearchParams(window.location.search).has("assistant") ? <AssistantPanel /> : overlay ? <SpeechOverlay /> : <App />}
  </React.StrictMode>,
);
