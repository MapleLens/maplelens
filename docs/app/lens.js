// The page side of MapleLens in the browser: starts the engine worker, feeds it frames (dropping frames while
// it is busy, so it never falls behind), keeps learnt letters in localStorage, and runs the OCR fallback.

const base = new URL(".", import.meta.url);
const FONTS = ["fonts/DejaVuSansCondensed.ttf", "fonts/DejaVuSansCondensed-Bold.ttf"].map(f => new URL(f, base).href);
/** Tesseract (the browser's OCR engine), served from this site (scripts/vendor.mjs: packages checked against
 *  the npm registry's integrity, files against SHA256SUMS). Never from a CDN: whatever runs in this page can read the
 *  Discord webhook, the Telegram token and the phone key it keeps. A new version is a new folder, so no browser mixes two. */
const TESSERACT_DIR = new URL("vendor/tesseract-7.0.0/", base).href;
/** Jobs the page keeps waiting for the OCR engine: a title or a name goes stale in a moment, but every line of a
 *  shop's list is needed (a burst of sales is three lines each), so none of those is dropped. */
const MAX_OCR_QUEUE = { title: 4, name: 4, map: 4, shop: 48 };
const GLYPHS_KEY = font => `maplelens.glyphs.${font}`;
/** The training sessions (the one being played and the history), kept in this browser. */
const TRAINING_KEY = "maplelens.training";

function storage(action) {
  try { return action(window.localStorage); } catch { return undefined; } // private windows may refuse
}

export class Lens {
  /**
   * @param {object} options
   * @param {(state: object, fps: number, ms: number, grab: number) => void} options.onState  every processed frame (ms: the
   *   engine's time on it; grab: the worker's, turning the bitmap into pixels)
   * @param {(which: "card"|"rank"|"training", image: ImageData, about: object) => void} options.onCard
   *   a card was redrawn; `about` says what it shows (an item card: {name, price, trend}; the rank panel: {read,
   *   status, rank}, see `Engine::rank_about`; the training card: nothing)
   * @param {(history: object[]) => void} [options.onTrainingHistory]  the finished training sessions, oldest first
   * @param {(message: string) => void} [options.onError]
   * @param {boolean} [options.ocr]  allow downloading the OCR fallback (~10 MB, once)
   */
  static create(options) {
    return new Promise((resolve, reject) => {
      const lens = new Lens(options, resolve, reject);
      lens.worker.postMessage({
        type: "init",
        fonts: FONTS,
        cardScale: window.devicePixelRatio || 1,
        ocr: Boolean(options.ocr),
        glyphs: Object.fromEntries(["title", "name"].map(f => [f, storage(s => s.getItem(GLYPHS_KEY(f)))]).filter(([, v]) => v)),
        training: storage(s => s.getItem(TRAINING_KEY)) || "",
      });
    });
  }

  constructor(options, resolve, reject) {
    this.options = options;
    this.busy = false;
    this.idleWaiters = [];
    this.ocrEnabled = Boolean(options.ocr);
    this.ocrQueue = [];
    this.testResolvers = [];
    this.qrResolvers = [];
    this.catalogWaiters = new Map();
    this.catalogCount = 0;
    this.tesseract = null;
    this.tesseractLoading = null;
    this.worker = new Worker(new URL("worker.js?v=fe227e15edf6", base), { type: "module" });
    this.worker.onerror = event => reject(new Error(event.message || "the engine could not start"));
    this.counts = {};
    this.bytes = 0;
    this.worker.onmessage = ({ data }) => {
      this.counts[data.type] = (this.counts[data.type] || 0) + 1;
      if (data.json) this.bytes += data.json.length;
      switch (data.type) {
        case "ready": this.version = data.version; this.items = data.items; if (this.ocrEnabled) this.loadTesseract(); resolve(this); break;
        case "idle": this.busy = false; this.idleWaiters.splice(0).forEach(resolve => resolve()); break;
        case "state": options.onState(data.state, data.fps, data.ms, data.grab); break;
        case "card": options.onCard(data.which, new ImageData(new Uint8ClampedArray(data.pixels.buffer), data.width, data.height), data.about); break;
        case "glyphs": storage(s => s.setItem(GLYPHS_KEY(data.font), data.json)); break;
        case "training-save": storage(s => s.setItem(TRAINING_KEY, data.json)); options.onTrainingHistory?.(data.history); break;
        case "ocr-job": this.queueOcr(data); break;
        case "notify-status": options.onNotify?.(data); break;
        case "catalog-loaded": this.catalogWaiters.get(data.id)?.(data); this.catalogWaiters.delete(data.id); break;
        case "notify-test-result": this.testResolvers.shift()?.(data); break;
        case "phone-qr": this.qrResolvers.shift()?.(data.modules); break;
        case "picture": options.onPicture?.(data.name, data.url); break;
        case "error": (options.onError || console.error)(data.message); break;
      }
    };
  }

  /** Resolves once the engine is ready for the next frame. */
  idle() {
    return this.busy ? new Promise(resolve => this.idleWaiters.push(resolve)) : Promise.resolve();
  }

  /** Offer a frame of the game window (an ImageBitmap or a VideoFrame, handed over); dropped while the previous one is
   *  still being read. */
  frame(bitmap, { inset = 0, scale = 0 } = {}) {
    if (this.busy) {
      bitmap.close();
      return false;
    }
    this.busy = true;
    this.worker.postMessage({ type: "frame", bitmap, inset, scale }, [bitmap]);
    return true;
  }

  /** Hand the engine a MesoWatch price list; resolves to the number of items, rejects when it is not one. The items
   *  named in `unpictured` that the list knows come back as pictures (`onPicture`). */
  loadCatalog(json, unpictured = []) {
    const id = ++this.catalogCount;
    return new Promise((resolve, reject) => {
      this.catalogWaiters.set(id, ({ items, error }) => (error ? reject(new Error(error)) : resolve(items)));
      this.worker.postMessage({ type: "catalog", id, json, unpictured });
    });
  }

  /** Follow the player's Store Permit in the frames (sales, visitors, the store opening and closing). */
  setShop(enabled) {
    this.worker.postMessage({ type: "shop", enabled });
  }

  /** Pause or resume the training session: neither time nor EXP counts while paused. */
  trainingPause(paused) {
    this.worker.postMessage({ type: "training-pause", paused });
  }

  /** End the training session (into the history) and start a new one. */
  trainingNewSession() {
    this.worker.postMessage({ type: "training-new" });
  }

  /** Whether the training card shows the EXP rates. */
  trainingShow(exp) {
    this.worker.postMessage({ type: "training-show", exp });
  }

  /** Report sold items to a Discord webhook, a Telegram bot's sendMessage URL and the phones paired with a key (the
   *  engine ignores anything that is not one): {webhook, telegramWebhook, phone, sales, store}. */
  setNotify(settings) {
    this.worker.postMessage({ type: "notify", ...settings });
  }

  /** Send a test message to "discord", "telegram" or "phone" with {webhook, telegramWebhook, phone}; resolves to
   *  {ok, text, telegramWebhook?}: the whole URL, when a Telegram URL without its chat was completed. */
  testNotify(service, settings) {
    return new Promise(resolve => {
      this.testResolvers.push(resolve);
      this.worker.postMessage({ type: "notify-test", service, ...settings });
    });
  }

  /** The QR code of a phone pairing address: resolves to width² bytes, 1 = dark. */
  phoneQr(url) {
    return new Promise(resolve => {
      this.qrResolvers.push(resolve);
      this.worker.postMessage({ type: "phone-qr", url });
    });
  }

  enableOcr(enabled) {
    this.ocrEnabled = enabled;
    this.worker.postMessage({ type: "ocr-enabled", enabled });
    if (enabled) {
      this.loadTesseract();
      this.runOcr();
    } else this.ocrQueue = [];
  }

  /** Download and start Tesseract (the script, its worker, the WebAssembly core and the English data: about 7 MB, then
   *  cached by the browser). Started as soon as the engine is, so the first unknown text does not wait for it. */
  loadTesseract() {
    this.tesseractLoading ??= (async () => {
      // The ESM build has only a default export (the whole API); a named import finds nothing.
      const module = await import(`${TESSERACT_DIR}tesseract.esm.min.js`);
      const { createWorker } = module.default ?? module;
      // Every path given, so nothing falls back to Tesseract's CDN defaults; the worker loads from its own file, not a
      // blob wrapper (the page's Content Security Policy allows workers from this site only).
      this.tesseract = await createWorker("eng", 1, {
        workerPath: `${TESSERACT_DIR}worker.min.js`,
        corePath: TESSERACT_DIR,
        langPath: TESSERACT_DIR,
        workerBlobURL: false,
      });
      await this.tesseract.setParameters({ tessedit_pageseg_mode: "6" }); // a block of text: one or two title lines
    })();
    this.tesseractLoading.catch(error => {
      (this.options.onError || console.error)(`OCR fallback unavailable: ${error.message || error}`);
      this.ocrEnabled = false;
    });
    return this.tesseractLoading;
  }

  /** Keep the newest few jobs: an older one is for a frame that is long gone. */
  queueOcr(job) {
    if (!this.ocrEnabled) return;
    this.ocrQueue.push(job);
    const limit = MAX_OCR_QUEUE[job.font] ?? 4;
    const same = this.ocrQueue.filter(j => j.font === job.font);
    if (same.length > limit) this.ocrQueue.splice(this.ocrQueue.indexOf(same[0]), 1);
    this.runOcr();
  }

  async runOcr() {
    if (!this.ocrEnabled || this.ocrRunning || !this.ocrQueue.length) return;
    this.ocrRunning = true;
    try {
      await this.loadTesseract();
      while (this.ocrQueue.length) {
        // A tooltip's title or a player's name is wanted now; the minimap's map and the shop's lines can wait.
        const urgent = this.ocrQueue.findIndex(j => j.font === "title" || j.font === "name");
        const job = this.ocrQueue.splice(Math.max(0, urgent), 1)[0];
        // A shop's lines and the minimap's are one line each; a title may take two.
        await this.tesseract.setParameters({ tessedit_pageseg_mode: job.font === "shop" || job.font === "map" ? "7" : "6" });
        const canvas = new OffscreenCanvas(job.width, job.height);
        canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(job.pixels.buffer), job.width, job.height), 0, 0);
        const { data } = await this.tesseract.recognize(await canvas.convertToBlob());
        this.worker.postMessage({ type: "ocr-answer", id: job.id, text: data.text.replace(/\s+/g, " ").trim() });
      }
    } catch (error) {
      (this.options.onError || console.error)(`OCR fallback unavailable: ${error.message || error}`);
      this.ocrEnabled = false;
    } finally {
      this.ocrRunning = false;
    }
  }

  close() {
    this.worker.terminate();
    this.tesseract?.terminate();
  }
}

/** Draw `image` (device pixels) into `canvas`, shown at CSS width = device width / devicePixelRatio, or the
 *  container's width if that is narrower; the height follows the image's proportions. */
export function paint(canvas, image) {
  canvas.width = image.width;
  canvas.height = image.height;
  canvas.style.width = `min(100%, ${image.width / (window.devicePixelRatio || 1)}px)`;
  canvas.style.height = "auto";
  canvas.getContext("2d").putImageData(image, 0, 0);
}
