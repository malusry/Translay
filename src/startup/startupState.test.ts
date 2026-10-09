import { expect, test } from "vitest";
import { confirmationDelay, startupLabel } from "./startupState";
test("elapsed animation time cannot make an unready app ready", () => {
  expect(confirmationDelay("loading", 9000)).toBeNull();
  expect(confirmationDelay("ready", 200)).toBe(1400);
  expect(confirmationDelay("ready", 4000)).toBe(0);
});
test("missing configuration is distinct from ready and repeat is brief", () => {
  expect(startupLabel("setup")).toBe("待配置");
  expect(confirmationDelay("setup", 500)).toBe(1100);
  expect(confirmationDelay("already", 200)).toBe(0);
});
