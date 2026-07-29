import type {
  AnimationEventHandler,
  CSSProperties,
  RefObject,
} from "react";

import type { CapturePayload } from "../shared/types";
import type { MotionPhase } from "./motionMachine";
import { contentText, statusText } from "./overlayState";
import { CloseIcon, CopyIcon, RetryIcon } from "./OverlayIcons";

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
  departingStatusText: string | null;
  onHoveredChange: (hovered: boolean) => void;
  onSurfaceAnimationEnd: AnimationEventHandler<HTMLDivElement>;
  onCopy: () => void;
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
  departingStatusText,
  onHoveredChange,
  onSurfaceAnimationEnd,
  onCopy,
  onRetry,
  onDismiss,
}: OverlayViewProps) {
  return (
    <main
      ref={overlayRef}
      key={payload.requestId}
      className={`overlay ${payload.phase} motion-${activeMotionPhase} ${
        isCompact ? "compact" : ""
      }`}
      style={motionStyle}
      data-motion-state={activeMotionPhase}
      data-reveal-stage={activeMotionPhase}
      aria-live="polite"
      onMouseEnter={() => onHoveredChange(true)}
      onMouseLeave={() => onHoveredChange(false)}
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
        <span className="status">
          {(payload.phase === "capturing" ||
            payload.phase === "translating") && (
            <span className="progress-dot" aria-hidden="true" />
          )}
          {statusText(payload)}
        </span>
        <div className="tools">
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
          {isFailure && (
            <button
              className="overlay-icon-button retry-button"
              type="button"
              onClick={onRetry}
              aria-label="重新翻译"
              title="重新翻译"
            >
              <RetryIcon />
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
          {contentText(payload)}
        </section>
      )}
    </main>
  );
}
