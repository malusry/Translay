export interface ScreenRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface LanguageProfile {
  languageHint: string | null;
  dominantScript: string | null;
  mixedScripts: boolean;
  requiresProviderDetection: boolean;
}

export type TranslationMode = "conversational" | "academic";

// Frontend payload contract emitted by the Rust capture pipeline.
export type CapturePhase =
  | "waiting"
  | "capturing"
  | "translating"
  | "translated"
  | "captureFailed"
  | "translationFailed";

export interface CapturePayload {
  requestId: number;
  phase: CapturePhase;
  success: boolean;
  text: string;
  applicationName: string;
  processId: number;
  captureMethod: string;
  elapsedMs: number;
  selectionRect: ScreenRect | null;
  errorCode: string | null;
  errorMessage: string | null;
  focusPreserved: boolean;
  clipboardRestored: boolean | null;
  warningCode: string | null;
  languageProfile: LanguageProfile | null;
  translationMode: TranslationMode | null;
  toneNote?: string | null;
}

export interface ExplanationTerm {
  term: string;
  translation: string;
  explanation: string;
}

export interface ExplanationContent {
  coreExplanation: string;
  keyConcepts: ExplanationTerm[];
  caveat: string;
}
