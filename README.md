# Lumen (Desktop Fences Fork)

Fork of [nhatnguyends/Lumen](https://github.com/nhatnguyends/Lumen) — Native Rust desktop widgets for Windows: **Desktop Fences**, App Dock, Clock, Calendar, YouTube Music, Notes and Control Center. Widget UI shares one Rust/Win32 process and wallpaper compositor.

## Desktop Fences (`widgets/fences`)

Stardock-style liquid/frosted glass desktop folder fences built natively in Rust:

- **Multi-Fence Folder Containers & Magnetic Corner Snapping:** Independent glass windows backed by real folders (configured in `config/fences.json`), with hardware-accelerated dragging from the title bar, resizing from all 4 edges & corners, and magnetic edge/corner snapping against neighboring fences and screen work-area borders.
- **External Drag-and-Drop, `+` Quick-Add Menu & Recycle Bin Delete:** Drag files, folders, or shortcuts directly from Windows Explorer into any fence, or click the header `+` button / right-click menu to **Add Program / App / File...**, **Add Folder Shortcut...**, **Move Folder into This Fence...**, **New Subfolder**, or **Create New Fence from Folder...**. Press `Delete` or right-click an item to open, reveal in Explorer, or send to the Recycle Bin.
- **Icon Drag-and-Drop Reorder, Cross-Fence Move & Floating Drag Ghost:** Drag icons within a fence to switch to **Manual Order**, or drag single/multi-selected items directly into another fence window (or out onto the Windows Desktop) with a floating topmost glass drag-ghost card (icon, title, and multi-select count badge) following the cursor across the entire screen.
- **Auto-Route Rules (`Images`, `PC Programs`, `Downloads`) & Live Folder Watch:** Built-in background watcher monitors `Desktop` and `Downloads` every second (ignoring incomplete browser `.crdownload`/`.part`/`.tmp` downloads until writing finishes), automatically routing matching files into the **Images**, **PC Programs**, or **Downloads** fences and refreshing any fence immediately when files are added or removed. Rules and custom file extensions can be toggled or edited per-fence from **Lumen Settings → Desktop Fences → Settings** or each fence's right-click **`Auto-Route Rule`** menu.
- **View Modes, Image Gallery Thumbnails & Tile Text Size:** Switch per-fence between **Standard Grid**, **Photo Gallery (Large Images)**, and **Compact Tiles** with optional image thumbnail previews, **Icon Size (`24/32/48px`)**, and **Tile Text Size (`9px` Extra Small, `10px` Compact, `11px` Small, `13px` Normal, `15px` Large, `17px` Extra Large)**.
- **Desktop Marquee Cluster Drag & `Win + D` Persistence:** Drag a marquee box across the desktop to select and move any cluster of fences and widgets together as one unit. Fences and desktop-layer widgets exclude Aero Peek (`DWMWA_EXCLUDED_FROM_PEEK`) and automatically stay above `WorkerW`/`Progman` without Z-order flicker when pressing `Win + D` (Show Desktop).
- **120Hz Fade on Hover & Desktop Double-Click Quick-Hide:** Fences can subtly dim when idle (`5%`, `15%`, `25%`, `50%`, or `100%`) and smoothly animate to `100%` at 120Hz when hovered. Double-click an empty area of the Windows desktop to hide or reveal all fences and desktop widgets together.
- **HD Icons & True RGB ClearType Typography:** Extracts 48×48 shell icons (`SHIL_EXTRALARGE`) and renders crisp Win32 GDI `CLEARTYPE_NATURAL_QUALITY` RGB sub-pixel text over dark smoked glass.
- **HD Music Artwork & 2× Supersampled Spinning Disc:** YouTube Music and Windows SMTC fallback automatically fetch high-resolution cover art (`w544-h544` / `maxresdefault.jpg` / `hq720.jpg`), auto-trim letterbox bars, center-crop at native resolution with `Lanczos3` filtering, and render spinning vinyl discs at 2× supersampled resolution.
- **Full Vietnamese UI Localization & Vietnamese Lunar Calendar (`Lịch Âm`):** The entire application (General Settings, system tray, surface palettes, prompt dialogs) and all 7 native widgets (App Dock, Desktop Fences, Calendar, Clock, Control Center, Sticky Notes, and YouTube Music) are localized into Vietnamese. The Calendar widget includes built-in astronomical Vietnamese Lunar Calendar (`UTC+7`) calculation (`Date::lunar`), displaying lunar day subtitles (`dd` or `1/mm` on the 1st of a lunar month, highlighting *Mùng 1* and *Rằm*), full lunar date & *Can Chi* year (`ÂL dd/mm · Năm Can Chi`) in the calendar header/date card, and a toggle in Calendar Settings (**Hiển thị lịch âm**).
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
