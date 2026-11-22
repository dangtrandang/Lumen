//! Stardock-style liquid glass desktop folder fences with 120Hz fade-on-hover.
pub mod model;
pub(crate) mod package;

use crate::{
    compositor::{Buffer, WindowShadow},
    glass::Glass,
    model::wide,
    motion::Tween,
    render::{self, Canvas},
};
use model::{FenceSpec, FencesConfig};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    mem::{size_of, zeroed},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    rc::Rc,
    time::{Instant, SystemTime},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{
        Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        },
        LibraryLoader::GetModuleHandleW,
        Threading::GetCurrentProcessId,
    },
    UI::{
        Controls::{
            Dialogs::{
                GetOpenFileNameW, OPENFILENAMEW, OFN_ALLOWMULTISELECT, OFN_EXPLORER,
                OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_NODEREFERENCELINKS, OFN_PATHMUSTEXIST,
            },
            WM_MOUSELEAVE,
        },
        HiDpi::*,
        Input::{
            GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
            RAWINPUTHEADER, RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEMOUSE,
            KeyboardAndMouse::{
                GetDoubleClickTime, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent,
                TME_LEAVE, TRACKMOUSEEVENT, VK_DELETE,
            },
        },
        Shell::{
            BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, DragAcceptFiles, DragFinish,
            DragQueryFileW, HDROP, SHBrowseForFolderW, SHFILEINFOW, SHFILEOPSTRUCTW,
            SHFileOperationW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_SYSICONINDEX, SHGetFileInfoW,
            SHGetImageList, SHGetPathFromIDListW, SHIL_EXTRALARGE, ShellExecuteW,
        },
        WindowsAndMessaging::*,
    },
};

const CLASS: &str = "LumenNativeFenceWindow";
const MARQUEE_CLASS: &str = "LumenDesktopMarqueeOverlay";
const DRAG_GHOST_CLASS: &str = "LumenFenceDragGhostOverlay";
const FADE_TIMER: usize = 201;
const ZORDER_WATCH_TIMER: usize = 202;
const FENCES_SETTINGS_CHANGED: u32 = WM_APP + 195;
const EASE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
const HEADER_H: f32 = 34.0;
const RADIUS: f32 = 18.0;
const PAD_X: f32 = 12.0;

const RI_MOUSE_LEFT_BUTTON_DOWN: u16 = 0x0001;
const RI_MOUSE_LEFT_BUTTON_UP: u16 = 0x0002;
const CLEARTYPE_NATURAL_QUALITY_VAL: u32 = 6;

const CMD_FADE: usize = 1001;
const CMD_ROLL: usize = 1002;
const CMD_SORT_DATE: usize = 1003;
const CMD_SORT_NAME: usize = 1004;
const CMD_OPEN_FOLDER: usize = 1005;
const CMD_REFRESH: usize = 1006;
const CMD_HIDE: usize = 1007;
const CMD_OP_5: usize = 1010;
const CMD_OP_15: usize = 1011;
const CMD_OP_25: usize = 1012;
const CMD_OP_50: usize = 1013;
const CMD_OP_100: usize = 1014;
const CMD_ICON_24: usize = 1020;
const CMD_ICON_32: usize = 1021;
const CMD_ICON_48: usize = 1022;
const CMD_FILTER_ALL: usize = 1030;
const CMD_FILTER_FOLDERS: usize = 1031;
const CMD_FILTER_FILES: usize = 1032;
const CMD_FOLDERS_FIRST: usize = 1033;
const CMD_SORT_MANUAL: usize = 1034;

const CMD_TEXT_9: usize = 1038;
const CMD_TEXT_10: usize = 1039;
const CMD_TEXT_11: usize = 1040;
const CMD_TEXT_13: usize = 1041;
const CMD_TEXT_15: usize = 1042;
const CMD_TEXT_17: usize = 1043;

const CMD_VIEW_GRID: usize = 1050;
const CMD_VIEW_GALLERY: usize = 1051;
const CMD_VIEW_COMPACT: usize = 1052;
const CMD_TOGGLE_THUMBS: usize = 1053;

const CMD_ADD_FILES: usize = 1060;
const CMD_ADD_FOLDER_LINK: usize = 1061;
const CMD_MOVE_FOLDER_IN: usize = 1062;
const CMD_NEW_SUBFOLDER: usize = 1063;
const CMD_NEW_FENCE_FROM_FOLDER: usize = 1064;

const CMD_ITEM_OPEN: usize = 1070;
const CMD_ITEM_REVEAL: usize = 1071;
const CMD_ITEM_DELETE: usize = 1072;

const CMD_RULE_TOGGLE: usize = 1080;
const CMD_RULE_PRESET_IMAGES: usize = 1081;
const CMD_RULE_PRESET_PROGRAMS: usize = 1082;
const CMD_RULE_PRESET_DOWNLOADS: usize = 1083;
const CMD_RULE_EDIT_EXTS: usize = 1084;
const CMD_RULE_RUN_NOW: usize = 1085;
const CMD_RULE_OPEN_SETTINGS: usize = 1086;

fn cell_dims(spec: &FenceSpec) -> (f32, f32, f32) {
    let base_icon = match spec.view_mode.as_str() {
        "gallery" => spec.icon_size.max(72),
        "compact" => spec.icon_size.min(24),
        _ => spec.icon_size,
    };
    let icon = base_icon.clamp(20, 96) as f32;
    let text_extra = (spec.text_size.clamp(8, 20) - 13).max(0) as f32 * 2.5;
    let (cw, ch) = match spec.view_mode.as_str() {
        "gallery" => (
            (icon + 28.0 + text_extra).max(96.0),
            icon + 34.0 + text_extra,
        ),
        "compact" => (
            (icon + 38.0 + text_extra).max(62.0),
            icon + 34.0 + text_extra,
        ),
        _ => (
            (icon + 46.0 + text_extra).max(70.0),
            icon + 42.0 + text_extra,
        ),
    };
    (icon, cw, ch)
}

/// Magnetically snaps a dragged fence `(nx, ny, w, h)` to neighboring fences' corners/edges
/// (aligned left/right/top/bottom, or adjacent with a 12px gap) and monitor work-area margins (16px).
pub fn snap_fence_position(
    nx: i32,
    ny: i32,
    w: i32,
    h: i32,
    others: &[[i32; 4]],
    work: [i32; 4],
) -> [i32; 2] {
    const SNAP_DIST: i32 = 14;
    const GAP: i32 = 12;
    const MARGIN: i32 = 16;

    let mut best_x = nx;
    let mut best_dx = SNAP_DIST + 1;
    let mut best_y = ny;
    let mut best_dy = SNAP_DIST + 1;

    let mut try_x = |cand: i32| {
        let d = (nx - cand).abs();
        if d <= SNAP_DIST && d < best_dx {
            best_dx = d;
            best_x = cand;
        }
    };
    let mut try_y = |cand: i32| {
        let d = (ny - cand).abs();
        if d <= SNAP_DIST && d < best_dy {
            best_dy = d;
            best_y = cand;
        }
    };

    if work[2] > work[0] && work[3] > work[1] {
        try_x(work[0] + MARGIN);
        try_x(work[2] - MARGIN - w);
        try_y(work[1] + MARGIN);
        try_y(work[3] - MARGIN - h);
    }

    for &[ol, ot, or, ob] in others {
        // Horizontal edge & corner alignments + 12px magnetic gap
        try_x(ol);
        try_x(or - w);
        try_x(or + GAP);
        try_x(ol - GAP - w);
        try_x(or);
        try_x(ol - w);

        // Vertical edge & corner alignments + 12px magnetic gap
        try_y(ot);
        try_y(ob - h);
        try_y(ob + GAP);
        try_y(ot - GAP - h);
        try_y(ob);
        try_y(ot - h);
    }

    [best_x, best_y]
}

fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "bmp" | "ico"
            )
        })
}

pub fn load_image_thumbnail(path: &Path, thumb_sz: u32) -> Option<render::Image> {
    if !is_image_file(path) {
        return None;
    }
    let img = image::open(path).ok()?;
    let scaled = img.thumbnail(thumb_sz, thumb_sz).into_rgba8();
    let (sw, sh) = scaled.dimensions();
    if sw == 0 || sh == 0 {
        return None;
    }
    let side = thumb_sz.max(32);
    let mut canvas = vec![0u8; (side * side * 4) as usize];
    let ox = (side.saturating_sub(sw)) / 2;
    let oy = (side.saturating_sub(sh)) / 2;
    let raw = scaled.into_raw();
    let rad = (side as f32 * 0.12).clamp(4.0, 12.0);

    for y in 0..sh {
        for x in 0..sw {
            // Rounded corner mask on the photo rectangle
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let fw = sw as f32;
            let fh = sh as f32;
            let dx = if fx < rad {
                rad - fx
            } else if fx > fw - rad {
                fx - (fw - rad)
            } else {
                0.0
            };
            let dy = if fy < rad {
                rad - fy
            } else if fy > fh - rad {
                fy - (fh - rad)
            } else {
                0.0
            };
            let dist = (dx * dx + dy * dy).sqrt();
            let corner_alpha = if dist > rad + 0.5 {
                0.0
            } else if dist > rad - 0.5 {
                (rad + 0.5 - dist).clamp(0.0, 1.0)
            } else {
                1.0
            };
            if corner_alpha <= 0.0 {
                continue;
            }
            let src_i = ((y * sw + x) * 4) as usize;
            let dst_x = (ox + x).min(side - 1);
            let dst_y = (oy + y).min(side - 1);
            let dst_i = ((dst_y * side + dst_x) * 4) as usize;
            let a = ((raw[src_i + 3] as f32) * corner_alpha).round() as u32;
            let r = ((raw[src_i] as u32 * a + 127) / 255) as u8;
            let g = ((raw[src_i + 1] as u32 * a + 127) / 255) as u8;
            let b = ((raw[src_i + 2] as u32 * a + 127) / 255) as u8;
            canvas[dst_i] = r;
            canvas[dst_i + 1] = g;
            canvas[dst_i + 2] = b;
            canvas[dst_i + 3] = a as u8;
        }
    }
    render::Image::from_rgba(side, side, canvas)
}

pub fn rect_intersects(a: [i32; 4], b: [i32; 4]) -> bool {
    a[2] >= b[0] && a[0] <= b[2] && a[3] >= b[1] && a[1] <= b[3]
}

/// Renders true RGB sub-pixel ClearType text (`CLEARTYPE_NATURAL_QUALITY`) via Win32 GDI
/// directly onto a 32-bit DIB `HDC` (matching Windows File Explorer sharpness on 1080p/4K).
pub unsafe fn draw_cleartype_text(
    dc: HDC,
    text: &str,
    px_rect: [i32; 4],
    px_height: i32,
    weight: i32,
    rgb_color: u32,
    dt_flags: u32,
) {
    if dc.is_null() || text.is_empty() || px_rect[2] <= px_rect[0] || px_rect[3] <= px_rect[1] {
        return;
    }
    let font = CreateFontW(
        -px_height.max(8),
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        OUT_TT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32,
        CLEARTYPE_NATURAL_QUALITY_VAL,
        (DEFAULT_PITCH | FF_DONTCARE) as u32,
        wide("Segoe UI").as_ptr(),
    );
    if font.is_null() {
        return;
    }
    let old_font = SelectObject(dc, font as _);
    let old_mode = SetBkMode(dc, TRANSPARENT as i32);
    let r = (rgb_color >> 16) & 0xff;
    let g = (rgb_color >> 8) & 0xff;
    let b = rgb_color & 0xff;
    let colorref = (b << 16) | (g << 8) | r;
    let old_color = SetTextColor(dc, colorref);

    let mut rc = RECT {
        left: px_rect[0],
        top: px_rect[1],
        right: px_rect[2],
        bottom: px_rect[3],
    };
    let mut wtext = wide(text);
    let len = wtext.len().saturating_sub(1) as i32;
    DrawTextW(dc, wtext.as_mut_ptr(), len, &mut rc, dt_flags);

    SetTextColor(dc, old_color);
    SetBkMode(dc, old_mode);
    SelectObject(dc, old_font);
    DeleteObject(font as _);
}

#[derive(Clone, Debug)]
struct FenceEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    modified: SystemTime,
}

impl FenceEntry {
    fn file_key(&self) -> String {
        self.path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.name)
            .to_lowercase()
    }
}

fn reorder_entries_by_index(
    entries: &mut Vec<FenceEntry>,
    from_idx: usize,
    to_idx: usize,
) -> Vec<String> {
    if from_idx < entries.len() && to_idx < entries.len() && from_idx != to_idx {
        let item = entries.remove(from_idx);
        entries.insert(to_idx, item);
    }
    entries.iter().map(|e| e.file_key()).collect()
}

pub fn move_entry_to_folder(src: &Path, dst_folder: &Path) -> Result<PathBuf, String> {
    if !src.exists() {
        return Err("source does not exist".into());
    }
    std::fs::create_dir_all(dst_folder).map_err(|e| e.to_string())?;
    let file_name = src
        .file_name()
        .ok_or_else(|| "invalid source file name".to_string())?;
    let dst = dst_folder.join(file_name);
    if src == dst {
        return Ok(dst);
    }
    std::fs::rename(src, &dst).map_err(|e| e.to_string())?;
    Ok(dst)
}

/// Creates a Windows `.lnk` shell shortcut at `lnk_path` pointing to `target` (file, `.exe`, or folder).
pub unsafe fn create_shell_shortcut(target: &Path, lnk_path: &Path) -> Result<PathBuf, String> {
    if let Some(parent) = lnk_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let init = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0;
    let clsid_shell_link =
        windows_sys::core::GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
    let iid_shell_link_w =
        windows_sys::core::GUID::from_u128(0x000214f9_0000_0000_c000_000000000046);
    let iid_persist_file =
        windows_sys::core::GUID::from_u128(0x0000010b_0000_0000_c000_000000000046);

    let mut psl: *mut std::ffi::c_void = null_mut();
    let hr = CoCreateInstance(
        &clsid_shell_link,
        null_mut(),
        CLSCTX_INPROC_SERVER,
        &iid_shell_link_w,
        &mut psl,
    );
    if hr < 0 || psl.is_null() {
        if init {
            CoUninitialize();
        }
        return Err(format!("CoCreateInstance(IShellLinkW) failed: {hr:#x}"));
    }

    type QueryInterfaceFn = unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const windows_sys::core::GUID,
        *mut *mut std::ffi::c_void,
    ) -> i32;
    type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
    type SetStrFn = unsafe extern "system" fn(*mut std::ffi::c_void, *const u16) -> i32;
    type SaveFn = unsafe extern "system" fn(*mut std::ffi::c_void, *const u16, i32) -> i32;

    let sl_vtbl = *(psl as *const *const usize);
    let query_interface: QueryInterfaceFn = std::mem::transmute(*sl_vtbl.add(0));
    let sl_release: ReleaseFn = std::mem::transmute(*sl_vtbl.add(2));
    let set_working_dir: SetStrFn = std::mem::transmute(*sl_vtbl.add(9));
    let set_path: SetStrFn = std::mem::transmute(*sl_vtbl.add(20));

    let wtarget = wide(target);
    let _ = set_path(psl, wtarget.as_ptr());
    let work_dir = if target.is_dir() {
        Some(target.to_path_buf())
    } else {
        target.parent().map(Path::to_path_buf)
    };
    if let Some(wd) = work_dir {
        let wdir = wide(&wd);
        let _ = set_working_dir(psl, wdir.as_ptr());
    }

    let mut ppf: *mut std::ffi::c_void = null_mut();
    let qhr = query_interface(psl, &iid_persist_file, &mut ppf);
    let mut saved_ok = false;
    if qhr >= 0 && !ppf.is_null() {
        let pf_vtbl = *(ppf as *const *const usize);
        let pf_release: ReleaseFn = std::mem::transmute(*pf_vtbl.add(2));
        let pf_save: SaveFn = std::mem::transmute(*pf_vtbl.add(6));
        let wlnk = wide(lnk_path);
        saved_ok = pf_save(ppf, wlnk.as_ptr(), 1) >= 0;
        let _ = pf_release(ppf);
    }
    let _ = sl_release(psl);
    if init {
        CoUninitialize();
    }
    if saved_ok && lnk_path.exists() {
        Ok(lnk_path.to_path_buf())
    } else {
        Err("IPersistFile::Save failed".into())
    }
}

fn unique_dest_path(dst_folder: &Path, base_name: &str, ext_with_dot: &str) -> PathBuf {
    let first = dst_folder.join(format!("{base_name}{ext_with_dot}"));
    if !first.exists() {
        return first;
    }
    for n in 2..999 {
        let cand = dst_folder.join(format!("{base_name} ({n}){ext_with_dot}"));
        if !cand.exists() {
            return cand;
        }
    }
    first
}

/// Imports an external file, `.exe` program, `.lnk` shortcut, or folder into `dst_folder`.
/// - If `prefer_shortcut` is true or `src` is an installed `.exe` outside Desktop, creates a `.lnk` shortcut.
/// - If `src` is a `.lnk`/`.url` outside Desktop, copies the shortcut into `dst_folder`.
/// - Otherwise tries moving `src` into `dst_folder`, falling back to file copy or folder `.lnk` shortcut across drives.
pub unsafe fn import_external_path_into_fence(
    src: &Path,
    dst_folder: &Path,
    prefer_shortcut: bool,
) -> Result<PathBuf, String> {
    if !src.exists() {
        return Err("source path does not exist".into());
    }
    std::fs::create_dir_all(dst_folder).map_err(|e| e.to_string())?;
    if src.parent().is_some_and(|p| p == dst_folder) {
        return Ok(src.to_path_buf());
    }
    let file_name = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "invalid file name".to_string())?;
    let ext_lower = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let from_desktop = src
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("Desktop"));

    if prefer_shortcut || (ext_lower == "exe" && !from_desktop) {
        let stem = if src.is_dir() {
            file_name
        } else {
            src.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(file_name)
        };
        let lnk_dst = unique_dest_path(dst_folder, stem, ".lnk");
        return create_shell_shortcut(src, &lnk_dst);
    }

    let dst = dst_folder.join(file_name);
    if matches!(ext_lower.as_str(), "lnk" | "url") && !from_desktop {
        let target_dst = if dst.exists() {
            let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or(file_name);
            unique_dest_path(dst_folder, stem, &format!(".{ext_lower}"))
        } else {
            dst
        };
        std::fs::copy(src, &target_dst).map_err(|e| e.to_string())?;
        return Ok(target_dst);
    }

    if !dst.exists() && std::fs::rename(src, &dst).is_ok() {
        return Ok(dst);
    }

    if src.is_file() {
        let target_dst = if dst.exists() {
            let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or(file_name);
            let dot_ext = if ext_lower.is_empty() {
                String::new()
            } else {
                format!(".{ext_lower}")
            };
            unique_dest_path(dst_folder, stem, &dot_ext)
        } else {
            dst
        };
        std::fs::copy(src, &target_dst).map_err(|e| e.to_string())?;
        return Ok(target_dst);
    }

    let lnk_dst = unique_dest_path(dst_folder, file_name, ".lnk");
    create_shell_shortcut(src, &lnk_dst)
}

pub fn create_new_subfolder(dst_folder: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dst_folder).map_err(|e| e.to_string())?;
    let path = unique_dest_path(dst_folder, "New Folder", "");
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Moves `path` (file, shortcut, or folder) to the Windows Recycle Bin,
/// falling back to `remove_file` / `remove_dir_all` if Recycle Bin is unavailable.
pub unsafe fn delete_entry_path(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let mut double_null = wide(path);
    double_null.push(0);
    let mut op: SHFILEOPSTRUCTW = zeroed();
    op.wFunc = 3; // FO_DELETE
    op.pFrom = double_null.as_ptr();
    // FOF_ALLOWUNDO (0x40) | FOF_NOCONFIRMATION (0x10) | FOF_NOERRORUI (0x400) | FOF_SILENT (0x04)
    op.fFlags = 0x0454;
    let rc = SHFileOperationW(&mut op);
    if rc == 0 && !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| e.to_string())
    } else {
        std::fs::remove_file(path).map_err(|e| e.to_string())
    }
}

#[derive(Clone, Copy)]
struct ItemDrag {
    idx: usize,
    start_pt: [f32; 2],
    cur_pt: [f32; 2],
    active: bool,
}

struct SharedState {
    root: PathBuf,
    config: FencesConfig,
    glass: Rc<RefCell<Glass>>,
    dpi: f32,
    smoke: bool,
    quick_hidden: bool,
    last_desk_click_ms: Option<Instant>,
    last_desk_click_pt: [i32; 2],
    desk_drag_start: Option<[i32; 2]>,
    marquee_hwnd: HWND,
    drag_ghost_hwnd: HWND,
    drag_ghost_buf: Option<Buffer>,
    settings_ui: Option<crate::settings_ui::Host>,
    watch_baseline: SystemTime,
    watch_tick: u32,
    folder_sigs: HashMap<String, u64>,
    routed_files: HashSet<PathBuf>,
    windows: Vec<Rc<RefCell<FenceWindow>>>,
}

pub struct Host {
    shared: Rc<RefCell<SharedState>>,
}

struct FenceWindow {
    hwnd: HWND,
    spec_id: String,
    shared: Rc<RefCell<SharedState>>,
    entries: Vec<FenceEntry>,
    icons: HashMap<String, render::Image>,
    frame: Option<Buffer>,
    background: Option<Buffer>,
    background_key: String,
    shadow: Option<WindowShadow>,
    hovered: bool,
    hover_item: Option<usize>,
    hover_roll: bool,
    hover_add: bool,
    selected: HashSet<usize>,
    marquee: Option<[f32; 4]>,
    item_drag: Option<ItemDrag>,
    group_selected: bool,
    scroll_y: f32,
    alpha_tween: Tween,
    started: Instant,
    dragging: Option<[i32; 4]>,
    resizing: Option<ResizeDrag>,
    visible: bool,
}

#[derive(Clone, Copy)]
struct ResizeDrag {
    edge: u8,
    start_x: i32,
    start_y: i32,
    orig_l: i32,
    orig_t: i32,
    orig_r: i32,
    orig_b: i32,
}

fn hit_resize_edge(lx: f32, ly: f32, w: f32, h: f32, rolled_up: bool) -> u8 {
    if rolled_up {
        return 0;
    }
    let m = 8.0;
    let mut edge = 0u8;
    if lx <= m {
        edge |= 1;
    } else if lx >= w - m {
        edge |= 2;
    }
    if ly <= m {
        edge |= 4;
    } else if ly >= h - m {
        edge |= 8;
    }
    if edge == 0 && lx >= w - 16.0 && ly >= h - 16.0 {
        edge = 2 | 8;
    }
    edge
}

pub fn should_open(root: &Path) -> bool {
    let path = FencesConfig::path(root);
    if path.exists() {
        FencesConfig::load(root).open
    } else {
        true
    }
}

impl Host {
    pub unsafe fn create(
        root: &Path,
        glass: Rc<RefCell<Glass>>,
        inspect: bool,
        smoke: bool,
    ) -> Result<Self, String> {
        register_class()?;
        let dpi = GetDpiForSystem() as f32 / 96.0;
        let config = FencesConfig::load(root);
        let marquee_hwnd = if smoke {
            null_mut()
        } else {
            let mh = create_marquee_overlay();
            crate::fusion::set_marquee_overlay(mh);
            mh
        };
        let drag_ghost_hwnd = if smoke {
            null_mut()
        } else {
            create_drag_ghost_overlay()
        };
        let shared = Rc::new(RefCell::new(SharedState {
            root: root.into(),
            config: config.clone(),
            glass,
            dpi,
            smoke,
            quick_hidden: false,
            last_desk_click_ms: None,
            last_desk_click_pt: [0, 0],
            desk_drag_start: None,
            marquee_hwnd,
            drag_ghost_hwnd,
            drag_ghost_buf: None,
            settings_ui: None,
            watch_baseline: SystemTime::now(),
            watch_tick: 0,
            folder_sigs: HashMap::new(),
            routed_files: HashSet::new(),
            windows: Vec::new(),
        }));
        let mut windows = Vec::new();
        for spec in &config.fences {
            if !spec.visible {
                continue;
            }
            if let Ok(win) = create_fence_window(spec, Rc::clone(&shared), inspect) {
                windows.push(win);
            }
        }
        if let Some(first) = windows.first() {
            if !smoke {
                let first_hwnd = first.borrow().hwnd;
                register_raw_mouse_sink(first_hwnd);
                SetTimer(first_hwnd, ZORDER_WATCH_TIMER, 100, None);
            }
        }
        shared.borrow_mut().windows = windows;
        log_zorder_snapshot("fences-init", &shared);
        Ok(Self { shared })
    }

    pub unsafe fn tick(&mut self) {
        check_and_repair_zorder(&self.shared);
    }

    pub unsafe fn show_settings(&mut self) {
        open_fences_settings(&self.shared);
    }

    pub unsafe fn show(&mut self) {
        self.shared.borrow_mut().config.open = true;
        let root = self.shared.borrow().root.clone();
        let _ = self.shared.borrow().config.save(&root);
        let wins = self.shared.borrow().windows.clone();
        for w in wins {
            let mut b = w.borrow_mut();
            b.visible = true;
            ShowWindow(b.hwnd, SW_SHOWNOACTIVATE);
            crate::desktop_layer::pin(b.hwnd);
            b.paint();
        }
    }

    pub unsafe fn hide(&mut self) {
        self.shared.borrow_mut().config.open = false;
        let root = self.shared.borrow().root.clone();
        let _ = self.shared.borrow().config.save(&root);
        let mhwnd = self.shared.borrow().marquee_hwnd;
        if !mhwnd.is_null() {
            ShowWindow(mhwnd, SW_HIDE);
        }
        let ghwnd = self.shared.borrow().drag_ghost_hwnd;
        if !ghwnd.is_null() {
            ShowWindow(ghwnd, SW_HIDE);
        }
        let wins = self.shared.borrow().windows.clone();
        for w in wins {
            let mut b = w.borrow_mut();
            b.visible = false;
            ShowWindow(b.hwnd, SW_HIDE);
            if let Some(s) = &b.shadow {
                s.hide();
            }
        }
    }

    pub fn is_visible(&self) -> bool {
        self.shared.borrow().windows.iter().any(|w| w.borrow().visible)
    }

    pub fn hwnd(&self) -> HWND {
        self.shared
            .borrow()
            .windows
            .first()
            .map(|w| w.borrow().hwnd)
            .unwrap_or(null_mut())
    }

    pub unsafe fn refresh(&mut self) {
        let wins = self.shared.borrow().windows.clone();
        for w in wins {
            let hwnd = {
                let mut b = w.borrow_mut();
                b.reload_entries();
                b.background_key.clear();
                b.hwnd
            };
            InvalidateRect(hwnd, null(), 0);
        }
    }
}

unsafe fn register_raw_mouse_sink(hwnd: HWND) {
    let rid = RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x02,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    };
    let _ = RegisterRawInputDevices(&rid, 1, size_of::<RAWINPUTDEVICE>() as u32);
}

unsafe fn log_zorder_snapshot(tag: &str, shared: &Rc<RefCell<SharedState>>) {
    let shell_host = crate::desktop_layer::desktop_shell_host();
    let progman = FindWindowW(wide("Progman").as_ptr(), null());
    let fg = GetForegroundWindow();
    let sh_ex = if !shell_host.is_null() {
        GetWindowLongPtrW(shell_host, GWL_EXSTYLE)
    } else {
        0
    };
    let pm_ex = if !progman.is_null() {
        GetWindowLongPtrW(progman, GWL_EXSTYLE)
    } else {
        0
    };
    let mut fence_states = Vec::new();
    let wins = shared.borrow().windows.clone();
    for w in &wins {
        if let Ok(b) = w.try_borrow() {
            let vis = IsWindowVisible(b.hwnd) != 0;
            let iconic = IsIconic(b.hwnd) != 0;
            let below = crate::desktop_layer::is_below_desktop(b.hwnd);
            let ex = GetWindowLongPtrW(b.hwnd, GWL_EXSTYLE);
            let owner = GetWindowLongPtrW(b.hwnd, GWLP_HWNDPARENT) as HWND;
            let mut pt = POINT { x: 0, y: 0 };
            let mut wr: RECT = zeroed();
            GetWindowRect(b.hwnd, &mut wr);
            pt.x = (wr.left + wr.right) / 2;
            pt.y = (wr.top + wr.bottom) / 2;
            let hit = WindowFromPoint(pt);
            let hit_root = if !hit.is_null() {
                GetAncestor(hit, GA_ROOT)
            } else {
                null_mut()
            };
            fence_states.push(format!(
                "id={} hwnd={:?} vis={}(model={}) iconic={} below_desk={} ex=0x{:x} owner={:?}({}) center_hit={:?}({}/root={:?}:{})",
                b.spec_id,
                b.hwnd,
                vis,
                b.visible,
                iconic,
                below,
                ex,
                owner,
                crate::desktop_layer::hwnd_class(owner),
                hit,
                crate::desktop_layer::hwnd_class(hit),
                hit_root,
                crate::desktop_layer::hwnd_class(hit_root),
            ));
        }
    }
    let mut stack = Vec::new();
    let mut cur = GetTopWindow(null_mut());
    for _ in 0..256 {
        if cur.is_null() || stack.len() >= 18 {
            break;
        }
        if IsWindowVisible(cur) != 0 {
            let cls = crate::desktop_layer::hwnd_class(cur);
            let title = crate::desktop_layer::hwnd_title(cur);
            let ex = GetWindowLongPtrW(cur, GWL_EXSTYLE);
            let iconic = IsIconic(cur) != 0;
            stack.push(format!(
                "{:?}:{}:{:?}(ex=0x{:x},ic={},dl={})",
                cur,
                cls,
                title,
                ex,
                iconic as u8,
                crate::desktop_layer::contains(cur) as u8
            ));
        }
        cur = GetWindow(cur, GW_HWNDNEXT);
    }
    crate::logger::info(
        "win-d",
        format!(
            "[{}] fg={:?}({}:{:?}) shell_host={:?}({},ex=0x{:x}) progman={:?}(ex=0x{:x}) fences=[{}] top_z=[{}]",
            tag,
            fg,
            crate::desktop_layer::hwnd_class(fg),
            crate::desktop_layer::hwnd_title(fg),
            shell_host,
            crate::desktop_layer::hwnd_class(shell_host),
            sh_ex,
            progman,
            pm_ex,
            fence_states.join("; "),
            stack.join(" -> ")
        ),
    );
}

unsafe fn check_and_repair_zorder(shared: &Rc<RefCell<SharedState>>) {
    if shared.borrow().quick_hidden {
        return;
    }
    if crate::desktop_layer::repair_if_below_desktop() {
        log_zorder_snapshot("watch-repaired-win-d", shared);
    }
    let should_watch = {
        let Ok(mut sh) = shared.try_borrow_mut() else {
            return;
        };
        sh.watch_tick = sh.watch_tick.wrapping_add(1);
        sh.watch_tick % 10 == 0
    };
    if should_watch {
        auto_watch_and_route(shared, false);
    }
}

unsafe fn create_drag_ghost_overlay() -> HWND {
    let class = wide(DRAG_GHOST_CLASS);
    let instance = GetModuleHandleW(null());
    let wc = WNDCLASSW {
        lpfnWndProc: Some(DefWindowProcW),
        hInstance: instance,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        lpszClassName: class.as_ptr(),
        ..zeroed()
    };
    let _ = RegisterClassW(&wc);
    CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        class.as_ptr(),
        wide("").as_ptr(),
        WS_POPUP,
        0,
        0,
        1,
        1,
        null_mut(),
        null_mut(),
        instance,
        null(),
    )
}

unsafe fn hide_drag_ghost(shared: &Rc<RefCell<SharedState>>) {
    let ghwnd = shared.try_borrow().map(|s| s.drag_ghost_hwnd).unwrap_or(null_mut());
    if !ghwnd.is_null() {
        ShowWindow(ghwnd, SW_HIDE);
    }
}

unsafe fn update_drag_ghost(win: &mut FenceWindow, idrag: ItemDrag, screen_pt: POINT, dpi: f32) {
    let ghost_hwnd = win
        .shared
        .try_borrow()
        .map(|s| s.drag_ghost_hwnd)
        .unwrap_or(null_mut());
    if ghost_hwnd.is_null() || idrag.idx >= win.entries.len() {
        return;
    }
    let Some(spec) = win.spec() else {
        return;
    };
    let (icon_sz, cell_w, cell_h) = cell_dims(&spec);
    let tile_font_px = if spec.text_size <= 0 {
        13
    } else {
        spec.text_size.clamp(9, 20)
    };
    let gw_px = (cell_w * dpi).round().max(48.0) as i32;
    let gh_px = (cell_h * dpi).round().max(48.0) as i32;
    let path = win.entries[idrag.idx].path.clone();
    let name = win.entries[idrag.idx].name.clone();
    let multi_count = if win.selected.len() > 1 && win.selected.contains(&idrag.idx) {
        win.selected.len()
    } else {
        1
    };

    let Some(mut buf) = Buffer::new(gw_px, gh_px, dpi, 10.0) else {
        return;
    };
    {
        let batch = render::AlphaBatch::new(buf.dc, dpi);
        let c = Canvas::new(buf.dc, dpi);
        c.round([1.0, 1.0, cell_w - 1.0, cell_h - 1.0], 10.0, 0x162038, 225, false, false);
        c.round([1.0, 1.0, cell_w - 1.0, cell_h - 1.0], 10.0, 0x7c9cff, 245, true, false);
        if let Some(img) = win.icon_for(&path) {
            let ix = ((cell_w - icon_sz) * 0.5).round();
            c.image(img, [ix, 6.0, ix + icon_sz, 6.0 + icon_sz]);
        }
        if multi_count > 1 {
            c.round([cell_w - 24.0, 4.0, cell_w - 4.0, 22.0], 9.0, 0x4f8cff, 245, false, false);
        }
        drop(batch);
    }
    draw_cleartype_text(
        buf.dc,
        &name,
        [
            (2.0 * dpi).round() as i32,
            ((8.0 + icon_sz) * dpi).round() as i32,
            ((cell_w - 2.0) * dpi).round() as i32,
            ((cell_h - 2.0) * dpi).round() as i32,
        ],
        (tile_font_px as f32 * dpi).round() as i32,
        600,
        0xffffff,
        DT_CENTER | DT_WORDBREAK | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
    if multi_count > 1 {
        let badge = format!("{multi_count}");
        draw_cleartype_text(
            buf.dc,
            &badge,
            [
                ((cell_w - 24.0) * dpi).round() as i32,
                (4.0 * dpi).round() as i32,
                ((cell_w - 4.0) * dpi).round() as i32,
                (22.0 * dpi).round() as i32,
            ],
            (11.0 * dpi).round() as i32,
            700,
            0xffffff,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
    }
    let gx = screen_pt.x - gw_px / 2;
    let gy = screen_pt.y - ((icon_sz * 0.5 + 6.0) * dpi).round() as i32;
    SetWindowPos(
        ghost_hwnd,
        HWND_TOPMOST,
        gx,
        gy,
        gw_px,
        gh_px,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
    buf.present_with_alpha_opacity(ghost_hwnd, false, 240);
    if let Ok(mut sh) = win.shared.try_borrow_mut() {
        sh.drag_ghost_buf = Some(buf);
    }
}

fn is_ignored_entry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "desktop.ini" || lower == "thumbs.db" || lower == ".ds_store"
}

fn folder_signature(dir: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(dir) else {
        return 0;
    };
    let mut acc = 1469598103934665603u64;
    let mut count = 0u64;
    for entry in rd.flatten() {
        let name = entry.file_name();
        let lossy = name.to_string_lossy();
        if is_ignored_entry(&lossy) {
            continue;
        }
        count += 1;
        let mut h = 1469598103934665603u64;
        for b in lossy.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        if let Ok(meta) = entry.metadata() {
            h ^= meta.len();
            if let Ok(m) = meta.modified() {
                if let Ok(d) = m.duration_since(SystemTime::UNIX_EPOCH) {
                    h ^= d.as_secs();
                }
            }
        }
        acc = acc.wrapping_add(h);
    }
    acc ^ (count.wrapping_mul(0x9e3779b97f4a7c15))
}

fn parse_rule_extensions(exts: &str) -> Vec<String> {
    exts.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .map(|s| s.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn is_temp_download_file(name: &str, ext: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if is_ignored_entry(&lower) || lower.starts_with("~$") || lower.starts_with('.') {
        return true;
    }
    matches!(
        ext,
        "crdownload" | "part" | "tmp" | "opdownload" | "download" | "partial" | "bak"
    )
}

fn is_file_ready_for_routing(path: &Path, meta: &fs::Metadata, baseline: SystemTime, force_existing: bool) -> bool {
    if !meta.is_file() {
        return false;
    }
    let now = SystemTime::now();
    let newest_ts = meta
        .modified()
        .ok()
        .into_iter()
        .chain(meta.created().ok())
        .max()
        .unwrap_or(now);
    if !force_existing && newest_ts < baseline {
        return false;
    }
    if let Ok(modified) = meta.modified() {
        if let Ok(age) = now.duration_since(modified) {
            if age.as_millis() < 800 {
                return false;
            }
        }
    }
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .is_ok()
}

unsafe fn auto_watch_and_route(shared: &Rc<RefCell<SharedState>>, force_existing: bool) {
    let (rules_enabled, watch_downloads, baseline, specs) = {
        let Ok(sh) = shared.try_borrow() else {
            return;
        };
        (
            sh.config.rules_enabled,
            sh.config.watch_downloads,
            sh.watch_baseline,
            sh.config.fences.clone(),
        )
    };

    if rules_enabled || force_existing {
        let mut active_rules: Vec<(String, PathBuf, Vec<String>, bool)> = specs
            .iter()
            .filter(|f| f.visible && f.auto_rule_enabled && !f.auto_rule_exts.trim().is_empty())
            .map(|f| {
                let is_dl = f.id == "downloads" || f.title.eq_ignore_ascii_case("downloads");
                (
                    f.id.clone(),
                    PathBuf::from(&f.folder),
                    parse_rule_extensions(&f.auto_rule_exts),
                    is_dl,
                )
            })
            .filter(|(_, _, exts, _)| !exts.is_empty())
            .collect();
        // Prioritize specific rules (Images, PC Programs, custom rules) before Downloads
        active_rules.sort_by_key(|(_, _, _, is_dl)| *is_dl);

        if !active_rules.is_empty() {
            let home = model::user_home();
            let mut watch_dirs = vec![home.join("Desktop")];
            if watch_downloads {
                let dl_dir = home.join("Downloads");
                if !watch_dirs.contains(&dl_dir) {
                    watch_dirs.push(dl_dir);
                }
            }
            for src_dir in watch_dirs {
                let Ok(rd) = fs::read_dir(&src_dir) else {
                    continue;
                };
                for entry in rd.flatten() {
                    let path = entry.path();
                    let Ok(meta) = entry.metadata() else {
                        continue;
                    };
                    if !meta.is_file() {
                        continue;
                    }
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if ext.is_empty() || is_temp_download_file(&file_name, &ext) {
                        continue;
                    }
                    if !force_existing {
                        let already = shared
                            .try_borrow()
                            .map(|s| s.routed_files.contains(&path))
                            .unwrap_or(false);
                        if already {
                            continue;
                        }
                    }
                    if !is_file_ready_for_routing(&path, &meta, baseline, force_existing) {
                        continue;
                    }
                    let matched_dst = active_rules
                        .iter()
                        .find(|(_, _, exts, _)| exts.iter().any(|e| e == &ext || e == "*"))
                        .map(|(_, dst, _, _)| dst.clone());
                    let Some(dst_folder) = matched_dst else {
                        continue;
                    };
                    if path.parent().map(|p| p == dst_folder.as_path()).unwrap_or(false) {
                        continue;
                    }
                    if let Ok(new_path) = move_entry_to_folder(&path, &dst_folder) {
                        crate::logger::info(
                            "fences-rule",
                            format!("Auto-routed {:?} -> {:?}", path, new_path),
                        );
                        if let Ok(mut sh) = shared.try_borrow_mut() {
                            sh.routed_files.insert(new_path);
                        }
                    }
                }
            }
        }
    }

    // Check folder signatures for all visible fences and reload any that changed
    let wins = shared
        .try_borrow()
        .map(|s| s.windows.clone())
        .unwrap_or_default();
    for w in wins {
        let (spec_id, folder, visible) = {
            let Ok(b) = w.try_borrow() else {
                continue;
            };
            let Some(spec) = b.spec() else {
                continue;
            };
            (b.spec_id.clone(), PathBuf::from(spec.folder), b.visible)
        };
        if !visible {
            continue;
        }
        let sig = folder_signature(&folder);
        let changed = {
            let Ok(mut sh) = shared.try_borrow_mut() else {
                continue;
            };
            let prev = sh.folder_sigs.insert(spec_id, sig);
            prev != Some(sig)
        };
        if changed {
            if let Ok(mut b) = w.try_borrow_mut() {
                b.reload_entries();
                b.paint();
            }
        }
    }
}

unsafe fn build_fences_settings_rows(config: &FencesConfig) -> Vec<crate::settings_ui::Row> {
    let mut rows = vec![
        crate::settings_ui::Row::section("GLOBAL AUTO-ROUTE"),
        crate::settings_ui::Row::toggle(
            1,
            "Auto-Route Rules (Desktop & Downloads)",
            "Automatically sort newly downloaded or added files into matching Fences",
            config.rules_enabled,
        ),
        crate::settings_ui::Row::toggle(
            2,
            "Watch Windows Downloads Folder",
            "Route newly downloaded images, installers & files from Downloads into Fences",
            config.watch_downloads,
        ),
        crate::settings_ui::Row::accent_button(
            3,
            "Run Auto-Route Rules Now on Desktop & Downloads",
            0x4f8cff,
        ),
        crate::settings_ui::Row::button(
            4,
            "Reset Default 3 Rules (Images, PC Programs, Downloads)",
        ),
        crate::settings_ui::Row::section("FENCE RULES & EXTENSIONS"),
    ];
    for (i, f) in config.fences.iter().enumerate() {
        let base_id = 100 + (i as u32) * 10;
        let rule_summary = if f.auto_rule_exts.trim().is_empty() {
            "No rule extensions configured".to_string()
        } else {
            format!("Extensions: {}", f.auto_rule_exts)
        };
        rows.push(crate::settings_ui::Row::toggle(
            base_id,
            format!("Rule: {}", f.title),
            rule_summary,
            f.auto_rule_enabled && !f.auto_rule_exts.trim().is_empty(),
        ));
        rows.push(crate::settings_ui::Row::button(
            base_id + 1,
            format!("Edit \"{}\" Rule Extensions...", f.title),
        ));
    }
    rows
}

unsafe fn open_fences_settings(shared: &Rc<RefCell<SharedState>>) {
    let (notify_hwnd, glass, rows) = {
        let Ok(sh) = shared.try_borrow() else {
            return;
        };
        let hwnd = sh
            .windows
            .first()
            .and_then(|w| w.try_borrow().ok().map(|b| b.hwnd))
            .unwrap_or(null_mut());
        (hwnd, Rc::clone(&sh.glass), build_fences_settings_rows(&sh.config))
    };
    if notify_hwnd.is_null() {
        return;
    }
    let Ok(mut sh) = shared.try_borrow_mut() else {
        return;
    };
    if sh.settings_ui.is_none() {
        sh.settings_ui = crate::settings_ui::Host::create(
            notify_hwnd,
            "Desktop Fences Rules",
            "3 Built-in Rules (Images, PC Programs, Downloads) + Custom Fence Rules",
            glass,
            rows.clone(),
            FENCES_SETTINGS_CHANGED,
        )
        .ok();
    }
    if let Some(ui) = &mut sh.settings_ui {
        ui.set_rows(rows);
        ui.show();
    }
}

unsafe fn handle_fences_settings_changed(shared: &Rc<RefCell<SharedState>>) {
    let changes = {
        let Ok(mut sh) = shared.try_borrow_mut() else {
            return;
        };
        let Some(ui) = &mut sh.settings_ui else {
            return;
        };
        ui.take_changes()
    };
    if changes.is_empty() {
        return;
    }

    let mut edit_idx: Option<(usize, String, String)> = None;
    let mut run_now = false;
    let prompt_owner: HWND;
    {
        let Ok(mut sh) = shared.try_borrow_mut() else {
            return;
        };
        prompt_owner = sh
            .settings_ui
            .as_ref()
            .map(|u| u.hwnd())
            .unwrap_or(null_mut());
        for change in changes {
            let id = change.id;
            let bval = matches!(change.value, crate::settings_ui::Value::Bool(true));
            match id {
                1 => {
                    sh.config.rules_enabled = bval;
                    sh.watch_baseline = SystemTime::now();
                }
                2 => {
                    sh.config.watch_downloads = bval;
                    sh.watch_baseline = SystemTime::now();
                }
                3 => {
                    run_now = true;
                }
                4 => {
                    for f in &mut sh.config.fences {
                        let (en, exts) = model::default_rule_for_fence(&f.id, &f.title);
                        if en {
                            f.auto_rule_enabled = true;
                            f.auto_rule_exts = exts;
                        }
                    }
                    sh.config.rules_enabled = true;
                    sh.config.watch_downloads = true;
                }
                _ if id >= 100 => {
                    let idx = ((id - 100) / 10) as usize;
                    let sub = (id - 100) % 10;
                    if let Some(f) = sh.config.fences.get_mut(idx) {
                        if sub == 0 {
                            f.auto_rule_enabled = bval;
                            if f.auto_rule_enabled && f.auto_rule_exts.trim().is_empty() {
                                let (_, def_exts) = model::default_rule_for_fence(&f.id, &f.title);
                                if !def_exts.is_empty() {
                                    f.auto_rule_exts = def_exts;
                                }
                            }
                        } else if sub == 1 {
                            edit_idx = Some((idx, f.title.clone(), f.auto_rule_exts.clone()));
                        }
                    }
                }
                _ => {}
            }
        }
        let root = sh.root.clone();
        let _ = sh.config.save(&root);
    }

    if let Some((idx, title, current_exts)) = edit_idx {
        let prompt_title = format!("Rule Extensions for \"{title}\" (comma-separated)");
        if let Some(updated) = crate::text_prompt::choose(prompt_owner, &prompt_title, &current_exts) {
            if let Ok(mut sh) = shared.try_borrow_mut() {
                if let Some(f) = sh.config.fences.get_mut(idx) {
                    let cleaned = updated.trim().to_string();
                    f.auto_rule_exts = cleaned.clone();
                    f.auto_rule_enabled = !cleaned.is_empty();
                }
                let root = sh.root.clone();
                let _ = sh.config.save(&root);
            }
        }
    }

    if run_now {
        auto_watch_and_route(shared, true);
    }

    let rows = shared
        .try_borrow()
        .ok()
        .map(|sh| build_fences_settings_rows(&sh.config));
    if let (Some(rows), Ok(mut sh)) = (rows, shared.try_borrow_mut()) {
        if let Some(ui) = &mut sh.settings_ui {
            ui.set_rows(rows);
        }
    }
}

unsafe fn create_marquee_overlay() -> HWND {
    let class = wide(MARQUEE_CLASS);
    let instance = GetModuleHandleW(null());
    let wc = WNDCLASSW {
        lpfnWndProc: Some(marquee_proc),
        hInstance: instance,
        hCursor: LoadCursorW(null_mut(), IDC_SIZEALL),
        hbrBackground: CreateSolidBrush(0x00ff8c4f),
        lpszClassName: class.as_ptr(),
        ..zeroed()
    };
    let _ = RegisterClassW(&wc);
    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        class.as_ptr(),
        wide("").as_ptr(),
        WS_POPUP | WS_BORDER,
        0,
        0,
        0,
        0,
        null_mut(),
        null_mut(),
        instance,
        null(),
    );
    if !hwnd.is_null() {
        SetLayeredWindowAttributes(hwnd, 0, 85, LWA_ALPHA);
    }
    hwnd
}

unsafe extern "system" fn marquee_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(null_mut(), IDC_SIZEALL));
            1
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as isize,
        WM_LBUTTONDOWN => {
            let mut pt: POINT = zeroed();
            GetCursorPos(&mut pt);
            if crate::fusion::begin_cluster_drag(pt) {
                SetCapture(hwnd);
            }
            0
        }
        WM_MOUSEMOVE => {
            let mut pt: POINT = zeroed();
            GetCursorPos(&mut pt);
            crate::fusion::update_cluster_drag(pt);
            0
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED | WM_CANCELMODE => {
            if msg == WM_LBUTTONUP {
                ReleaseCapture();
            }
            crate::fusion::end_cluster_drag();
            0
        }
        WM_RBUTTONDOWN => {
            crate::fusion::clear_widget_selection();
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn register_class() -> Result<(), String> {
    let class = wide(CLASS);
    let instance = GetModuleHandleW(null());
    let wc = WNDCLASSW {
        style: CS_DBLCLKS,
        lpfnWndProc: Some(proc),
        hInstance: instance,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        lpszClassName: class.as_ptr(),
        ..zeroed()
    };
    if RegisterClassW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

unsafe fn create_fence_window(
    spec: &FenceSpec,
    shared: Rc<RefCell<SharedState>>,
    inspect: bool,
) -> Result<Rc<RefCell<FenceWindow>>, String> {
    let dpi = shared.borrow().dpi;
    let w = (spec.size[0].max(200) as f32 * dpi).round() as i32;
    let h = if spec.rolled_up {
        (HEADER_H * dpi).round() as i32
    } else {
        (spec.size[1].max(120) as f32 * dpi).round() as i32
    };
    let initial_alpha = if spec.fade_on_hover {
        spec.idle_opacity.clamp(0.05, 1.0)
    } else {
        1.0
    };
    let win = Rc::new(RefCell::new(FenceWindow {
        hwnd: null_mut(),
        spec_id: spec.id.clone(),
        shared: Rc::clone(&shared),
        entries: Vec::new(),
        icons: HashMap::new(),
        frame: None,
        background: None,
        background_key: String::new(),
        shadow: None,
        hovered: false,
        hover_item: None,
        hover_roll: false,
        hover_add: false,
        selected: HashSet::new(),
        marquee: None,
        item_drag: None,
        group_selected: false,
        scroll_y: 0.0,
        alpha_tween: Tween::fixed(initial_alpha),
        started: Instant::now(),
        dragging: None,
        resizing: None,
        visible: spec.visible,
    }));
    win.borrow_mut().reload_entries();

    let raw = Rc::into_raw(Rc::clone(&win));
    let hwnd = CreateWindowExW(
        crate::gpu::window_ex_style()
            | if inspect {
                WS_EX_APPWINDOW
            } else {
                WS_EX_TOOLWINDOW
            },
        wide(CLASS).as_ptr(),
        wide(&format!("Lumen · {}", spec.title)).as_ptr(),
        WS_POPUP,
        spec.position[0],
        spec.position[1],
        w,
        h,
        null_mut(),
        null_mut(),
        GetModuleHandleW(null()),
        raw as _,
    );
    if hwnd.is_null() {
        let _ = Rc::from_raw(raw);
        return Err(std::io::Error::last_os_error().to_string());
    }
    win.borrow_mut().hwnd = hwnd;
    DragAcceptFiles(hwnd, 1);
    let _ = ChangeWindowMessageFilterEx(hwnd, WM_DROPFILES, 1, null_mut());
    let _ = ChangeWindowMessageFilterEx(hwnd, 0x0049, 1, null_mut());
    crate::desktop_layer::register(hwnd, true);
    crate::fusion::register_widget(hwnd);
    if spec.visible && !shared.borrow().smoke {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        crate::desktop_layer::pin(hwnd);
        win.borrow_mut().paint();
    }
    Ok(win)
}

impl FenceWindow {
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }

    fn spec(&self) -> Option<FenceSpec> {
        self.shared
            .borrow()
            .config
            .fences
            .iter()
            .find(|f| f.id == self.spec_id)
            .cloned()
    }

    fn update_spec(&mut self, f: impl FnOnce(&mut FenceSpec)) {
        let root = self.shared.borrow().root.clone();
        {
            let mut sh = self.shared.borrow_mut();
            if let Some(spec) = sh.config.fences.iter_mut().find(|x| x.id == self.spec_id) {
                f(spec);
            }
            let _ = sh.config.save(&root);
        }
    }

    unsafe fn reload_entries(&mut self) {
        self.entries.clear();
        let Some(spec) = self.spec() else {
            return;
        };
        let Ok(rd) = std::fs::read_dir(&spec.folder) else {
            return;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.eq_ignore_ascii_case("desktop.ini") || name.starts_with('.') {
                continue;
            }
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
            if spec.filter == "folders" && !is_dir {
                continue;
            }
            if spec.filter == "files" && is_dir {
                continue;
            }
            let modified = meta
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            let display = if name.to_ascii_lowercase().ends_with(".lnk") {
                name[..name.len() - 4].to_string()
            } else {
                name.to_string()
            };
            self.entries.push(FenceEntry {
                name: display,
                path,
                is_dir,
                modified,
            });
        }
        if spec.sort_by == "manual" && !spec.custom_order.is_empty() {
            let order_map: HashMap<String, usize> = spec
                .custom_order
                .iter()
                .enumerate()
                .map(|(i, k)| (k.to_lowercase(), i))
                .collect();
            self.entries.sort_by(|a, b| {
                let ia = order_map.get(&a.file_key()).copied().unwrap_or(usize::MAX);
                let ib = order_map.get(&b.file_key()).copied().unwrap_or(usize::MAX);
                ia.cmp(&ib)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
        } else if spec.sort_by == "date" {
            self.entries.sort_by(|a, b| {
                let ord = a.modified.cmp(&b.modified);
                let ord = if spec.sort_desc { ord.reverse() } else { ord };
                if spec.folders_first {
                    b.is_dir.cmp(&a.is_dir).then(ord)
                } else {
                    ord
                }
            });
        } else {
            self.entries.sort_by(|a, b| {
                let name_ord = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                if spec.folders_first {
                    b.is_dir.cmp(&a.is_dir).then(name_ord)
                } else {
                    name_ord
                }
            });
        }
    }

    unsafe fn icon_for(&mut self, path: &Path) -> Option<&render::Image> {
        let show_thumbs = self.spec().is_none_or(|s| s.show_thumbnails);
        let key = format!(
            "{}#thumb={}",
            path.to_string_lossy(),
            if show_thumbs { 1 } else { 0 }
        );
        if !self.icons.contains_key(&key) {
            if show_thumbs && is_image_file(path) {
                if let Some(thumb) = load_image_thumbnail(path, 96) {
                    self.icons.insert(key.clone(), thumb);
                    return self.icons.get(&key);
                }
            }
            let raw_path = path.to_string_lossy().into_owned();
            let wpath = wide(&raw_path);
            let mut info: SHFILEINFOW = zeroed();
            let mut hicon: HICON = null_mut();
            if SHGetFileInfoW(
                wpath.as_ptr(),
                0,
                &mut info,
                size_of::<SHFILEINFOW>() as u32,
                SHGFI_SYSICONINDEX,
            ) != 0
            {
                let mut ilist: *mut std::ffi::c_void = null_mut();
                let iid = windows_sys::core::GUID::from_u128(0x46eb5926_582e_4017_9fdf_e8998daa0950);
                if SHGetImageList(SHIL_EXTRALARGE as i32, &iid, &mut ilist) == 0 && !ilist.is_null() {
                    type GetIconFn = unsafe extern "system" fn(*mut std::ffi::c_void, i32, u32, *mut HICON) -> i32;
                    let vtbl = *(ilist as *const *const usize);
                    let get_icon: GetIconFn = std::mem::transmute(*vtbl.add(10));
                    let _ = get_icon(ilist, info.iIcon, 0x0001, &mut hicon);
                    type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
                    let release: ReleaseFn = std::mem::transmute(*vtbl.add(2));
                    let _ = release(ilist);
                }
            }
            if hicon.is_null() {
                let mut info2: SHFILEINFOW = zeroed();
                SHGetFileInfoW(
                    wpath.as_ptr(),
                    0,
                    &mut info2,
                    size_of::<SHFILEINFOW>() as u32,
                    SHGFI_ICON | SHGFI_LARGEICON,
                );
                hicon = info2.hIcon;
            }
            if !hicon.is_null() {
                let img = render::Image::from_icon(hicon);
                DestroyIcon(hicon);
                if let Some(i) = img {
                    self.icons.insert(key.clone(), i);
                }
            }
        }
        self.icons.get(&key)
    }

    unsafe fn set_hover(&mut self, hovered: bool) {
        if self.hovered == hovered {
            return;
        }
        self.hovered = hovered;
        let Some(spec) = self.spec() else {
            return;
        };
        let target = if !spec.fade_on_hover || hovered {
            1.0
        } else {
            spec.idle_opacity.clamp(0.05, 1.0)
        };
        let now = self.now();
        self.alpha_tween.target(target, now, 180.0, EASE);
        SetTimer(self.hwnd, FADE_TIMER, 8, None);
        InvalidateRect(self.hwnd, null(), 0);
    }

    unsafe fn toggle_roll(&mut self) {
        let mut rolled = false;
        let mut target_w = 340;
        let mut target_h = 240;
        self.update_spec(|s| {
            s.rolled_up = !s.rolled_up;
            rolled = s.rolled_up;
            target_w = s.size[0].max(200);
            target_h = s.size[1].max(120);
        });
        let dpi = self.shared.borrow().dpi;
        let w = (target_w as f32 * dpi).round() as i32;
        let h = if rolled {
            (HEADER_H * dpi).round() as i32
        } else {
            (target_h as f32 * dpi).round() as i32
        };
        self.background_key.clear();
        SetWindowPos(
            self.hwnd,
            null_mut(),
            0,
            0,
            w,
            h,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        self.paint();
    }

    unsafe fn paint(&mut self) {
        let Some(spec) = self.spec() else {
            return;
        };
        let dpi = (GetDpiForWindow(self.hwnd).max(96) as f32) / 96.0;
        let mut r: RECT = zeroed();
        GetClientRect(self.hwnd, &mut r);
        if r.right <= 0 || r.bottom <= 0 {
            return;
        }
        let w = r.right as f32 / dpi;
        let h = r.bottom as f32 / dpi;
        let now = self.now();
        let alpha_f = if spec.fade_on_hover {
            self.alpha_tween.sample(now).clamp(0.05, 1.0)
        } else {
            1.0
        };
        let alpha_u8 = (alpha_f * 255.0).round() as u8;

        let mut frame = match self.frame.take() {
            Some(f) if f.width == r.right && f.height == r.bottom && (f.dpi - dpi).abs() < 0.01 => f,
            _ => match Buffer::new(r.right, r.bottom, dpi, RADIUS) {
                Some(f) => f,
                None => return,
            },
        };

        let bg_key = {
            let wr = crate::fusion::window_rect(self.hwnd);
            let glass = self.shared.borrow().glass.clone();
            let g = glass.borrow();
            format!(
                "{}:{}:{}:{}:{}:{}:{}",
                wr.left,
                wr.top,
                r.right,
                r.bottom,
                (dpi * 100.0).round() as i32,
                crate::glass::frosted_glass(),
                g.revision()
            )
        };
        if self.background_key != bg_key
            || self
                .background
                .as_ref()
                .is_none_or(|b| b.width != r.right || b.height != r.bottom)
        {
            self.shared.borrow().glass.borrow().draw_blended(
                frame.dc,
                self.hwnd,
                r.right,
                r.bottom,
                115,
            );
            if let Some(bg) = self
                .background
                .take()
                .filter(|b| b.width == r.right && b.height == r.bottom)
                .or_else(|| Buffer::plain(r.right, r.bottom, dpi))
            {
                bg.copy_from(frame.dc);
                self.background = Some(bg);
            }
            self.background_key = bg_key;
        } else if let Some(bg) = &self.background {
            frame.copy_from(bg.dc);
        }

        let mut text_draws: Vec<(String, [i32; 4], i32, i32, u32, u32)> = Vec::new();
        let batch = render::AlphaBatch::new(frame.dc, dpi);
        {
            let c = Canvas::new(frame.dc, dpi);
            c.round([0.0, 0.0, w, h], RADIUS, 0x0b0f1c, 150, false, false);
            if self.group_selected || crate::fusion::is_widget_selected(self.hwnd) {
                c.round([1.0, 1.0, w - 1.0, h - 1.0], RADIUS, 0x7c9cff, 235, true, false);
                c.round([2.0, 2.0, w - 2.0, h - 2.0], RADIUS, 0x7c9cff, 160, true, false);
            } else {
                c.round([0.5, 0.5, w - 0.5, h - 0.5], RADIUS, 0xffffff, 38, true, false);
            }
            c.round([1.0, 1.0, w - 1.0, HEADER_H], RADIUS, 0x05070e, 95, false, false);

            let title_str = format!("{}  ({})", spec.title, self.entries.len());
            text_draws.push((
                title_str,
                [
                    (14.0 * dpi).round() as i32,
                    (2.0 * dpi).round() as i32,
                    ((w - 68.0) * dpi).round() as i32,
                    ((HEADER_H - 2.0) * dpi).round() as i32,
                ],
                (15.0 * dpi).round() as i32,
                600,
                0xffffff,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
            ));

            let add_bg_alpha = if self.hover_add { 45 } else { 0 };
            if add_bg_alpha > 0 {
                c.round([w - 62.0, 5.0, w - 36.0, 29.0], 6.0, 0xffffff, add_bg_alpha, false, false);
            }
            text_draws.push((
                "+".into(),
                [
                    ((w - 62.0) * dpi).round() as i32,
                    (3.0 * dpi).round() as i32,
                    ((w - 36.0) * dpi).round() as i32,
                    (29.0 * dpi).round() as i32,
                ],
                (17.0 * dpi).round() as i32,
                600,
                0xffffff,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            ));

            let roll_bg_alpha = if self.hover_roll { 45 } else { 0 };
            if roll_bg_alpha > 0 {
                c.round([w - 34.0, 5.0, w - 8.0, 29.0], 6.0, 0xffffff, roll_bg_alpha, false, false);
            }
            let glyph = if spec.rolled_up { "▸" } else { "▾" };
            text_draws.push((
                glyph.into(),
                [
                    ((w - 34.0) * dpi).round() as i32,
                    (4.0 * dpi).round() as i32,
                    ((w - 8.0) * dpi).round() as i32,
                    (30.0 * dpi).round() as i32,
                ],
                (15.0 * dpi).round() as i32,
                600,
                0xe8e8e8,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            ));

            if !spec.rolled_up {
                let (icon_sz, cell_w, cell_h) = cell_dims(&spec);
                let cols = ((w - PAD_X * 2.0) / cell_w).floor().max(1.0) as usize;
                let total_rows = (self.entries.len() + cols - 1) / cols;
                let view_h = (h - HEADER_H - 8.0).max(0.0);
                let max_scroll = (total_rows as f32 * cell_h - view_h).max(0.0);
                if self.scroll_y > max_scroll {
                    self.scroll_y = max_scroll;
                }
                let start_x = (PAD_X + ((w - PAD_X * 2.0) - cols as f32 * cell_w) * 0.5).round();
                let entry_count = self.entries.len();
                let tile_font_px = (spec.text_size.clamp(8, 22) as f32 * dpi).round() as i32;
                let active_drag_idx = self.item_drag.filter(|d| d.active).map(|d| d.idx);
                for idx in 0..entry_count {
                    let row = idx / cols;
                    let col = idx % cols;
                    let x = (start_x + col as f32 * cell_w).round();
                    let y = (HEADER_H + 6.0 + row as f32 * cell_h - self.scroll_y).round();
                    if y + cell_h < HEADER_H || y > h {
                        continue;
                    }
                    if active_drag_idx.is_some() && self.hover_item == Some(idx) {
                        c.round([x + 2.0, y + 2.0, x + cell_w - 2.0, y + cell_h - 2.0], 10.0, 0x4f8cff, 70, false, false);
                        c.round([x + 2.0, y + 2.0, x + cell_w - 2.0, y + cell_h - 2.0], 10.0, 0x7c9cff, 220, true, false);
                    } else if self.selected.contains(&idx) {
                        c.round([x + 3.0, y + 2.0, x + cell_w - 3.0, y + cell_h - 2.0], 10.0, 0x7c9cff, 65, false, false);
                    } else if self.hover_item == Some(idx) {
                        c.round([x + 3.0, y + 2.0, x + cell_w - 3.0, y + cell_h - 2.0], 10.0, 0xffffff, 35, false, false);
                    }
                    let path = self.entries[idx].path.clone();
                    let name = self.entries[idx].name.clone();
                    if let Some(img) = self.icon_for(&path) {
                        let ix = (x + (cell_w - icon_sz) * 0.5).round();
                        c.image(img, [ix, y + 6.0, ix + icon_sz, y + 6.0 + icon_sz]);
                    }
                    text_draws.push((
                        name,
                        [
                            ((x + 2.0) * dpi).round() as i32,
                            ((y + 8.0 + icon_sz) * dpi).round() as i32,
                            ((x + cell_w - 2.0) * dpi).round() as i32,
                            ((y + cell_h - 1.0) * dpi).round() as i32,
                        ],
                        tile_font_px,
                        400,
                        if active_drag_idx == Some(idx) { 0x99aacc } else { 0xffffff },
                        DT_CENTER | DT_WORDBREAK | DT_END_ELLIPSIS | DT_NOPREFIX,
                    ));
                }
                if let Some(idrag) = self.item_drag.filter(|d| d.active) {
                    let has_ghost_win = self
                        .shared
                        .try_borrow()
                        .map(|s| !s.drag_ghost_hwnd.is_null())
                        .unwrap_or(false);
                    if !has_ghost_win && idrag.idx < self.entries.len() {
                        let gx = (idrag.cur_pt[0] - cell_w * 0.5).round();
                        let gy = (idrag.cur_pt[1] - icon_sz * 0.5).round();
                        c.round([gx, gy, gx + cell_w, gy + cell_h], 10.0, 0x162038, 195, false, false);
                        c.round([gx, gy, gx + cell_w, gy + cell_h], 10.0, 0x7c9cff, 230, true, false);
                        let path = self.entries[idrag.idx].path.clone();
                        let name = self.entries[idrag.idx].name.clone();
                        if let Some(img) = self.icon_for(&path) {
                            let ix = (gx + (cell_w - icon_sz) * 0.5).round();
                            c.image(img, [ix, gy + 6.0, ix + icon_sz, gy + 6.0 + icon_sz]);
                        }
                        text_draws.push((
                            name,
                            [
                                ((gx + 2.0) * dpi).round() as i32,
                                ((gy + 8.0 + icon_sz) * dpi).round() as i32,
                                ((gx + cell_w - 2.0) * dpi).round() as i32,
                                ((gy + cell_h - 1.0) * dpi).round() as i32,
                            ],
                            tile_font_px,
                            600,
                            0xffffff,
                            DT_CENTER | DT_WORDBREAK | DT_END_ELLIPSIS | DT_NOPREFIX,
                        ));
                    }
                }
                if let Some([sx, sy, cx, cy]) = self.marquee {
                    let ml = sx.min(cx);
                    let mt = sy.min(cy);
                    let mr = sx.max(cx);
                    let mb = sy.max(cy);
                    if mr - ml > 2.0 && mb - mt > 2.0 {
                        c.round([ml, mt, mr, mb], 4.0, 0x4f8cff, 55, false, false);
                        c.round([ml, mt, mr, mb], 4.0, 0x7c9cff, 180, true, false);
                    }
                }
                c.round([w - 11.0, h - 11.0, w - 5.0, h - 5.0], 2.0, 0xffffff, 55, false, false);
            }
        }
        drop(batch);

        for (txt, px_rect, px_h, weight, color, flags) in &text_draws {
            draw_cleartype_text(frame.dc, txt, *px_rect, *px_h, *weight, *color, *flags);
        }

        frame.present_with_alpha_opacity(self.hwnd, false, alpha_u8);
        self.frame = Some(frame);

        if self.shadow.is_none() {
            self.shadow = WindowShadow::new(self.hwnd);
        }
        if let Some(shadow) = &mut self.shadow {
            if self.resizing.is_some() {
                shadow.hide();
            } else {
                shadow.sync(self.hwnd, dpi, alpha_u8);
            }
        }
    }

    unsafe fn hit_item(&self, lx: f32, ly: f32) -> Option<usize> {
        let spec = self.spec()?;
        if spec.rolled_up || ly < HEADER_H {
            return None;
        }
        let dpi = (GetDpiForWindow(self.hwnd).max(96) as f32) / 96.0;
        let mut r: RECT = zeroed();
        GetClientRect(self.hwnd, &mut r);
        let w = r.right as f32 / dpi;
        let (_, cell_w, cell_h) = cell_dims(&spec);
        let cols = ((w - PAD_X * 2.0) / cell_w).floor().max(1.0) as usize;
        let start_x = PAD_X + ((w - PAD_X * 2.0) - cols as f32 * cell_w) * 0.5;
        if lx < start_x || lx >= start_x + cols as f32 * cell_w {
            return None;
        }
        let col = ((lx - start_x) / cell_w) as usize;
        let row = ((ly - HEADER_H - 6.0 + self.scroll_y) / cell_h).floor();
        if row < 0.0 {
            return None;
        }
        let idx = row as usize * cols + col;
        if idx < self.entries.len() {
            Some(idx)
        } else {
            None
        }
    }

    unsafe fn hit_slot(&self, lx: f32, ly: f32) -> Option<usize> {
        if self.entries.is_empty() {
            return None;
        }
        let spec = self.spec()?;
        if spec.rolled_up || ly < HEADER_H {
            return None;
        }
        let dpi = (GetDpiForWindow(self.hwnd).max(96) as f32) / 96.0;
        let mut r: RECT = zeroed();
        GetClientRect(self.hwnd, &mut r);
        let w = r.right as f32 / dpi;
        let (_, cell_w, cell_h) = cell_dims(&spec);
        let cols = ((w - PAD_X * 2.0) / cell_w).floor().max(1.0) as usize;
        let start_x = PAD_X + ((w - PAD_X * 2.0) - cols as f32 * cell_w) * 0.5;
        let col = (((lx - start_x) / cell_w).floor().max(0.0) as usize).min(cols - 1);
        let row = ((ly - HEADER_H - 6.0 + self.scroll_y) / cell_h).floor().max(0.0) as usize;
        let idx = (row * cols + col).min(self.entries.len() - 1);
        Some(idx)
    }

    unsafe fn open_path(path: &Path) {
        let p = wide(path);
        ShellExecuteW(
            null_mut(),
            wide("open").as_ptr(),
            p.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        );
    }

    unsafe fn reveal_path(path: &Path) {
        let arg = format!("/select,\"{}\"", path.to_string_lossy());
        ShellExecuteW(
            null_mut(),
            wide("open").as_ptr(),
            wide("explorer.exe").as_ptr(),
            wide(&arg).as_ptr(),
            null(),
            SW_SHOWNORMAL,
        );
    }

    unsafe fn delete_selected_for_window(win: &RefCell<FenceWindow>) {
        let paths = {
            let Ok(b) = win.try_borrow() else {
                return;
            };
            if b.selected.is_empty() {
                return;
            }
            let mut idxs: Vec<usize> = b.selected.iter().copied().collect();
            idxs.sort_unstable();
            let mut out = Vec::new();
            for idx in idxs {
                if let Some(e) = b.entries.get(idx) {
                    out.push(e.path.clone());
                }
            }
            out
        };
        let mut any_deleted = false;
        for p in paths {
            if delete_entry_path(&p).is_ok() {
                any_deleted = true;
            }
        }
        if any_deleted {
            if let Ok(mut b) = win.try_borrow_mut() {
                b.selected.clear();
                b.hover_item = None;
                b.reload_entries();
                b.paint();
            }
        }
    }

    unsafe fn pick_files_dialog(hwnd: HWND) -> Vec<PathBuf> {
        let mut buffer = vec![0u16; 32768];
        let filter = wide(
            "Programs, Shortcuts & All Files\0*.*\0Applications & Shortcuts (*.exe;*.lnk;*.url)\0*.exe;*.lnk;*.url\0",
        );
        let title = wide("Select Programs, Shortcuts or Files to Add to Fence");
        let mut dialog: OPENFILENAMEW = zeroed();
        dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        dialog.hwndOwner = hwnd;
        dialog.lpstrFilter = filter.as_ptr();
        dialog.lpstrFile = buffer.as_mut_ptr();
        dialog.nMaxFile = buffer.len() as u32;
        dialog.lpstrTitle = title.as_ptr();
        dialog.Flags = OFN_NODEREFERENCELINKS
            | OFN_EXPLORER
            | OFN_FILEMUSTEXIST
            | OFN_PATHMUSTEXIST
            | OFN_NOCHANGEDIR
            | OFN_ALLOWMULTISELECT;
        if GetOpenFileNameW(&mut dialog) == 0 {
            return Vec::new();
        }
        let parts: Vec<String> = buffer
            .split(|c| *c == 0)
            .take_while(|s| !s.is_empty())
            .map(String::from_utf16_lossy)
            .collect();
        if parts.len() <= 1 {
            parts.into_iter().map(PathBuf::from).collect()
        } else {
            parts[1..]
                .iter()
                .map(|p| PathBuf::from(&parts[0]).join(p))
                .collect()
        }
    }

    unsafe fn browse_folder_dialog(hwnd: HWND, prompt: &str) -> Option<PathBuf> {
        let initialized = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0;
        let title = wide(prompt);
        let mut display = [0u16; 260];
        let mut browse: BROWSEINFOW = zeroed();
        browse.hwndOwner = hwnd;
        browse.pszDisplayName = display.as_mut_ptr();
        browse.lpszTitle = title.as_ptr();
        browse.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE;
        let selected = SHBrowseForFolderW(&browse);
        let mut out = None;
        if !selected.is_null() {
            let mut path = [0u16; 260];
            if SHGetPathFromIDListW(selected, path.as_mut_ptr()) != 0 {
                let len = path.iter().position(|c| *c == 0).unwrap_or(path.len());
                out = Some(PathBuf::from(String::from_utf16_lossy(&path[..len])));
            }
            CoTaskMemFree(selected as _);
        }
        if initialized {
            CoUninitialize();
        }
        out
    }

    unsafe fn apply_menu_command(&mut self, cmd: usize) {
        let Some(spec) = self.spec() else {
            return;
        };
        match cmd {
            CMD_FADE => {
                let mut fade = false;
                self.update_spec(|s| {
                    s.fade_on_hover = !s.fade_on_hover;
                    fade = s.fade_on_hover;
                });
                let now = self.now();
                let target = if !fade || self.hovered { 1.0 } else { spec.idle_opacity.clamp(0.05, 1.0) };
                self.alpha_tween.target(target, now, 180.0, EASE);
                SetTimer(self.hwnd, FADE_TIMER, 8, None);
                InvalidateRect(self.hwnd, null(), 0);
            }
            CMD_OP_5 | CMD_OP_15 | CMD_OP_25 | CMD_OP_50 | CMD_OP_100 => {
                let val = match cmd {
                    CMD_OP_5 => 0.05,
                    CMD_OP_15 => 0.15,
                    CMD_OP_25 => 0.25,
                    CMD_OP_50 => 0.50,
                    _ => 1.0,
                };
                self.update_spec(|s| {
                    s.idle_opacity = val;
                    s.fade_on_hover = val < 0.99;
                });
                let now = self.now();
                let target = if val >= 0.99 || self.hovered { 1.0 } else { val };
                self.alpha_tween.target(target, now, 180.0, EASE);
                SetTimer(self.hwnd, FADE_TIMER, 8, None);
                InvalidateRect(self.hwnd, null(), 0);
            }
            CMD_ICON_24 | CMD_ICON_32 | CMD_ICON_48 => {
                let sz = match cmd {
                    CMD_ICON_24 => 24,
                    CMD_ICON_48 => 48,
                    _ => 32,
                };
                self.update_spec(|s| s.icon_size = sz);
                InvalidateRect(self.hwnd, null(), 0);
            }
            CMD_TEXT_9 | CMD_TEXT_10 | CMD_TEXT_11 | CMD_TEXT_13 | CMD_TEXT_15 | CMD_TEXT_17 => {
                let tsz = match cmd {
                    CMD_TEXT_9 => 9,
                    CMD_TEXT_10 => 10,
                    CMD_TEXT_11 => 11,
                    CMD_TEXT_15 => 15,
                    CMD_TEXT_17 => 17,
                    _ => 13,
                };
                self.update_spec(|s| s.text_size = tsz);
                InvalidateRect(self.hwnd, null(), 0);
            }
            CMD_VIEW_GRID | CMD_VIEW_GALLERY | CMD_VIEW_COMPACT => {
                let vm = match cmd {
                    CMD_VIEW_GALLERY => "gallery",
                    CMD_VIEW_COMPACT => "compact",
                    _ => "grid",
                };
                self.update_spec(|s| {
                    s.view_mode = vm.into();
                    if vm == "gallery" {
                        s.show_thumbnails = true;
                    }
                });
                self.scroll_y = 0.0;
                InvalidateRect(self.hwnd, null(), 0);
            }
            CMD_TOGGLE_THUMBS => {
                self.update_spec(|s| s.show_thumbnails = !s.show_thumbnails);
                self.icons.clear();
                InvalidateRect(self.hwnd, null(), 0);
            }
            CMD_FILTER_ALL | CMD_FILTER_FOLDERS | CMD_FILTER_FILES => {
                let flt = match cmd {
                    CMD_FILTER_FOLDERS => "folders",
                    CMD_FILTER_FILES => "files",
                    _ => "all",
                };
                self.update_spec(|s| s.filter = flt.into());
                self.scroll_y = 0.0;
                self.reload_entries();
                InvalidateRect(self.hwnd, null(), 0);
            }
            CMD_FOLDERS_FIRST => {
                self.update_spec(|s| s.folders_first = !s.folders_first);
                self.reload_entries();
                InvalidateRect(self.hwnd, null(), 0);
            }
            CMD_ROLL => self.toggle_roll(),
            CMD_SORT_MANUAL => {
                let order: Vec<String> = self.entries.iter().map(|e| e.file_key()).collect();
                self.update_spec(|s| {
                    s.sort_by = "manual".into();
                    s.custom_order = order;
                });
                self.reload_entries();
                InvalidateRect(self.hwnd, null(), 0);
            }
            CMD_SORT_DATE => {
                self.update_spec(|s| {
                    s.sort_by = "date".into();
                    s.sort_desc = true;
                });
                self.reload_entries();
                InvalidateRect(self.hwnd, null(), 0);
            }
            CMD_SORT_NAME => {
                self.update_spec(|s| {
                    s.sort_by = "name".into();
                    s.sort_desc = false;
                });
                self.reload_entries();
                InvalidateRect(self.hwnd, null(), 0);
            }
            CMD_NEW_SUBFOLDER => {
                if let Ok(created) = create_new_subfolder(Path::new(&spec.folder)) {
                    self.reload_entries();
                    if let Some(pos) = self.entries.iter().position(|e| e.path == created) {
                        self.selected.clear();
                        self.selected.insert(pos);
                    }
                    self.paint();
                }
            }
            CMD_REFRESH => {
                self.reload_entries();
                InvalidateRect(self.hwnd, null(), 0);
            }
            CMD_HIDE => {
                self.update_spec(|s| s.visible = false);
                self.visible = false;
                ShowWindow(self.hwnd, SW_HIDE);
                if let Some(sh) = &self.shadow {
                    sh.hide();
                }
            }
            _ => {}
        }
    }
}

unsafe fn execute_menu_command(win: &RefCell<FenceWindow>, cmd: usize) {
    match cmd {
        CMD_ADD_FILES => {
            let (hwnd, folder) = {
                let b = win.borrow();
                let Some(spec) = b.spec() else {
                    return;
                };
                (b.hwnd, spec.folder)
            };
            let paths = FenceWindow::pick_files_dialog(hwnd);
            if paths.is_empty() {
                return;
            }
            let dst_dir = Path::new(&folder);
            let mut added = false;
            for p in paths {
                if import_external_path_into_fence(&p, dst_dir, false).is_ok() {
                    added = true;
                }
            }
            if added {
                let mut b = win.borrow_mut();
                b.reload_entries();
                b.paint();
            }
        }
        CMD_ADD_FOLDER_LINK | CMD_MOVE_FOLDER_IN => {
            let as_shortcut = cmd == CMD_ADD_FOLDER_LINK;
            let (hwnd, folder) = {
                let b = win.borrow();
                let Some(spec) = b.spec() else {
                    return;
                };
                (b.hwnd, spec.folder)
            };
            let prompt = if as_shortcut {
                "Choose a Folder to Add as a Shortcut inside This Fence"
            } else {
                "Choose a Folder to Move into This Fence"
            };
            if let Some(folder_path) = FenceWindow::browse_folder_dialog(hwnd, prompt) {
                let dst_dir = Path::new(&folder);
                if import_external_path_into_fence(&folder_path, dst_dir, as_shortcut).is_ok() {
                    let mut b = win.borrow_mut();
                    b.reload_entries();
                    b.paint();
                }
            }
        }
        CMD_NEW_FENCE_FROM_FOLDER => {
            let (hwnd, shared) = {
                let b = win.borrow();
                (b.hwnd, Rc::clone(&b.shared))
            };
            let Some(folder_path) = FenceWindow::browse_folder_dialog(
                hwnd,
                "Choose any Folder to Create a New Desktop Fence",
            ) else {
                return;
            };
            let title = folder_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Folder")
                .to_string();
            let id = format!(
                "fence-{}",
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(1)
            );
            let mut wr: RECT = zeroed();
            GetWindowRect(hwnd, &mut wr);
            let new_spec = FenceSpec {
                id,
                title,
                folder: folder_path.to_string_lossy().into_owned(),
                position: [(wr.left + 36).min(1400), (wr.top + 36).min(700)],
                size: [360, 260],
                ..FenceSpec::default()
            };
            {
                let root = shared.borrow().root.clone();
                let mut sh = shared.borrow_mut();
                sh.config.fences.push(new_spec.clone());
                let _ = sh.config.save(&root);
            }
            if let Ok(new_win) = create_fence_window(&new_spec, Rc::clone(&shared), false) {
                shared.borrow_mut().windows.push(new_win);
            }
        }
        CMD_OPEN_FOLDER => {
            let folder = win.try_borrow().ok().and_then(|b| b.spec()).map(|s| s.folder);
            if let Some(f) = folder {
                FenceWindow::open_path(Path::new(&f));
            }
        }
        CMD_ITEM_OPEN => {
            let path = win.try_borrow().ok().and_then(|b| {
                let &idx = b.selected.iter().next()?;
                b.entries.get(idx).map(|e| e.path.clone())
            });
            if let Some(p) = path {
                FenceWindow::open_path(&p);
            }
        }
        CMD_ITEM_REVEAL => {
            let path = win.try_borrow().ok().and_then(|b| {
                let &idx = b.selected.iter().next()?;
                b.entries.get(idx).map(|e| e.path.clone())
            });
            if let Some(p) = path {
                FenceWindow::reveal_path(&p);
            }
        }
        CMD_ITEM_DELETE => {
            FenceWindow::delete_selected_for_window(win);
        }
        CMD_RULE_TOGGLE => {
            if let Ok(mut b) = win.try_borrow_mut() {
                b.update_spec(|s| {
                    s.auto_rule_enabled = !s.auto_rule_enabled;
                    if s.auto_rule_enabled && s.auto_rule_exts.trim().is_empty() {
                        let (_, def_exts) = model::default_rule_for_fence(&s.id, &s.title);
                        if !def_exts.is_empty() {
                            s.auto_rule_exts = def_exts;
                        }
                    }
                });
            }
        }
        CMD_RULE_PRESET_IMAGES => {
            if let Ok(mut b) = win.try_borrow_mut() {
                b.update_spec(|s| {
                    s.auto_rule_enabled = true;
                    s.auto_rule_exts = model::RULE_EXTS_IMAGES.into();
                });
            }
        }
        CMD_RULE_PRESET_PROGRAMS => {
            if let Ok(mut b) = win.try_borrow_mut() {
                b.update_spec(|s| {
                    s.auto_rule_enabled = true;
                    s.auto_rule_exts = model::RULE_EXTS_PROGRAMS.into();
                });
            }
        }
        CMD_RULE_PRESET_DOWNLOADS => {
            if let Ok(mut b) = win.try_borrow_mut() {
                b.update_spec(|s| {
                    s.auto_rule_enabled = true;
                    s.auto_rule_exts = model::RULE_EXTS_DOWNLOADS.into();
                });
            }
        }
        CMD_RULE_EDIT_EXTS => {
            let info = win
                .try_borrow()
                .ok()
                .and_then(|b| b.spec().map(|s| (b.hwnd, s.title, s.auto_rule_exts)));
            if let Some((hwnd, title, cur_exts)) = info {
                let prompt = format!("Auto-Route Extensions for \"{title}\" (comma-separated)");
                if let Some(updated) = crate::text_prompt::choose(hwnd, &prompt, &cur_exts) {
                    if let Ok(mut b) = win.try_borrow_mut() {
                        let cleaned = updated.trim().to_string();
                        b.update_spec(|s| {
                            s.auto_rule_exts = cleaned.clone();
                            s.auto_rule_enabled = !cleaned.is_empty();
                        });
                    }
                }
            }
        }
        CMD_RULE_RUN_NOW => {
            let shared = win.try_borrow().ok().map(|b| Rc::clone(&b.shared));
            if let Some(sh) = shared {
                auto_watch_and_route(&sh, true);
            }
        }
        CMD_RULE_OPEN_SETTINGS => {
            let shared = win.try_borrow().ok().map(|b| Rc::clone(&b.shared));
            if let Some(sh) = shared {
                open_fences_settings(&sh);
            }
        }
        _ => {
            if let Ok(mut b) = win.try_borrow_mut() {
                b.apply_menu_command(cmd);
            }
        }
    }
}

unsafe fn run_add_menu(win: &RefCell<FenceWindow>, sx: i32, sy: i32) {
    let hwnd = win.borrow().hwnd;
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    AppendMenuW(menu, MF_STRING, CMD_ADD_FILES, wide("Add Program / App / File...").as_ptr());
    AppendMenuW(menu, MF_STRING, CMD_ADD_FOLDER_LINK, wide("Add Folder Shortcut...").as_ptr());
    AppendMenuW(menu, MF_STRING, CMD_MOVE_FOLDER_IN, wide("Move Folder into This Fence...").as_ptr());
    AppendMenuW(menu, MF_STRING, CMD_NEW_SUBFOLDER, wide("New Subfolder").as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, CMD_NEW_FENCE_FROM_FOLDER, wide("Create New Fence from Folder...").as_ptr());

    SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        sx,
        sy,
        0,
        hwnd,
        null(),
    ) as usize;
    DestroyMenu(menu);
    if cmd != 0 {
        execute_menu_command(win, cmd);
    }
}

unsafe fn run_context_menu(win: &RefCell<FenceWindow>, mut sx: i32, mut sy: i32) {
    let (hwnd, spec, sel_count, first_sel_name) = {
        let mut b = win.borrow_mut();
        if sx != -1 || sy != -1 {
            let mut cpt = POINT { x: sx, y: sy };
            ScreenToClient(b.hwnd, &mut cpt);
            let dpi = (GetDpiForWindow(b.hwnd).max(96) as f32) / 96.0;
            let lx = cpt.x as f32 / dpi;
            let ly = cpt.y as f32 / dpi;
            if let Some(hit_idx) = b.hit_item(lx, ly) {
                if !b.selected.contains(&hit_idx) {
                    b.selected.clear();
                    b.selected.insert(hit_idx);
                    b.paint();
                }
            }
        }
        let first_name = b
            .selected
            .iter()
            .next()
            .and_then(|&i| b.entries.get(i))
            .map(|e| e.name.clone());
        (b.hwnd, b.spec(), b.selected.len(), first_name)
    };
    let Some(spec) = spec else {
        return;
    };
    if sx == -1 && sy == -1 {
        let mut pt: POINT = zeroed();
        GetCursorPos(&mut pt);
        sx = pt.x;
        sy = pt.y;
    }
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    let add_menu = CreatePopupMenu();
    let op_menu = CreatePopupMenu();
    let view_menu = CreatePopupMenu();
    let icon_menu = CreatePopupMenu();
    let text_menu = CreatePopupMenu();
    let flt_menu = CreatePopupMenu();
    let rule_menu = CreatePopupMenu();
    let flag = |b: bool| MF_STRING | if b { MF_CHECKED } else { MF_UNCHECKED };

    let rule_active = spec.auto_rule_enabled && !spec.auto_rule_exts.trim().is_empty();
    let rule_toggle_label = if rule_active {
        format!("Enable Auto-Route Rule ({})", spec.auto_rule_exts)
    } else {
        "Enable Auto-Route Rule for This Fence".to_string()
    };
    AppendMenuW(rule_menu, flag(rule_active), CMD_RULE_TOGGLE, wide(&rule_toggle_label).as_ptr());
    AppendMenuW(rule_menu, MF_SEPARATOR, 0, null());
    AppendMenuW(
        rule_menu,
        flag(spec.auto_rule_enabled && spec.auto_rule_exts == model::RULE_EXTS_IMAGES),
        CMD_RULE_PRESET_IMAGES,
        wide("Preset: Images (png, jpg, webp, gif, svg...)").as_ptr(),
    );
    AppendMenuW(
        rule_menu,
        flag(spec.auto_rule_enabled && spec.auto_rule_exts == model::RULE_EXTS_PROGRAMS),
        CMD_RULE_PRESET_PROGRAMS,
        wide("Preset: PC Programs (exe, msi, lnk, bat...)").as_ptr(),
    );
    AppendMenuW(
        rule_menu,
        flag(spec.auto_rule_enabled && spec.auto_rule_exts == model::RULE_EXTS_DOWNLOADS),
        CMD_RULE_PRESET_DOWNLOADS,
        wide("Preset: Downloads (zip, rar, pdf, docx, mp4...)").as_ptr(),
    );
    AppendMenuW(rule_menu, MF_SEPARATOR, 0, null());
    AppendMenuW(rule_menu, MF_STRING, CMD_RULE_EDIT_EXTS, wide("Custom Extensions...").as_ptr());
    AppendMenuW(rule_menu, MF_STRING, CMD_RULE_RUN_NOW, wide("Run Auto-Route Rules Now").as_ptr());
    AppendMenuW(rule_menu, MF_STRING, CMD_RULE_OPEN_SETTINGS, wide("All Fence Rules Settings...").as_ptr());

    AppendMenuW(add_menu, MF_STRING, CMD_ADD_FILES, wide("Add Program / App / File...").as_ptr());
    AppendMenuW(add_menu, MF_STRING, CMD_ADD_FOLDER_LINK, wide("Add Folder Shortcut...").as_ptr());
    AppendMenuW(add_menu, MF_STRING, CMD_MOVE_FOLDER_IN, wide("Move Folder into This Fence...").as_ptr());
    AppendMenuW(add_menu, MF_STRING, CMD_NEW_SUBFOLDER, wide("New Subfolder").as_ptr());
    AppendMenuW(add_menu, MF_SEPARATOR, 0, null());
    AppendMenuW(add_menu, MF_STRING, CMD_NEW_FENCE_FROM_FOLDER, wide("Create New Fence from Folder...").as_ptr());

    let op_pct = (spec.idle_opacity * 100.0).round() as i32;
    AppendMenuW(op_menu, flag(spec.fade_on_hover && op_pct <= 8), CMD_OP_5, wide("5% (Nearly Hidden)").as_ptr());
    AppendMenuW(op_menu, flag(spec.fade_on_hover && op_pct > 8 && op_pct <= 20), CMD_OP_15, wide("15% (Subtle)").as_ptr());
    AppendMenuW(op_menu, flag(spec.fade_on_hover && op_pct > 20 && op_pct <= 35), CMD_OP_25, wide("25% (Dim)").as_ptr());
    AppendMenuW(op_menu, flag(spec.fade_on_hover && op_pct > 35 && op_pct < 99), CMD_OP_50, wide("50% (Half)").as_ptr());
    AppendMenuW(op_menu, flag(!spec.fade_on_hover || op_pct >= 99), CMD_OP_100, wide("100% (Always Clear)").as_ptr());

    AppendMenuW(view_menu, flag(spec.view_mode == "grid" || spec.view_mode.is_empty()), CMD_VIEW_GRID, wide("Standard Grid").as_ptr());
    AppendMenuW(view_menu, flag(spec.view_mode == "gallery"), CMD_VIEW_GALLERY, wide("Photo Gallery (Large Images)").as_ptr());
    AppendMenuW(view_menu, flag(spec.view_mode == "compact"), CMD_VIEW_COMPACT, wide("Compact Tiles").as_ptr());
    AppendMenuW(view_menu, MF_SEPARATOR, 0, null());
    AppendMenuW(view_menu, flag(spec.show_thumbnails), CMD_TOGGLE_THUMBS, wide("Show Image Thumbnails").as_ptr());

    AppendMenuW(icon_menu, flag(spec.icon_size <= 26), CMD_ICON_24, wide("Small (24px)").as_ptr());
    AppendMenuW(icon_menu, flag(spec.icon_size > 26 && spec.icon_size < 40), CMD_ICON_32, wide("Normal (32px)").as_ptr());
    AppendMenuW(icon_menu, flag(spec.icon_size >= 40), CMD_ICON_48, wide("Large (48px)").as_ptr());

    let ts = if spec.text_size <= 0 { 13 } else { spec.text_size };
    AppendMenuW(text_menu, flag(ts <= 9), CMD_TEXT_9, wide("Extra Small (9px)").as_ptr());
    AppendMenuW(text_menu, flag(ts == 10), CMD_TEXT_10, wide("Compact (10px)").as_ptr());
    AppendMenuW(text_menu, flag(ts == 11), CMD_TEXT_11, wide("Small (11px)").as_ptr());
    AppendMenuW(text_menu, flag(ts > 11 && ts <= 13), CMD_TEXT_13, wide("Normal (13px)").as_ptr());
    AppendMenuW(text_menu, flag(ts > 13 && ts <= 15), CMD_TEXT_15, wide("Large (15px)").as_ptr());
    AppendMenuW(text_menu, flag(ts >= 16), CMD_TEXT_17, wide("Extra Large (17px)").as_ptr());

    AppendMenuW(flt_menu, flag(spec.filter == "all"), CMD_FILTER_ALL, wide("Show All").as_ptr());
    AppendMenuW(flt_menu, flag(spec.filter == "folders"), CMD_FILTER_FOLDERS, wide("Folders Only").as_ptr());
    AppendMenuW(flt_menu, flag(spec.filter == "files"), CMD_FILTER_FILES, wide("Files & Apps Only").as_ptr());
    AppendMenuW(flt_menu, MF_SEPARATOR, 0, null());
    AppendMenuW(flt_menu, flag(spec.sort_by == "name"), CMD_SORT_NAME, wide("Sort by Name").as_ptr());
    AppendMenuW(flt_menu, flag(spec.sort_by == "date"), CMD_SORT_DATE, wide("Sort by Date (Newest First)").as_ptr());
    AppendMenuW(flt_menu, flag(spec.sort_by == "manual"), CMD_SORT_MANUAL, wide("Manual Order (Drag Icons)").as_ptr());
    AppendMenuW(flt_menu, flag(spec.folders_first), CMD_FOLDERS_FIRST, wide("Folders First").as_ptr());

    if sel_count > 0 {
        let open_label = if let Some(ref name) = first_sel_name {
            format!("Open \"{name}\"")
        } else {
            "Open".to_string()
        };
        let del_label = if sel_count > 1 {
            format!("Delete Selected ({sel_count} items)")
        } else if let Some(ref name) = first_sel_name {
            format!("Delete \"{name}\"")
        } else {
            "Delete".to_string()
        };
        AppendMenuW(menu, MF_STRING, CMD_ITEM_OPEN, wide(&open_label).as_ptr());
        AppendMenuW(menu, MF_STRING, CMD_ITEM_REVEAL, wide("Show in Explorer").as_ptr());
        AppendMenuW(menu, MF_STRING, CMD_ITEM_DELETE, wide(&del_label).as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
    }

    AppendMenuW(menu, MF_POPUP, add_menu as usize, wide("Add Folder / Program (+ )").as_ptr());
    AppendMenuW(menu, MF_POPUP, rule_menu as usize, wide("Auto-Route Rule").as_ptr());
    AppendMenuW(menu, MF_POPUP, view_menu as usize, wide("View Mode & Thumbnails").as_ptr());
    AppendMenuW(menu, MF_POPUP, icon_menu as usize, wide("Icon Size").as_ptr());
    AppendMenuW(menu, MF_POPUP, text_menu as usize, wide("Tile Text Size").as_ptr());
    AppendMenuW(menu, MF_POPUP, flt_menu as usize, wide("Filter & Sort").as_ptr());
    AppendMenuW(menu, flag(spec.fade_on_hover), CMD_FADE, wide("Fade on Hover").as_ptr());
    AppendMenuW(menu, MF_POPUP, op_menu as usize, wide("Idle Opacity").as_ptr());
    AppendMenuW(menu, flag(spec.rolled_up), CMD_ROLL, wide("Roll Up to Title Bar").as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, CMD_OPEN_FOLDER, wide("Open Folder in Explorer").as_ptr());
    AppendMenuW(menu, MF_STRING, CMD_REFRESH, wide("Refresh").as_ptr());
    AppendMenuW(menu, MF_STRING, CMD_HIDE, wide("Hide This Fence").as_ptr());

    SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        sx,
        sy,
        0,
        hwnd,
        null(),
    ) as usize;
    DestroyMenu(menu);

    if cmd != 0 {
        execute_menu_command(win, cmd);
    }
}

unsafe fn window_class_name(hwnd: HWND) -> String {
    if hwnd.is_null() {
        return String::new();
    }
    let mut cls = [0u16; 64];
    let len = GetClassNameW(hwnd, cls.as_mut_ptr(), 64).max(0) as usize;
    String::from_utf16_lossy(&cls[..len])
}

unsafe fn is_desktop_point(pt: POINT) -> bool {
    let target = WindowFromPoint(pt);
    if target.is_null() {
        return false;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(target, &mut pid);
    if pid == GetCurrentProcessId() {
        return false;
    }
    let cls = window_class_name(target);
    let root = GetAncestor(target, GA_ROOT);
    let root_cls = window_class_name(root);
    let is_std_desk = |s: &str| {
        matches!(
            s,
            "Progman" | "WorkerW" | "SHELLDLL_DefView" | "SysListView32"
        )
    };
    if is_std_desk(&cls) || is_std_desk(&root_cls) {
        return true;
    }
    let check_hwnd = if !root.is_null() { root } else { target };
    let mut wr: RECT = zeroed();
    GetWindowRect(check_hwnd, &mut wr);
    let sw = GetSystemMetrics(SM_CXSCREEN);
    let sh = GetSystemMetrics(SM_CYSCREEN);
    if (wr.right - wr.left) >= sw && (wr.bottom - wr.top) >= sh {
        // Fullscreen background/live-wallpaper host sitting below desktop-layer widgets
        let mut cur = check_hwnd;
        for _ in 0..512 {
            cur = GetWindow(cur, GW_HWNDPREV);
            if cur.is_null() {
                break;
            }
            if crate::desktop_layer::contains(cur) {
                return true;
            }
        }
    }
    false
}

unsafe fn handle_raw_mouse(lparam: LPARAM, shared: &Rc<RefCell<SharedState>>) {
    let mut raw: RAWINPUT = zeroed();
    let mut sz = size_of::<RAWINPUT>() as u32;
    if GetRawInputData(
        lparam as HRAWINPUT,
        RID_INPUT,
        &mut raw as *mut _ as _,
        &mut sz,
        size_of::<RAWINPUTHEADER>() as u32,
    ) == u32::MAX
    {
        return;
    }
    if raw.header.dwType != RIM_TYPEMOUSE {
        return;
    }
    let flags = raw.data.mouse.Anonymous.Anonymous.usButtonFlags;
    let l_down = (flags & RI_MOUSE_LEFT_BUTTON_DOWN) != 0;
    let l_up = (flags & RI_MOUSE_LEFT_BUTTON_UP) != 0;

    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);

    let mut toggle_hide = false;
    let mut clear_group = false;
    let mut drag_box: Option<[i32; 4]> = None;
    let mut end_drag = false;
    let mhwnd = shared.borrow().marquee_hwnd;

    if l_down {
        if is_desktop_point(pt) {
            let mut sh = shared.borrow_mut();
            sh.desk_drag_start = Some([pt.x, pt.y]);
            clear_group = true;
            let now = Instant::now();
            let dbl_ms = GetDoubleClickTime() as u128;
            let is_dbl = sh.last_desk_click_ms.is_some_and(|prev| {
                now.duration_since(prev).as_millis() <= dbl_ms
                    && (pt.x - sh.last_desk_click_pt[0]).abs() <= 8
                    && (pt.y - sh.last_desk_click_pt[1]).abs() <= 8
            });
            if is_dbl {
                sh.last_desk_click_ms = None;
                sh.desk_drag_start = None;
                sh.quick_hidden = !sh.quick_hidden;
                toggle_hide = true;
            } else {
                sh.last_desk_click_ms = Some(now);
                sh.last_desk_click_pt = [pt.x, pt.y];
            }
        } else {
            let target = WindowFromPoint(pt);
            let root = if !target.is_null() {
                GetAncestor(target, GA_ROOT)
            } else {
                null_mut()
            };
            let clicked_selected = (target == mhwnd && !mhwnd.is_null())
                || (!target.is_null() && crate::fusion::is_widget_selected(target))
                || (!root.is_null() && crate::fusion::is_widget_selected(root));
            if !clicked_selected {
                clear_group = true;
            }
            let mut sh = shared.borrow_mut();
            sh.last_desk_click_ms = None;
            sh.desk_drag_start = None;
        }
    } else if l_up {
        let mut sh = shared.borrow_mut();
        if sh.desk_drag_start.take().is_some() {
            end_drag = true;
        }
    } else {
        let sh = shared.borrow();
        if let Some([sx, sy]) = sh.desk_drag_start {
            if (pt.x - sx).abs() > 8 || (pt.y - sy).abs() > 8 {
                drag_box = Some([sx.min(pt.x), sy.min(pt.y), sx.max(pt.x), sy.max(pt.y)]);
            }
        }
    }

    if clear_group {
        crate::fusion::clear_widget_selection();
        let wins = shared.borrow().windows.clone();
        for w in wins {
            let mut b = w.borrow_mut();
            if b.group_selected {
                b.group_selected = false;
                b.paint();
            }
        }
    }

    if let Some([ml, mt, mr, mb]) = drag_box {
        if !mhwnd.is_null() {
            let ex = GetWindowLongPtrW(mhwnd, GWL_EXSTYLE);
            if ex & (WS_EX_TRANSPARENT as isize) == 0 {
                SetWindowLongPtrW(mhwnd, GWL_EXSTYLE, ex | (WS_EX_TRANSPARENT as isize));
            }
            SetWindowPos(
                mhwnd,
                HWND_TOPMOST,
                ml,
                mt,
                (mr - ml).max(2),
                (mb - mt).max(2),
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        crate::fusion::select_widgets_in_rect([ml, mt, mr, mb]);
        let wins = shared.borrow().windows.clone();
        for w in wins {
            let mut b = w.borrow_mut();
            if !b.visible {
                continue;
            }
            let hit = crate::fusion::is_widget_selected(b.hwnd);
            if b.group_selected != hit {
                b.group_selected = hit;
                b.paint();
            }
        }
    }

    if end_drag {
        crate::fusion::sync_marquee_to_selection();
    }

    if toggle_hide {
        if !mhwnd.is_null() {
            ShowWindow(mhwnd, SW_HIDE);
        }
        let dock_hwnd = crate::window::host_hwnd();
        if !dock_hwnd.is_null() {
            PostMessageW(dock_hwnd, crate::window::TOGGLE_QUICK_HIDE, 0, 0);
            return;
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lp as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        return 1;
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<FenceWindow>;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let win = &*ptr;
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            BeginPaint(hwnd, &mut ps);
            if let Ok(mut b) = win.try_borrow_mut() {
                b.paint();
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_INPUT => {
            if let Ok(b) = win.try_borrow() {
                let shared = b.shared.clone();
                drop(b);
                handle_raw_mouse(lp, &shared);
            }
            0
        }
        WM_TIMER if wp == FADE_TIMER => {
            let Ok(b) = win.try_borrow() else {
                return 0;
            };
            let now = b.now();
            let done = !b.alpha_tween.moving(now);
            drop(b);
            if done {
                KillTimer(hwnd, FADE_TIMER);
            }
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_TIMER if wp == ZORDER_WATCH_TIMER => {
            if let Ok(b) = win.try_borrow() {
                let shared = b.shared.clone();
                drop(b);
                check_and_repair_zorder(&shared);
            }
            0
        }
        WM_SETCURSOR => {
            let dpi = (GetDpiForWindow(hwnd).max(96) as f32) / 96.0;
            let rolled = win
                .try_borrow()
                .ok()
                .is_some_and(|b| b.spec().is_some_and(|s| s.rolled_up));
            let mut pt: POINT = zeroed();
            GetCursorPos(&mut pt);
            ScreenToClient(hwnd, &mut pt);
            let mut r: RECT = zeroed();
            GetClientRect(hwnd, &mut r);
            let edge = hit_resize_edge(pt.x as f32 / dpi, pt.y as f32 / dpi, r.right as f32 / dpi, r.bottom as f32 / dpi, rolled);
            let cursor_id = match edge {
                5 | 10 => IDC_SIZENWSE,
                6 | 9 => IDC_SIZENESW,
                1 | 2 => IDC_SIZEWE,
                4 | 8 => IDC_SIZENS,
                _ => IDC_ARROW,
            };
            SetCursor(LoadCursorW(null_mut(), cursor_id));
            1
        }
        WM_MOUSEMOVE => {
            let Ok(mut b) = win.try_borrow_mut() else {
                return 0;
            };
            b.set_hover(true);
            let mut pt: POINT = zeroed();
            GetCursorPos(&mut pt);
            if let Some([sx, sy, wx, wy]) = b.dragging {
                let dx = pt.x - sx;
                let dy = pt.y - sy;
                let raw_nx = wx + dx;
                let raw_ny = wy + dy;
                let mut cur_r: RECT = zeroed();
                GetWindowRect(hwnd, &mut cur_r);
                let fw = cur_r.right - cur_r.left;
                let fh = cur_r.bottom - cur_r.top;
                let mut others = Vec::new();
                let wins = b.shared.borrow().windows.clone();
                for ow in wins {
                    if let Ok(ob) = ow.try_borrow() {
                        if ob.hwnd != hwnd && ob.visible {
                            let mut or: RECT = zeroed();
                            GetWindowRect(ob.hwnd, &mut or);
                            others.push([or.left, or.top, or.right, or.bottom]);
                        }
                    }
                }
                let mut work = [0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)];
                let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                if !mon.is_null() {
                    let mut mi: MONITORINFO = zeroed();
                    mi.cbSize = size_of::<MONITORINFO>() as u32;
                    if GetMonitorInfoW(mon, &mut mi) != 0 {
                        work = [
                            mi.rcWork.left,
                            mi.rcWork.top,
                            mi.rcWork.right,
                            mi.rcWork.bottom,
                        ];
                    }
                }
                let [nx, ny] = snap_fence_position(raw_nx, raw_ny, fw, fh, &others, work);
                SetWindowPos(hwnd, null_mut(), nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                let dpi = (GetDpiForWindow(hwnd).max(96) as f32) / 96.0;
                if let Some(sh) = &mut b.shadow {
                    sh.sync(hwnd, dpi, 255);
                }
                return 0;
            }
            if let Some(rd) = b.resizing {
                let dpi = (GetDpiForWindow(hwnd).max(96) as f32) / 96.0;
                let min_w = (220.0 * dpi).round() as i32;
                let min_h = (120.0 * dpi).round() as i32;
                let dx = pt.x - rd.start_x;
                let dy = pt.y - rd.start_y;
                let mut l = rd.orig_l;
                let mut t = rd.orig_t;
                let mut r = rd.orig_r;
                let mut bt = rd.orig_b;
                if rd.edge & 1 != 0 {
                    l = (rd.orig_l + dx).min(rd.orig_r - min_w);
                }
                if rd.edge & 2 != 0 {
                    r = (rd.orig_r + dx).max(rd.orig_l + min_w);
                }
                if rd.edge & 4 != 0 {
                    t = (rd.orig_t + dy).min(rd.orig_b - min_h);
                }
                if rd.edge & 8 != 0 {
                    bt = (rd.orig_b + dy).max(rd.orig_t + min_h);
                }
                SetWindowPos(hwnd, null_mut(), l, t, r - l, bt - t, SWP_NOZORDER | SWP_NOACTIVATE);
                b.background_key.clear();
                b.paint();
                return 0;
            }
            let dpi = (GetDpiForWindow(hwnd).max(96) as f32) / 96.0;
            let lx = (lp as i16) as f32 / dpi;
            let ly = ((lp >> 16) as i16) as f32 / dpi;
            if let Some(mut idrag) = b.item_drag {
                let dist = ((lx - idrag.start_pt[0]).powi(2) + (ly - idrag.start_pt[1]).powi(2)).sqrt();
                if dist > 5.0 {
                    idrag.active = true;
                }
                idrag.cur_pt = [lx, ly];
                b.item_drag = Some(idrag);
                if idrag.active {
                    b.hover_item = b.hit_slot(lx, ly);
                    update_drag_ghost(&mut b, idrag, pt, dpi);
                    b.paint();
                }
                return 0;
            }
            if let Some([sx, sy, _, _]) = b.marquee {
                b.marquee = Some([sx, sy, lx, ly]);
                let ml = sx.min(lx);
                let mt = sy.min(ly);
                let mr = sx.max(lx);
                let mb = sy.max(ly);
                b.selected.clear();
                if let Some(spec) = b.spec() {
                    let mut r: RECT = zeroed();
                    GetClientRect(hwnd, &mut r);
                    let w = r.right as f32 / dpi;
                    let (_, cell_w, cell_h) = cell_dims(&spec);
                    let cols = ((w - PAD_X * 2.0) / cell_w).floor().max(1.0) as usize;
                    let start_x = (PAD_X + ((w - PAD_X * 2.0) - cols as f32 * cell_w) * 0.5).round();
                    for idx in 0..b.entries.len() {
                        let row = idx / cols;
                        let col = idx % cols;
                        let x = start_x + col as f32 * cell_w;
                        let y = HEADER_H + 6.0 + row as f32 * cell_h - b.scroll_y;
                        if x + cell_w >= ml && x <= mr && y + cell_h >= mt && y <= mb {
                            b.selected.insert(idx);
                        }
                    }
                }
                b.paint();
                return 0;
            }
            let mut r: RECT = zeroed();
            GetClientRect(hwnd, &mut r);
            let w = r.right as f32 / dpi;
            let roll_hit = ly < HEADER_H && lx >= w - 36.0 && lx <= w - 6.0;
            let add_hit = ly < HEADER_H && lx >= w - 64.0 && lx < w - 36.0;
            let item_hit = b.hit_item(lx, ly);
            if b.hover_roll != roll_hit || b.hover_add != add_hit || b.hover_item != item_hit {
                b.hover_roll = roll_hit;
                b.hover_add = add_hit;
                b.hover_item = item_hit;
                InvalidateRect(hwnd, null(), 0);
            }
            let mut tme = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut tme);
            0
        }
        WM_MOUSELEAVE => {
            let Ok(mut b) = win.try_borrow_mut() else {
                return 0;
            };
            b.hover_item = None;
            b.hover_roll = false;
            b.hover_add = false;
            b.set_hover(false);
            0
        }
        WM_LBUTTONDOWN => {
            let Ok(mut b) = win.try_borrow_mut() else {
                return 0;
            };
            let dpi = (GetDpiForWindow(hwnd).max(96) as f32) / 96.0;
            let lx = (lp as i16) as f32 / dpi;
            let ly = ((lp >> 16) as i16) as f32 / dpi;
            let mut r: RECT = zeroed();
            GetClientRect(hwnd, &mut r);
            let w = r.right as f32 / dpi;
            let h = r.bottom as f32 / dpi;
            let rolled = b.spec().is_some_and(|s| s.rolled_up);
            if ly < HEADER_H && lx >= w - 36.0 && lx <= w - 6.0 {
                b.toggle_roll();
                return 0;
            }
            if ly < HEADER_H && lx >= w - 64.0 && lx < w - 36.0 {
                drop(b);
                let mut pt: POINT = zeroed();
                GetCursorPos(&mut pt);
                run_add_menu(win, pt.x, pt.y);
                return 0;
            }
            let edge = hit_resize_edge(lx, ly, w, h, rolled);
            if edge != 0 {
                let mut pt: POINT = zeroed();
                let mut wr: RECT = zeroed();
                GetCursorPos(&mut pt);
                GetWindowRect(hwnd, &mut wr);
                b.resizing = Some(ResizeDrag {
                    edge,
                    start_x: pt.x,
                    start_y: pt.y,
                    orig_l: wr.left,
                    orig_t: wr.top,
                    orig_r: wr.right,
                    orig_b: wr.bottom,
                });
                SetCapture(hwnd);
                return 0;
            }
            let hit = b.hit_item(lx, ly);
            if ly < HEADER_H || b.group_selected || crate::fusion::is_widget_selected(hwnd) {
                let mut pt: POINT = zeroed();
                let mut wr: RECT = zeroed();
                GetCursorPos(&mut pt);
                GetWindowRect(hwnd, &mut wr);
                b.dragging = Some([pt.x, pt.y, wr.left, wr.top]);
                SetCapture(hwnd);
                return 0;
            }
            if let Some(idx) = hit {
                if !b.selected.contains(&idx) {
                    b.selected.clear();
                    b.selected.insert(idx);
                }
                b.item_drag = Some(ItemDrag {
                    idx,
                    start_pt: [lx, ly],
                    cur_pt: [lx, ly],
                    active: false,
                });
                SetFocus(hwnd);
                SetCapture(hwnd);
            } else {
                b.selected.clear();
                b.marquee = Some([lx, ly, lx, ly]);
                SetFocus(hwnd);
                SetCapture(hwnd);
            }
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_LBUTTONUP => {
            let mut cross_fence_move: Option<(Option<Rc<RefCell<FenceWindow>>>, Vec<PathBuf>, String)> = None;
            if let Ok(mut b) = win.try_borrow_mut() {
                if let Some(idrag) = b.item_drag.take() {
                    hide_drag_ghost(&b.shared);
                    ReleaseCapture();
                    if idrag.active && idrag.idx < b.entries.len() {
                        let mut pt: POINT = zeroed();
                        GetCursorPos(&mut pt);
                        let mut my_wr: RECT = zeroed();
                        GetWindowRect(hwnd, &mut my_wr);
                        let inside_self = pt.x >= my_wr.left
                            && pt.x <= my_wr.right
                            && pt.y >= my_wr.top
                            && pt.y <= my_wr.bottom;
                        if inside_self {
                            if let Some(dst_idx) = b.hit_slot(idrag.cur_pt[0], idrag.cur_pt[1]) {
                                let order = reorder_entries_by_index(&mut b.entries, idrag.idx, dst_idx);
                                b.selected.clear();
                                b.selected.insert(dst_idx);
                                b.update_spec(|s| {
                                    s.sort_by = "manual".into();
                                    s.custom_order = order;
                                });
                            }
                        } else {
                            let wins = b.shared.borrow().windows.clone();
                            let mut target_win = None;
                            let mut target_folder = String::new();
                            for ow in wins {
                                if let Ok(ob) = ow.try_borrow() {
                                    if ob.hwnd != hwnd && ob.visible {
                                        let mut or: RECT = zeroed();
                                        GetWindowRect(ob.hwnd, &mut or);
                                        if pt.x >= or.left
                                            && pt.x <= or.right
                                            && pt.y >= or.top
                                            && pt.y <= or.bottom
                                        {
                                            if let Some(ospec) = ob.spec() {
                                                target_folder = ospec.folder;
                                                target_win = Some(Rc::clone(&ow));
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                            let mut paths = Vec::new();
                            if b.selected.len() > 1 && b.selected.contains(&idrag.idx) {
                                let mut idxs: Vec<usize> = b.selected.iter().copied().collect();
                                idxs.sort_unstable();
                                for i in idxs {
                                    if let Some(e) = b.entries.get(i) {
                                        paths.push(e.path.clone());
                                    }
                                }
                            } else if let Some(e) = b.entries.get(idrag.idx) {
                                paths.push(e.path.clone());
                            }
                            if let Some(tw) = target_win {
                                cross_fence_move = Some((Some(tw), paths, target_folder));
                            } else if is_desktop_point(pt) {
                                let desk_dir = model::user_home().join("Desktop");
                                cross_fence_move = Some((None, paths, desk_dir.to_string_lossy().into_owned()));
                            }
                        }
                    }
                    b.paint();
                } else {
                    let was_drag = b.dragging.take().is_some();
                    let was_resize = b.resizing.take().is_some();
                    let was_marquee = b.marquee.take().is_some();
                    if was_drag || was_resize || was_marquee {
                        ReleaseCapture();
                        if was_drag || was_resize {
                            let mut wr: RECT = zeroed();
                            GetWindowRect(hwnd, &mut wr);
                            let dpi = (GetDpiForWindow(hwnd).max(96) as f32) / 96.0;
                            b.update_spec(|s| {
                                s.position = [wr.left, wr.top];
                                if was_resize && !s.rolled_up {
                                    s.size = [
                                        ((wr.right - wr.left) as f32 / dpi).round() as i32,
                                        ((wr.bottom - wr.top) as f32 / dpi).round() as i32,
                                    ];
                                }
                            });
                            b.background_key.clear();
                        }
                        b.paint();
                    }
                }
            }
            if let Some((target_win_opt, paths, dst_folder)) = cross_fence_move {
                let dst_dir = Path::new(&dst_folder);
                let mut any_moved = false;
                for p in paths {
                    if move_entry_to_folder(&p, dst_dir).is_ok() {
                        any_moved = true;
                    }
                }
                if any_moved {
                    if let Ok(mut b) = win.try_borrow_mut() {
                        b.selected.clear();
                        b.reload_entries();
                        b.paint();
                    }
                    if let Some(target_win) = target_win_opt {
                        if let Ok(mut ob) = target_win.try_borrow_mut() {
                            ob.reload_entries();
                            ob.paint();
                        }
                    }
                }
            }
            0
        }
        WM_CAPTURECHANGED | WM_CANCELMODE => {
            if let Ok(mut b) = win.try_borrow_mut() {
                if b.item_drag.take().is_some() {
                    hide_drag_ghost(&b.shared);
                    b.paint();
                }
            }
            0
        }
        FENCES_SETTINGS_CHANGED => {
            let shared = win.try_borrow().ok().map(|b| Rc::clone(&b.shared));
            if let Some(sh) = shared {
                handle_fences_settings_changed(&sh);
            }
            0
        }
        WM_EXITSIZEMOVE => {
            let Ok(mut b) = win.try_borrow_mut() else {
                return 0;
            };
            let mut wr: RECT = zeroed();
            GetWindowRect(hwnd, &mut wr);
            b.update_spec(|s| s.position = [wr.left, wr.top]);
            b.background_key.clear();
            b.paint();
            0
        }
        WM_LBUTTONDBLCLK => {
            let dpi = (GetDpiForWindow(hwnd).max(96) as f32) / 96.0;
            let lx = (lp as i16) as f32 / dpi;
            let ly = ((lp >> 16) as i16) as f32 / dpi;
            let mut to_open = None;
            if let Ok(mut b) = win.try_borrow_mut() {
                b.item_drag = None;
                if ly < HEADER_H {
                    b.toggle_roll();
                } else if let Some(idx) = b.hit_item(lx, ly) {
                    to_open = b.entries.get(idx).map(|e| e.path.clone());
                } else {
                    b.toggle_roll();
                }
            }
            if let Some(path) = to_open {
                FenceWindow::open_path(&path);
            }
            0
        }
        WM_MOUSEWHEEL => {
            let Ok(mut b) = win.try_borrow_mut() else {
                return 0;
            };
            let delta = ((wp >> 16) as u16 as i16) as f32;
            b.scroll_y = (b.scroll_y - delta / 120.0 * 48.0).max(0.0);
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_KEYDOWN => {
            if wp == VK_DELETE as usize {
                FenceWindow::delete_selected_for_window(win);
            }
            0
        }
        WM_DROPFILES => {
            let drop = wp as HDROP;
            let count = DragQueryFileW(drop, u32::MAX, null_mut(), 0);
            let mut dropped_paths = Vec::new();
            for n in 0..count {
                let len = DragQueryFileW(drop, n, null_mut(), 0);
                if len > 0 {
                    let mut buf = vec![0u16; len as usize + 1];
                    DragQueryFileW(drop, n, buf.as_mut_ptr(), len + 1);
                    dropped_paths.push(PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])));
                }
            }
            DragFinish(drop);
            if let Ok(mut b) = win.try_borrow_mut() {
                if let Some(spec) = b.spec() {
                    let dst_dir = Path::new(&spec.folder);
                    let mut added = false;
                    for p in dropped_paths {
                        if import_external_path_into_fence(&p, dst_dir, false).is_ok() {
                            added = true;
                        }
                    }
                    if added {
                        b.reload_entries();
                        b.paint();
                    }
                }
            }
            0
        }
        WM_CONTEXTMENU => {
            let x = (lp as i16) as i32;
            let y = ((lp >> 16) as i16) as i32;
            run_context_menu(win, x, y);
            0
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Rc::from_raw(ptr));
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cleartype_text_renders_crisp_subpixels() {
        unsafe {
            let buf = Buffer::plain(200, 48, 1.0).expect("buffer");
            let c = Canvas::new(buf.dc, 1.0);
            c.round([0.0, 0.0, 200.0, 48.0], 8.0, 0x0b0f1c, 255, false, false);
            draw_cleartype_text(
                buf.dc,
                "Tarot & HNHH (13)",
                [10, 4, 190, 40],
                15,
                600,
                0xffffff,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            let pixels = buf.snapshot();
            let bright_count = pixels
                .chunks_exact(4)
                .filter(|px| px[0] > 200 && px[1] > 200 && px[2] > 200)
                .count();
            let subpixel_cleartype_count = pixels
                .chunks_exact(4)
                .filter(|px| {
                    let (b, g, r) = (px[0] as i32, px[1] as i32, px[2] as i32);
                    (r - b).abs() > 12 || (r - g).abs() > 12 || (g - b).abs() > 12
                })
                .count();
            assert!(bright_count > 40, "must render bright white glyph stems (got {bright_count})");
            assert!(
                subpixel_cleartype_count > 20,
                "must render true RGB sub-pixel ClearType edges (got {subpixel_cleartype_count})"
            );
        }
    }

    #[test]
    fn test_desktop_marquee_multi_fence_intersect_and_group_move() {
        let fence_rects = [
            ("web", [20, 740, 360, 990]),
            ("games", [376, 740, 716, 990]),
            ("ai", [732, 740, 1152, 990]),
            ("downloads", [1540, 20, 1900, 360]),
        ];
        let marquee = [10, 700, 800, 1000];
        let selected: Vec<&str> = fence_rects
            .iter()
            .filter(|(_, r)| rect_intersects(*r, marquee))
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(selected, vec!["web", "games", "ai"]);

        let (dx, dy) = (120, -80);
        let moved: Vec<[i32; 2]> = fence_rects
            .iter()
            .filter(|(id, _)| selected.contains(id))
            .map(|(_, r)| [r[0] + dx, r[1] + dy])
            .collect();
        assert_eq!(moved, vec![[140, 660], [496, 660], [852, 660]]);
    }

    #[test]
    fn test_fusion_multi_widget_block_move() {
        let _lock = crate::native_ui_test_lock();
        unsafe {
            crate::fusion::init(Rc::new(RefCell::new(Glass::new(0.0, 1.0))));
            let make = |x: i32, y: i32| {
                let h = CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    wide("STATIC").as_ptr(),
                    wide("Lumen block test").as_ptr(),
                    WS_POPUP,
                    x,
                    y,
                    120,
                    100,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null(),
                );
                ShowWindow(h, SW_SHOWNOACTIVATE);
                crate::fusion::register_widget(h);
                h
            };
            let w1 = make(100, 100);
            let w2 = make(230, 100);
            let w3 = make(900, 700);

            let count = crate::fusion::select_widgets_in_rect([80, 80, 400, 250]);
            assert_eq!(count, 2, "must select w1 and w2 inside marquee");
            assert!(crate::fusion::is_widget_selected(w1));
            assert!(crate::fusion::is_widget_selected(w2));
            assert!(!crate::fusion::is_widget_selected(w3));

            // Drag the selected cluster via begin_cluster_drag / update_cluster_drag / end_cluster_drag
            assert!(crate::fusion::begin_cluster_drag(POINT { x: 150, y: 150 }));
            assert!(crate::fusion::update_cluster_drag(POINT { x: 200, y: 180 }));
            crate::fusion::end_cluster_drag();

            let mut r1: RECT = zeroed();
            let mut r2: RECT = zeroed();
            let mut r3: RECT = zeroed();
            GetWindowRect(w1, &mut r1);
            GetWindowRect(w2, &mut r2);
            GetWindowRect(w3, &mut r3);
            assert_eq!([r1.left, r1.top], [150, 130], "w1 must move by (+50, +30)");
            assert_eq!([r2.left, r2.top], [280, 130], "w2 must move with w1 as a single block");
            assert_eq!([r3.left, r3.top], [900, 700], "unselected w3 must remain untouched");

            crate::fusion::clear_widget_selection();
            for h in [w1, w2, w3] {
                DestroyWindow(h);
            }
        }
    }

    #[test]
    fn test_magnetic_fence_corner_and_edge_snapping() {
        let others = [
            [20, 740, 360, 990],  // Web fence
            [372, 740, 712, 990], // Games fence
        ];
        let work = [0, 0, 1920, 1040];

        // Dragging near the right of Games fence (x=712 + 12px gap = 724) and near top corner y=740
        let snapped = snap_fence_position(730, 746, 420, 250, &others, work);
        assert_eq!(snapped, [724, 740], "must magnetically snap to 12px gap + top corner alignment");

        // Dragging above Web fence (left edge near 20, bottom near 740 - 12px gap - 340h = 388)
        let snapped_above = snap_fence_position(25, 393, 380, 340, &others, work);
        assert_eq!(snapped_above, [20, 388], "must magnetically align left edges and 12px vertical gap");
    }

    #[test]
    fn test_reorder_items_and_image_thumbnail_generation() {
        let mut entries = vec![
            FenceEntry {
                name: "Alpha".into(),
                path: PathBuf::from(r"C:\test\alpha.lnk"),
                is_dir: false,
                modified: SystemTime::UNIX_EPOCH,
            },
            FenceEntry {
                name: "Beta".into(),
                path: PathBuf::from(r"C:\test\beta.lnk"),
                is_dir: false,
                modified: SystemTime::UNIX_EPOCH,
            },
            FenceEntry {
                name: "Gamma".into(),
                path: PathBuf::from(r"C:\test\gamma.lnk"),
                is_dir: false,
                modified: SystemTime::UNIX_EPOCH,
            },
        ];
        let order = reorder_entries_by_index(&mut entries, 0, 2);
        assert_eq!(
            order,
            vec!["beta.lnk".to_string(), "gamma.lnk".to_string(), "alpha.lnk".to_string()]
        );
        assert_eq!(entries[2].name, "Alpha");
    }

    #[test]
    fn test_external_import_shortcut_and_delete_entry() {
        unsafe {
            let tmp_base = std::env::temp_dir().join(format!(
                "lumen_fence_test_{}",
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(1)
            ));
            let src_dir = tmp_base.join("external_src");
            let fence_dir = tmp_base.join("fence_dst");
            std::fs::create_dir_all(&src_dir).expect("create src_dir");
            std::fs::create_dir_all(&fence_dir).expect("create fence_dir");

            // 1. Test importing an external .exe creates a .lnk shortcut inside the fence without deleting the original .exe
            let fake_exe = src_dir.join("MyProgram.exe");
            std::fs::write(&fake_exe, b"MZfake").expect("write fake exe");
            let imported_lnk =
                import_external_path_into_fence(&fake_exe, &fence_dir, false).expect("import exe");
            assert!(imported_lnk.exists(), "must create .lnk shortcut inside fence");
            assert!(
                imported_lnk
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
            );
            assert!(fake_exe.exists(), "original .exe must remain intact");

            // 2. Test creating a new subfolder inside the fence
            let sub = create_new_subfolder(&fence_dir).expect("create subfolder");
            assert!(sub.is_dir(), "subfolder must exist");

            // 3. Test deleting both the shortcut and subfolder
            delete_entry_path(&imported_lnk).expect("delete shortcut");
            assert!(!imported_lnk.exists(), "shortcut must be deleted");
            delete_entry_path(&sub).expect("delete subfolder");
            assert!(!sub.exists(), "subfolder must be deleted");

            let _ = std::fs::remove_dir_all(&tmp_base);
        }
    }
}
