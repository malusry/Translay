import "./uiFonts.css";

// Start local loads while the WebView is still initializing. No system install
// or network service is needed, and failed loads retain the CSS fallback.
export const uiFontsReady = Promise.all([
  document.fonts.load('400 13px "TranslayInterface"', "配置"),
  document.fonts.load('400 13px "TranslayDisplay"', "日常配置"),
  document.fonts.load('400 13px "TranslayReading"', "学习"),
]).then(
  () => undefined,
  (error: unknown) => console.warn("[Translay] local UI font loading failed", error),
);
