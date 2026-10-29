# Build the Windows installer: `./scripts/build-installer.ps1 [-Exe PATH] [-OutDir DIR] [-Version X.Y.Z] [-Install]`
#
# Compiles installer/maplelens.iss into maplelens-v<version>-setup.exe and its .sha256 (the zip's format), and prints
# the installer's path. The version defaults to the workspace's. Inno Setup 7 is used when installed, else 6.6+; on
# your own machine: `winget install JRSoftware.InnoSetup.7 --scope user`. -Install (CI) installs the pinned 7 for this
# user when there is none: windows-latest only has 6.7 preinstalled, and Chocolatey has no 7.
param(
    [string]$Exe = 'target/release/maplelens.exe',
    [string]$OutDir = '.',
    [string]$Version,
    [switch]$Install
)

$ErrorActionPreference = 'Stop'
# The official release, checked like the vendored scripts (scripts/vendor.mjs): this exact file or nothing.
$pinned = @{
    Url    = 'https://github.com/jrsoftware/issrc/releases/download/is-7_1_1/innosetup-7.1.1-x64.exe'
    Sha256 = '8a5a6711c3766a6e5fb74f280ba0e3551e719ce94538d901fdbd1f8f9920b3f0'
}

function Find-Iscc([string[]]$editions) {
    foreach ($edition in $editions) {
        foreach ($folder in @(${env:ProgramFiles(x86)}, $env:ProgramFiles, (Join-Path $env:LOCALAPPDATA 'Programs'))) {
            $path = if ($folder) { Join-Path $folder "Inno Setup $edition\ISCC.exe" }
            if ($path -and (Test-Path $path)) { return $path }
        }
    }
}

$iscc = Find-Iscc 7
if (-not $iscc -and $Install) {
    $download = Join-Path ([IO.Path]::GetTempPath()) (Split-Path $pinned.Url -Leaf)
    Invoke-WebRequest $pinned.Url -OutFile $download
    $hash = (Get-FileHash $download -Algorithm SHA256).Hash.ToLower()
    if ($hash -ne $pinned.Sha256) { throw "Inno Setup download has SHA-256 $hash, expected $($pinned.Sha256)" }
    $setup = Start-Process $download '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', '/CURRENTUSER' -Wait -PassThru
    if ($setup.ExitCode) { throw "Inno Setup's installer failed with exit code $($setup.ExitCode)" }
    Remove-Item $download
    $iscc = Find-Iscc 7
}
if (-not $iscc) { $iscc = Find-Iscc 6 }
if (-not $iscc) { throw 'Inno Setup not found: winget install JRSoftware.InnoSetup.7 --scope user' }

$root = Split-Path $PSScriptRoot
if (-not $Version) {
    $Version = (cargo pkgid --manifest-path (Join-Path $root 'Cargo.toml') -p maplelens) -replace '.*[#@]', ''
    if ($LASTEXITCODE) { throw 'cargo pkgid failed' }
}
$exePath = (Resolve-Path $Exe).Path
New-Item -ItemType Directory -Force $OutDir | Out-Null
$out = (Resolve-Path $OutDir).Path

Write-Host "Compiling with $iscc"
& $iscc /Qp "/DAppVersion=$Version" "/DExeSource=$exePath" "/O$out" (Join-Path $root 'installer\maplelens.iss') | Out-Host
if ($LASTEXITCODE) { throw "ISCC failed with exit code $LASTEXITCODE" }

$name = "maplelens-v$Version-setup.exe"
$setup = Join-Path $out $name
(Get-FileHash $setup -Algorithm SHA256).Hash.ToLower() + "  $name" | Out-File "$setup.sha256" -Encoding ascii
$setup
