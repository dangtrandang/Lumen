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
    mem::{size_of, zeroed},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    rc::Rc,
    time::{Instant, SystemTime},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentProcessId},
    UI::{
        Controls::WM_MOUSELEAVE,
        HiDpi::*,
        Input::{
            GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
            RAWINPUTHEADER, RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEMOUSE,
            KeyboardAndMouse::{
                GetDoubleClickTime, ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE,
                TRACKMOUSEEVENT,
            },
        },
        Shell::{
            SHGetFileInfoW, SHGetImageList, ShellExecuteW, SHFILEINFOW, SHGFI_ICON,
            SHGFI_LARGEICON, SHGFI_SYSICONINDEX, SHIL_EXTRALARGE,
        },
        WindowsAndMessaging::*,
    },
};

const CLASS: &str = "LumenNativeFenceWindow";
const MARQUEE_CLASS: &str = "LumenDesktopMarqueeOverlay";
const FADE_TIMER: usize = 201;
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

const CMD_TEXT_11: usize = 1040;
const CMD_TEXT_13: usize = 1041;
const CMD_TEXT_15: usize = 1042;
const CMD_TEXT_17: usize = 1043;

const CMD_VIEW_GRID: usize = 1050;
const CMD_VIEW_GALLERY: usize = 1051;
const CMD_VIEW_COMPACT: usize = 1052;
const CMD_TOGGLE_THUMBS: usize = 1053;

fn cell_dims(spec: &FenceSpec) -> (f32, f32, f32) {
    let base_icon = match spec.view_mode.as_str() {
        "gallery" => spec.icon_size.max(72),
        "compact" => spec.icon_size.min(24),
        _ => spec.icon_size,
    };
    let icon = base_icon.clamp(20, 96) as f32;
    let text_extra = (spec.text_size.clamp(10, 20) - 13).max(0) as f32 * 2.5;
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
                register_raw_mouse_sink(first.borrow().hwnd);
            }
        }
        shared.borrow_mut().windows = windows;
        Ok(Self { shared })
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
                    ((w - 42.0) * dpi).round() as i32,
                    ((HEADER_H - 2.0) * dpi).round() as i32,
                ],
                (15.0 * dpi).round() as i32,
                600,
                0xffffff,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
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
                let tile_font_px = (spec.text_size.clamp(10, 22) as f32 * dpi).round() as i32;
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
                    if idrag.idx < self.entries.len() {
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

    unsafe fn open_item(&self, idx: usize) {
        if let Some(entry) = self.entries.get(idx) {
            let p = wide(&entry.path);
            ShellExecuteW(
                null_mut(),
                wide("open").as_ptr(),
                p.as_ptr(),
                null(),
                null(),
                SW_SHOWNORMAL,
            );
        }
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
            CMD_TEXT_11 | CMD_TEXT_13 | CMD_TEXT_15 | CMD_TEXT_17 => {
                let tsz = match cmd {
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
            CMD_OPEN_FOLDER => {
                let p = wide(&spec.folder);
                ShellExecuteW(null_mut(), wide("open").as_ptr(), p.as_ptr(), null(), null(), SW_SHOWNORMAL);
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

unsafe fn run_context_menu(win: &RefCell<FenceWindow>, mut sx: i32, mut sy: i32) {
    let (hwnd, spec) = {
        let b = win.borrow();
        (b.hwnd, b.spec())
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
    let op_menu = CreatePopupMenu();
    let view_menu = CreatePopupMenu();
    let icon_menu = CreatePopupMenu();
    let text_menu = CreatePopupMenu();
    let flt_menu = CreatePopupMenu();
    let flag = |b: bool| MF_STRING | if b { MF_CHECKED } else { MF_UNCHECKED };

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
    AppendMenuW(text_menu, flag(ts <= 11), CMD_TEXT_11, wide("Small (11px)").as_ptr());
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
        win.borrow_mut().apply_menu_command(cmd);
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
            win.borrow_mut().paint();
            EndPaint(hwnd, &ps);
            0
        }
        WM_INPUT => {
            let shared = win.borrow().shared.clone();
            handle_raw_mouse(lp, &shared);
            0
        }
        WM_TIMER if wp == FADE_TIMER => {
            let b = win.borrow();
            let now = b.now();
            let done = !b.alpha_tween.moving(now);
            drop(b);
            if done {
                KillTimer(hwnd, FADE_TIMER);
            }
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_SETCURSOR => {
            let b = win.borrow();
            let dpi = (GetDpiForWindow(hwnd).max(96) as f32) / 96.0;
            let rolled = b.spec().is_some_and(|s| s.rolled_up);
            drop(b);
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
            let mut b = win.borrow_mut();
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
            let item_hit = b.hit_item(lx, ly);
            if b.hover_roll != roll_hit || b.hover_item != item_hit {
                b.hover_roll = roll_hit;
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
            let mut b = win.borrow_mut();
            b.hover_item = None;
            b.hover_roll = false;
            b.set_hover(false);
            0
        }
        WM_LBUTTONDOWN => {
            let mut b = win.borrow_mut();
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
                SetCapture(hwnd);
            } else {
                b.selected.clear();
                b.marquee = Some([lx, ly, lx, ly]);
                SetCapture(hwnd);
            }
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_LBUTTONUP => {
            let mut cross_fence_move: Option<(Rc<RefCell<FenceWindow>>, Vec<PathBuf>, String)> = None;
            {
                let mut b = win.borrow_mut();
                if let Some(idrag) = b.item_drag.take() {
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
                            if let Some(tw) = target_win {
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
                                cross_fence_move = Some((tw, paths, target_folder));
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
            if let Some((target_win, paths, dst_folder)) = cross_fence_move {
                let dst_dir = Path::new(&dst_folder);
                let mut any_moved = false;
                for p in paths {
                    if move_entry_to_folder(&p, dst_dir).is_ok() {
                        any_moved = true;
                    }
                }
                if any_moved {
                    {
                        let mut b = win.borrow_mut();
                        b.selected.clear();
                        b.reload_entries();
                        b.paint();
                    }
                    if let Ok(mut ob) = target_win.try_borrow_mut() {
                        ob.reload_entries();
                        ob.paint();
                    }
                }
            }
            0
        }
        WM_EXITSIZEMOVE => {
            let mut b = win.borrow_mut();
            let mut wr: RECT = zeroed();
            GetWindowRect(hwnd, &mut wr);
            b.update_spec(|s| s.position = [wr.left, wr.top]);
            b.background_key.clear();
            b.paint();
            0
        }
        WM_LBUTTONDBLCLK => {
            let mut b = win.borrow_mut();
            b.item_drag = None;
            let dpi = (GetDpiForWindow(hwnd).max(96) as f32) / 96.0;
            let lx = (lp as i16) as f32 / dpi;
            let ly = ((lp >> 16) as i16) as f32 / dpi;
            if ly < HEADER_H {
                b.toggle_roll();
            } else if let Some(idx) = b.hit_item(lx, ly) {
                b.open_item(idx);
            } else {
                b.toggle_roll();
            }
            0
        }
        WM_MOUSEWHEEL => {
            let mut b = win.borrow_mut();
            let delta = ((wp >> 16) as u16 as i16) as f32;
            b.scroll_y = (b.scroll_y - delta / 120.0 * 48.0).max(0.0);
            InvalidateRect(hwnd, null(), 0);
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
}
