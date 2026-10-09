import {
  type CSSProperties,
  useCallback,
  useEffect,
  useLayoutEffect,
  useReducer,
  useRef,
  useState,
} from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { CapturePayload } from "../shared/types";
import {
  attachLifecycleSynchronization,
  startBoundedPolling,
  syncLatestCapture as synchronizeLatestCapture,
} from "./overlayDelivery";
import {
  applyPayload,
  conversationalToneNote,
  shouldBridgeLoadingToResult,
  shouldAcceptCapturePayload,
  shouldShowExplanationAction,
  statusText,
  waitingPayload,
} from "./overlayState";
import {
  DISMISS_SAFETY_TIMEOUT_MS,
  calculateMaterializeTiming,
  calculateRevealOrigin,
  LOADING_OVERLAY_HEIGHT,
  LOADING_OVERLAY_WIDTH,
  MATERIALIZE_SAFETY_PADDING_MS,
  readWindowGeometry,
  shouldAnimateLoadingResult,
  type WindowGeometry,
} from "./overlayMotion";
import {
  initialMotionState,
  reduceMotionState,
  type MotionState,
} from "./motionMachine";
import {
  applyExplanationFailure,
  applyExplanationResult,
  beginExplanationLoad,
  developmentExplanationPreview,
  explanationErrorMessage,
  formatExplanationForCopy,
  idleExplanationLoadState,
  shouldStartExplanationLoad,
  type ExplanationLoadState,
} from "./explanationState";
import {
  acknowledgeCapture,
  copyTranslation as copyTranslationToClipboard,
  dismissOverlay,
  explainTranslation,
  cancelExplanation,
  fitOverlayHeight,
  getLatestCapture,
  notifyOverlayFrontendReady,
  onCaptureResult,
  onOverlayDismissRequested,
  retryCapture,
  setOverlayHovered,
} from "./overlayIpc";
import { developmentPreview } from "./overlayPreview";
import { OverlayView } from "./OverlayView";
import "./overlay.css";

const ENABLE_STAGED_REVEAL = true;
const EXPLANATION_PANEL_HEIGHT = 292;
const EXPLANATION_PANEL_GAP = 8;
const EXPLANATION_REVEAL_SAFETY_TIMEOUT_MS = 520;
const EXPLANATION_RETURN_SAFETY_TIMEOUT_MS = 260;

type ExplanationPhase =
  | "closed"
  | "preparing"
  | "revealing"
  | "open"
  | "closing"
  | "collapsing";

type ExplanationState = {
  requestId: number | null;
  phase: ExplanationPhase;
};

const closedExplanationState: ExplanationState = {
  requestId: null,
  phase: "closed",
};

type MotionDebugFrame = {
  elapsedMs: number;
  stage: string;
  surfaceOpacity: string;
  surfaceTransform: string;
  surfaceWidth: string;
  surfaceHeight: string;
  surfaceClipPath: string;
  surfaceAnimation: string;
  contentOpacity: string;
  contentTransform: string;
  contentAnimation: string;
  controlsOpacity: string;
  controlsTransform: string;
  controlsAnimation: string;
};

type MotionDebugWindow = Window & {
  __translayMotionFrames?: MotionDebugFrame[];
};

export function Overlay() {
  const previewRef = useRef(developmentPreview());
  const [payload, setPayload] = useState(previewRef.current ?? waitingPayload);
  const [copiedRequestId, setCopiedRequestId] = useState<number | null>(null);
  const retryLayoutRequestRef = useRef<number | null>(null);
  const retryRequestRef = useRef<number | null>(null);
  const [retryRequestId, setRetryRequestId] = useState<number | null>(null);
  const [compactOverflowRequestId, setCompactOverflowRequestId] = useState<number | null>(null);
  const [copyFailedRequestId, setCopyFailedRequestId] = useState<number | null>(
    null,
  );
  const [explanationLoadState, setExplanationLoadState] =
    useState<ExplanationLoadState>(idleExplanationLoadState);
  const [explanationCopiedRequestId, setExplanationCopiedRequestId] = useState<
    number | null
  >(null);
  const [explanationCopyFailedRequestId, setExplanationCopyFailedRequestId] =
    useState<number | null>(null);
  const [explanationState, setExplanationState] = useState<ExplanationState>(
    closedExplanationState,
  );
  const [departingStatus, setDepartingStatus] = useState<{
    requestId: number;
    text: string;
  } | null>(null);
  const initialPreviewMotion: MotionState = previewRef.current
    ? {
        requestId: previewRef.current.requestId,
        phase:
          previewRef.current.phase === "capturing" ||
          previewRef.current.phase === "translating"
            ? "loading"
            : "settled",
        loadingGeometry: null,
        dismissReason: null,
      }
    : initialMotionState;
  const [motionState, dispatchMotion] = useReducer(
    reduceMotionState,
    initialPreviewMotion,
  );
  const payloadRef = useRef(previewRef.current ?? waitingPayload);
  const explanationStateRef = useRef<ExplanationState>(closedExplanationState);
  const explanationLoadStateRef = useRef<ExplanationLoadState>(
    idleExplanationLoadState(),
  );
  const explanationLoadSequenceRef = useRef(0);
  const explanationBaseHeightRef = useRef<number | null>(null);
  const explanationSequenceRef = useRef(0);
  const explanationFirstFrameRef = useRef(0);
  const explanationSecondFrameRef = useRef(0);
  const explanationTimerRef = useRef<number | undefined>(undefined);
  const explanationCollapseRequestRef = useRef<number | null>(null);
  const motionStateRef = useRef(motionState);
  motionStateRef.current = motionState;
  const overlayRef = useRef<HTMLElement | null>(null);
  const interactingRef = useRef(false);
  const copyHoldRef = useRef<number | null>(null);
  const copyHoldTimerRef = useRef<number | undefined>(undefined);
  const copyAttemptRef = useRef(0);
  useEffect(() => {
    copyHoldRef.current = null;
    copyAttemptRef.current += 1;
    window.clearTimeout(copyHoldTimerRef.current);
    return () => {
      copyAttemptRef.current += 1;
      window.clearTimeout(copyHoldTimerRef.current);
    };
  }, [payload.requestId]);
  const loadingGeometryRef = useRef<{
    requestId: number;
    geometry: WindowGeometry;
  } | null>(null);
  const loadingStartedRef = useRef<{ requestId: number; at: number } | null>(null);
  const pendingRevealAcknowledgementRef = useRef<number | null>(null);
  const pollingRef = useRef<(() => void) | undefined>(undefined);
  const departingStatusTimerRef = useRef<number | undefined>(undefined);
  const deliveryErrorReportedRef = useRef(false);
  const dismissCompletionRequestRef = useRef<number | null>(null);

  const clearExplanationSchedule = useCallback(() => {
    window.cancelAnimationFrame(explanationFirstFrameRef.current);
    window.cancelAnimationFrame(explanationSecondFrameRef.current);
    window.clearTimeout(explanationTimerRef.current);
    explanationFirstFrameRef.current = 0;
    explanationSecondFrameRef.current = 0;
    explanationTimerRef.current = undefined;
  }, []);

  const commitExplanationState = useCallback((next: ExplanationState) => {
    explanationStateRef.current = next;
    setExplanationState(next);
  }, []);

  const commitExplanationLoadState = useCallback(
    (next: ExplanationLoadState) => {
      explanationLoadStateRef.current = next;
      setExplanationLoadState(next);
    },
    [],
  );

  const cancelPendingExplanation = useCallback(() => {
    const load = explanationLoadStateRef.current;
    if (load.status !== "loading" || load.requestId === null) return;
    const attemptId = explanationLoadSequenceRef.current;
    explanationLoadSequenceRef.current += 1;
    commitExplanationLoadState(idleExplanationLoadState());
    if (!previewRef.current) {
      void cancelExplanation(load.requestId, attemptId).catch((error) => {
        console.error("[Translay] cancel explanation failed", error);
      });
    }
  }, [commitExplanationLoadState]);

  useEffect(() => () => cancelPendingExplanation(), [cancelPendingExplanation]);

  const resetExplanationState = useCallback(() => {
    cancelPendingExplanation();
    explanationSequenceRef.current += 1;
    explanationLoadSequenceRef.current += 1;
    clearExplanationSchedule();
    explanationCollapseRequestRef.current = null;
    explanationBaseHeightRef.current = null;
    commitExplanationState(closedExplanationState);
    commitExplanationLoadState(idleExplanationLoadState());
    setExplanationCopiedRequestId(null);
    setExplanationCopyFailedRequestId(null);
  }, [
    cancelPendingExplanation,
    clearExplanationSchedule,
    commitExplanationLoadState,
    commitExplanationState,
  ]);

  const releaseExplanationPin = useCallback((requestId: number) => {
    if (previewRef.current) return;
    const hovered = copyHoldRef.current === requestId || interactingRef.current || (overlayRef.current?.matches(":hover") ?? false);
    void setOverlayHovered(requestId, hovered);
  }, []);

  const reportDeliveryError = useCallback(
    (stage: string, error: unknown) => {
      console.error(`[Translay] overlay IPC failed during ${stage}`, error);
      // A delivery/acknowledgement failure is not a translation result. Keep
      // the last accepted request, including when an older IPC finishes late.
      // Only an overlay that has never received a request needs this fallback.
      if (payloadRef.current.requestId > 0) return;
      if (deliveryErrorReportedRef.current) return;
      deliveryErrorReportedRef.current = true;
      const failurePayload: CapturePayload = {
        ...waitingPayload,
        errorCode: "OVERLAY_IPC_FAILED",
        errorMessage: "界面通信未连接，请重新启动 Translay",
      };
      payloadRef.current = failurePayload;
      setPayload(failurePayload);
    },
    [],
  );

  const applyLatestPayload = useCallback((next: CapturePayload): boolean => {
    if (!shouldAcceptCapturePayload(payloadRef.current, next)) return false;
    const current = payloadRef.current;
    if (next.requestId > current.requestId) {
      resetExplanationState();
    }
    if (
      next.requestId === current.requestId &&
      motionStateRef.current.phase === "dismissing" &&
      motionStateRef.current.dismissReason === "manual"
    ) {
      return true;
    }
    if (next.requestId > current.requestId) {
      retryLayoutRequestRef.current = current.phase === "translationFailed" &&
        retryRequestRef.current === current.requestId && next.phase === "translating"
        ? next.requestId : null;
    }
    if (shouldBridgeLoadingToResult(current, next)) {
      const loadingGeometry =
        loadingGeometryRef.current?.requestId === next.requestId
          ? loadingGeometryRef.current.geometry
          : null;
      const animate = ENABLE_STAGED_REVEAL && shouldAnimateLoadingResult(
        loadingStartedRef.current?.requestId === next.requestId
          ? performance.now() - loadingStartedRef.current.at : 0,
      );
      dispatchMotion(
        animate
          ? {
              type: "prepare",
              requestId: next.requestId,
              loadingGeometry,
            }
          : { type: "settle-immediately", requestId: next.requestId },
      );
      pendingRevealAcknowledgementRef.current = animate
        ? next.requestId
        : null;
      window.clearTimeout(departingStatusTimerRef.current);
      const bridge = {
        requestId: current.requestId,
        text: statusText(current),
      };
      setDepartingStatus(animate ? bridge : null);
      departingStatusTimerRef.current = window.setTimeout(() => {
        setDepartingStatus((visible) =>
          visible?.requestId === bridge.requestId ? null : visible,
        );
      }, 200);
    } else if (next.requestId > current.requestId) {
      window.clearTimeout(departingStatusTimerRef.current);
      setDepartingStatus(null);
      pendingRevealAcknowledgementRef.current = null;
      const nextIsLoading =
        next.phase === "capturing" || next.phase === "translating";
      dispatchMotion(
        nextIsLoading
          ? { type: "loading", requestId: next.requestId }
          : { type: "settle-immediately", requestId: next.requestId },
      );
    }
    payloadRef.current = applyPayload(current, next);
    setPayload(payloadRef.current);
    setCopiedRequestId((current) =>
      current === payloadRef.current.requestId ? current : null,
    );
    setCopyFailedRequestId((current) =>
      current === payloadRef.current.requestId ? current : null,
    );
    return true;
  }, [resetExplanationState]);

  useEffect(() => {
    const motionPreview = new URLSearchParams(window.location.search).get(
      "preview",
    );
    if (
      !import.meta.env.DEV ||
      (motionPreview !== "motion" && motionPreview !== "motion-compact")
    ) {
      return;
    }

    const debugWindow = window as MotionDebugWindow;
    const frames: MotionDebugFrame[] = [];
    debugWindow.__translayMotionFrames = frames;
    document.documentElement.dataset.translayMotionFrames = "[]";
    let frameTimer: number | undefined;
    let stopTimer: number | undefined;

    const transitionTimer = window.setTimeout(() => {
      const startedAt = performance.now();
      const recordFrame = () => {
        const root = overlayRef.current;
        const surface = root?.querySelector<HTMLElement>(".overlay-surface");
        const content = root?.querySelector<HTMLElement>(".translation");
        const controls = root?.querySelector<HTMLElement>(".tools");
        if (root && surface && content && controls) {
          const surfaceStyle = window.getComputedStyle(surface);
          const contentStyle = window.getComputedStyle(content);
          const controlsStyle = window.getComputedStyle(controls);
          frames.push({
            elapsedMs: Math.round(performance.now() - startedAt),
            stage: root.dataset.revealStage ?? "",
            surfaceOpacity: surfaceStyle.opacity,
            surfaceTransform: surfaceStyle.transform,
            surfaceWidth: surfaceStyle.width,
            surfaceHeight: surfaceStyle.height,
            surfaceClipPath: surfaceStyle.clipPath,
            surfaceAnimation: surfaceStyle.animationName,
            contentOpacity: contentStyle.opacity,
            contentTransform: contentStyle.transform,
            contentAnimation: contentStyle.animationName,
            controlsOpacity: controlsStyle.opacity,
            controlsTransform: controlsStyle.transform,
            controlsAnimation: controlsStyle.animationName,
          });
          document.documentElement.dataset.translayMotionFrames =
            JSON.stringify(frames);
        }
      };

      applyLatestPayload({
        ...waitingPayload,
        requestId: 42,
        phase: "translated",
        success: true,
        text:
          motionPreview === "motion-compact"
            ? "简短译文"
            : "翻译应当像系统原本就具备的能力一样自然存在，不打断阅读，也不要求用户切换窗口。真正好的动效会让译文从原位安静地成形。",
        errorMessage: null,
        applicationName: "Microsoft Edge",
        captureMethod: "UI Automation",
        translationMode: "conversational",
      });
      frameTimer = window.setInterval(recordFrame, 16);
      stopTimer = window.setTimeout(() => {
        window.clearInterval(frameTimer);
        recordFrame();
      }, 1_350);
    }, 900);

    return () => {
      window.clearTimeout(transitionTimer);
      window.clearInterval(frameTimer);
      window.clearTimeout(stopTimer);
      delete debugWindow.__translayMotionFrames;
    };
  }, [applyLatestPayload]);

  useEffect(() => {
    if (
      !import.meta.env.DEV ||
      new URLSearchParams(window.location.search).get("preview") !== "dismiss"
    ) {
      return;
    }

    const frames: Array<{
      elapsedMs: number;
      state: string;
      surfaceOpacity: string;
      surfaceTransform: string;
      surfaceAnimation: string;
      surfaceLeft: number;
      surfaceTop: number;
      surfaceWidth: number;
      surfaceHeight: number;
      contentOpacity: string;
      contentTransform: string;
    }> = [];
    document.documentElement.dataset.translayDismissFrames = "[]";
    let frameTimer: number | undefined;
    const startedAt = performance.now();
    const recordFrame = () => {
      const root = overlayRef.current;
      const surface = root?.querySelector<HTMLElement>(".overlay-surface");
      const content = root?.querySelector<HTMLElement>(".translation");
      if (!root || !surface || !content) return;
      const surfaceStyle = window.getComputedStyle(surface);
      const contentStyle = window.getComputedStyle(content);
      const surfaceRect = surface.getBoundingClientRect();
      frames.push({
        elapsedMs: Math.round(performance.now() - startedAt),
        state: root.dataset.motionState ?? "",
        surfaceOpacity: surfaceStyle.opacity,
        surfaceTransform: surfaceStyle.transform,
        surfaceAnimation: surfaceStyle.animationName,
        surfaceLeft: Math.round(surfaceRect.left),
        surfaceTop: Math.round(surfaceRect.top),
        surfaceWidth: Math.round(surfaceRect.width),
        surfaceHeight: Math.round(surfaceRect.height),
        contentOpacity: contentStyle.opacity,
        contentTransform: contentStyle.transform,
      });
      document.documentElement.dataset.translayDismissFrames =
        JSON.stringify(frames);
    };
    frameTimer = window.setInterval(recordFrame, 8);
    const dismissTimer = window.setTimeout(() => {
      dispatchMotion({
        type: "dismiss",
        requestId: payloadRef.current.requestId,
        reason: "manual",
      });
    }, 120);
    const stopTimer = window.setTimeout(() => {
      window.clearInterval(frameTimer);
      recordFrame();
    }, 940);

    return () => {
      window.clearInterval(frameTimer);
      window.clearTimeout(dismissTimer);
      window.clearTimeout(stopTimer);
      delete document.documentElement.dataset.translayDismissFrames;
    };
  }, []);

  useEffect(
    () => () => window.clearTimeout(departingStatusTimerRef.current),
    [],
  );

  useEffect(
    () => () => {
      explanationSequenceRef.current += 1;
      clearExplanationSchedule();
    },
    [clearExplanationSchedule],
  );

  const acknowledgeDeliveredCapture = useCallback((requestId: number) => {
    if (pendingRevealAcknowledgementRef.current === requestId) {
      // Delivery is complete, but reading time must begin only after the
      // materialize sequence has settled.
      return Promise.resolve(true);
    }
    return acknowledgeCapture(requestId);
  }, []);

  const syncLatestCapture = useCallback(
    () =>
      synchronizeLatestCapture({
        getLatest: getLatestCapture,
        apply: applyLatestPayload,
        acknowledge: acknowledgeDeliveredCapture,
        onError: reportDeliveryError,
      }),
    [
      acknowledgeDeliveredCapture,
      applyLatestPayload,
      reportDeliveryError,
    ],
  );

  const startPolling = useCallback(() => {
    if (pollingRef.current) return;
    let cancel: (() => void) | undefined;
    cancel = startBoundedPolling(syncLatestCapture, {
      intervalMs: 75,
      durationMs: 1_500,
      onStop: () => {
        if (pollingRef.current === cancel) pollingRef.current = undefined;
      },
    });
    pollingRef.current = cancel;
  }, [syncLatestCapture]);

  useEffect(() => {
    if (previewRef.current) return;
    let disposed = false;
    let unlistenCapture: UnlistenFn | undefined;
    let unlistenDismiss: UnlistenFn | undefined;
    let detachLifecycle: (() => void) | undefined;

    const synchronizeAndPoll = () => {
      if (disposed) return;
      void syncLatestCapture();
      startPolling();
    };
    const initialize = async () => {
      try {
        const disposeListener = await onCaptureResult(synchronizeAndPoll);
        if (disposed) {
          disposeListener();
          return;
        }
        unlistenCapture = disposeListener;
        const disposeDismissListener = await onOverlayDismissRequested(
          (requestId) => {
            if (
              Number.isSafeInteger(requestId) &&
              requestId === payloadRef.current.requestId
            ) {
              const explanation = explanationStateRef.current;
              if (
                copyHoldRef.current === requestId ||
                interactingRef.current ||
                overlayRef.current?.matches(":hover") ||
                (explanation.requestId === requestId &&
                  explanation.phase !== "closed")
              ) {
                void setOverlayHovered(requestId, true);
                return;
              }
              dispatchMotion({
                type: "dismiss",
                requestId,
                reason: "automatic",
              });
            }
          },
        );
        if (disposed) {
          disposeDismissListener();
          return;
        }
        unlistenDismiss = disposeDismissListener;
        await notifyOverlayFrontendReady();
        detachLifecycle = attachLifecycleSynchronization(
          document,
          window,
          synchronizeAndPoll,
        );

        // Listener first, getter second. Polling covers the case where this
        // initial getter is null and all later events are dropped while the
        // hidden WebView is suspended.
        synchronizeAndPoll();
      } catch (error) {
        reportDeliveryError("initialize", error);
      }
    };

    void initialize();
    return () => {
      disposed = true;
      unlistenCapture?.();
      unlistenDismiss?.();
      detachLifecycle?.();
      pollingRef.current?.();
      pollingRef.current = undefined;
    };
  }, [reportDeliveryError, startPolling, syncLatestCapture]);

  const dismiss = useCallback(() => {
    const requestId = payloadRef.current.requestId;
    if (requestId > 0) {
      if (pendingRevealAcknowledgementRef.current === requestId) {
        pendingRevealAcknowledgementRef.current = null;
      }
      dispatchMotion({
        type: "dismiss",
        requestId,
        reason: "manual",
      });
    }
  }, []);

  const setHovered = useCallback((hovered: boolean) => {
    interactingRef.current = hovered;
    const requestId = payloadRef.current.requestId;
    if (requestId > 0) {
      if (
        hovered &&
        motionStateRef.current.requestId === requestId &&
        motionStateRef.current.phase === "dismissing" &&
        motionStateRef.current.dismissReason === "automatic"
      ) {
        dispatchMotion({ type: "cancel-dismiss", requestId });
      }
      const explanation = explanationStateRef.current;
      const keepVisible =
        copyHoldRef.current === requestId ||
        (explanation.requestId === requestId && explanation.phase !== "closed");
      void setOverlayHovered(requestId, keepVisible ? true : hovered);
    }
  }, []);

  const completeDismiss = useCallback(async (requestId: number) => {
    const current = motionStateRef.current;
    if (
      current.requestId !== requestId ||
      current.phase !== "dismissing" ||
      dismissCompletionRequestRef.current === requestId
    ) {
      return;
    }
    dismissCompletionRequestRef.current = requestId;
    try {
      const hidden = await dismissOverlay(requestId);
      // The native reply can arrive after a different selection has opened.
      if (payloadRef.current.requestId !== requestId) return;
      if (hidden) {
        resetExplanationState();
        dispatchMotion({ type: "hidden", requestId });
      } else {
        dismissCompletionRequestRef.current = null;
        dispatchMotion({ type: "settle-immediately", requestId });
      }
    } catch (error) {
      if (payloadRef.current.requestId !== requestId || previewRef.current) {
        return;
      }
      dismissCompletionRequestRef.current = null;
      console.error("[Translay] native overlay dismissal failed", error);
      dispatchMotion({ type: "settle-immediately", requestId });
    }
  }, [resetExplanationState]);

  const retry = useCallback(() => {
    const current = payloadRef.current;
    if (retryRequestRef.current === current.requestId ||
        !["captureFailed", "translationFailed"].includes(current.phase)) return;
    retryRequestRef.current = current.requestId;
    setRetryRequestId(current.requestId);
    setCopiedRequestId(null);
    setCopyFailedRequestId(null);
    void retryCapture(current.requestId).then(() => syncLatestCapture()).catch(() => {
      if (payloadRef.current.requestId !== current.requestId) return;
      applyLatestPayload({ ...current, errorMessage: "未能开始重试，请再试一次" });
    }).finally(() => {
      if (retryRequestRef.current !== current.requestId) return;
      retryRequestRef.current = null;
      setRetryRequestId(null);
    });
  }, [applyLatestPayload, syncLatestCapture]);

  const copyTranslation = useCallback(async () => {
    const current = payloadRef.current;
    if (current.phase !== "translated" || !current.text) return;
    const attempt = ++copyAttemptRef.current;
    copyHoldRef.current = current.requestId;
    window.clearTimeout(copyHoldTimerRef.current);
    setHovered(interactingRef.current);
    if (motionStateRef.current.dismissReason === "automatic") {
      dispatchMotion({ type: "cancel-dismiss", requestId: current.requestId });
    }
    const stillCurrent = () =>
      payloadRef.current.requestId === current.requestId &&
      copyAttemptRef.current === attempt;
    try {
      await copyTranslationToClipboard(current.text);
      if (!stillCurrent()) return;
      setCopyFailedRequestId(null);
      setCopiedRequestId(current.requestId);
    } catch {
      if (!stillCurrent()) return;
      setCopiedRequestId(null);
      setCopyFailedRequestId(current.requestId);
    } finally {
      if (stillCurrent()) {
        // Give both success and failure feedback a full second after IPC settles.
        copyHoldTimerRef.current = window.setTimeout(() => {
          if (!stillCurrent()) return;
          copyHoldRef.current = null;
          setHovered(interactingRef.current);
        }, 1_000);
      }
    }
  }, [setHovered]);

  const requestExplanation = useCallback(
    (requestId: number, force = false) => {
      const currentPayload = payloadRef.current;
      if (
        currentPayload.requestId !== requestId ||
        !shouldShowExplanationAction(currentPayload)
      ) {
        return;
      }
      const currentLoad = explanationLoadStateRef.current;
      if (!force && !shouldStartExplanationLoad(currentLoad, requestId)) {
        return;
      }

      const sequence = Math.max(Date.now(), explanationLoadSequenceRef.current + 1);
      explanationLoadSequenceRef.current = sequence;
      commitExplanationLoadState(beginExplanationLoad(requestId));
      setExplanationCopiedRequestId(null);
      setExplanationCopyFailedRequestId(null);

      const load = previewRef.current
        ? Promise.resolve(developmentExplanationPreview)
        : explainTranslation(requestId, sequence);
      void load
        .then((content) => {
          if (explanationLoadSequenceRef.current !== sequence) return;
          const currentRequestId = payloadRef.current.requestId;
          if (content === null) {
            const failed = applyExplanationFailure(
              explanationLoadStateRef.current,
              currentRequestId,
              requestId,
              "当前译文已失效，请重新划词",
            );
            if (failed !== explanationLoadStateRef.current) {
              commitExplanationLoadState(failed);
            }
            return;
          }
          const resolved = applyExplanationResult(
            explanationLoadStateRef.current,
            currentRequestId,
            requestId,
            content,
          );
          if (resolved !== explanationLoadStateRef.current) {
            commitExplanationLoadState(resolved);
          }
        })
        .catch((error) => {
          if (explanationLoadSequenceRef.current !== sequence) return;
          const failed = applyExplanationFailure(
            explanationLoadStateRef.current,
            payloadRef.current.requestId,
            requestId,
            explanationErrorMessage(error),
          );
          if (failed !== explanationLoadStateRef.current) {
            commitExplanationLoadState(failed);
          }
        });
    },
    [commitExplanationLoadState],
  );

  const retryExplanation = useCallback(() => {
    const requestId = payloadRef.current.requestId;
    requestExplanation(requestId, true);
  }, [requestExplanation]);

  const copyExplanation = useCallback(async () => {
    const current = explanationLoadStateRef.current;
    if (
      current.status !== "ready" ||
      current.requestId !== payloadRef.current.requestId ||
      current.content === null
    ) {
      return;
    }

    try {
      await copyTranslationToClipboard(formatExplanationForCopy(current.content));
      setExplanationCopyFailedRequestId(null);
      setExplanationCopiedRequestId(current.requestId);
    } catch {
      setExplanationCopiedRequestId(null);
      setExplanationCopyFailedRequestId(current.requestId);
    }
  }, []);

  const completeExplanationReveal = useCallback(
    (requestId: number) => {
      const explanation = explanationStateRef.current;
      if (
        explanation.requestId !== requestId ||
        explanation.phase !== "revealing"
      ) {
        return;
      }
      window.clearTimeout(explanationTimerRef.current);
      explanationTimerRef.current = undefined;
      commitExplanationState({ requestId, phase: "open" });
    },
    [commitExplanationState],
  );

  const completeExplanationCollapse = useCallback(
    (requestId: number) => {
      const explanation = explanationStateRef.current;
      if (
        explanation.requestId !== requestId ||
        (explanation.phase !== "closing" &&
          explanation.phase !== "collapsing") ||
        explanationCollapseRequestRef.current === requestId
      ) {
        return;
      }

      const baseHeight = explanationBaseHeightRef.current;
      explanationCollapseRequestRef.current = requestId;
      clearExplanationSchedule();
      const collapse =
        previewRef.current || baseHeight === null
          ? Promise.resolve(true)
          : fitOverlayHeight(requestId, baseHeight, true);

      void collapse
        .catch((error) => {
          console.error(
            "[Translay] explanation preview collapse failed",
            error,
          );
          return false;
        })
        .finally(() => {
          if (explanationCollapseRequestRef.current !== requestId) return;
          explanationCollapseRequestRef.current = null;
          const latest = explanationStateRef.current;
          if (
            latest.requestId === requestId &&
            (latest.phase === "closing" || latest.phase === "collapsing")
          ) {
            explanationBaseHeightRef.current = null;
            commitExplanationState(closedExplanationState);
            releaseExplanationPin(requestId);
          }
        });
    },
    [
      clearExplanationSchedule,
      commitExplanationState,
      releaseExplanationPin,
    ],
  );

  const closeExplanation = useCallback(() => {
    const explanation = explanationStateRef.current;
    if (
      explanation.requestId === null ||
      explanation.phase === "closed" ||
      explanation.phase === "closing" ||
      explanation.phase === "collapsing"
    ) {
      return;
    }

    cancelPendingExplanation();
    explanationSequenceRef.current += 1;
    clearExplanationSchedule();
    if (explanation.phase === "preparing") {
      commitExplanationState({
        requestId: explanation.requestId,
        phase: "collapsing",
      });
      completeExplanationCollapse(explanation.requestId);
      return;
    }

    const requestId = explanation.requestId;
    commitExplanationState({
      requestId,
      phase: "closing",
    });
    explanationTimerRef.current = window.setTimeout(
      () => completeExplanationCollapse(requestId),
      EXPLANATION_RETURN_SAFETY_TIMEOUT_MS,
    );
  }, [
    cancelPendingExplanation,
    clearExplanationSchedule,
    commitExplanationState,
    completeExplanationCollapse,
  ]);

  const toggleExplanation = useCallback(() => {
    const current = payloadRef.current;
    if (!shouldShowExplanationAction(current)) return;
    const explanation = explanationStateRef.current;
    if (
      explanation.requestId === current.requestId &&
      explanation.phase !== "closed"
    ) {
      closeExplanation();
      return;
    }

    const baseHeight = previewRef.current
      ? Math.min(window.innerHeight, 150)
      : window.innerHeight;
    const sequence = explanationSequenceRef.current + 1;
    explanationSequenceRef.current = sequence;
    clearExplanationSchedule();
    explanationCollapseRequestRef.current = null;
    explanationBaseHeightRef.current = baseHeight;
    commitExplanationState({
      requestId: current.requestId,
      phase: "preparing",
    });
    requestExplanation(current.requestId);

    const fit = previewRef.current
      ? Promise.resolve(true)
      : fitOverlayHeight(
          current.requestId,
          baseHeight + EXPLANATION_PANEL_GAP + EXPLANATION_PANEL_HEIGHT,
          true,
        );
    if (!previewRef.current) {
      void setOverlayHovered(current.requestId, true);
    }

    void fit
      .then((fitted) => {
        const latest = explanationStateRef.current;
        if (
          explanationSequenceRef.current !== sequence ||
          latest.requestId !== current.requestId ||
          latest.phase !== "preparing"
        ) {
          return;
        }
        if (!fitted) {
          resetExplanationState();
          releaseExplanationPin(current.requestId);
          return;
        }

        explanationFirstFrameRef.current = window.requestAnimationFrame(() => {
          overlayRef.current?.getBoundingClientRect();
          explanationSecondFrameRef.current = window.requestAnimationFrame(
            () => {
              const ready = explanationStateRef.current;
              if (
                explanationSequenceRef.current !== sequence ||
                ready.requestId !== current.requestId ||
                ready.phase !== "preparing"
              ) {
                return;
              }
              commitExplanationState({
                requestId: current.requestId,
                phase: "revealing",
              });
              explanationTimerRef.current = window.setTimeout(
                () => completeExplanationReveal(current.requestId),
                EXPLANATION_REVEAL_SAFETY_TIMEOUT_MS,
              );
            },
          );
        });
      })
      .catch((error) => {
        console.error("[Translay] explanation preview expansion failed", error);
        const latest = explanationStateRef.current;
        if (
          explanationSequenceRef.current === sequence &&
          latest.requestId === current.requestId &&
          latest.phase === "preparing"
        ) {
          resetExplanationState();
          releaseExplanationPin(current.requestId);
        }
      });
  }, [
    clearExplanationSchedule,
    closeExplanation,
    commitExplanationState,
    completeExplanationReveal,
    releaseExplanationPin,
    requestExplanation,
    resetExplanationState,
  ]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") dismiss();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dismiss]);

  const isFailure =
    payload.phase === "captureFailed" ||
    payload.phase === "translationFailed";
  const isTranslated = payload.phase === "translated";
  const isLoading =
    payload.phase === "capturing" || payload.phase === "translating";
  const explanationPhase: ExplanationPhase =
    explanationState.requestId === payload.requestId &&
    shouldShowExplanationAction(payload)
      ? explanationState.phase
      : "closed";
  const explanationExpanded = explanationPhase !== "closed";
  const isCompact =
    isTranslated &&
    compactOverflowRequestId !== payload.requestId &&
    !conversationalToneNote(payload) &&
    !payload.text.includes("\n") &&
    Array.from(payload.text).length <= 18;
  useLayoutEffect(() => {
    if (!isCompact || payload.translationMode !== "conversational") return;
    const translation = overlayRef.current?.querySelector<HTMLElement>(".translation");
    if (!translation) return;
    const checkWidth = () => {
      if (translation.clientWidth > 0 && translation.scrollWidth > translation.clientWidth + 1) {
        setCompactOverflowRequestId(payload.requestId);
      }
    };
    checkWidth();
    const observer = new ResizeObserver(checkWidth);
    observer.observe(translation);
    return () => observer.disconnect();
  }, [isCompact, payload.requestId, payload.text, payload.translationMode]);

  const copied = copiedRequestId === payload.requestId;
  const copyFailed = copyFailedRequestId === payload.requestId;
  const activeMotionPhase =
    motionState.requestId === payload.requestId
      ? motionState.phase
      : isLoading
        ? "loading"
        : "settled";
  const targetGeometry = readWindowGeometry(window);
  const revealOrigin = calculateRevealOrigin(
    motionState.requestId === payload.requestId
      ? motionState.loadingGeometry
      : null,
    targetGeometry,
  );
  const materializeTiming = calculateMaterializeTiming(
    revealOrigin,
    targetGeometry,
  );
  const motionStyle = {
    "--motion-origin-left": `${revealOrigin.left - 1}px`,
    "--motion-origin-top": `${revealOrigin.top - 1}px`,
    "--motion-origin-width": `${revealOrigin.width + 2}px`,
    "--motion-origin-height": `${revealOrigin.height + 2}px`,
    "--motion-loading-left": `${revealOrigin.left}px`,
    "--motion-loading-top": `${revealOrigin.top}px`,
    "--motion-loading-width": `${revealOrigin.width}px`,
    "--motion-loading-height": `${revealOrigin.height}px`,
    "--motion-materialize": `${materializeTiming.durationMs}ms`,
    "--motion-delay-content": `${materializeTiming.contentDelayMs}ms`,
    "--motion-delay-controls": `${materializeTiming.controlsDelayMs}ms`,
    "--translation-overlay-height": `${
      explanationBaseHeightRef.current ?? window.innerHeight
    }px`,
  } as CSSProperties;

  useLayoutEffect(() => {
    if (!isLoading || payload.requestId === 0) return;
    if (loadingStartedRef.current?.requestId !== payload.requestId) {
      loadingStartedRef.current = { requestId: payload.requestId, at: performance.now() };
    }
    const geometry = readWindowGeometry(window);
    const previewMode = new URLSearchParams(window.location.search).get(
      "preview",
    );
    const simulatesNativeLoadingWindow =
      import.meta.env.DEV &&
      (previewMode === "motion" || previewMode === "motion-compact");
    loadingGeometryRef.current = {
      requestId: payload.requestId,
      geometry: simulatesNativeLoadingWindow
        ? {
            ...geometry,
            width: LOADING_OVERLAY_WIDTH,
            height: LOADING_OVERLAY_HEIGHT,
          }
        : geometry,
    };
  }, [isLoading, payload.requestId, payload.phase]);

  const measureDesiredHeight = useCallback((): number | null => {
    const overlay =
      overlayRef.current?.querySelector<HTMLElement>(".overlay") ?? null;
    const header = overlay?.querySelector<HTMLElement>(".overlay-header");
    const translation = overlay?.querySelector<HTMLElement>(".translation");
    const status = overlay?.querySelector<HTMLElement>(".status");
    const tools = overlay?.querySelector<HTMLElement>(".tools");
    if (!overlay || !header || !translation || !status || !tools) return null;

    const style = window.getComputedStyle(overlay);
    const verticalChrome =
      Number.parseFloat(style.paddingTop) +
      Number.parseFloat(style.paddingBottom) +
      Number.parseFloat(style.borderTopWidth) +
      Number.parseFloat(style.borderBottomWidth);
    const translationStyle = window.getComputedStyle(translation);

    const measurement = translation.cloneNode(true) as HTMLElement;
    measurement.style.position = "fixed";
    measurement.style.left = "-10000px";
    measurement.style.top = "0";
    measurement.style.width = `${translation.clientWidth}px`;
    measurement.style.height = "auto";
    measurement.style.minHeight = "0";
    measurement.style.flex = "none";
    measurement.style.overflow = "visible";
    measurement.style.visibility = "hidden";
    document.body.appendChild(measurement);
    const contentHeight = measurement.scrollHeight;
    measurement.remove();

    const desiredHeight = isCompact
      ? verticalChrome +
        Math.max(status.offsetHeight, tools.offsetHeight, contentHeight) +
        18
      : verticalChrome +
        header.offsetHeight +
        Number.parseFloat(translationStyle.marginTop) +
        contentHeight +
        18;
    return Math.max(60, Math.min(320, Math.ceil(desiredHeight)));
  }, [isCompact]);

  useLayoutEffect(() => {
    if (
      !ENABLE_STAGED_REVEAL ||
      activeMotionPhase !== "preparing" ||
      payload.requestId === 0
    ) {
      return;
    }

    let measureFrame = 0;
    let firstSettleFrame = 0;
    let secondSettleFrame = 0;
    let cancelled = false;
    const beginReveal = () => {
      dispatchMotion({ type: "reveal", requestId: payload.requestId });
    };

    measureFrame = window.requestAnimationFrame(() => {
      const desiredHeight = measureDesiredHeight();
      const fit =
        desiredHeight !== null &&
        Math.abs(window.innerHeight - desiredHeight) > 2
          ? fitOverlayHeight(payload.requestId, desiredHeight)
          : Promise.resolve(true);

      void fit
        .catch((error) => {
          console.error("[Translay] pre-reveal height fit failed", error);
        })
        .finally(() => {
          if (cancelled) return;
          // The native window has reached its final bounds. Commit two stable
          // transparent frames before materializing the visual surface.
          firstSettleFrame = window.requestAnimationFrame(() => {
            overlayRef.current?.getBoundingClientRect();
            secondSettleFrame = window.requestAnimationFrame(beginReveal);
          });
        });
    });
    const safetyTimer = window.setTimeout(
      beginReveal,
      materializeTiming.durationMs + MATERIALIZE_SAFETY_PADDING_MS,
    );

    return () => {
      cancelled = true;
      window.cancelAnimationFrame(measureFrame);
      window.cancelAnimationFrame(firstSettleFrame);
      window.cancelAnimationFrame(secondSettleFrame);
      window.clearTimeout(safetyTimer);
    };
  }, [
    activeMotionPhase,
    materializeTiming.durationMs,
    measureDesiredHeight,
    payload.requestId,
  ]);

  const finishReveal = useCallback((requestId: number) => {
    dispatchMotion({ type: "settle", requestId });
    if (pendingRevealAcknowledgementRef.current === requestId) {
      pendingRevealAcknowledgementRef.current = null;
      void acknowledgeCapture(requestId);
    }
  }, []);

  useEffect(() => {
    if (activeMotionPhase !== "revealing") return;
    const safetyTimer = window.setTimeout(
      () => finishReveal(payload.requestId),
      materializeTiming.durationMs + MATERIALIZE_SAFETY_PADDING_MS,
    );
    return () => window.clearTimeout(safetyTimer);
  }, [
    activeMotionPhase,
    finishReveal,
    materializeTiming.durationMs,
    payload.requestId,
  ]);

  useEffect(() => {
    if (activeMotionPhase !== "dismissing") return;
    const safetyTimer = window.setTimeout(
      () => void completeDismiss(payload.requestId),
      DISMISS_SAFETY_TIMEOUT_MS,
    );
    return () => window.clearTimeout(safetyTimer);
  }, [activeMotionPhase, completeDismiss, payload.requestId]);

  useLayoutEffect(() => {
    if (
      previewRef.current ||
      payload.requestId === 0 ||
      isLoading ||
      explanationExpanded ||
      activeMotionPhase !== "settled"
    ) {
      return;
    }

    const fitToContent = () => {
      const desiredHeight = measureDesiredHeight();
      if (
        desiredHeight !== null &&
        Math.abs(window.innerHeight - desiredHeight) > 2
      ) {
        // Once readable, keep its current position; native code still clamps
        // the resized surface to the monitor's work area when necessary.
        void fitOverlayHeight(payload.requestId, desiredHeight, true);
      }
    };
    const timers = [
      window.setTimeout(fitToContent, 40),
      window.setTimeout(fitToContent, 220),
    ];

    return () => timers.forEach((timer) => window.clearTimeout(timer));
  }, [
    activeMotionPhase,
    explanationExpanded,
    isLoading,
    measureDesiredHeight,
    payload.phase,
    payload.requestId,
    payload.text,
  ]);

  return (
    <OverlayView
      overlayRef={overlayRef}
      payload={payload}
      activeMotionPhase={activeMotionPhase}
      motionStyle={motionStyle}
      isCompact={isCompact}
      isTranslated={isTranslated}
      isFailure={isFailure}
      isLoading={isLoading}
      retainRetryLayout={retryLayoutRequestRef.current === payload.requestId && isLoading}
      copied={copied}
      copyFailed={copyFailed}
      retryPending={retryRequestId === payload.requestId}
      explanationPhase={explanationPhase}
      explanationLoadState={explanationLoadState}
      explanationCopied={
        explanationCopiedRequestId === payload.requestId
      }
      explanationCopyFailed={
        explanationCopyFailedRequestId === payload.requestId
      }
      departingStatusText={
        departingStatus?.requestId === payload.requestId
          ? departingStatus.text
          : null
      }
      onHoveredChange={setHovered}
      onSurfaceAnimationEnd={(event) => {
        if (event.currentTarget !== event.target) return;
        if (activeMotionPhase === "revealing") {
          finishReveal(payload.requestId);
        } else if (
          activeMotionPhase === "dismissing" &&
          (event.animationName === "surface-air-dissolve" ||
            event.animationName === "content-dismiss-reduced")
        ) {
          void completeDismiss(payload.requestId);
        }
      }}
      onCopy={() => void copyTranslation()}
      onToggleExplanation={toggleExplanation}
      onCloseExplanation={closeExplanation}
      onCopyExplanation={() => void copyExplanation()}
      onRetryExplanation={retryExplanation}
      onExplanationAnimationEnd={(event) => {
        if (event.currentTarget !== event.target) return;
        if (explanationPhase === "revealing") {
          completeExplanationReveal(payload.requestId);
        } else if (explanationPhase === "closing") {
          completeExplanationCollapse(payload.requestId);
        }
      }}
      onRetry={retry}
      onDismiss={dismiss}
    />
  );
}
