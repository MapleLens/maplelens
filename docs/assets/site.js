// The home page's own script (kept out of the page so its Content Security Policy needs no 'unsafe-inline'): analytics
// and the download buttons.

// GoatCounter: set your site code (the "xxx" in xxx.goatcounter.com). Empty means no analytics are loaded.
const GOATCOUNTER_CODE = "maplelens";
if (GOATCOUNTER_CODE) {
  const s = document.createElement("script");
  s.async = true;
  // GoatCounter's count.v4.js, kept on this site: this origin is the web app's, whose localStorage holds the player's
  // Discord webhook, Telegram token and phone key, so no script from another server runs here.
  s.src = new URL("goatcounter-count.v4.js", document.currentScript.src).href;
  s.dataset.goatcounter = `https://${GOATCOUNTER_CODE}.goatcounter.com/count`;
  document.head.appendChild(s);
  // Download clicks show up as their own events next to page views.
  document.querySelectorAll(".js-dl").forEach(a => a.addEventListener("click", () =>
    window.goatcounter?.count?.({path: "download", title: a.textContent.trim(), event: true})));
}

// Point every download button at the newest Windows zip; the releases page stays as the fallback.
fetch("https://api.github.com/repos/MapleLens/maplelens/releases/latest")
  .then(r => r.ok ? r.json() : Promise.reject(r.status))
  .then(rel => {
    const zip = rel.assets.find(a => /windows\.zip$/i.test(a.name));
    if (!zip) return;
    document.querySelectorAll(".js-dl").forEach(a => a.href = zip.browser_download_url);
    document.querySelectorAll(".js-dl-label").forEach(s => s.textContent = `Download ${rel.tag_name}`);
    const size = zip.size < 10485760 ? `${(zip.size / 1048576).toFixed(1)} MB` : `${Math.round(zip.size / 1048576)} MB`;
    document.querySelectorAll(".js-dl-meta").forEach(s => s.textContent = `${zip.name} · ${size} · Windows 10/11`);
  })
  .catch(() => {});

// On a phone: the steps to connect it to MapleLens on the computer, for the Windows app or the web app.
const connect = document.getElementById("connect");
if (connect) {
  document.querySelectorAll(".js-connect").forEach(b => b.addEventListener("click", () => {
    connect.showModal();
    window.goatcounter?.count?.({path: "connect-phone", title: "Connect to MapleLens", event: true});
  }));
  document.getElementById("connect-close").addEventListener("click", () => connect.close());
  // A tap on the dimmed backdrop closes it, as on every phone.
  connect.addEventListener("click", event => { if (event.target === connect) connect.close(); });
  const ways = connect.querySelectorAll(".seg button");
  ways.forEach(b => b.addEventListener("click", () => {
    ways.forEach(o => o.setAttribute("aria-pressed", String(o === b)));
    connect.querySelectorAll("ol[data-way]").forEach(ol => ol.hidden = ol.dataset.way !== b.dataset.way);
  }));
  // The computer needs MapleLens too: the phone's share sheet sends it there (mail, a chat), else the link is copied.
  const share = document.getElementById("connect-share");
  share.addEventListener("click", async () => {
    const url = "https://maplelens.github.io/maplelens/";
    try {
      if (navigator.share) await navigator.share({title: "MapleLens", url});
      else {
        await navigator.clipboard.writeText(url);
        share.textContent = "Link copied";
      }
    } catch {}
  });
}
