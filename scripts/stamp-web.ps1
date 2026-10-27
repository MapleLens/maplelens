# Stamp the web app's own files with `?v=<hash>` so a browser never mixes a cached old file with a new one (GitHub
# Pages lets browsers cache for 10 minutes; old JavaScript glue with a new engine would not start).
#
# The hash covers the app's JavaScript, the engine's glue and the shared stylesheet, with the stamps themselves left out, so it only
# changes when they do and is the same on every machine (the .wasm bytes are not: Rust embeds build paths). The
# glue changes whenever the engine's API does, which moves the .wasm's stamp along with it.
# `./scripts/build-web.ps1` runs this; CI checks the stamps are current.
$ErrorActionPreference = 'Stop'
$app = Join-Path $PSScriptRoot '..\docs\app'
$stamp = '\?v=[0-9a-f]{12}'
# What the stamp is a hash of; the site's home page and privacy page only carry it (their copy changes should not need a re-stamp).
$hashed = 'index.html', 'app.js', 'cards.js', 'cardfeed.js', 'rankpanel.js', 'prices.js', 'phone/index.html', 'phone/phone.js', 'phone/relay.js', 'phone/pull.js', 'storebook.js',
    'mapleqr.js', 'lens.js', 'worker.js', 'pkg/maplelens_web.js', '../assets/maple.css', '../assets/site.js', '../assets/demo.js'
$files = $hashed + '../index.html', '../privacy/index.html'
$text = @{}
foreach ($file in $files) { $text[$file] = [IO.File]::ReadAllText((Join-Path $app $file)) -replace "`r`n", "`n" }

$sha = [Security.Cryptography.SHA256]::Create()
$joined = ($hashed | ForEach-Object { $text[$_] -replace $stamp, '' }) -join "`0"
$hash = (-join ($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($joined)) | ForEach-Object { $_.ToString('x2') })).Substring(0, 12)

# Each link in the chain: index.html -> app.js -> lens.js -> worker.js -> glue + .wasm (the site's demo loads lens.js),
# and the phone app's page -> phone.js -> relay.js (the service worker is not stamped: its address is its identity).
$links = @{
    '../index.html' = @('assets/site\.js', 'assets/demo\.js', 'maple\.css')
    '../privacy/index.html' = @('maple\.css')
    '../assets/demo.js' = @('\.\./app/lens\.js')
    'phone/index.html' = @('"phone\.js')
    'phone/phone.js' = @('\./relay\.js', '\./pull\.js')
    'index.html' = @('app\.js', 'lens\.js', 'maple\.css')
    'app.js'     = @('\./lens\.js', '\./cards\.js', '\./cardfeed\.js', '\./prices\.js', '\./phone/relay\.js', '\./storebook\.js', '\./mapleqr\.js')
    'cards.js'   = @('\./lens\.js', '\./cardfeed\.js', '\./rankpanel\.js')
    'lens.js'    = @('"worker\.js')
    'worker.js'  = @('\./pkg/maplelens_web\.js', '\./pkg/maplelens_web_bg\.wasm')
}
foreach ($file in $links.Keys) {
    $updated = $text[$file]
    foreach ($link in $links[$file]) {
        $updated = [regex]::Replace($updated, "($link)($stamp)?", "`$1?v=$hash")
    }
    if ($updated -ne $text[$file]) {
        [IO.File]::WriteAllText((Join-Path $app $file), $updated)
    }
}
Write-Host "docs/app stamped v=$hash"
