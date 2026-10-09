export type StartupPhase = "loading" | "ready" | "setup" | "already";
export interface StartupSnapshot { generation: number; phase: StartupPhase; visible: boolean }
export function confirmationDelay(phase: StartupPhase, elapsed: number): number | null {
  if (phase === "loading") return null;
  return Math.max(0, (phase === "already" ? 150 : 1600) - elapsed);
}
export function startupLabel(phase: StartupPhase) {
  return {loading:"正在启动", ready:"已就绪", setup:"待配置", already:"已在运行"}[phase];
}
