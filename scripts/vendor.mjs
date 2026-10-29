// Third-party code the site serves itself: Tesseract for the web app (docs/app/vendor/tesseract-<version>/) and the QR
// decoder and the iPhone install card for the phone app (docs/app/phone/vendor/jsqr-<version>/, pwa-install-<version>/). No code or data from a CDN runs in these pages:
// they share one origin, whose storage holds the player's Discord webhook and phone key.
//
//   node scripts/vendor.mjs            download the pinned packages, check them, copy what the pages use
//   node scripts/vendor.mjs --check    check the copied files against each folder's SHA256SUMS (offline)
//   node scripts/vendor.mjs --verify   --check, then download the pinned packages again and compare the copied files
//                                      with theirs byte for byte (CI)
//
// --check alone proves the files and SHA256SUMS agree, which a change to both passes; --verify proves they are what
// npm holds for the pinned integrity, so a vendored file can only change with a version that is on npm.
//
// Each package is fetched with `npm pack --ignore-scripts` (nothing of it runs) and must match the sha512 below,
// which is the npm registry's own integrity for that exact version. Updating: change a version and its integrity
// (`npm view <package>@<version> dist.integrity`), run this, test the page (OCR, scanning), commit the new folder
// and the paths in docs/app/lens.js or docs/app/phone/phone.js. A new version is a new folder: no browser mixes two.
import { createHash } from "node:crypto";
import { execFileSync, execSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const TESSERACT = [
  {
    name: "tesseract.js",
    version: "7.0.0",
    integrity: "sha512-exPBkd+z+wM1BuMkx/Bjv43OeLBxhL5kKWsz/9JY+DXcXdiBjiAch0V49QR3oAJqCaL5qURE0vx9Eo+G5YE7mA==",
    files: { "dist/tesseract.esm.min.js": "tesseract.esm.min.js", "dist/worker.min.js": "worker.min.js", "LICENSE.md": "LICENSE-tesseract.js.md" },
  },
  {
    name: "tesseract.js-core",
    version: "7.0.0",
    integrity: "sha512-WnNH518NzmbSq9zgTPeoF8c+xmilS8rFIl1YKbk/ptuuc7p6cLNELNuPAzcmsYw450ca6bLa8j3t0VAtq435Vw==",
    // The app runs Tesseract with the LSTM engine only; the worker picks one of these by the browser's SIMD support.
    files: {
      "tesseract-core-relaxedsimd-lstm.wasm.js": "tesseract-core-relaxedsimd-lstm.wasm.js",
      "tesseract-core-simd-lstm.wasm.js": "tesseract-core-simd-lstm.wasm.js",
      "tesseract-core-lstm.wasm.js": "tesseract-core-lstm.wasm.js",
      "LICENSE": "LICENSE-tesseract.js-core",
    },
  },
  {
    name: "@tesseract.js-data/eng",
    version: "1.0.0",
    integrity: "sha512-mbTumm6KQPUHyzTPQaF3ObXYnx0SqqfV2nabqFVQBwD6Kl7PhGSLSzOlfFTWy0P3BjghaSKA2W9GB19Jk+ZcTg==",
    // What Tesseract.js fetches for "eng" with the LSTM engine when no langPath is given.
    files: { "4.0.0_best_int/eng.traineddata.gz": "eng.traineddata.gz" },
  },
];
const JSQR = [
  {
    name: "jsqr",
    version: "1.4.0",
    integrity: "sha512-dxLob7q65Xg2DvstYkRpkYtmKm2sPJ9oFhrhmudT1dZvNFFTlroai3AWSpLey/w5vMcLBXRgOJsbXpdN9HzU/A==",
    files: { "dist/jsQR.js": "jsQR.js", "LICENSE": "LICENSE-jsqr" },
  },
];
// The phone app's install card on iPhone, which has no install prompt a page can call: the card littlepolls.com shows,
// with each browser's steps. Self-contained (lit inside); it fetches only the page's own manifest.
const PWA_INSTALL = [
  {
    name: "@khmyznikov/pwa-install",
    version: "0.6.3",
    integrity: "sha512-1GoPVcltXCZgXa7kovAYqodSWNmUW26RaNoF6KbSGfEZ6r6OLwHrUm/+vYFlDUBaIgYHo5Xi301bN4mG51KZRA==",
    files: { "dist/pwa-install.bundle.js": "pwa-install.bundle.js", "LICENSE": "LICENSE-pwa-install" },
  },
];
const DOCS = join(import.meta.dirname, "..", "docs", "app");
const SETS = [
  { out: join(DOCS, "vendor", `tesseract-${TESSERACT[0].version}`), packages: TESSERACT },
  { out: join(DOCS, "phone", "vendor", `jsqr-${JSQR[0].version}`), packages: JSQR },
  { out: join(DOCS, "phone", "vendor", `pwa-install-${PWA_INSTALL[0].version}`), packages: PWA_INSTALL },
];

const sha256 = path => createHash("sha256").update(readFileSync(path)).digest("hex");

function check(OUT) {
  const SUMS = join(OUT, "SHA256SUMS");
  const listed = readFileSync(SUMS, "utf8").trim().split("\n").map(line => line.split(/\s+/));
  const present = readdirSync(OUT).filter(f => f !== "SHA256SUMS").sort();
  const names = listed.map(([, name]) => name).sort();
  if (JSON.stringify(present) !== JSON.stringify(names)) throw new Error(`files differ from SHA256SUMS: ${present} vs ${names}`);
  for (const [sum, name] of listed) {
    if (sha256(join(OUT, name)) !== sum) throw new Error(`${name} does not match SHA256SUMS`);
  }
  console.log(`${listed.length} files match ${SUMS}`);
}

/** Each package fetched from npm and checked against its pinned integrity; `use(pkg, dir)` gets its unpacked folder. */
function fetchPackages(PACKAGES, use) {
  const work = mkdtempSync(join(tmpdir(), "vendor-"));
  try {
    for (const pkg of PACKAGES) {
      const dir = mkdtempSync(join(work, "pkg-"));
      // The name and version are this file's constants, never input.
      const tarball = execSync(`npm pack ${pkg.name}@${pkg.version} --ignore-scripts --silent`, { cwd: dir }).toString().trim().split("\n").pop();
      const integrity = `sha512-${createHash("sha512").update(readFileSync(join(dir, tarball))).digest("base64")}`;
      if (integrity !== pkg.integrity) throw new Error(`${pkg.name}@${pkg.version}: integrity ${integrity}, expected ${pkg.integrity}`);
      execFileSync("tar", ["xzf", tarball], { cwd: dir });
      use(pkg, join(dir, "package"));
      console.log(`${pkg.name}@${pkg.version}: integrity ok`);
    }
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

function vendor({ out: OUT, packages: PACKAGES }) {
  rmSync(OUT, { recursive: true, force: true });
  mkdirSync(OUT, { recursive: true });
  const sums = [];
  fetchPackages(PACKAGES, (pkg, dir) => {
    for (const [from, to] of Object.entries(pkg.files)) {
      copyFileSync(join(dir, from), join(OUT, to));
      sums.push(`${sha256(join(OUT, to))}  ${to}`);
    }
  });
  writeFileSync(join(OUT, "SHA256SUMS"), `${sums.sort((a, b) => a.slice(66).localeCompare(b.slice(66))).join("\n")}\n`);
  check(OUT);
}

function verify({ out: OUT, packages: PACKAGES }) {
  check(OUT);
  const expected = PACKAGES.flatMap(pkg => Object.values(pkg.files)).sort();
  const present = readdirSync(OUT).filter(f => f !== "SHA256SUMS").sort();
  if (JSON.stringify(present) !== JSON.stringify(expected)) throw new Error(`${OUT} holds ${present}, the packages give ${expected}`);
  fetchPackages(PACKAGES, (pkg, dir) => {
    for (const [from, to] of Object.entries(pkg.files)) {
      if (!readFileSync(join(dir, from)).equals(readFileSync(join(OUT, to)))) {
        throw new Error(`${to} is not ${pkg.name}@${pkg.version}'s ${from}`);
      }
    }
  });
  console.log(`${OUT}: every file is its pinned package's`);
}

for (const set of SETS) {
  if (process.argv.includes("--check")) check(set.out);
  else if (process.argv.includes("--verify")) verify(set);
  else vendor(set);
}
