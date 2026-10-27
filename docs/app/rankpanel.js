// The web app's rank panel: who the Character Info window shows (classicrank.com's picture of the character, the name,
// level and job) and their three ranks, drawn in HTML in the card view. The Windows app draws its own beside the game's
// window, which shows the name and the character already; the floating window has neither beside it, and the picture
// is how the player sees that the name was read right.

/** Only pictures and links from classicrank.com: the answer is another site's, and the page's policy allows no other. */
const CLASSICRANK = "https://classicrank.com/";
/** ClassicRank asks for a visible link wherever its data is shown. */
const HOME = "https://classicrank.com";

/** The look, added to the card view's sheet. Nothing wraps, so the panel's height does not depend on its width. */
export const RANK_CSS = `
.rk{box-sizing:border-box;display:flex;flex-direction:column;gap:9px;padding:10px;border-radius:9px;border:1px solid #767896;
  background:#222550;box-shadow:inset 0 0 0 1px #a0a2be33,0 6px 12px #0007;color:#eef3ff;font:bold 13px Tahoma,Verdana,sans-serif}
.rk-who{display:flex;align-items:center;gap:11px;min-width:0}
.rk-photo{position:relative;flex:none;width:72px;height:72px;border-radius:8px;overflow:hidden;display:grid;place-items:center;
  background:radial-gradient(circle at 50% 62%,#46559a,#1a2148 72%);border:1px solid #4a5390}
.rk-photo img{position:absolute;left:50%;top:50%;width:104px;height:104px;transform:translate(-50%,-50%);image-rendering:pixelated}
.rk-photo span{font-size:26px;color:#9fb0d8}
.rk[data-status=loading] .rk-photo{animation:rk-wait 1.2s ease-in-out infinite alternate}
@keyframes rk-wait{from{opacity:.55}to{opacity:1}}
.rk-id{min-width:0;display:flex;flex-direction:column;gap:3px}
.rk-id > *{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.rk-name{font-size:17px;color:#fff}
.rk-job{font-size:12px;font-weight:normal;color:#c9d5fb}
.rk-job b{color:#ffd34d}
.rk-note{font-size:12px;font-weight:normal;color:#9fb0d8}
.rk-tiles{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:6px}
.rk-tile{min-width:0;padding:6px 2px 5px;border-radius:7px;background:#161b44;border:1px solid #333c78;text-align:center}
.rk-tile b{display:block;font-size:13px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.rk-tile span{display:block;margin-top:2px;font-size:9.5px;letter-spacing:.08em;text-transform:uppercase;color:#9fb0d8}
.rk-tile.top b{color:#ffd34d}
.rk-tile.bot{grid-column:1/-1}.rk-tile.bot b{color:#ffb060}
.rk-foot{display:flex;justify-content:space-between;gap:8px;padding-top:6px;border-top:1px solid #c8cceb22;
  font-size:10.5px;font-weight:normal;color:#8394c4;white-space:nowrap}
.rk-foot a{color:#9fc4ff;text-decoration:none}
.rk-foot a:hover{text-decoration:underline}
@media (prefers-reduced-motion:reduce){.rk[data-status=loading] .rk-photo{animation:none}}
`;

const NOTES = {
  loading: "Looking up rank…",
  missing: "Not ranked on classicrank.com yet",
  failed: "Rank unavailable · retrying shortly",
};

/** `#12,345`, or a dash when the API has none. */
function place(value) {
  return Number.isFinite(value) ? `#${value.toLocaleString("en-US")}` : "—";
}

/** "12h ago", as the cards word it. */
export function ago(iso, now = Date.now()) {
  const at = Date.parse(iso);
  if (!Number.isFinite(at)) return "";
  const s = Math.max(0, (now - at) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

function ours(url) {
  return typeof url === "string" && url.startsWith(CLASSICRANK) ? url : "";
}

/** The panel for `info`, the engine's `rank_about` ({read, status, rank}), as an element of `doc`. */
export function rankPanel(doc, info) {
  const el = (tag, className = "", props = {}) => Object.assign(doc.createElement(tag), { className, ...props });
  const rank = info.rank;
  const name = rank?.name || info.read || "";
  const root = el("section", "rk");
  root.dataset.status = info.status;

  const photo = el("div", "rk-photo");
  const initial = el("span", "", { textContent: name.slice(0, 1).toUpperCase() });
  const src = ours(rank?.photo);
  if (src) {
    const img = el("img", "", { src, alt: name, referrerPolicy: "no-referrer" });
    img.addEventListener("error", () => img.replaceWith(initial), { once: true });
    photo.append(img);
  } else photo.append(initial);

  const id = el("div", "rk-id");
  id.append(el("b", "rk-name", { textContent: name }));
  if (rank?.level || rank?.job) {
    const job = el("span", "rk-job");
    if (rank.level) job.append(el("b", "", { textContent: `Lv. ${rank.level}` }));
    const rest = [rank.job, Number.isFinite(rank.fame) ? `Fame ${rank.fame}` : ""].filter(Boolean).join(" · ");
    if (rest) job.append(`${rank.level ? " · " : ""}${rest}`);
    id.append(job);
  }
  if (!rank) id.append(el("span", "rk-note", { textContent: NOTES[info.status] || NOTES.loading }));
  const who = el("div", "rk-who");
  who.append(photo, id);
  root.append(who);

  if (rank) {
    const tile = (value, label, kind = "") => {
      const t = el("div", `rk-tile ${kind}`);
      t.append(el("b", "", { textContent: place(value) }), el("span", "", { textContent: label }));
      return t;
    };
    const tiles = el("div", "rk-tiles");
    // The API reuses overall_rank for the Botters list, so job and fame ranks would mislead.
    if (rank.botting) tiles.append(tile(rank.overall_rank, "Botter rank · flagged as botting", "bot"));
    else tiles.append(tile(rank.overall_rank, "World", "top"), tile(rank.job_rank, "Job"), tile(rank.fame_rank, "Fame"));
    root.append(tiles);
  }

  const foot = el("div", "rk-foot");
  const updated = rank?.last_checked ? ago(rank.last_checked) : "";
  foot.append(el("span", "", { textContent: updated ? `updated ${updated}` : "ranks from" }));
  foot.append(el("a", "", { href: ours(rank?.profile_url) || HOME, target: "_blank", rel: "noopener", textContent: "classicrank.com ↗" }));
  root.append(foot);
  return root;
}
