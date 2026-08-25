import React from "react";
import ReactDOM from "react-dom/client";
import "./shared/base.css";

const selectedView = new URLSearchParams(window.location.search).get("view");
const root = ReactDOM.createRoot(document.getElementById("root")!);

async function renderSelectedView() {
  if (selectedView === "settings") {
    const { Settings } = await import("./settings/Settings");
    root.render(
      <React.StrictMode>
        <Settings />
      </React.StrictMode>,
    );
    return;
  }

  if (selectedView === "selection-button") {
    const { SelectionButton } = await import(
      "./selection-button/SelectionButton"
    );
    root.render(<SelectionButton />);
    return;
  }

  if (selectedView === "tray-feedback") {
    const { TrayFeedback } = await import("./tray-feedback/TrayFeedback");
    root.render(<TrayFeedback />);
    return;
  }

  const { Overlay } = await import("./overlay/Overlay");
  root.render(
    <React.StrictMode>
      <Overlay />
    </React.StrictMode>,
  );
}

void renderSelectedView();
