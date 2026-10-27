// Third-party code the site serves itself: Tesseract for the web app (docs/app/vendor/tesseract-<version>/) and the QR
// decoder for the phone app (docs/app/phone/vendor/jsqr-<version>/). No code or data from a CDN runs in these pages:
// they share one origin, whose storage holds the player's Discord webhook and phone key.
//
//   node scripts/vendor.mjs           download the pinned packages, check them, copy what the pages use
//   node scripts/vendor.mjs --check   check the copied files against each folder's SHA256SUMS (CI)
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
const DOCS = join(import.meta.dirname, "..", "docs", "app");
const SETS = [
  { out: join(DOCS, "vendor", `tesseract-${TESSERACT[0].version}`), packages: TESSERACT },
  { out: join(DOCS, "phone", "vendor", `jsqr-${JSQR[0].version}`), packages: JSQR },
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

function vendor({ out: OUT, packages: PACKAGES }) {
  const work = mkdtempSync(join(tmpdir(), "vendor-"));
  try {
    rmSync(OUT, { recursive: true, force: true });
    mkdirSync(OUT, { recursive: true });
    const sums = [];
    for (const pkg of PACKAGES) {
      const dir = mkdtempSync(join(work, "pkg-"));
      // The name and version are this file's constants, never input.
      const tarball = execSync(`npm pack ${pkg.name}@${pkg.version} --ignore-scripts --silent`, { cwd: dir }).toString().trim().split("\n").pop();
      const integrity = `sha512-${createHash("sha512").update(readFileSync(join(dir, tarball))).digest("base64")}`;
      if (integrity !== pkg.integrity) throw new Error(`${pkg.name}@${pkg.version}: integrity ${integrity}, expected ${pkg.integrity}`);
      execFileSync("tar", ["xzf", tarball], { cwd: dir });
      for (const [from, to] of Object.entries(pkg.files)) {
        copyFileSync(join(dir, "package", from), join(OUT, to));
        sums.push(`${sha256(join(OUT, to))}  ${to}`);
      }
      console.log(`${pkg.name}@${pkg.version}: integrity ok`);
    }
    writeFileSync(join(OUT, "SHA256SUMS"), `${sums.sort((a, b) => a.slice(66).localeCompare(b.slice(66))).join("\n")}\n`);
    check(OUT);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

for (const set of SETS) {
  if (process.argv.includes("--check")) check(set.out);
  else vendor(set);
}
