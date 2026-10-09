// A retained text highlight is not ongoing interaction. Protect the drag until
// release, including when the pointer leaves the surface with the button down.
export function observeOverlayInteraction(
  surface: HTMLElement,
  host: Window,
  onChange: (active: boolean) => void,
): () => void {
  let inside = surface.matches(":hover");
  let dragging = false;
  let last: boolean | undefined;
  const publish = () => {
    const active = inside || dragging;
    if (active !== last) {
      last = active;
      onChange(active);
    }
  };
  const enter = () => { inside = true; publish(); };
  const leave = () => { inside = false; publish(); };
  const down = (event: MouseEvent) => {
    if (event.button === 0) { dragging = true; publish(); }
  };
  const up = () => { dragging = false; publish(); };
  const move = (event: MouseEvent) => {
    // Recover if release happened outside the WebView and mouseup was lost.
    if (dragging && (event.buttons & 1) === 0) up();
  };
  const blur = () => { inside = false; dragging = false; publish(); };
  surface.addEventListener("mouseenter", enter);
  surface.addEventListener("mouseleave", leave);
  surface.addEventListener("mousedown", down);
  host.addEventListener("mouseup", up);
  host.addEventListener("mousemove", move);
  host.addEventListener("blur", blur);
  publish();
  return () => {
    surface.removeEventListener("mouseenter", enter);
    surface.removeEventListener("mouseleave", leave);
    surface.removeEventListener("mousedown", down);
    host.removeEventListener("mouseup", up);
    host.removeEventListener("mousemove", move);
    host.removeEventListener("blur", blur);
  };
}
