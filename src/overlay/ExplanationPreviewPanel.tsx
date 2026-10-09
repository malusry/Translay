import { useEffect, useState, type AnimationEventHandler } from "react";

import type { ExplanationLoadState } from "./explanationState";
import { MathText } from "./MathText";
import { CloseIcon, CopyIcon, RetryIcon } from "./OverlayIcons";

export type ExplanationPanelPhase = "revealing" | "open" | "closing";

interface ExplanationPreviewPanelProps {
  phase: ExplanationPanelPhase;
  loadState: ExplanationLoadState;
  copied: boolean;
  copyFailed: boolean;
  onCopy: () => void;
  onRetry: () => void;
  onClose: () => void;
  onAnimationEnd: AnimationEventHandler<HTMLElement>;
}

export function ExplanationPreviewPanel({
  phase,
  loadState,
  copied,
  copyFailed,
  onCopy,
  onRetry,
  onClose,
  onAnimationEnd,
}: ExplanationPreviewPanelProps) {
  const [waitingLonger, setWaitingLonger] = useState(false);
  useEffect(() => {
    setWaitingLonger(false);
    if (loadState.status !== "loading") return;
    const timer = window.setTimeout(() => setWaitingLonger(true), 8000);
    return () => window.clearTimeout(timer);
  }, [loadState]);

  const ready = loadState.status === "ready" && loadState.content !== null;

  return (
    <aside
      id="explanation-panel"
      className={`explanation-panel ${phase}`}
      aria-label="详细解释"
      aria-busy={loadState.status === "loading"}
      onAnimationEnd={onAnimationEnd}
    >
      <header className="explanation-header">
        <span className="explanation-heading">
          <span className="explanation-heading-dot" aria-hidden="true" />
          详细解释
        </span>
        <div className="explanation-tools">
          <button
            className={`overlay-icon-button copy-button ${
              copied ? "confirmed" : copyFailed ? "failed" : ""
            }`}
            type="button"
            disabled={!ready}
            onClick={onCopy}
            aria-label={
              copied ? "已复制" : copyFailed ? "复制失败" : "复制详细解释"
            }
            title={copied ? "已复制" : copyFailed ? "复制失败" : "复制解释"}
          >
            <CopyIcon confirmed={copied} failed={copyFailed} />
          </button>
          <button
            className="overlay-icon-button close-button"
            type="button"
            onClick={onClose}
            aria-label="关闭详细解释"
            title="关闭"
          >
            <CloseIcon />
          </button>
        </div>
      </header>

      {loadState.status === "loading" && (
        <div className="explanation-content explanation-feedback" role="status">
          <span className="explanation-loading-dot" aria-hidden="true" />
          <span>{waitingLonger ? "仍在生成解释，可随时关闭" : "正在理解选中内容，请稍候…"}</span>
        </div>
      )}

      {loadState.status === "error" && (
        <div className="explanation-content explanation-feedback" role="alert">
          <p>{loadState.errorMessage ?? "暂时无法生成解释"}</p>
          <button
            className="overlay-icon-button explanation-retry-button"
            type="button"
            onClick={onRetry}
            aria-label="重新生成解释"
            title="重新解释"
          >
            <RetryIcon />
          </button>
        </div>
      )}

      {ready && loadState.content && (
        <div className="explanation-content">
          <section className="explanation-section">
            <p>
              <MathText text={loadState.content.coreExplanation} />
            </p>
            {loadState.content.caveat && (
              <p className="explanation-caveat">
                <MathText text={loadState.content.caveat} />
              </p>
            )}
          </section>

          {loadState.content.keyConcepts.length > 0 && (
            <section className="explanation-section">
              <h3>关键概念</h3>
              <dl className="explanation-terms">
                {loadState.content.keyConcepts.map((term, index) => (
                  <div key={`${index}-${term.term}`}>
                    <dt>
                      <span>
                        <MathText text={term.term} />
                      </span>
                      {term.translation && (
                        <small>
                          <MathText text={term.translation} />
                        </small>
                      )}
                    </dt>
                    <dd>
                      <MathText text={term.explanation} />
                    </dd>
                  </div>
                ))}
              </dl>
            </section>
          )}


        </div>
      )}
    </aside>
  );
}
