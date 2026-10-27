// The live demo: the web app's engine, run on the sample screenshot. Loaded only on request (about 2 MB).
const run = document.getElementById("demo-run");
const status = document.getElementById("demo-status");
run.addEventListener("click", async () => {
  run.disabled = true;
  status.textContent = "loading the engine…";
  const { Lens, paint } = await import("../app/lens.js?v=347aacee3ee9");
  const img = document.querySelector("#demo .frame img");
  // The screenshot is lazy-loaded: a click before it scrolled into view would find no pixels yet.
  img.loading = "eager";
  if (!img.complete) await new Promise((loaded) => img.addEventListener("load", loaded, { once: true }));
  const started = performance.now();
  const lens = await Lens.create({
    onState(state) {
      const k = img.clientWidth / img.naturalWidth;
      const boxes = [];
      if (state.hover?.at) boxes.push({ r: [...state.hover.at, ...state.hover.size], c: "" });
      if (state.info) boxes.push({ r: state.info.rect, c: "info" });
      document.getElementById("demo-boxes").replaceChildren(...boxes.map(({ r: [x, y, w, h], c }) => {
        const el = document.createElement("div");
        el.className = `box ${c}`;
        Object.assign(el.style, { left: `${x * k}px`, top: `${y * k}px`, width: `${w * k}px`, height: `${h * k}px` });
        return el;
      }));
      status.textContent = [state.hover && `read "${state.hover.title}"`, state.info?.name && `read "${state.info.name}"`].filter(Boolean).join(" · ") || "looking…";
    },
    onCard(which, image) {
      const canvas = document.getElementById(`demo-${which}`);
      paint(canvas, image);
      canvas.hidden = false;
      document.querySelector("#demo-out p").hidden = true;
      run.hidden = true;
    },
  });
  // Replay the screenshot for a few seconds: long enough for the glance card to grow and the rank to arrive.
  const bitmap = await createImageBitmap(img);
  const tick = async () => {
    lens.frame(await createImageBitmap(bitmap), { scale: 1 });
    if (performance.now() - started < 6000) setTimeout(tick, 150);
    else { lens.close(); status.textContent = "done: the cards above came from the engine"; }
  };
  tick();
});
