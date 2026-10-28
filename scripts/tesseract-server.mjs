// The web app's OCR natively, for scene_bench's browser path (MAPLELENS_TESSERACT=http://127.0.0.1:8799): the page's
// tesseract.js 7.0.0 and its English data (docs/app/vendor/tesseract-7.0.0), the page's page-segmentation modes, behind
// a local endpoint the bench asks synchronously. POST /ocr?psm=6|7 with a PNG body -> text.
//   npm install --prefix <scratch> tesseract.js@7.0.0
//   NODE_PATH=<scratch>/node_modules node scripts/tesseract-server.mjs docs/app/vendor/tesseract-7.0.0 [workers]
// Development only: nothing of it is served by the site.
import http from "node:http";
import { createRequire } from "node:module";
const { createWorker, createScheduler } = createRequire(import.meta.url)(process.env.NODE_PATH + "/tesseract.js");
const LANG = process.argv[2]; // the folder holding eng.traineddata.gz (docs/app/vendor/tesseract-7.0.0)
const N = Number(process.argv[3] || 6);
const schedulers = {};
for (const psm of ["6", "7"]) {
  const s = createScheduler();
  for (let i = 0; i < N; i++) {
    const w = await createWorker("eng", 1, { langPath: LANG, cachePath: LANG + "/../../../../target/tess-cache" });
    await w.setParameters({ tessedit_pageseg_mode: psm });
    s.addWorker(w);
  }
  schedulers[psm] = s;
}
// A worker that fails on one image must not take the server down: the bench treats a failed read as no text.
process.on("uncaughtException", e => console.error("ocr error:", e.message));
http.createServer((req, res) => {
  const chunks = [];
  req.on("data", c => chunks.push(c));
  req.on("end", async () => {
    try {
      const psm = new URL(req.url, "http://x").searchParams.get("psm") === "7" ? "7" : "6";
      if (Buffer.concat(chunks).length < 64) { res.end(""); return; }
      const { data } = await schedulers[psm].addJob("recognize", Buffer.concat(chunks));
      res.end(data.text.replace(/\s+/g, " ").trim());
    } catch (e) { res.statusCode = 500; res.end(String(e)); }
  });
}).listen(8799, "127.0.0.1", () => console.log("ocr ready on 8799"));
