use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem;
use std::ptr;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use glib::translate::ToGlibPtr;
use gtk::{gdk, glib};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::UpdateWindow;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CallWindowProcW, CreatePopupMenu, DefWindowProcW, DestroyMenu, GWLP_WNDPROC,
    GetCursorPos, HICON, IDI_APPLICATION, LoadIconW, MF_STRING, SW_HIDE, SW_SHOW,
    SetForegroundWindow, SetWindowLongPtrW, ShowWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, WM_APP, WM_LBUTTONUP, WM_NCDESTROY, WM_RBUTTONUP, WNDPROC,
};

use crate::logging;
use crate::windows::win32_helpers::wide;

type VoidCallbackHandle = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
type WeakVoidCallbackHandle = Weak<RefCell<Option<Rc<dyn Fn()>>>>;

const WM_WINDOWS_GTK_TRAYICON: u32 = WM_APP + 91;
const WINDOWS_GTK_TRAY_ICON_ID: u32 = 1;
const WINDOWS_GTK_TRAY_MENU_SHOW: usize = 1;
const WINDOWS_GTK_TRAY_MENU_SETTINGS: usize = 2;
const WINDOWS_GTK_TRAY_MENU_QUIT: usize = 3;

unsafe extern "C" {
    fn gdk_win32_surface_get_handle(surface: *mut gdk::ffi::GdkSurface) -> *mut c_void;
}

thread_local! {
    static WINDOWS_GTK_TRAY_CONTROLLERS: RefCell<HashMap<isize, Weak<WindowsGtkTrayControllerInner>>> =
        RefCell::new(HashMap::new());
}

#[derive(Clone)]
pub(super) struct WindowsGtkTrayController {
    inner: Rc<WindowsGtkTrayControllerInner>,
}

struct WindowsGtkTrayControllerInner {
    window: adw::ApplicationWindow,
    open_settings_handle: WeakVoidCallbackHandle,
    force_quit_requested: Rc<Cell<bool>>,
    hwnd: Cell<HWND>,
    previous_wndproc: Cell<isize>,
    tray_icon_added: Cell<bool>,
    window_hidden_to_tray: Cell<bool>,
    product_display_name: String,
}

impl WindowsGtkTrayController {
    pub(super) fn new(
        window: &adw::ApplicationWindow,
        open_settings_handle: WeakVoidCallbackHandle,
        force_quit_requested: Rc<Cell<bool>>,
        product_display_name: String,
    ) -> Self {
        Self {
            inner: Rc::new(WindowsGtkTrayControllerInner {
                window: window.clone(),
                open_settings_handle,
                force_quit_requested,
                hwnd: Cell::new(ptr::null_mut()),
                previous_wndproc: Cell::new(0),
                tray_icon_added: Cell::new(false),
                window_hidden_to_tray: Cell::new(false),
                product_display_name,
            }),
        }
    }

    pub(super) fn install(&self) -> bool {
        if self.inner.tray_icon_added.get() {
            return true;
        }

        let Some(hwnd) = gtk_window_hwnd(&self.inner.window) else {
            logging::info("Windows GTK tray icon unavailable because the shell HWND is not ready");
            return false;
        };

        self.inner.hwnd.set(hwnd);
        WINDOWS_GTK_TRAY_CONTROLLERS.with(|controllers| {
            controllers
                .borrow_mut()
                .insert(hwnd as isize, Rc::downgrade(&self.inner));
        });

        if self.inner.previous_wndproc.get() == 0 {
            let previous = unsafe {
                SetWindowLongPtrW(
                    hwnd,
                    GWLP_WNDPROC,
                    windows_gtk_tray_wndproc as usize as isize,
                )
            };
            if previous == 0 {
                WINDOWS_GTK_TRAY_CONTROLLERS.with(|controllers| {
                    controllers.borrow_mut().remove(&(hwnd as isize));
                });
                logging::info(
                    "Windows GTK tray icon unavailable because the shell message hook failed",
                );
                return false;
            }
            self.inner.previous_wndproc.set(previous);
        }

        let mut notify = windows_gtk_tray_icon_data(hwnd);
        fill_wide_buffer(&mut notify.szTip, &self.inner.product_display_name);
        notify.hIcon = load_windows_tray_icon();
        if notify.hIcon.is_null() {
            logging::info("Windows GTK tray icon unavailable because no icon could be loaded");
            return false;
        }

        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &notify) } != 0;
        self.inner.tray_icon_added.set(added);
        if added {
            logging::info("installed Windows GTK tray icon");
        } else {
            logging::error("failed to install Windows GTK tray icon");
        }
        added
    }

    pub(super) fn hide_window_to_tray(&self) -> bool {
        if !self.install() {
            return false;
        }

        let hwnd = self.inner.hwnd.get();
        if hwnd.is_null() {
            return false;
        }

        unsafe {
            ShowWindow(hwnd, SW_HIDE);
        }
        self.inner.window_hidden_to_tray.set(true);
        self.sync_tooltip();
        logging::info("hiding Windows GTK shell window to tray");
        true
    }

    fn restore_window_from_tray(&self) {
        let hwnd = self.inner.hwnd.get();
        if !hwnd.is_null() {
            unsafe {
                ShowWindow(hwnd, SW_SHOW);
                SetForegroundWindow(hwnd);
                UpdateWindow(hwnd);
            }
        }
        self.inner.window_hidden_to_tray.set(false);
        self.sync_tooltip();
        self.inner.window.present();
    }

    fn open_settings_from_tray(&self) {
        self.restore_window_from_tray();
        let Some(open_settings_handle) = self.inner.open_settings_handle.upgrade() else {
            return;
        };
        let Some(open_settings) = open_settings_handle.borrow().as_ref().cloned() else {
            return;
        };
        glib::idle_add_local_once(move || open_settings());
    }

    fn quit_from_tray(&self) {
        self.inner.force_quit_requested.set(true);
        self.inner.window_hidden_to_tray.set(false);
        self.sync_tooltip();
        let window = self.inner.window.clone();
        glib::idle_add_local_once(move || window.close());
    }

    fn show_tray_menu(&self) {
        let hwnd = self.inner.hwnd.get();
        if hwnd.is_null() {
            return;
        }

        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return;
        }

        unsafe {
            AppendMenuW(
                menu,
                MF_STRING,
                WINDOWS_GTK_TRAY_MENU_SHOW,
                wide("Show / Restore").as_ptr(),
            );
            AppendMenuW(
                menu,
                MF_STRING,
                WINDOWS_GTK_TRAY_MENU_SETTINGS,
                wide("Open Settings").as_ptr(),
            );
            AppendMenuW(
                menu,
                MF_STRING,
                WINDOWS_GTK_TRAY_MENU_QUIT,
                wide("Quit").as_ptr(),
            );
        }

        let mut point = POINT { x: 0, y: 0 };
        unsafe {
            GetCursorPos(&mut point);
            SetForegroundWindow(hwnd);
        }

        let selected = unsafe {
            TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_RETURNCMD,
                point.x,
                point.y,
                0,
                hwnd,
                ptr::null(),
            )
        };

        match selected as usize {
            WINDOWS_GTK_TRAY_MENU_SHOW => self.restore_window_from_tray(),
            WINDOWS_GTK_TRAY_MENU_SETTINGS => self.open_settings_from_tray(),
            WINDOWS_GTK_TRAY_MENU_QUIT => self.quit_from_tray(),
            _ => {}
        }

        unsafe {
            DestroyMenu(menu);
        }
    }

    fn sync_tooltip(&self) {
        let hwnd = self.inner.hwnd.get();
        if hwnd.is_null() || !self.inner.tray_icon_added.get() {
            return;
        }

        let mut notify = windows_gtk_tray_icon_data(hwnd);
        let tooltip = if self.inner.window_hidden_to_tray.get() {
            format!("{} (hidden to background)", self.inner.product_display_name)
        } else {
            self.inner.product_display_name.clone()
        };
        fill_wide_buffer(&mut notify.szTip, &tooltip);
        notify.hIcon = load_windows_tray_icon();
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &notify);
        }
    }

    pub(super) fn shutdown(&self) {
        let hwnd = self.inner.hwnd.get();
        if hwnd.is_null() {
            return;
        }

        if self.inner.tray_icon_added.replace(false) {
            let notify = windows_gtk_tray_icon_data(hwnd);
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &notify);
            }
        }
        self.inner.window_hidden_to_tray.set(false);

        let previous = self.inner.previous_wndproc.replace(0);
        if previous != 0 {
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_WNDPROC, previous);
            }
        }

        WINDOWS_GTK_TRAY_CONTROLLERS.with(|controllers| {
            controllers.borrow_mut().remove(&(hwnd as isize));
        });
        self.inner.hwnd.set(ptr::null_mut());
    }
}

unsafe extern "system" fn windows_gtk_tray_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let controller = WINDOWS_GTK_TRAY_CONTROLLERS.with(|controllers| {
        controllers
            .borrow()
            .get(&(hwnd as isize))
            .and_then(Weak::upgrade)
    });

    if let Some(inner) = controller.as_ref() {
        let tray_controller = WindowsGtkTrayController {
            inner: inner.clone(),
        };
        match message {
            WM_WINDOWS_GTK_TRAYICON => {
                match lparam as u32 {
                    WM_LBUTTONUP => tray_controller.restore_window_from_tray(),
                    WM_RBUTTONUP => tray_controller.show_tray_menu(),
                    _ => {}
                }
                return 0;
            }
            WM_NCDESTROY => {
                let previous = inner.previous_wndproc.get();
                tray_controller.shutdown();
                if previous != 0 {
                    let previous_wndproc: WNDPROC = unsafe { mem::transmute(previous) };
                    return unsafe {
                        CallWindowProcW(previous_wndproc, hwnd, message, wparam, lparam)
                    };
                }
            }
            _ => {}
        }

        let previous = inner.previous_wndproc.get();
        if previous != 0 {
            let previous_wndproc: WNDPROC = unsafe { mem::transmute(previous) };
            return unsafe { CallWindowProcW(previous_wndproc, hwnd, message, wparam, lparam) };
        }
    }

    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn gtk_window_hwnd(window: &adw::ApplicationWindow) -> Option<HWND> {
    let widget = window.upcast_ref::<gtk::Widget>();
    let native = widget.native()?;
    let surface = native.surface()?;
    let handle = unsafe { gdk_win32_surface_get_handle(surface.to_glib_none().0) };
    (!handle.is_null()).then_some(handle as HWND)
}

fn windows_gtk_tray_icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: WINDOWS_GTK_TRAY_ICON_ID,
        uFlags: NIF_MESSAGE | NIF_TIP | NIF_ICON,
        uCallbackMessage: WM_WINDOWS_GTK_TRAYICON,
        ..unsafe { mem::zeroed() }
    }
}

fn load_windows_tray_icon() -> HICON {
    let module = unsafe { GetModuleHandleW(ptr::null()) };
    let resource_icon = unsafe { LoadIconW(module, 1usize as *const u16) };
    if !resource_icon.is_null() {
        return resource_icon;
    }
    unsafe { LoadIconW(ptr::null_mut(), IDI_APPLICATION) }
}

fn fill_wide_buffer(buffer: &mut [u16], value: &str) {
    if buffer.is_empty() {
        return;
    }

    for slot in buffer.iter_mut() {
        *slot = 0;
    }

    let writable_len = buffer.len().saturating_sub(1);
    for (slot, unit) in buffer
        .iter_mut()
        .take(writable_len)
        .zip(value.encode_utf16())
    {
        *slot = unit;
    }
}
