import React from "react";
import ReactDOM from "react-dom/client";
import "./shared/base.css";

const isSettings =
  new URLSearchParams(window.location.search).get("view") === "settings";
const root = ReactDOM.createRoot(document.getElementById("root")!);

async function renderSelectedView() {
  if (isSettings) {
    const { Settings } = await import("./settings/Settings");
    root.render(
      <React.StrictMode>
        <Settings />
      </React.StrictMode>,
    );
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
