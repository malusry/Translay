(async () => {
  if (window.__translayAcceptanceRunning) return;
  window.__translayAcceptanceRunning = true;
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
  async function until(predicate) {
    const deadline = performance.now() + 5000;
    while (!predicate()) {
      if (performance.now() > deadline) throw new Error("UI did not reach expected phase");
      await wait(40);
    }
  }
  const phase = name => document.querySelector(".overlay-stage")?.classList.contains("explanation-" + name);
  async function record(stage) {
    const controls = [...document.querySelectorAll(".tools button, .explanation-tools button")];
    const controlsInside = controls.every(button => {
      const r = button.getBoundingClientRect();
      return r.width > 0 && r.height > 0 && r.left >= 0 && r.top >= 0 &&
        r.right <= innerWidth + 1 && r.bottom <= innerHeight + 1;
    });
    await invoke("acceptance_checkpoint", {stage,dom:{
      controlsInside, width:innerWidth,height:innerHeight,
      translationScrollTop:document.querySelector(".translation").scrollTop
    }});
  }
  try {
    await until(() => document.querySelector(".overlay-stage")?.dataset.motionState === "settled");
    await wait(350);
    const text = document.querySelector(".translation");
    text.scrollTop = 100;
    const originalScroll = text.scrollTop, originalHeight = innerHeight;
    await record("collapsed-before");
    document.querySelector(".explain-button").click();
    await until(() => phase("open"));
    if (innerHeight <= originalHeight) throw new Error("Explanation did not expand native window");
    if (Math.abs(text.scrollTop-originalScroll)>1) throw new Error("Translation scroll moved on expansion");
    await record("expanded");
    document.querySelector("#explanation-panel .close-button").click();
    await until(() => phase("closed"));
    await wait(350);
    if (Math.abs(innerHeight-originalHeight)>2) throw new Error("Collapsed height was not restored");
    if (Math.abs(text.scrollTop-originalScroll)>1) throw new Error("Translation scroll moved on collapse");
    await record("collapsed-after");
    await invoke("acceptance_finish",{error:null});
  } catch(error) {
    await invoke("acceptance_finish",{error:String(error)});
  }
})();
