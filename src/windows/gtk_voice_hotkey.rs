#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
mod imp {
    use std::sync::mpsc;
    use std::thread;

    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW,
        TranslateMessage, WM_HOTKEY, WM_QUIT,
    };

    use crate::windows::shortcut_capture;

    const WINDOWS_GTK_VOICE_HOTKEY_ID: i32 = 0x5456;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum WindowsGlobalHotkeyEvent {
        Activated,
    }

    pub struct WindowsGlobalHotkeyHandle {
        thread_id: u32,
        join_handle: Option<thread::JoinHandle<()>>,
    }

    impl WindowsGlobalHotkeyHandle {
        pub fn start(
            shortcut: String,
            event_tx: mpsc::Sender<WindowsGlobalHotkeyEvent>,
        ) -> Result<Self, String> {
            let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<u32, String>>(1);
            let join_handle = thread::spawn(move || {
                let thread_id = unsafe { GetCurrentThreadId() };
                ensure_message_queue();
                let registered = register_hotkey(&shortcut);
                if !registered {
                    let _ = ready_tx.send(Err(format!(
                        "could not register Windows GTK voice hotkey {shortcut}"
                    )));
                    return;
                }
                let _ = ready_tx.send(Ok(thread_id));
                run_hotkey_message_loop(event_tx);
                unsafe {
                    UnregisterHotKey(
                        std::ptr::null_mut::<std::ffi::c_void>() as HWND,
                        WINDOWS_GTK_VOICE_HOTKEY_ID,
                    );
                }
            });

            match ready_rx
                .recv()
                .unwrap_or_else(|_| Err("hotkey worker exited before registration".into()))
            {
                Ok(thread_id) => Ok(Self {
                    thread_id,
                    join_handle: Some(join_handle),
                }),
                Err(error) => {
                    let _ = join_handle.join();
                    Err(error)
                }
            }
        }
    }

    impl Drop for WindowsGlobalHotkeyHandle {
        fn drop(&mut self) {
            unsafe {
                PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
            }
            if let Some(join_handle) = self.join_handle.take() {
                let _ = join_handle.join();
            }
        }
    }

    fn ensure_message_queue() {
        let mut message = empty_message();
        unsafe {
            PeekMessageW(
                &mut message,
                std::ptr::null_mut::<std::ffi::c_void>() as HWND,
                0,
                0,
                PM_NOREMOVE,
            );
        }
    }

    fn register_hotkey(shortcut: &str) -> bool {
        let Some((ctrl, shift, alt, super_key, virtual_key)) =
            shortcut_capture::registration_parts(shortcut)
        else {
            return false;
        };
        let mut modifiers = MOD_NOREPEAT;
        if ctrl {
            modifiers |= MOD_CONTROL;
        }
        if shift {
            modifiers |= MOD_SHIFT;
        }
        if alt {
            modifiers |= MOD_ALT;
        }
        if super_key {
            modifiers |= MOD_WIN;
        }
        unsafe {
            RegisterHotKey(
                std::ptr::null_mut::<std::ffi::c_void>() as HWND,
                WINDOWS_GTK_VOICE_HOTKEY_ID,
                modifiers,
                virtual_key,
            ) != 0
        }
    }

    fn run_hotkey_message_loop(event_tx: mpsc::Sender<WindowsGlobalHotkeyEvent>) {
        let mut message = empty_message();
        loop {
            let result = unsafe {
                GetMessageW(
                    &mut message,
                    std::ptr::null_mut::<std::ffi::c_void>() as HWND,
                    0,
                    0,
                )
            };
            if result <= 0 {
                break;
            }
            if message.message == WM_HOTKEY
                && message.wParam == WINDOWS_GTK_VOICE_HOTKEY_ID as usize
            {
                let _ = event_tx.send(WindowsGlobalHotkeyEvent::Activated);
            } else {
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
    }

    fn empty_message() -> MSG {
        MSG {
            hwnd: std::ptr::null_mut::<std::ffi::c_void>() as HWND,
            message: 0,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: Default::default(),
        }
    }
}

#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
pub use imp::{WindowsGlobalHotkeyEvent, WindowsGlobalHotkeyHandle};
