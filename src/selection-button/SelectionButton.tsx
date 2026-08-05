import { useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

import "./selectionButton.css";

export function SelectionButton() {
  const translatingRef = useRef(false);

  const translateSelection = () => {
    if (translatingRef.current) return;
    translatingRef.current = true;
    void invoke<boolean>("translate_detected_selection")
      .catch((error) => {
        console.error("[Translay] selection button trigger failed", error);
      })
      .finally(() => {
        translatingRef.current = false;
      });
  };

  return (
    <main className="selection-button-shell">
      <button
        className="selection-button"
        type="button"
        aria-label="翻译选中文字"
        title="翻译选中文字"
        onClick={translateSelection}
        onContextMenu={(event) => event.preventDefault()}
      >
        <svg
          className="selection-button-mark"
          viewBox="0 0 48 48"
          aria-hidden="true"
        >
          <path
            className="selection-button-mark-halo"
            d="M23 9.2V24.5C23 30.3 28.1 34.4 33.7 32.9C40.6 30.9 42.9 21.3 38.7 14.5C33.4 6.8 21.9 6 13.3 12.5C4.6 19.4 5.4 31.7 13.7 38.8C22 45.5 33.8 43.8 37.9 36.8"
            strokeWidth="11"
          />
          <path
            className="selection-button-mark-halo"
            d="M19.8 14.4H27"
            strokeWidth="7"
          />
          <path
            className="selection-button-mark-core"
            d="M23 9.2V24.5C23 30.3 28.1 34.4 33.7 32.9C40.6 30.9 42.9 21.3 38.7 14.5C33.4 6.8 21.9 6 13.3 12.5C4.6 19.4 5.4 31.7 13.7 38.8C22 45.5 33.8 43.8 37.9 36.8"
            strokeWidth="7.5"
          />
          <path
            className="selection-button-mark-core"
            d="M19.8 14.4H27"
            strokeWidth="3.8"
          />
        </svg>
      </button>
    </main>
  );
}
