import { useEffect, useState } from "react";
import { observeOverlayInteraction } from "./overlayInteraction";
import type {
  AnimationEventHandler,
  CSSProperties,
  RefObject,
} from "react";

import type { CapturePayload } from "../shared/types";
import {
  ExplanationPreviewPanel,
  type ExplanationPanelPhase,
} from "./ExplanationPreviewPanel";
import type { ExplanationLoadState } from "./explanationState";
import { TranslationText } from "./TranslationText";
import type { MotionPhase } from "./motionMachine";
import {
  contentText,
  conversationalToneNote,
  shouldShowExplanationAction,
  statusText,
  isSelectionHint,
} from "./overlayState";
import {
  CloseIcon,
  CopyIcon,
  ExplanationIcon,
} from "./OverlayIcons";

interface OverlayViewProps {
  overlayRef: RefObject<HTMLElement | null>;
  payload: CapturePayload;
  activeMotionPhase: MotionPhase;
  motionStyle: CSSProperties;
  isCompact: boolean;
  isTranslated: boolean;
  isFailure: boolean;
  isLoading: boolean;
  copied: boolean;
  copyFailed: boolean;
  retryPending?: boolean;
  retainRetryLayout?: boolean;
  explanationPhase:
    | "closed"
    | "preparing"
    | ExplanationPanelPhase
    | "collapsing";
  explanationLoadState: ExplanationLoadState;
  explanationCopied: boolean;
  explanationCopyFailed: boolean;
  departingStatusText: string | null;
  onHoveredChange: (hovered: boolean) => void;
  onSurfaceAnimationEnd: AnimationEventHandler<HTMLDivElement>;
  onCopy: () => void;
  onToggleExplanation: () => void;
  onCloseExplanation: () => void;
  onCopyExplanation: () => void;
  onRetryExplanation: () => void;
  onExplanationAnimationEnd: AnimationEventHandler<HTMLElement>;
  onRetry: () => void;
  onDismiss: () => void;
}

export function OverlayView({
  overlayRef,
  payload,
  activeMotionPhase,
  motionStyle,
  isCompact,
  isTranslated,
  isFailure,
  isLoading,
  copied,
  copyFailed,
  retryPending = false,
  retainRetryLayout = false,
  explanationPhase,
  explanationLoadState,
  explanationCopied,
  explanationCopyFailed,
  departingStatusText,
  onHoveredChange,
  onSurfaceAnimationEnd,
  onCopy,
  onToggleExplanation,
  onCloseExplanation,
  onCopyExplanation,
  onRetryExplanation,
  onExplanationAnimationEnd,
  onRetry,
  onDismiss,
}: OverlayViewProps) {
  const selectionHint = isSelectionHint(payload);
  const showFailure = isFailure && !selectionHint;
  useEffect(() => {
    const element = overlayRef.current;
    if (!element) return;
    return observeOverlayInteraction(element, window, onHoveredChange);
  }, [payload.requestId, overlayRef, onHoveredChange]);

  const [slowRequestId, setSlowRequestId] = useState<number | null>(null);
  useEffect(() => {
    setSlowRequestId(null);
    if (payload.phase !== "translating") return;
    const timer = window.setTimeout(() => setSlowRequestId(payload.requestId), 8000);
    return () => window.clearTimeout(timer);
  }, [payload.requestId, payload.phase]);
  const loadingLabel = payload.phase === "translating" && slowRequestId === payload.requestId
    ? "仍在翻译" : statusText(payload);

  const showExplanationAction = shouldShowExplanationAction(payload);
  const translationContent = contentText(payload);
  const toneNote = conversationalToneNote(payload);
  const explanationExpanded = explanationPhase !== "closed";
  const explanationPanelPhase: ExplanationPanelPhase | null =
    explanationPhase === "revealing" ||
    explanationPhase === "open" ||
    explanationPhase === "closing"
      ? explanationPhase
      : null;

  return (
    <main
      ref={overlayRef}
      key={payload.requestId}
      className={`overlay-stage motion-${activeMotionPhase} explanation-${explanationPhase} ${
        explanationExpanded ? "explanation-expanded" : ""
      }`}
      style={motionStyle}
      data-motion-state={activeMotionPhase}
      data-reveal-stage={activeMotionPhase}
      aria-live="polite"
    >
      <section
        className={`overlay ${payload.phase} motion-${activeMotionPhase} ${
          isCompact ? "compact" : ""
        } ${showExplanationAction ? "has-explanation-action" : ""}`}
        data-retry-layout={retainRetryLayout || undefined}
        aria-label="翻译浮层"
      >
        <div
          className="overlay-surface"
          aria-hidden="true"
          onAnimationEnd={onSurfaceAnimationEnd}
        />
        {departingStatusText !== null && (
          <div className="departing-loading-shell" aria-hidden="true">
            <div className="departing-loading">
              <span className="departing-dot" />
              {departingStatusText}
            </div>
          </div>
        )}
        <header className="overlay-header">
          <div className="status" data-status={showFailure && !retryPending ? "failed" : isLoading || retryPending ? "loading" : isTranslated ? (payload.translationMode === "academic" ? "study" : "daily") : "neutral"}>
            {(payload.phase === "capturing" ||
              payload.phase === "translating" || showFailure) && (
              <span className="progress-dot" aria-hidden="true" />
            )}
            <span>{retryPending ? "翻译中" : loadingLabel}</span>
            {showFailure && (
              <button
                className="overlay-icon-button retry-button"
                type="button"
                disabled={retryPending}
                aria-hidden={retryPending || undefined}
                style={retryPending ? { visibility: "hidden" } : undefined}
                aria-busy={retryPending}
                onClick={onRetry}
                aria-label="重新翻译"
                title="重试翻译"
              >
                <span className="retry-glyph" aria-hidden="true">↻</span>
              </button>
            )}
          </div>
          <div className="tools">
            {showExplanationAction && (
              <button
                className={`overlay-icon-button explain-button ${
                  explanationExpanded ? "active" : ""
                }`}
                type="button"
                onClick={onToggleExplanation}
                aria-label={
                  explanationExpanded ? "收起详细解释" : "查看详细解释"
                }
                aria-expanded={explanationExpanded}
                aria-controls="explanation-panel"
                title={explanationExpanded ? "收起详细解释" : "详细解释"}
              >
                <ExplanationIcon />
              </button>
            )}
            {isTranslated && (
              <button
                className={`overlay-icon-button copy-button ${
                  copied ? "confirmed" : copyFailed ? "failed" : ""
                }`}
                type="button"
                onClick={onCopy}
                aria-label={
                  copied ? "已复制" : copyFailed ? "复制失败" : "复制译文"
                }
                title={copied ? "已复制" : copyFailed ? "复制失败" : "复制译文"}
              >
                <CopyIcon confirmed={copied} failed={copyFailed} />
              </button>
            )}
            {payload.requestId > 0 && (
              <button
                className="overlay-icon-button close-button"
                type="button"
                onClick={onDismiss}
                aria-label="关闭翻译"
                title="关闭"
              >
                <CloseIcon />
              </button>
            )}
          </div>
        </header>

        {!isLoading && (
          <section
            key={payload.phase}
            className={`translation ${payload.success ? "" : "empty"}`}
            aria-label={payload.success ? "翻译结果" : "翻译提示"}
            dir="auto"
          >
            {payload.phase === "translated" &&
            payload.success ? (
              <TranslationText text={translationContent} academic={payload.translationMode === "academic"} />
            ) : (
              translationContent
            )}
            {toneNote && <p className="tone-note" aria-label="语气说明">{toneNote}</p>}
          </section>
        )}
      </section>

      {explanationPanelPhase && (
        <ExplanationPreviewPanel
          phase={explanationPanelPhase}
          loadState={explanationLoadState}
          copied={explanationCopied}
          copyFailed={explanationCopyFailed}
          onCopy={onCopyExplanation}
          onRetry={onRetryExplanation}
          onClose={onCloseExplanation}
          onAnimationEnd={onExplanationAnimationEnd}
        />
      )}
    </main>
  );
}
