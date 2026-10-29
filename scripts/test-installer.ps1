# Install, check and uninstall the Windows installer silently: `./scripts/test-installer.ps1 maplelens-vX.Y.Z-setup.exe`
#
# Into a scratch folder, leaving nothing installed (per user or per machine, as installer/maplelens.iss says): Setup refuses while MapleLens runs (the app's own
# mutex, and one this user cannot open, as when the app runs as administrator); the files, the uninstall entry, the
# Start Menu shortcut and the installed exe's --version; then the uninstaller removes the app's own files too (the log,
# the updater's leftovers) and its "Start with Windows" entry, and keeps the player's data in %LOCALAPPDATA%\MapleLens.
# Refuses to run where MapleLens is installed already: Setup would upgrade that install.
param([Parameter(Mandatory = $true)][string]$Setup)

$ErrorActionPreference = 'Stop'
$Setup = (Resolve-Path $Setup).Path
if ((Split-Path $Setup -Leaf) -notmatch '^maplelens-v(.+)-setup\.exe$') { throw "not an installer's name: $Setup" }
$version = $Matches[1]
# installer/maplelens.iss AppId; Inno Setup adds "_is1", under HKCU per user and HKLM per machine.
$uninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{702546F1-91F5-4C7A-BED5-11F8C72791DC}_is1'
$entries = "HKCU:\$uninstallKey", "HKLM:\$uninstallKey"
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
# main.rs `already_running`.
$mutexName = 'Local\MapleLens.SingleInstance'
$shortcuts = 'Programs', 'CommonPrograms' | ForEach-Object { Join-Path ([Environment]::GetFolderPath($_)) 'MapleLens.lnk' }
foreach ($entry in $entries) {
    if (Test-Path $entry) { throw "MapleLens is installed here already ($((Get-ItemProperty $entry).InstallLocation))" }
}
foreach ($shortcut in $shortcuts) { if (Test-Path $shortcut) { throw "a MapleLens shortcut exists already: $shortcut" } }

# A runner's %TEMP% can be an 8.3 path, which Setup writes out long.
$temp = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$scratch = Join-Path $temp "maplelens-setup-test-$PID"
$dir = Join-Path $scratch 'MapleLens'
$exe = Join-Path $dir 'maplelens.exe'
$data = Join-Path $env:LOCALAPPDATA 'MapleLens'
$dataMade = -not (Test-Path $data)
$marker = Join-Path $data "installer-test-$PID.txt"
$runBefore = (Get-ItemProperty $runKey -ErrorAction SilentlyContinue).MapleLens
New-Item -ItemType Directory -Force $scratch | Out-Null

function Check([bool]$ok, [string]$what) {
    if (-not $ok) { throw "FAILED: $what" }
    Write-Host "ok: $what"
}

function Run-Setup([string]$log) {
    $arguments = '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=`"$dir`"", "/LOG=`"$(Join-Path $scratch $log)`""
    (Start-Process $Setup $arguments -Wait -PassThru).ExitCode
}

# The uninstaller hands over to a copy of itself in %TEMP% and removes its own file last: wait for that.
function Wait-Gone([string]$path) {
    $deadline = (Get-Date).AddSeconds(30)
    while ((Test-Path $path) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
    -not (Test-Path $path)
}

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class MapleLensTestMutex {
    [StructLayout(LayoutKind.Sequential)]
    struct SecurityAttributes { public int Length; public IntPtr Descriptor; public int Inherit; }
    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool ConvertStringSecurityDescriptorToSecurityDescriptorW(string sddl, uint revision, out IntPtr descriptor, IntPtr size);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern IntPtr CreateMutexW(ref SecurityAttributes attributes, bool initialOwner, string name);
    [DllImport("kernel32.dll")]
    public static extern bool CloseHandle(IntPtr handle);
    // A mutex only SYSTEM may open: what a copy running as administrator looks like to a Setup that is not.
    public static IntPtr Create(string name, string sddl) {
        var attributes = new SecurityAttributes { Length = Marshal.SizeOf(typeof(SecurityAttributes)) };
        if (sddl != null && !ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl, 1, out attributes.Descriptor, IntPtr.Zero))
            throw new System.ComponentModel.Win32Exception();
        var handle = CreateMutexW(ref attributes, false, name);
        if (handle == IntPtr.Zero) throw new System.ComponentModel.Win32Exception();
        return handle;
    }
}
'@

try {
    foreach ($case in @(@{ Name = 'running'; Sddl = $null }, @{ Name = 'running as administrator'; Sddl = 'D:(A;;GA;;;SY)' })) {
        $mutex = [MapleLensTestMutex]::Create($mutexName, $case.Sddl)
        try { $code = Run-Setup "setup-$($case.Name -replace ' ', '-').log" } finally { [void][MapleLensTestMutex]::CloseHandle($mutex) }
        Check ($code -ne 0 -and -not (Test-Path $exe)) "Setup refuses while MapleLens is $($case.Name) (exit code $code)"
    }

    $code = Run-Setup 'setup.log'
    Check ($code -eq 0) "Setup succeeds (exit code $code)"
    foreach ($file in 'maplelens.exe', 'LICENSE', 'README.md', 'unins000.exe') {
        Check (Test-Path (Join-Path $dir $file)) "installed $file"
    }
    $entry = $entries | Where-Object { Test-Path $_ } | Select-Object -First 1
    Check ($null -ne $entry) "uninstall entry ($entry)"
    $installed = Get-ItemProperty $entry
    Check ($installed.DisplayVersion -eq $version) "uninstall entry version $($installed.DisplayVersion)"
    Check ($installed.InstallLocation.TrimEnd('\') -eq $dir) "uninstall entry location $($installed.InstallLocation)"
    Check (@($shortcuts | Where-Object { Test-Path $_ }).Count -eq 1) 'Start Menu shortcut'
    $reported = (& $exe --version | Out-String).Trim()
    Check ($reported -eq "maplelens $version") "installed exe reports '$reported'"
    $uninstaller = Join-Path $dir 'unins000.exe'
    $mutex = [MapleLensTestMutex]::Create($mutexName, 'D:(A;;GA;;;SY)')
    try { $code = (Start-Process $uninstaller '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait -PassThru).ExitCode }
    finally { [void][MapleLensTestMutex]::CloseHandle($mutex) }
    Check ($code -ne 0 -and (Test-Path $exe) -and (Test-Path $entry)) "the uninstaller refuses while MapleLens runs (exit code $code)"

    # What the app adds next to itself, and its "Start with Windows" entry (startup.rs writes the exe, quoted).
    foreach ($file in 'maplelens.log', 'maplelens.log.1', 'maplelens.old.exe', 'maplelens.new.exe') {
        Set-Content (Join-Path $dir $file) 'test'
    }
    Set-ItemProperty $runKey MapleLens "`"$exe`""
    New-Item -ItemType Directory -Force $data | Out-Null
    Set-Content $marker 'installer test: the uninstaller must keep this folder'

    $uninstall = Start-Process $uninstaller '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait -PassThru
    Check ($uninstall.ExitCode -eq 0) "uninstaller succeeds (exit code $($uninstall.ExitCode))"
    Check (Wait-Gone $dir) 'install folder removed, with the log and the updater leftovers'
    Check (-not (Test-Path $entry)) 'uninstall entry removed'
    Check (-not ($shortcuts | Where-Object { Test-Path $_ })) 'Start Menu shortcut removed'
    Check ($null -eq (Get-ItemProperty $runKey -ErrorAction SilentlyContinue).MapleLens) 'Start with Windows entry removed'
    Check (Test-Path $marker) "player data kept ($data)"
}
finally {
    if (Test-Path (Join-Path $dir 'unins000.exe')) {
        Start-Process (Join-Path $dir 'unins000.exe') '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait | Out-Null
        [void](Wait-Gone $dir)
    }
    if ($null -ne $runBefore) { Set-ItemProperty $runKey MapleLens $runBefore }
    else { Remove-ItemProperty $runKey MapleLens -ErrorAction SilentlyContinue }
    Remove-Item $marker -ErrorAction SilentlyContinue
    if ($dataMade -and (Test-Path $data) -and -not (Get-ChildItem $data -Force)) { Remove-Item $data }
    Get-ChildItem $scratch -Filter *.log -ErrorAction SilentlyContinue | ForEach-Object {
        if ($env:GITHUB_ACTIONS) { Write-Host "::group::$($_.Name)"; Get-Content $_.FullName | Write-Host; Write-Host '::endgroup::' }
    }
    Remove-Item $scratch -Recurse -Force -ErrorAction SilentlyContinue
}
