#[cfg(target_os = "linux")]
use std::ffi::{CStr, CString};
#[cfg(target_os = "linux")]
use std::mem::MaybeUninit;
#[cfg(target_os = "linux")]
use std::os::raw::{c_char, c_int, c_long, c_uint, c_ulong, c_void};
#[cfg(target_os = "linux")]
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::sync::mpsc;
#[cfg(target_os = "linux")]
use std::thread;
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
use crate::voice::hotkey::HotkeySpec;

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxGlobalHotkeyEvent {
    Pressed,
    Released,
}

#[cfg(target_os = "linux")]
pub struct LinuxGlobalHotkeyHandle {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl Drop for LinuxGlobalHotkeyHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[cfg(target_os = "linux")]
impl LinuxGlobalHotkeyHandle {
    pub fn start(
        shortcut: String,
        tx: mpsc::Sender<LinuxGlobalHotkeyEvent>,
    ) -> Result<Self, String> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_none() {
            return Err("global hotkeys are unavailable on Wayland without XWayland".into());
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = stop.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let join = thread::spawn(move || {
            let result = run_x11_hotkey_loop(shortcut, tx, stop_for_thread, ready_tx);
            if let Err(error) = result {
                eprintln!("TerminalTiler voice global hotkey stopped: {error}");
            }
        });

        match ready_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                join: Some(join),
            }),
            Ok(Err(error)) => {
                stop.store(true, Ordering::Relaxed);
                let _ = join.join();
                Err(error)
            }
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                let _ = join.join();
                Err("timed out registering X11 global hotkey".into())
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn run_x11_hotkey_loop(
    shortcut: String,
    tx: mpsc::Sender<LinuxGlobalHotkeyEvent>,
    stop: Arc<AtomicBool>,
    ready_tx: mpsc::SyncSender<Result<(), String>>,
) -> Result<(), String> {
    let spec = HotkeySpec::parse(&shortcut).ok_or_else(|| "invalid voice hotkey".to_string())?;
    let x11 = X11::open()?;
    let display = unsafe { (x11.open_display)(std::ptr::null()) };
    if display.is_null() {
        let _ = ready_tx.send(Err("could not open X11 display".into()));
        return Ok(());
    }

    let root = unsafe { (x11.default_root_window)(display) };
    let key_name = CString::new(spec.key.as_str()).map_err(|error| error.to_string())?;
    let keysym = unsafe { (x11.string_to_keysym)(key_name.as_ptr()) };
    if keysym == 0 {
        unsafe {
            (x11.close_display)(display);
        }
        let _ = ready_tx.send(Err(format!("X11 could not resolve key '{}'", spec.key)));
        return Ok(());
    }
    let keycode = unsafe { (x11.keysym_to_keycode)(display, keysym) };
    if keycode == 0 {
        unsafe {
            (x11.close_display)(display);
        }
        let _ = ready_tx.send(Err(format!("X11 could not map key '{}'", spec.key)));
        return Ok(());
    }

    let modifiers = x11_modifiers(&spec);
    for modifier in lock_modifier_variants(modifiers) {
        unsafe {
            (x11.grab_key)(display, keycode as c_int, modifier, root, 0, 1, 1);
        }
    }
    unsafe {
        (x11.select_input)(display, root, KEY_PRESS_MASK | KEY_RELEASE_MASK);
        (x11.flush)(display);
    }
    let _ = ready_tx.send(Ok(()));

    let mut pressed = false;
    while !stop.load(Ordering::Relaxed) {
        while unsafe { (x11.pending)(display) } > 0 {
            let mut event = MaybeUninit::<XEvent>::zeroed();
            unsafe {
                (x11.next_event)(display, event.as_mut_ptr());
                let event = event.assume_init();
                let event_type = event.type_;
                if event_type != KEY_PRESS && event_type != KEY_RELEASE {
                    continue;
                }
                let key = event.xkey;
                if key.keycode != keycode || normalize_state(key.state) != modifiers {
                    continue;
                }
                match event_type {
                    KEY_PRESS if !pressed => {
                        pressed = true;
                        let _ = tx.send(LinuxGlobalHotkeyEvent::Pressed);
                    }
                    KEY_RELEASE => {
                        if x11_release_is_auto_repeat(&x11, display, keycode, key.time) {
                            continue;
                        }
                        pressed = false;
                        let _ = tx.send(LinuxGlobalHotkeyEvent::Released);
                    }
                    _ => {}
                }
            }
        }
        thread::sleep(Duration::from_millis(16));
    }

    for modifier in lock_modifier_variants(modifiers) {
        unsafe {
            (x11.ungrab_key)(display, keycode as c_int, modifier, root);
        }
    }
    unsafe {
        (x11.flush)(display);
        (x11.close_display)(display);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn x11_modifiers(spec: &HotkeySpec) -> c_uint {
    let mut modifiers = 0;
    if spec.shift {
        modifiers |= SHIFT_MASK;
    }
    if spec.ctrl {
        modifiers |= CONTROL_MASK;
    }
    if spec.alt {
        modifiers |= MOD1_MASK;
    }
    if spec.super_key {
        modifiers |= MOD4_MASK;
    }
    modifiers
}

#[cfg(target_os = "linux")]
fn lock_modifier_variants(base: c_uint) -> [c_uint; 4] {
    [
        base,
        base | LOCK_MASK,
        base | MOD2_MASK,
        base | LOCK_MASK | MOD2_MASK,
    ]
}

#[cfg(target_os = "linux")]
fn normalize_state(state: c_uint) -> c_uint {
    state & !(LOCK_MASK | MOD2_MASK)
}

#[cfg(target_os = "linux")]
struct X11 {
    handle: *mut c_void,
    open_display: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    close_display: unsafe extern "C" fn(*mut c_void) -> c_int,
    default_root_window: unsafe extern "C" fn(*mut c_void) -> c_ulong,
    string_to_keysym: unsafe extern "C" fn(*const c_char) -> c_ulong,
    keysym_to_keycode: unsafe extern "C" fn(*mut c_void, c_ulong) -> c_uint,
    grab_key:
        unsafe extern "C" fn(*mut c_void, c_int, c_uint, c_ulong, c_int, c_int, c_int) -> c_int,
    ungrab_key: unsafe extern "C" fn(*mut c_void, c_int, c_uint, c_ulong) -> c_int,
    select_input: unsafe extern "C" fn(*mut c_void, c_ulong, c_long) -> c_int,
    pending: unsafe extern "C" fn(*mut c_void) -> c_int,
    next_event: unsafe extern "C" fn(*mut c_void, *mut XEvent) -> c_int,
    peek_event: unsafe extern "C" fn(*mut c_void, *mut XEvent) -> c_int,
    flush: unsafe extern "C" fn(*mut c_void) -> c_int,
}

#[cfg(target_os = "linux")]
unsafe fn x11_release_is_auto_repeat(
    x11: &X11,
    display: *mut c_void,
    keycode: c_uint,
    release_time: c_ulong,
) -> bool {
    if unsafe { (x11.pending)(display) } <= 0 {
        return false;
    }
    let mut next = MaybeUninit::<XEvent>::zeroed();
    unsafe {
        (x11.peek_event)(display, next.as_mut_ptr());
        let next = next.assume_init();
        if next.type_ != KEY_PRESS {
            return false;
        }
        let next_key = next.xkey;
        if next_key.keycode != keycode || next_key.time != release_time {
            return false;
        }
    }

    let mut repeated_press = MaybeUninit::<XEvent>::zeroed();
    unsafe {
        (x11.next_event)(display, repeated_press.as_mut_ptr());
    }
    true
}

#[cfg(target_os = "linux")]
impl X11 {
    fn open() -> Result<Self, String> {
        let library = CString::new("libX11.so.6").unwrap();
        let handle = unsafe { libc::dlopen(library.as_ptr(), libc::RTLD_NOW) };
        if handle.is_null() {
            return Err(dl_error());
        }
        unsafe {
            Ok(Self {
                handle,
                open_display: load_symbol(handle, c"XOpenDisplay")?,
                close_display: load_symbol(handle, c"XCloseDisplay")?,
                default_root_window: load_symbol(handle, c"XDefaultRootWindow")?,
                string_to_keysym: load_symbol(handle, c"XStringToKeysym")?,
                keysym_to_keycode: load_symbol(handle, c"XKeysymToKeycode")?,
                grab_key: load_symbol(handle, c"XGrabKey")?,
                ungrab_key: load_symbol(handle, c"XUngrabKey")?,
                select_input: load_symbol(handle, c"XSelectInput")?,
                pending: load_symbol(handle, c"XPending")?,
                next_event: load_symbol(handle, c"XNextEvent")?,
                peek_event: load_symbol(handle, c"XPeekEvent")?,
                flush: load_symbol(handle, c"XFlush")?,
            })
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for X11 {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.handle);
        }
    }
}

#[cfg(target_os = "linux")]
unsafe fn load_symbol<T: Copy>(handle: *mut c_void, name: &CStr) -> Result<T, String> {
    let symbol = unsafe { libc::dlsym(handle, name.as_ptr()) };
    if symbol.is_null() {
        return Err(dl_error());
    }
    Ok(unsafe { std::mem::transmute_copy(&symbol) })
}

#[cfg(target_os = "linux")]
fn dl_error() -> String {
    let error = unsafe { libc::dlerror() };
    if error.is_null() {
        "unknown dynamic loader error".into()
    } else {
        unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Clone, Copy)]
struct XKeyEvent {
    type_: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut c_void,
    window: c_ulong,
    root: c_ulong,
    subwindow: c_ulong,
    time: c_ulong,
    x: c_int,
    y: c_int,
    x_root: c_int,
    y_root: c_int,
    state: c_uint,
    keycode: c_uint,
    same_screen: c_int,
}

#[cfg(target_os = "linux")]
#[repr(C)]
union XEvent {
    type_: c_int,
    xkey: XKeyEvent,
    pad: [c_long; 24],
}

#[cfg(target_os = "linux")]
const KEY_PRESS: c_int = 2;
#[cfg(target_os = "linux")]
const KEY_RELEASE: c_int = 3;
#[cfg(target_os = "linux")]
const KEY_PRESS_MASK: c_long = 1;
#[cfg(target_os = "linux")]
const KEY_RELEASE_MASK: c_long = 1 << 1;
#[cfg(target_os = "linux")]
const SHIFT_MASK: c_uint = 1;
#[cfg(target_os = "linux")]
const LOCK_MASK: c_uint = 1 << 1;
#[cfg(target_os = "linux")]
const CONTROL_MASK: c_uint = 1 << 2;
#[cfg(target_os = "linux")]
const MOD1_MASK: c_uint = 1 << 3;
#[cfg(target_os = "linux")]
const MOD2_MASK: c_uint = 1 << 4;
#[cfg(target_os = "linux")]
const MOD4_MASK: c_uint = 1 << 6;
