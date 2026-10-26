# Replays live MapleStory streams through the real detection path and reports what it matched, so detections can
# be reviewed over time. Needs yt-dlp and ffmpeg on PATH. Frames are 1 per second of the stream's 1080p source.
#
#   ./scripts/stream-bench.ps1 -Channels name1,name2 -Seconds 300 -Out target\streams
#
# Per channel it writes frames\*.png, results.tsv (frame, hover ms, info ms, title read, catalog item, where,
# Character Info name) and hits\*.png (every tooltip crop), then prints a summary line. Stream footage is
# compressed and may be scaled, so it checks recall and false positives, not pixel-exact reading: never commit it
# as a fixture (fixtures must be native captures). A Twitch ad slate shows up as frames with no detections.
param(
    [Parameter(Mandatory)][string[]]$Channels,
    [int]$Seconds = 300,
    [string]$Out = "target\streams",
    # The status bar's outline does not survive compression, so the UI scale is given: tooltip width / 450.
    [double]$UiScale = 0.9,
    # Frames and clip are deleted afterwards (a few minutes of 1080p PNGs is several GB) unless this is set.
    [switch]$Keep
)

$ErrorActionPreference = "Stop"
cargo test --release -p maplelens --no-run 2>&1 | Out-Null
$exe = Get-ChildItem target\release\deps\maplelens-*.exe | Where-Object { $_.Name -notmatch "\.d$" } |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1

foreach ($channel in $Channels) {
    $dir = Join-Path $Out $channel
    New-Item -ItemType Directory -Force "$dir\frames" | Out-Null
    $url = (yt-dlp --no-update -q -g -f "1080p60__source_/best" "https://twitch.tv/$channel" | Select-Object -First 1)
    if (-not $url) { Write-Warning "$channel is offline"; continue }
    ffmpeg -loglevel error -y -i $url -t $Seconds -an -c copy "$dir\clip.mp4"
    ffmpeg -loglevel error -y -i "$dir\clip.mp4" -vf fps=1 "$dir\frames\%04d.png"
    $env:MAPLELENS_FRAMES = "$dir\frames"
    $env:MAPLELENS_OUT = "$dir\hits"
    $env:MAPLELENS_UI_SCALE = $UiScale
    & $exe.FullName stream_bench --ignored --nocapture |
        Where-Object { $_ -match '^\d{4}\t' } | Set-Content "$dir\results.tsv"
    if (-not $Keep) { Remove-Item -Recurse -Force "$dir\frames", "$dir\clip.mp4" }
    $rows = Get-Content "$dir\results.tsv" | ForEach-Object { , ($_ -split "`t") }
    $hover = ($rows | Where-Object { $_[3] }).Count
    $items = ($rows | Where-Object { $_[4] }).Count
    $windows = ($rows | Where-Object { $_[6] }).Count
    "{0}: {1} frames, {2} tooltips ({3} matched an item), {4} Character Info" -f $channel, $rows.Count, $hover, $items, $windows
}
