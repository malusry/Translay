import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "../../../src/shared/uiFonts";
import "../../../src/shared/base.css";
import "../../../src/settings/settings.css";
import "./style.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode><App /></React.StrictMode>,
);
