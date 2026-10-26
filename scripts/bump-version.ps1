# Set the release version: `./scripts/bump-version.ps1 0.2.1` (or patch / minor / major).
#
# The workspace version in Cargo.toml is the single source of truth (every crate inherits it). Then commit and tag
# `v<version>`; pushing the tag makes GitHub Actions build and publish the release.
param([Parameter(Mandatory = $true)][string]$Version)

$ErrorActionPreference = 'Stop'
$cargo = Join-Path $PSScriptRoot '..\Cargo.toml'
$text = Get-Content $cargo -Raw
$pattern = '(?m)^(\[workspace\.package\][^\[]*?\r?\nversion = ")(\d+)\.(\d+)\.(\d+)(")'
$match = [regex]::Match($text, $pattern)
if (-not $match.Success) { throw 'could not find the [workspace.package] version in Cargo.toml' }
$major, $minor, $patch = [int]$match.Groups[2].Value, [int]$match.Groups[3].Value, [int]$match.Groups[4].Value
switch ($Version) {
    'major' { $new = "$($major + 1).0.0" }
    'minor' { $new = "$major.$($minor + 1).0" }
    'patch' { $new = "$major.$minor.$($patch + 1)" }
    default {
        if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'usage: bump-version.ps1 <major|minor|patch|X.Y.Z>' }
        $new = $Version
    }
}
$updated = [regex]::Replace($text, $pattern, { param($m) $m.Groups[1].Value + $new + $m.Groups[5].Value })
[IO.File]::WriteAllText($cargo, $updated)
cargo update --workspace --quiet  # Cargo.lock records the crates' versions too
Write-Host "version -> $new"
Write-Host "next: git commit -am 'Release $new' && git tag v$new && git push --follow-tags"
