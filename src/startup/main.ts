import { mountStartup } from "./mountStartup";

const dispose = mountStartup(
  document.getElementById("root")!,
  new URLSearchParams(location.search).get("present") === "1",
);
window.addEventListener("pagehide", dispose, { once: true });
