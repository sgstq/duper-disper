use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use tracing::{debug, info};

/// Events emitted by the hotkey listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed,
    Released,
}

/// Virtual key code for the configured hotkey.
#[derive(Debug, Clone, Copy)]
pub struct HotkeyConfig {
    pub vk_code: u32,
    pub suppress: bool,
}

/// Parse a hotkey string like "CapsLock" or "F9" into a Windows virtual key code.
pub fn parse_hotkey(key_str: &str) -> Result<HotkeyConfig> {
    let key = key_str.trim().to_lowercase();
    let (vk_code, suppress) = match key.as_str() {
        "capslock" | "caps" => (0x14u32, true), // VK_CAPITAL - suppress to prevent toggle
        "f1" => (0x70, false),
        "f2" => (0x71, false),
        "f3" => (0x72, false),
        "f4" => (0x73, false),
        "f5" => (0x74, false),
        "f6" => (0x75, false),
        "f7" => (0x76, false),
        "f8" => (0x77, false),
        "f9" => (0x78, false),
        "f10" => (0x79, false),
        "f11" => (0x7A, false),
        "f12" => (0x7B, false),
        "scrolllock" => (0x91, true),
        "pause" => (0x13, false),
        "insert" => (0x2D, false),
        "space" => (0x20, false),
        "tab" => (0x09, false),
        "escape" | "esc" => (0x1B, false),
        s if s.len() == 1 => {
            let c = s.chars().next().unwrap();
            match c {
                'a'..='z' => (0x41 + (c as u32 - 'a' as u32), false),
                '0'..='9' => (0x30 + (c as u32 - '0' as u32), false),
                _ => anyhow::bail!("Unsupported key: {}", s),
            }
        }
        _ => anyhow::bail!("Unknown key: {}", key_str),
    };

    Ok(HotkeyConfig { vk_code, suppress })
}

/// Start listening for hotkey press/release using a low-level keyboard hook.
/// Returns a receiver for hotkey events. The hook runs on a dedicated thread
/// with its own message pump.
pub fn start_listener(
    config: HotkeyConfig,
    running: Arc<AtomicBool>,
) -> Result<mpsc::Receiver<HotkeyEvent>> {
    let (tx, rx) = mpsc::channel();

    std::thread::Builder::new()
        .name("hotkey-hook".into())
        .spawn(move || {
            if let Err(e) = run_hook_thread(config, tx, running) {
                tracing::error!("Hotkey hook thread failed: {}", e);
            }
        })
        .context("Failed to spawn hotkey hook thread")?;

    Ok(rx)
}

#[cfg(windows)]
fn run_hook_thread(
    config: HotkeyConfig,
    tx: mpsc::Sender<HotkeyEvent>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    use std::cell::RefCell;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
        HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
        WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    // Thread-local storage for the hook callback data.
    thread_local! {
        static HOOK_DATA: RefCell<Option<HookData>> = RefCell::new(None);
    }

    struct HookData {
        vk_code: u32,
        suppress: bool,
        tx: mpsc::Sender<HotkeyEvent>,
    }

    unsafe extern "system" fn hook_proc(
        n_code: i32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        if n_code >= 0 {
            let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
            let mut handled = false;

            HOOK_DATA.with(|data| {
                if let Some(ref data) = *data.borrow() {
                    if kb.vkCode == data.vk_code {
                        let msg = w_param.0 as u32;
                        let event = if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                            Some(HotkeyEvent::Pressed)
                        } else if msg == WM_KEYUP || msg == WM_SYSKEYUP {
                            Some(HotkeyEvent::Released)
                        } else {
                            None
                        };

                        if let Some(event) = event {
                            let _ = data.tx.send(event);
                        }

                        if data.suppress {
                            handled = true;
                        }
                    }
                }
            });

            if handled {
                return LRESULT(1); // Suppress the key
            }
        }

        unsafe { CallNextHookEx(HHOOK::default(), n_code, w_param, l_param) }
    }

    // Set up thread-local hook data
    HOOK_DATA.with(|data| {
        *data.borrow_mut() = Some(HookData {
            vk_code: config.vk_code,
            suppress: config.suppress,
            tx,
        });
    });

    // Install the hook
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) }
        .context("Failed to install keyboard hook")?;

    info!(
        "Low-level keyboard hook installed (vk=0x{:02X}, suppress={})",
        config.vk_code, config.suppress
    );

    // Run message pump - required for the hook to receive events
    let mut msg = MSG::default();
    while running.load(Ordering::SeqCst) {
        unsafe {
            // Use GetMessageW which blocks until a message arrives.
            // The hook callback fires from within GetMessage when keyboard events occur.
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 <= 0 {
                break; // WM_QUIT or error
            }
        }
    }

    unsafe {
        let _ = UnhookWindowsHookEx(hook);
    }
    debug!("Keyboard hook removed");

    Ok(())
}

#[cfg(not(windows))]
fn run_hook_thread(
    config: HotkeyConfig,
    tx: mpsc::Sender<HotkeyEvent>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    // Stub for non-Windows: just sleep until shutdown
    info!("Hotkey hook not available on this platform (stub mode)");
    while running.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(())
}
