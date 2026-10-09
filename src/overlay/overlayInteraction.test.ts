import { expect, test } from "vitest";
import { observeOverlayInteraction } from "./overlayInteraction";

function setup(hovered = false) {
  const surface = Object.assign(new EventTarget(), { matches: () => hovered });
  const host = new EventTarget();
  const changes: boolean[] = [];
  const stop = observeOverlayInteraction(
    surface as unknown as HTMLElement,
    host as unknown as Window,
    active => changes.push(active),
  );
  const send = (target: EventTarget, type: string, fields = {}) =>
    target.dispatchEvent(Object.assign(new Event(type), fields));
  return { surface, host, changes, stop, send };
}

test("hover protects reading and leaving releases it", () => {
  const t = setup();
  t.send(t.surface, "mouseenter");
  t.send(t.surface, "mouseleave");
  expect(t.changes).toEqual([false, true, false]);
});

test("selection drag remains protected outside until mouse release", () => {
  const t = setup(true);
  t.send(t.surface, "mousedown", { button: 0 });
  t.send(t.surface, "mouseleave");
  expect(t.changes).toEqual([true]);
  t.send(t.host, "mouseup");
  expect(t.changes).toEqual([true, false]);
});

test("releasing selection inside does not remove hover protection", () => {
  const t = setup(true);
  t.send(t.surface, "mousedown", { button: 0 });
  t.send(t.host, "mouseup");
  expect(t.changes).toEqual([true]);
  t.send(t.surface, "mouseleave");
  expect(t.changes).toEqual([true, false]);
});

test("lost mouse release and focus loss cannot leave a stale drag pin", () => {
  const t = setup(true);
  t.send(t.surface, "mousedown", { button: 0 });
  t.send(t.surface, "mouseleave");
  t.send(t.host, "mousemove", { buttons: 0 });
  expect(t.changes).toEqual([true, false]);
  t.send(t.surface, "mouseenter");
  t.send(t.surface, "mousedown", { button: 0 });
  t.send(t.host, "blur");
  expect(t.changes).toEqual([true, false, true, false]);
});

test("disposing a capture removes all listeners", () => {
  const t = setup();
  t.stop();
  t.send(t.surface, "mouseenter");
  t.send(t.surface, "mousedown", { button: 0 });
  t.send(t.host, "mouseup");
  expect(t.changes).toEqual([false]);
});
