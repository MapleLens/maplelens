//! Windows with a Rust window procedure: the class is registered once per name, and each window's handler
//! (a boxed closure) lives in the window's user data until the window is destroyed.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_BTNFACE, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, GWLP_USERDATA, GetWindowLongPtrW, HICON, IDC_ARROW, LoadCursorW,
    RegisterClassW, SetWindowLongPtrW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_NCCREATE, WM_NCDESTROY, WNDCLASSW,
};
use windows::core::{HSTRING, PCWSTR};

/// (hwnd, msg, wparam, lparam) -> Some(result) when handled, None for the default behaviour.
pub type Handler = Box<dyn FnMut(HWND, u32, WPARAM, LPARAM) -> Option<LRESULT>>;

unsafe extern "system" fn dispatch(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: GWLP_USERDATA holds the Box<Handler> pointer set at WM_NCCREATE until WM_NCDESTROY frees it.
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let handler = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Handler;
        if msg == WM_NCDESTROY && !handler.is_null() {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(handler));
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        if !handler.is_null()
            && let Some(result) = (*handler)(hwnd, msg, wparam, lparam)
        {
            return result;
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

/// Create a top-level window of class `class` (registered on first use) whose messages go to `handler`.
pub fn create_window(
    class: &str,
    title: &str,
    (ex, style): (WINDOW_EX_STYLE, WINDOW_STYLE),
    (x, y, w, h): (i32, i32, i32, i32),
    icon: HICON,
    handler: Handler,
) -> windows::core::Result<HWND> {
    let class_name = HSTRING::from(class);
    // SAFETY: the class struct points at strings that outlive the call; registering twice just fails harmlessly.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(dispatch),
            hInstance: instance.into(),
            hIcon: icon,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as usize as *mut _), // matches the static labels
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let boxed: *mut Handler = Box::into_raw(Box::new(handler));
        let hwnd = CreateWindowExW(
            ex,
            PCWSTR(class_name.as_ptr()),
            &HSTRING::from(title),
            style,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(instance.into()),
            Some(boxed as *const _),
        );
        // On failure the handler is leaked rather than freed: creation can fail after WM_NCCREATE, in which case
        // WM_NCDESTROY has already freed it, and a double free is far worse than a few bytes once.
        hwnd
    }
}
