import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import selectionMark from "../../assets/brand/selection-button-folded-ribbon/translay-folded-ribbon.svg?no-inline";

import "./selectionButton.css";

export function SelectionButton() {
  const translatingRef = useRef(false);
  const [visible, setVisible] = useState(true);
  const revision = useRef(0);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<{ revision: number; visible: boolean }>("selection-button-visibility", ({ payload }) => {
      if (disposed || payload.revision <= revision.current) return;
      revision.current = payload.revision;
      setVisible(payload.visible);
    }).then(stop => { if (disposed) stop(); else unlisten = stop; })
      .catch(error => console.error("[Translay] selection visibility listener failed", error));
    return () => { disposed = true; unlisten?.(); };
  }, []);

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
    <main className="selection-button-shell" data-visible={visible}>
      <button
        className="selection-button"
        type="button"
        aria-label="翻译选中文字"
        title="翻译选中文字"
        onClick={translateSelection}
        onContextMenu={(event) => event.preventDefault()}
      >
        <img
          className="selection-button-mark"
          src={selectionMark}
          alt=""
          aria-hidden="true"
          draggable={false}
          width={24}
          height={24}
        />
      </button>
    </main>
  );
}
