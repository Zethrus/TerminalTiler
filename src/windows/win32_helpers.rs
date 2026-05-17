#[cfg(target_os = "windows")]
use std::ffi::c_void;
#[cfg(target_os = "windows")]
use std::mem;
#[cfg(target_os = "windows")]
use std::ptr;

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{HINSTANCE, HWND};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, LoadCursorW,
    RegisterClassW, WINDOW_EX_STYLE, WNDCLASSW, WNDPROC,
};

#[cfg(target_os = "windows")]
pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
pub fn read_window_text(hwnd: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return String::new();
    }

    let mut buffer = vec![0u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..copied as usize])
}

#[cfg(target_os = "windows")]
pub fn register_window_class(
    instance: HINSTANCE,
    class_name: &str,
    window_proc: WNDPROC,
    error_label: &str,
) -> Result<(), String> {
    let class_name_wide = wide(class_name);
    let mut class = unsafe { mem::zeroed::<WNDCLASSW>() };
    class.style = windows_sys::Win32::UI::WindowsAndMessaging::CS_HREDRAW
        | windows_sys::Win32::UI::WindowsAndMessaging::CS_VREDRAW;
    class.lpfnWndProc = window_proc;
    class.hInstance = instance;
    class.hCursor = unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) };
    class.lpszClassName = class_name_wide.as_ptr();
    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(1410) {
            return Err(format!("RegisterClassW failed for {error_label}: {error}"));
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn create_child_window(
    parent: HWND,
    class_name: &str,
    text: &str,
    style: u32,
    control_id: isize,
) -> HWND {
    create_child_window_ex(
        parent,
        class_name,
        text,
        style,
        0 as WINDOW_EX_STYLE,
        control_id,
        ptr::null_mut(),
    )
}

#[cfg(target_os = "windows")]
pub fn create_child_window_with_ex_style(
    parent: HWND,
    class_name: &str,
    text: &str,
    style: u32,
    ex_style: WINDOW_EX_STYLE,
    control_id: isize,
) -> HWND {
    create_child_window_ex(
        parent,
        class_name,
        text,
        style,
        ex_style,
        control_id,
        ptr::null_mut(),
    )
}

#[cfg(target_os = "windows")]
pub fn create_child_window_ex(
    parent: HWND,
    class_name: &str,
    text: &str,
    style: u32,
    ex_style: WINDOW_EX_STYLE,
    control_id: isize,
    lp_param: *mut c_void,
) -> HWND {
    unsafe {
        CreateWindowExW(
            ex_style,
            wide(class_name).as_ptr(),
            wide(text).as_ptr(),
            style,
            0,
            0,
            0,
            0,
            parent,
            control_id as HMENU,
            GetModuleHandleW(ptr::null()),
            lp_param as *const c_void,
        )
    }
}
