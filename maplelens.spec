# PyInstaller spec: `pyinstaller maplelens.spec` -> dist/maplelens.exe (one file, console kept for logs/--help).
from PyInstaller.utils.hooks import collect_submodules

# winrt projections are loaded dynamically by name, so static analysis misses them.
hiddenimports = collect_submodules("winrt")

a = Analysis(
    ["src/maplelens/__main__.py"],
    pathex=["src"],
    datas=[("src/maplelens/assets", "maplelens/assets")],  # images read via __file__-relative paths
    hiddenimports=hiddenimports,
    excludes=["tkinter", "pytest"],
)
# cv2 ships a 27 MB FFmpeg DLL for video I/O; we only do still-image ops (resize, matchTemplate, imread).
a.binaries = [b for b in a.binaries if "opencv_videoio_ffmpeg" not in b[0]]
pyz = PYZ(a.pure)
exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    name="maplelens",
    icon="src/maplelens/assets/maplelens.ico",
    console=True,
    upx=False,  # UPX-packed exes trip antivirus heuristics
)
