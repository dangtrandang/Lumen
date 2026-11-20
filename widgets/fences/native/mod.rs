//! Stardock-style liquid glass desktop folder fences with 120Hz fade-on-hover.
pub mod model;
pub(crate) mod package;

use crate::{
    compositor::{Buffer, WindowShadow},
    d2d::{self, ParagraphAlign, TextAlign, TextStyle, WordWrap},
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
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::WM_MOUSELEAVE,
        HiDpi::*,
        Input::KeyboardAndMouse::{
            GetDoubleClickTime, GetKeyState, ReleaseCapture, SetCapture, TrackMouseEvent,
            TME_LEAVE, TRACKMOUSEEVENT, VK_LBUTTON,
        },
        Shell::{
            SHGetFileInfoW, SHGetImageList, ShellExecuteW, SHFILEINFOW, SHGFI_ICON,
            SHGFI_LARGEICON, SHGFI_SYSICONINDEX, SHIL_EXTRALARGE,
        },
        WindowsAndMessaging::*,
    },
};

const CLASS: &str = "LumenNativeFenceWindow";
const FADE_TIMER: usize = 201;
const DESKTOP_POLL_TIMER: usize = 202;
const EASE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
const HEADER_H: f32 = 34.0;
const RADIUS: f32 = 18.0;
const PAD_X: f32 = 12.0;

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

fn cell_dims(spec: &FenceSpec) -> (f32, f32, f32) {
    let icon = spec.icon_size.clamp(20, 64) as f32;
    let cw = (icon + 44.0).max(66.0);
    let ch = icon + 42.0;
    (icon, cw, ch)
}

#[derive(Clone)]
struct FenceEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    modified: SystemTime,
}

struct SharedState {
    root: PathBuf,
    config: FencesConfig,
    glass: Rc<RefCell<Glass>>,
    dpi: f32,
    smoke: bool,
    quick_hidden: bool,
    prev_lbtn_down: bool,
    last_desk_click_ms: Option<Instant>,
    last_desk_click_pt: [i32; 2],
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
    scroll_y: f32,
    alpha_tween: Tween,
    started: Instant,
    dragging: Option<[i32; 4]>, // [cursor_x, cursor_y, win_x, win_y]
    resizing: Option<ResizeDrag>,
    visible: bool,
}

#[derive(Clone, Copy)]
struct ResizeDrag {
    edge: u8, // bitmask: 1=left, 2=right, 4=top, 8=bottom
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
        let shared = Rc::new(RefCell::new(SharedState {
            root: root.into(),
            config: config.clone(),
            glass,
            dpi,
            smoke,
            quick_hidden: false,
            prev_lbtn_down: false,
            last_desk_click_ms: None,
            last_desk_click_pt: [0, 0],
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
                SetTimer(first.borrow().hwnd, DESKTOP_POLL_TIMER, 20, None);
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
        if spec.sort_by == "date" {
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
        let key = path.to_string_lossy().into_owned();
        if !self.icons.contains_key(&key) {
            let wpath = wide(&key);
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
                // IID_IImageList = {46EB5926-582E-4017-9FDF-E8998DAA0950}
                let iid = windows_sys::core::GUID::from_u128(0x46eb5926_582e_4017_9fdf_e8998daa0950);
                if SHGetImageList(SHIL_EXTRALARGE as i32, &iid, &mut ilist) == 0 && !ilist.is_null() {
                    // IImageList::GetIcon is vtable index 10
                    type GetIconFn = unsafe extern "system" fn(*mut std::ffi::c_void, i32, u32, *mut HICON) -> i32;
                    let vtbl = *(ilist as *const *const usize);
                    let get_icon: GetIconFn = std::mem::transmute(*vtbl.add(10));
                    let _ = get_icon(ilist, info.iIcon, 0x0001 /* ILD_TRANSPARENT */, &mut hicon);
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
        InvalidateRect(self.hwnd, null(), 0);
    }

    unsafe fn paint(&mut self) {
        let Some(spec) = self.spec() else {
            return;
        };
        let dpi = self.shared.borrow().dpi;
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
            Some(f) if f.width == r.right && f.height == r.bottom && f.dpi == dpi => f,
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
                dpi,
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

        let mut text_draws: Vec<(String, [f32; 4], f32, u16, TextAlign, ParagraphAlign, WordWrap, u32)> = Vec::new();
        let batch = render::AlphaBatch::new(frame.dc, dpi);
        {
            let c = Canvas::new(frame.dc, dpi);
            // Dark smoked-glass tint so icons & text stay crisp over bright wallpapers
            c.round([0.0, 0.0, w, h], RADIUS, 0x0b0f1c, 145, false, false);
            // Subtle outer rim
            c.round([0.5, 0.5, w - 0.5, h - 0.5], RADIUS, 0xffffff, 38, true, false);
            // Header tint
            c.round([1.0, 1.0, w - 1.0, HEADER_H], RADIUS, 0x05070e, 90, false, false);
            // Title
            let title_str = format!("{}  ({})", spec.title, self.entries.len());
            text_draws.push((
                title_str,
                [14.0, 2.0, w - 42.0, HEADER_H - 2.0],
                12.5,
                600,
                TextAlign::Leading,
                ParagraphAlign::Center,
                WordWrap::NoWrap,
                0xffffff,
            ));
            // Roll-up chevron button
            let roll_bg_alpha = if self.hover_roll { 45 } else { 0 };
            if roll_bg_alpha > 0 {
                c.round([w - 34.0, 5.0, w - 8.0, 29.0], 6.0, 0xffffff, roll_bg_alpha, false, false);
            }
            let glyph = if spec.rolled_up { "▸" } else { "▾" };
            text_draws.push((
                glyph.into(),
                [w - 34.0, 6.0, w - 8.0, 28.0],
                13.0,
                600,
                TextAlign::Center,
                ParagraphAlign::Center,
                WordWrap::NoWrap,
                0xe8e8e8,
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
                for idx in 0..entry_count {
                    let row = idx / cols;
                    let col = idx % cols;
                    let x = (start_x + col as f32 * cell_w).round();
                    let y = (HEADER_H + 6.0 + row as f32 * cell_h - self.scroll_y).round();
                    if y + cell_h < HEADER_H || y > h {
                        continue;
                    }
                    if self.selected.contains(&idx) {
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
                        [x + 3.0, y + 9.0 + icon_sz, x + cell_w - 3.0, y + cell_h - 1.0],
                        11.8,
                        500,
                        TextAlign::Center,
                        ParagraphAlign::Near,
                        WordWrap::Wrap,
                        0xffffff,
                    ));
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
                // Subtle bottom-right resize grip indicator
                c.round([w - 11.0, h - 11.0, w - 5.0, h - 5.0], 2.0, 0xffffff, 55, false, false);
            }
        }
        drop(batch);

        for (txt, rect, sz, weight, align, valign, wrap, color) in &text_draws {
            let style = TextStyle {
                font_family: "Segoe UI",
                size: *sz * dpi,
                weight: *weight,
                italic: false,
                align: *align,
                valign: *valign,
                wrap: *wrap,
                trim_ellipsis: true,
            };
            let shadow_rect = [
                (rect[0] * dpi).round(),
                (rect[1] * dpi + 1.0).round(),
                (rect[2] * dpi).round(),
                (rect[3] * dpi + 1.0).round(),
            ];
            d2d::draw_text_dc(frame.dc, r.right, r.bottom, txt, shadow_rect, &style, 0x000000, 170);
            let px_rect = [
                (rect[0] * dpi).round(),
                (rect[1] * dpi).round(),
                (rect[2] * dpi).round(),
                (rect[3] * dpi).round(),
            ];
            d2d::draw_text_dc(frame.dc, r.right, r.bottom, txt, px_rect, &style, *color, 255);
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
        let dpi = self.shared.borrow().dpi;
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
    let icon_menu = CreatePopupMenu();
    let flt_menu = CreatePopupMenu();
    let flag = |b: bool| MF_STRING | if b { MF_CHECKED } else { MF_UNCHECKED };

    let op_pct = (spec.idle_opacity * 100.0).round() as i32;
    AppendMenuW(op_menu, flag(spec.fade_on_hover && op_pct <= 8), CMD_OP_5, wide("5% (Nearly Hidden)").as_ptr());
    AppendMenuW(op_menu, flag(spec.fade_on_hover && op_pct > 8 && op_pct <= 20), CMD_OP_15, wide("15% (Subtle)").as_ptr());
    AppendMenuW(op_menu, flag(spec.fade_on_hover && op_pct > 20 && op_pct <= 35), CMD_OP_25, wide("25% (Dim)").as_ptr());
    AppendMenuW(op_menu, flag(spec.fade_on_hover && op_pct > 35 && op_pct < 99), CMD_OP_50, wide("50% (Half)").as_ptr());
    AppendMenuW(op_menu, flag(!spec.fade_on_hover || op_pct >= 99), CMD_OP_100, wide("100% (Always Clear)").as_ptr());

    AppendMenuW(icon_menu, flag(spec.icon_size <= 26), CMD_ICON_24, wide("Small (24px)").as_ptr());
    AppendMenuW(icon_menu, flag(spec.icon_size > 26 && spec.icon_size < 40), CMD_ICON_32, wide("Normal (32px)").as_ptr());
    AppendMenuW(icon_menu, flag(spec.icon_size >= 40), CMD_ICON_48, wide("Large (48px)").as_ptr());

    AppendMenuW(flt_menu, flag(spec.filter == "all"), CMD_FILTER_ALL, wide("Show All").as_ptr());
    AppendMenuW(flt_menu, flag(spec.filter == "folders"), CMD_FILTER_FOLDERS, wide("Folders Only").as_ptr());
    AppendMenuW(flt_menu, flag(spec.filter == "files"), CMD_FILTER_FILES, wide("Files & Apps Only").as_ptr());
    AppendMenuW(flt_menu, MF_SEPARATOR, 0, null());
    AppendMenuW(flt_menu, flag(spec.sort_by == "name"), CMD_SORT_NAME, wide("Sort by Name").as_ptr());
    AppendMenuW(flt_menu, flag(spec.sort_by == "date"), CMD_SORT_DATE, wide("Sort by Date (Newest First)").as_ptr());
    AppendMenuW(flt_menu, flag(spec.folders_first), CMD_FOLDERS_FIRST, wide("Folders First").as_ptr());

    AppendMenuW(menu, flag(spec.fade_on_hover), CMD_FADE, wide("Fade on Hover").as_ptr());
    AppendMenuW(menu, MF_POPUP, op_menu as usize, wide("Idle Opacity").as_ptr());
    AppendMenuW(menu, MF_POPUP, icon_menu as usize, wide("Icon Size").as_ptr());
    AppendMenuW(menu, MF_POPUP, flt_menu as usize, wide("Filter & Sort").as_ptr());
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

unsafe fn poll_desktop_double_click(shared: &Rc<RefCell<SharedState>>) {
    let down = (GetKeyState(VK_LBUTTON as i32) as u16 & 0x8000) != 0;
    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    let mut toggle = false;
    {
        let mut sh = shared.borrow_mut();
        let was_down = sh.prev_lbtn_down;
        sh.prev_lbtn_down = down;
        if down && !was_down {
            let target = WindowFromPoint(pt);
            let mut cls = [0u16; 64];
            let len = if !target.is_null() {
                GetClassNameW(target, cls.as_mut_ptr(), 64).max(0) as usize
            } else {
                0
            };
            let name = String::from_utf16_lossy(&cls[..len]);
            let is_desktop = matches!(
                name.as_str(),
                "Progman" | "WorkerW" | "SHELLDLL_DefView" | "SysListView32"
            );
            if is_desktop {
                let now = Instant::now();
                let dbl_ms = GetDoubleClickTime() as u128;
                let is_dbl = sh.last_desk_click_ms.is_some_and(|prev| {
                    now.duration_since(prev).as_millis() <= dbl_ms
                        && (pt.x - sh.last_desk_click_pt[0]).abs() <= 6
                        && (pt.y - sh.last_desk_click_pt[1]).abs() <= 6
                });
                if is_dbl {
                    sh.last_desk_click_ms = None;
                    sh.quick_hidden = !sh.quick_hidden;
                    toggle = true;
                } else {
                    sh.last_desk_click_ms = Some(now);
                    sh.last_desk_click_pt = [pt.x, pt.y];
                }
            } else {
                sh.last_desk_click_ms = None;
            }
        }
    }
    if toggle {
        let (hidden, wins) = {
            let sh = shared.borrow();
            (sh.quick_hidden, sh.windows.clone())
        };
        for w in wins {
            let mut b = w.borrow_mut();
            if !b.visible {
                continue;
            }
            if hidden {
                ShowWindow(b.hwnd, SW_HIDE);
                if let Some(s) = &b.shadow {
                    s.hide();
                }
            } else {
                ShowWindow(b.hwnd, SW_SHOWNOACTIVATE);
                crate::desktop_layer::pin(b.hwnd);
                b.paint();
            }
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
        WM_TIMER if wp == DESKTOP_POLL_TIMER => {
            let shared = win.borrow().shared.clone();
            poll_desktop_double_click(&shared);
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
            let dpi = b.shared.borrow().dpi;
            let rolled = b.spec().is_some_and(|s| s.rolled_up);
            drop(b);
            let mut pt: POINT = zeroed();
            GetCursorPos(&mut pt);
            ScreenToClient(hwnd, &mut pt);
            let mut r: RECT = zeroed();
            GetClientRect(hwnd, &mut r);
            let edge = hit_resize_edge(pt.x as f32 / dpi, pt.y as f32 / dpi, r.right as f32 / dpi, r.bottom as f32 / dpi, rolled);
            let cursor_id = match edge {
                5 | 10 => IDC_SIZENWSE, // L+T or R+B
                6 | 9 => IDC_SIZENESW,  // R+T or L+B
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
                let nx = wx + (pt.x - sx);
                let ny = wy + (pt.y - sy);
                SetWindowPos(hwnd, null_mut(), nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                let dpi = b.shared.borrow().dpi;
                if let Some(sh) = &mut b.shadow {
                    sh.sync(hwnd, dpi, 255);
                }
                return 0;
            }
            if let Some(rd) = b.resizing {
                let dpi = b.shared.borrow().dpi;
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
            let dpi = b.shared.borrow().dpi;
            let lx = (lp as i16) as f32 / dpi;
            let ly = ((lp >> 16) as i16) as f32 / dpi;
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
            let dpi = b.shared.borrow().dpi;
            let lx = (lp as i16) as f32 / dpi;
            let ly = ((lp >> 16) as i16) as f32 / dpi;
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
            if ly < HEADER_H {
                let mut pt: POINT = zeroed();
                let mut wr: RECT = zeroed();
                GetCursorPos(&mut pt);
                GetWindowRect(hwnd, &mut wr);
                b.dragging = Some([pt.x, pt.y, wr.left, wr.top]);
                SetCapture(hwnd);
                return 0;
            }
            if let Some(idx) = hit {
                b.selected.clear();
                b.selected.insert(idx);
            } else {
                b.selected.clear();
                b.marquee = Some([lx, ly, lx, ly]);
                SetCapture(hwnd);
            }
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_LBUTTONUP => {
            let mut b = win.borrow_mut();
            let was_drag = b.dragging.take().is_some();
            let was_resize = b.resizing.take().is_some();
            let was_marquee = b.marquee.take().is_some();
            if was_drag || was_resize || was_marquee {
                ReleaseCapture();
                if was_drag || was_resize {
                    let mut wr: RECT = zeroed();
                    GetWindowRect(hwnd, &mut wr);
                    let dpi = b.shared.borrow().dpi;
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
            0
        }
        WM_LBUTTONDBLCLK => {
            let mut b = win.borrow_mut();
            let dpi = b.shared.borrow().dpi;
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
