# Lumen (Desktop Fences Fork)

Fork of [nhatnguyends/Lumen](https://github.com/nhatnguyends/Lumen) — Native Rust desktop widgets for Windows: **Desktop Fences**, App Dock, Clock, Calendar, YouTube Music, Notes and Control Center. Widget UI shares one Rust/Win32 process and wallpaper compositor.

## Desktop Fences (`widgets/fences`)

Stardock-style liquid/frosted glass desktop folder fences built natively in Rust:

- **Multi-Fence Folder Containers:** Independent glass windows backed by real folders (configured in `config/fences.json`), with hardware-accelerated dragging from the title bar and resizing from all 4 edges & corners.
- **120Hz Fade on Hover:** Fences can subtly dim when idle (`5%`, `15%`, `25%`, `50%`, or `100%`) and smoothly animate to `100%` at 120Hz when hovered.
- **Desktop Double-Click Quick-Hide:** Double-click an empty area of the Windows desktop to hide or reveal all fences.
- **HD Icons & DirectWrite Typography:** Extracts 48×48 shell icons (`SHIL_EXTRALARGE`) and renders crisp DirectWrite text with a subtle drop shadow over dark smoked glass.
- **Marquee Multi-Select & Roll-Up:** Drag inside a fence to marquee-select multiple items, double-click a fence header to roll up/expand, or right-click any fence for **Fade on Hover**, **Idle Opacity**, **Icon Size (`24/32/48px`)**, **Filter & Sort** (`Show All`, `Folders Only`, `Files & Apps Only`, `Sort by Name`, `Sort by Date`, `Folders First`), **Open Folder**, and **Refresh**.
- **Auto-Update Disabled:** Upstream GitHub release auto-updates are disabled (`update::check` returns `Ok(None)`) so local fork builds are never overwritten.

## Portable download

Download `Lumen-<version>-windows-x86_64.zip` from
[GitHub Releases](https://github.com/nhatnguyends/Lumen/releases), extract the
whole archive to a writable folder, and open **Lumen.exe**. The native portable
edition runs natively without external runtimes: Python, Node, Rust and Visual Studio do not
need to be installed. Music playback uses installed Chrome or Edge and internet;
the other widgets do not require a browser. No Electron or WebView runtime is used.

The initial target is Windows 10/11 **x64**. Native ARM64 and 32-bit builds are
not supplied, and cross-machine compatibility still needs broader testing.
The binary is currently unsigned. Keep all extracted files together and use
a writable user folder rather than Program Files. Data stays in `config/` beside
the executable. Start with Windows defaults on and can be disabled in General
Settings. Launch once after moving the folder to refresh the startup path.

General Settings > Updates checks official stable releases, downloads and
verifies the portable ZIP, then restarts to apply it. Network work runs away from
the UI thread. Updates preserve `config/` and keep file backups in
`config/updates/`. Modified app files cause an update to stop rather than overwrite
local edits. GitHub HTTPS release metadata/digests provide integrity; this is not
an independently signed update feed. All six native widget implementations
currently update together with Lumen. Independent binary plugins remain planned.

## Optional Python launch (portable)

The portable ZIP also includes optional Python entry files: run
`python runtime/launch_lumen.py` from its extracted folder, or double-click
`runtime/Launch Lumen.pyw` if Python is associated with `.pyw` files. This option
requires installed Python 3.10+ but no pip packages, Rust or build tools. It still
launches the same Rust `Lumen.exe` and does not change Windows security checks.
Keep the executable and all runtime files together. Launcher logs are written
to `config/logs/python-launcher.log`.

## Run from source

Build prerequisites: Rust MSVC, Visual Studio C++ Build Tools, and Python 3.10+ (for source safety checks and packaging scripts).

Build and run in release mode:

```text
python scripts/check_safe.py
cargo build --release --locked --manifest-path native-dock/Cargo.toml
.\native-dock\target\release\lumen.exe
```

Local release builds output to `native-dock/target/release/lumen.exe` (debug builds to `native-dock/target/debug/lumen.exe`). Always stop any running `lumen.exe` instance before rebuilding, then launch it again afterwards. Portable packaging uses a separate static-CRT build under `native-dock/target/portable-debug/`, writing ZIPs under `share/`; no installer is generated.
YouTube Music playback runs natively in Rust against an owned Chrome or Edge profile over DevTools pipes; widget windows remain Rust. See [Music](docs/youtube-music.md).

## Widgets

Open the native tray menu to show the widget manager. Every widget has an On/Off
switch and independent settings. Appearance, size and position are remembered.
Packages live in `widgets/<id>/native/`; see [package development](widgets/README.md).

Control Center groups displays, audio outputs and microphones into one control
per family. Device names lead each row; newly connected devices appear in the
corresponding group. Main Display is identified with a disabled gray toggle.
Volume and microphone volume include mute on the icon. Edit supports drag/drop,
rename and remove. Add opens a separate searchable visual control manager and
keeps the widget geometry. Small layouts retain labels and scroll horizontally.
Visible draggable scrollbars indicate overflow; edge fades appear only where
more content remains. Soft Violet uses the raised pale glass material.

## Checks

```text
python scripts/check_safe.py
python -m unittest discover -s scripts -p "test_check_safe.py"
python -m unittest test_startup_windows
cargo test --locked --manifest-path native-dock/Cargo.toml
```

Settings live in `config/`. Legacy settings are retained for import; removing
Electron does not delete widget data or browser sign-in profiles.

## Credits and license

Created by **Nhat Ng**. Copyright © 2026 Nhat Ng.
Lumen's original source is licensed under [MIT](LICENSE.txt).

Thank you to the developers and contributors behind Rust, windows-rs, resvg,
tiny-skia, image, Serde, RustCrypto, zip-rs, Lucide, Feather, Python and all
transitive dependencies.
See [Credits](CREDITS.md) and the [third-party inventory](LICENSES/THIRD-PARTY-NOTICES.md).
Full license texts are included under `LICENSES/` in source and portable packages.
General Settings → Credits & licenses opens the acknowledgements.
Third-party components keep their own licenses; Lumen's MIT license does not replace them.
