// Pull to refresh, drawn by the page: the browser's own is off (`overscroll-behavior`), since an installed app has
// none on Android and iPhone's rubber band showed a blank white page above the bar. The page slides down over the
// sky and a meso spins while the store is asked again.

/** How far the finger must pull (after damping) for a release to refresh. */
const TRIGGER_PX = 72;
/** The furthest the page follows the finger. */
const MAX_PX = 128;
/** Where the page rests while refreshing: room for the coin. */
const HOLD_PX = 64;
/** The finger moves this much further than the page: the pull feels like a spring, not a drag. */
const DAMPING = 0.5;
/** A refresh shows its coin at least this long, so a fast answer still reads as "refreshed". */
const MIN_SPIN_MS = 650;

/**
 * Pulling `content` down from the top of the page runs `action` (awaited). `allowed()` says whether a pull may start
 * now (not while a sheet or the camera is up).
 */
export function pullToRefresh(content, indicator, action, allowed = () => true) {
  const label = indicator.querySelector(".ptr-label");
  let startY = null;
  let pull = 0;
  let busy = false;

  const place = (px, animate) => {
    content.style.transition = indicator.style.transition = animate ? "transform .28s cubic-bezier(.2,.9,.3,1.2)" : "none";
    content.style.transform = px ? `translateY(${px}px)` : "";
    indicator.style.transform = `translate(-50%, ${px - 52}px)`;
    indicator.style.setProperty("--turn", `${Math.round(px * 4)}deg`);
    indicator.style.opacity = String(Math.min(1, px / (TRIGGER_PX * 0.6)));
  };

  addEventListener("touchstart", event => {
    if (busy || event.touches.length !== 1 || scrollY > 0 || !allowed()) return;
    startY = event.touches[0].clientY;
    pull = 0;
    // Pinned to the bar's bottom edge: the coin comes out from under it.
    indicator.style.top = `${document.querySelector(".bar").getBoundingClientRect().bottom}px`;
  }, { passive: true });

  addEventListener("touchmove", event => {
    if (startY === null) return;
    const dy = event.touches[0].clientY - startY;
    if (dy <= 0 || scrollY > 0) {
      if (pull) place(0, false);
      pull = 0;
      return;
    }
    // Ours from here on: the page must not scroll or bounce under the finger.
    event.preventDefault();
    pull = Math.min(MAX_PX, dy * DAMPING);
    indicator.hidden = false;
    indicator.classList.toggle("ready", pull >= TRIGGER_PX);
    label.textContent = pull >= TRIGGER_PX ? "Release to refresh" : "Pull to refresh";
    place(pull, false);
  }, { passive: false });

  const release = async () => {
    if (startY === null) return;
    startY = null;
    if (pull < TRIGGER_PX) return settle();
    busy = true;
    indicator.classList.remove("ready");
    indicator.classList.add("spinning");
    label.textContent = "Refreshing…";
    place(HOLD_PX, true);
    const shown = new Promise(resolve => setTimeout(resolve, MIN_SPIN_MS));
    try {
      await Promise.all([action(), shown]);
    } finally {
      indicator.classList.remove("spinning");
      busy = false;
      settle();
    }
  };
  addEventListener("touchend", release);
  addEventListener("touchcancel", release);

  function settle() {
    pull = 0;
    place(0, true);
    setTimeout(() => { if (!pull && !busy) indicator.hidden = true; }, 300);
  }
}
