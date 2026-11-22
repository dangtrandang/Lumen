//! Keep widget and fused decoration HWNDs below application windows immediately,
//! including activation/drag requests; a periodic pin alone permits visible jumps.
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, Ordering};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Dwm::{DwmGetWindowAttribute, DwmSetWindowAttribute},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
const ID: usize = 0x4c44534b;
static LAST_LOGGED_ANCHOR: AtomicIsize = AtomicIsize::new(isize::MIN);

fn property() -> Vec<u16> {
    crate::model::wide("Lumen.DesktopLayer")
}
pub unsafe fn contains(hwnd: HWND) -> bool {
    GetPropW(hwnd, property().as_ptr()) as usize == 1
}
pub unsafe fn desktop_shell_host() -> HWND {
    let progman = FindWindowW(crate::model::wide("Progman").as_ptr(), null());
    let defview = crate::model::wide("SHELLDLL_DefView");
    if !progman.is_null()
        && !FindWindowExW(progman, null_mut(), defview.as_ptr(), null()).is_null()
    {
        return progman;
    }
    let worker_cls = crate::model::wide("WorkerW");
    let mut worker = null_mut();
    for _ in 0..128 {
        worker = FindWindowExW(null_mut(), worker, worker_cls.as_ptr(), null());
        if worker.is_null() {
            break;
        }
        if !FindWindowExW(worker, null_mut(), defview.as_ptr(), null()).is_null() {
            return worker;
        }
    }
    progman
}
unsafe fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    DwmGetWindowAttribute(hwnd, 14, &mut cloaked as *mut _ as _, 4) >= 0 && cloaked != 0
}
unsafe fn is_desktop_surface(hwnd: HWND, class: &str) -> bool {
    IsWindowVisible(hwnd) != 0 && matches!(class, "Progman" | "WorkerW")
}
unsafe fn is_real_app_window(hwnd: HWND, class: &str) -> bool {
    if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 || is_cloaked(hwnd) || contains(hwnd) {
        return false;
    }
    if matches!(
        class,
        "Progman"
            | "WorkerW"
            | "SHELLDLL_DefView"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "ThumbnailDeviceHelperWnd"
            | "NarratorHelperWindow"
            | "Windows.UI.Core.CoreWindow"
            | "CEF-OSC-WIDGET"
            | "IME"
            | "MSCTFIME UI"
            | "tooltips_class32"
            | "Dummy DWM Listener Window"
            | "EdgeUiInputTopWndClass"
            | "NativeHWNDHost"
            | "XamlWindow"
            | "SnipOverlayRootWindow"
            | "LumenNativeGlassShadow"
            | "LumenNativeFusionSurface"
            | "LumenDesktopMarqueeOverlay"
    ) {
        return false;
    }
    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if (ex & (WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE)) != 0 {
        return false;
    }
    #[cfg(not(test))]
    if (ex & WS_EX_TOOLWINDOW) != 0 && (ex & WS_EX_APPWINDOW) == 0 {
        return false;
    }
    if class == "ApplicationFrameWindow" && GetWindowTextLengthW(hwnd) <= 0 {
        return false;
    }
    let mut r: RECT = std::mem::zeroed();
    if GetWindowRect(hwnd, &mut r) == 0 || (r.right - r.left) <= 1 || (r.bottom - r.top) <= 1 {
        return false;
    }
    true
}
pub unsafe fn hwnd_class(hwnd: HWND) -> String {
    if hwnd.is_null() {
        return "NULL".into();
    }
    if hwnd == HWND_TOP {
        return "HWND_TOP".into();
    }
    if hwnd == HWND_BOTTOM {
        return "HWND_BOTTOM".into();
    }
    if hwnd == HWND_TOPMOST {
        return "HWND_TOPMOST".into();
    }
    if hwnd == HWND_NOTOPMOST {
        return "HWND_NOTOPMOST".into();
    }
    let mut name = [0u16; 128];
    let len = GetClassNameW(hwnd, name.as_mut_ptr(), 128).max(0) as usize;
    String::from_utf16_lossy(&name[..len])
}
pub unsafe fn hwnd_title(hwnd: HWND) -> String {
    if hwnd.is_null()
        || hwnd == HWND_TOP
        || hwnd == HWND_BOTTOM
        || hwnd == HWND_TOPMOST
        || hwnd == HWND_NOTOPMOST
    {
        return String::new();
    }
    let mut buf = [0u16; 128];
    let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 128).max(0) as usize;
    String::from_utf16_lossy(&buf[..len])
}
pub unsafe fn register(hwnd: HWND, enabled: bool) {
    if contains(hwnd) == enabled {
        return;
    }
    if enabled {
        let yes = 1u32;
        DwmSetWindowAttribute(hwnd, 11, &yes as *const _ as _, 4);
        DwmSetWindowAttribute(hwnd, 12, &yes as *const _ as _, 4);
        SetPropW(hwnd, property().as_ptr(), 1usize as HANDLE);
        SetWindowSubclass(hwnd, Some(proc), ID, 0);
    } else {
        RemovePropW(hwnd, property().as_ptr());
        RemoveWindowSubclass(hwnd, Some(proc), ID);
    }
}
unsafe fn anchor(hwnd: HWND) -> HWND {
    let mut current = GetTopWindow(null_mut());
    let mut last = null_mut();
    for _ in 0..4096 {
        if current.is_null() {
            break;
        }
        let class = hwnd_class(current);
        if is_desktop_surface(current, &class) {
            break;
        }
        if current != hwnd && is_real_app_window(current, &class) {
            last = current;
        }
        current = GetWindow(current, GW_HWNDNEXT);
    }
    let chosen = if !last.is_null() { last } else { HWND_TOP };
    let prev = LAST_LOGGED_ANCHOR.swap(chosen as isize, Ordering::Relaxed);
    if prev != chosen as isize {
        let shell_host = desktop_shell_host();
        let sh_ex = if !shell_host.is_null() {
            GetWindowLongPtrW(shell_host, GWL_EXSTYLE)
        } else {
            0
        };
        crate::logger::info(
            "win-d",
            format!(
                "anchor changed for hwnd={:?} ({}) -> chosen={:?} (class={}, title={:?}) shell_host={:?} (class={}, ex=0x{:x})",
                hwnd,
                hwnd_title(hwnd),
                chosen,
                hwnd_class(chosen),
                hwnd_title(chosen),
                shell_host,
                hwnd_class(shell_host),
                sh_ex
            ),
        );
    }
    chosen
}
pub unsafe fn is_below_desktop(hwnd: HWND) -> bool {
    if hwnd.is_null() || IsWindowVisible(hwnd) == 0 {
        return false;
    }
    let mut current = GetTopWindow(null_mut());
    for _ in 0..4096 {
        if current.is_null() {
            break;
        }
        if current == hwnd {
            return false;
        }
        let class = hwnd_class(current);
        if is_desktop_surface(current, &class) {
            return true;
        }
        current = GetWindow(current, GW_HWNDNEXT);
    }
    false
}
pub unsafe fn needs_repin(hwnd: HWND) -> bool {
    if hwnd.is_null() || IsWindowVisible(hwnd) == 0 {
        return false;
    }
    if (GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOPMOST as isize) != 0 {
        return true;
    }
    let mut seen_hwnd = false;
    let mut current = GetTopWindow(null_mut());
    for _ in 0..4096 {
        if current.is_null() {
            break;
        }
        if current == hwnd {
            seen_hwnd = true;
        } else {
            let class = hwnd_class(current);
            if is_desktop_surface(current, &class) {
                return !seen_hwnd;
            }
            if seen_hwnd && is_real_app_window(current, &class) {
                return true;
            }
        }
        current = GetWindow(current, GW_HWNDNEXT);
    }
    false
}
pub unsafe fn repair_if_below_desktop() -> bool {
    let mut seen_desktop = false;
    let mut all_desktop_layer = Vec::new();
    let mut any_below = false;
    let mut current = GetTopWindow(null_mut());
    for _ in 0..4096 {
        if current.is_null() {
            break;
        }
        if IsWindowVisible(current) != 0 {
            let class = hwnd_class(current);
            if is_desktop_surface(current, &class) {
                seen_desktop = true;
            } else if contains(current) {
                all_desktop_layer.push(current);
                if seen_desktop {
                    any_below = true;
                }
            }
        }
        current = GetWindow(current, GW_HWNDNEXT);
    }
    if !any_below || all_desktop_layer.is_empty() {
        return false;
    }
    let mut prev = anchor(all_desktop_layer[0]);
    for &w in &all_desktop_layer {
        SetWindowPos(
            w,
            prev,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
        prev = w;
    }
    true
}
pub unsafe fn pin(hwnd: HWND) {
    if !needs_repin(hwnd) {
        return;
    }
    let target = anchor(hwnd);
    if (GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOPMOST as isize) != 0 {
        SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
    SetWindowPos(
        hwnd,
        target,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
    );
}
unsafe extern "system" fn proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_WINDOWPOSCHANGING => {
            let position = &mut *(lp as *mut WINDOWPOS);
            if position.flags & SWP_NOZORDER == 0 {
                // A bridge can deliberately sit behind its own member. Preserve
                // that ordering, while rejecting TOP/TOPMOST/application anchors.
                if position.hwndInsertAfter == hwnd || !contains(position.hwndInsertAfter) {
                    if position.hwndInsertAfter != HWND_TOPMOST && !needs_repin(hwnd) {
                        position.flags |= SWP_NOZORDER;
                    } else {
                        position.hwndInsertAfter = anchor(hwnd);
                    }
                }
                position.flags |= SWP_NOACTIVATE | SWP_NOOWNERZORDER;
            }
        }
        WM_NCDESTROY => {
            register(hwnd, false);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activation_and_topmost_requests_keep_widgets_below_applications() {
        let _lock = crate::native_ui_test_lock();
        unsafe {
            let make = || {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    crate::model::wide("STATIC").as_ptr(),
                    crate::model::wide("Lumen desktop-layer test").as_ptr(),
                    WS_POPUP,
                    -20000,
                    -20000,
                    20,
                    20,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null(),
                )
            };
            let app = make();
            let widget = make();
            let bridge = make();
            assert!(!app.is_null() && !widget.is_null() && !bridge.is_null());
            for h in [app, widget, bridge] {
                ShowWindow(h, SW_SHOWNOACTIVATE);
            }
            register(widget, true);
            register(bridge, true);
            pin(widget);
            SetWindowPos(widget, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
            assert_eq!(
                GetWindowLongPtrW(widget, GWL_EXSTYLE) & WS_EX_TOPMOST as isize,
                0
            );
            let mut cursor = GetWindow(app, GW_HWNDNEXT);
            let mut below = false;
            for _ in 0..4096 {
                if cursor.is_null() {
                    break;
                }
                if cursor == widget {
                    below = true;
                    break;
                }
                cursor = GetWindow(cursor, GW_HWNDNEXT);
            }
            assert!(below, "widget must remain below a normal application");
            SetWindowPos(
                bridge,
                widget,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            assert_eq!(GetWindow(bridge, GW_HWNDPREV), widget);
            // Keep keyboard focus possible for Dock search; constrain stacking,
            // rather than making all widget input non-activating.
            for h in [bridge, widget, app] {
                DestroyWindow(h);
            }
        }
    }
}
