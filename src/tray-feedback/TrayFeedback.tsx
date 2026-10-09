import { useEffect, useRef, useState } from "react";
import "../shared/uiFonts";
import {
  onTrayFeedbackDismiss,
  onTrayFeedbackShow,
} from "./trayFeedbackIpc";
import "./trayFeedback.css";

type FeedbackPhase = "hidden" | "visible" | "leaving";

export function TrayFeedback() {
  const [message, setMessage] = useState("");
  const [mode, setMode] = useState<"conversational" | "academic">("conversational");
  const [phase, setPhase] = useState<FeedbackPhase>("hidden");
  const [animationKey, setAnimationKey] = useState(0);
  const generation = useRef(0);

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];

    void onTrayFeedbackShow((payload) => {
      if (disposed) {
        return;
      }
      generation.current = payload.generation;
      setMessage(payload.message);
      setMode(payload.mode);
      setAnimationKey(payload.generation);
      setPhase("visible");
    }).then((unlisten) => {
      if (disposed) {
        unlisten();
      } else {
        unlisteners.push(unlisten);
      }
    });

    void onTrayFeedbackDismiss((dismissedGeneration) => {
      if (!disposed && dismissedGeneration === generation.current) {
        setPhase("leaving");
      }
    }).then((unlisten) => {
      if (disposed) {
        unlisten();
      } else {
        unlisteners.push(unlisten);
      }
    });

    return () => {
      disposed = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  return (
    <main className={`tray-feedback-stage tray-feedback-stage--${phase}`}>
      <div key={animationKey} className="tray-feedback-pill" data-mode={mode} role="status">
        <span className="tray-feedback-label">{message}</span>
      </div>
    </main>
  );
}
