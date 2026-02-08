"""
PyInstaller spec file for FORGE (llmforge)

Builds a single-file binary for Linux and macOS.
Usage: pyinstaller pyinstaller.spec

For cross-platform builds, use Docker:
- Linux: pyinstaller pyinstaller.spec --clean
- macOS: pyinstaller pyinstaller.spec --clean --target-arch universal2
"""

import os
import sys
from pathlib import Path

# Project metadata
block_cipher = None
app_name = "forge"
app_version = "0.1.0"

# Directories
src_dir = Path("src/forge")
project_root = Path(".")

# Collect all pure Python modules
hiddenimports = [
    "textual.widgets",
    "textual.reactive",
    "textual.containers",
    "textual.app",
    "rich.text",
    "rich.console",
    "rich.table",
    "watchdog.observers",
    "watchdog.events",
    "orjson",
    "yaml",
    "click",
    "click.decorators",
    "click.formatting",
]

# Data files to include (config schemas, etc.)
datas = [
    # Add any data files here if needed
    # (str(src_dir / "schemas" / "*.yaml"), "forge/schemas"),
]

# Binary analysis settings
binaries_excludes = [
    # Exclude unnecessary binaries to reduce size
]

# Analysis
a = Analysis(
    [str(src_dir / "cli.py")],
    pathex=[str(project_root)],
    binaries=[],
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[
        # Exclude GUI and scientific libraries (safe to exclude)
        "tkinter",
        "matplotlib",
        "numpy",
        "pandas",
        "scipy",
        "PyQt5",
        "PyQt6",
        "PySide2",
        "PySide6",
        # Exclude development tools
        "IPython",
        "jupyter",
        "pytest",
        "mypy",
        "black",
        "ruff",
        "setuptools",
        "distutils",
        # Exclude unnecessary network/server modules
        "http.server",
        "xmlrpc",
        # Exclude database modules (not used by FORGE)
        "sqlite3",
        # Note: email is required by importlib.metadata, cannot exclude
    ],
    win_no_prefer_redirects=False,
    win_private_assemblies=False,
    cipher=block_cipher,
    noarchive=False,
    optimize=2,  # Python bytecode optimization
)

# Filter out unnecessary files
pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher, optimize=2)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.zipfiles,
    a.datas,
    [],
    name=app_name,
    debug=False,
    bootloader_ignore_signals=False,
    strip=True,  # Strip debug symbols to reduce size
    upx=True,  # Use UPX compression (if available)
    upx_exclude=[],
    runtime_tmpdir=None,
    console=True,  # FORGE is a terminal app
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
    # Icon can be added here for macOS
    # icon=str(src_dir / "assets" / "icon.icns"),
)
