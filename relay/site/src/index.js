// maple.observer: the project's short address, a permanent redirect to the site on GitHub Pages with the same path
// (maple.observer/app -> the web app). A redirect, not a copy: the site's origin holds the player's Discord webhook,
// Telegram token and phone key (AGENTS.md, "The site's origin holds secrets"), and a second origin would hold its own
// set, lost to the first.

const SITE = "https://maplelens.github.io/maplelens";

export default {
  fetch(request) {
    const url = new URL(request.url);
    return new Response(null, {
      status: 301,
      headers: { Location: `${SITE}${url.pathname}${url.search}`, "Cache-Control": "public, max-age=86400" },
    });
  },
};
