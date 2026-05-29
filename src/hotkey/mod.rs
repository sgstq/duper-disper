use anyhow::{Context, Result};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
#[cfg(windows)]
use tracing::debug;
use tracing::info;

/// Events emitted by the hotkey listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed,
    Released,
}

/// Parsed hotkey definition. A hotkey may be a single key or any key combination.
#[derive(Debug, Clone)]
pub struct HotkeyConfig {
    keys: Vec<HotkeyKey>,
    #[cfg(windows)]
    suppress_keys: HashSet<HotkeyKey>,
}

impl HotkeyConfig {
    pub fn keys(&self) -> &[HotkeyKey] {
        &self.keys
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyKey {
    Control,
    Shift,
    Alt,
    Meta,
    CapsLock,
    ScrollLock,
    Pause,
    Insert,
    Enter,
    Backspace,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Left,
    Right,
    Up,
    Down,
    Space,
    Tab,
    Escape,
    Plus,
    Minus,
    F(u8),
    Letter(char),
    Digit(char),
}

/// Parse a hotkey string like "CapsLock", "F9", or "Ctrl+Shift+Space".
pub fn parse_hotkey(key_str: &str) -> Result<HotkeyConfig> {
    let mut keys = Vec::new();
    let mut seen = HashSet::new();

    for token in key_str.split('+').map(str::trim) {
        if token.is_empty() {
            anyhow::bail!("Hotkey contains an empty key segment");
        }
        let key = parse_key_token(token)?;
        if seen.insert(key) {
            keys.push(key);
        }
    }

    if keys.is_empty() {
        anyhow::bail!("Hotkey cannot be empty");
    }

    #[cfg(windows)]
    let suppress_keys = keys
        .iter()
        .copied()
        .filter(|key| matches!(key, HotkeyKey::CapsLock | HotkeyKey::ScrollLock))
        .collect();

    Ok(HotkeyConfig {
        keys,
        #[cfg(windows)]
        suppress_keys,
    })
}

fn parse_key_token(token: &str) -> Result<HotkeyKey> {
    let token = token.trim().to_lowercase();
    let key = match token.as_str() {
        "ctrl" | "control" => HotkeyKey::Control,
        "shift" => HotkeyKey::Shift,
        "alt" | "option" | "opt" => HotkeyKey::Alt,
        "cmd" | "command" | "meta" | "super" | "win" | "windows" => HotkeyKey::Meta,
        "capslock" | "caps" => HotkeyKey::CapsLock,
        "scrolllock" => HotkeyKey::ScrollLock,
        "pause" => HotkeyKey::Pause,
        "insert" => HotkeyKey::Insert,
        "enter" | "return" => HotkeyKey::Enter,
        "backspace" => HotkeyKey::Backspace,
        "delete" | "del" => HotkeyKey::Delete,
        "home" => HotkeyKey::Home,
        "end" => HotkeyKey::End,
        "pageup" | "pgup" => HotkeyKey::PageUp,
        "pagedown" | "pgdown" | "pgdn" => HotkeyKey::PageDown,
        "left" | "arrowleft" => HotkeyKey::Left,
        "right" | "arrowright" => HotkeyKey::Right,
        "up" | "arrowup" => HotkeyKey::Up,
        "down" | "arrowdown" => HotkeyKey::Down,
        "space" => HotkeyKey::Space,
        "tab" => HotkeyKey::Tab,
        "escape" | "esc" => HotkeyKey::Escape,
        "plus" => HotkeyKey::Plus,
        "minus" => HotkeyKey::Minus,
        "f1" => HotkeyKey::F(1),
        "f2" => HotkeyKey::F(2),
        "f3" => HotkeyKey::F(3),
        "f4" => HotkeyKey::F(4),
        "f5" => HotkeyKey::F(5),
        "f6" => HotkeyKey::F(6),
        "f7" => HotkeyKey::F(7),
        "f8" => HotkeyKey::F(8),
        "f9" => HotkeyKey::F(9),
        "f10" => HotkeyKey::F(10),
        "f11" => HotkeyKey::F(11),
        "f12" => HotkeyKey::F(12),
        s if s.len() == 1 => {
            let c = s.chars().next().unwrap();
            match c {
                'a'..='z' => HotkeyKey::Letter(c),
                '0'..='9' => HotkeyKey::Digit(c),
                _ => anyhow::bail!("Unsupported key: {}", token),
            }
        }
        _ => anyhow::bail!("Unknown key: {}", token),
    };

    Ok(key)
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

#[derive(Default)]
struct ComboState {
    pressed_keys: HashSet<HotkeyKey>,
    is_active: bool,
}

impl ComboState {
    fn update(
        &mut self,
        config: &HotkeyConfig,
        changed_key: HotkeyKey,
        pressed: bool,
        tx: &mpsc::Sender<HotkeyEvent>,
    ) {
        if pressed {
            self.pressed_keys.insert(changed_key);
        } else {
            self.pressed_keys.remove(&changed_key);
        }

        let now_active = config
            .keys
            .iter()
            .all(|required_key| self.pressed_keys.contains(required_key));

        if !self.is_active && now_active {
            let _ = tx.send(HotkeyEvent::Pressed);
        } else if self.is_active && !now_active {
            let _ = tx.send(HotkeyEvent::Released);
        }

        self.is_active = now_active;
    }
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
        CallNextHookEx, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK,
        KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
        WM_SYSKEYUP,
    };

    thread_local! {
        static HOOK_DATA: RefCell<Option<HookData>> = const { RefCell::new(None) };
    }

    struct HookData {
        config: HotkeyConfig,
        combo_state: ComboState,
        tx: mpsc::Sender<HotkeyEvent>,
    }

    unsafe extern "system" fn hook_proc(
        n_code: i32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        if n_code >= 0 {
            let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
            let msg = w_param.0 as u32;
            let pressed = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let released = msg == WM_KEYUP || msg == WM_SYSKEYUP;
            let mut handled = false;

            if pressed || released {
                if let Some(key) = windows_vk_to_hotkey_key(kb.vkCode) {
                    HOOK_DATA.with(|data| {
                        if let Some(ref mut data) = *data.borrow_mut() {
                            data.combo_state.update(&data.config, key, pressed, &data.tx);
                            if data.config.suppress_keys.contains(&key) {
                                handled = true;
                            }
                        }
                    });
                }
            }

            if handled {
                return LRESULT(1);
            }
        }

        unsafe { CallNextHookEx(HHOOK::default(), n_code, w_param, l_param) }
    }

    HOOK_DATA.with(|data| {
        *data.borrow_mut() = Some(HookData {
            config,
            combo_state: ComboState::default(),
            tx,
        });
    });

    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) }
        .context("Failed to install keyboard hook")?;

    info!("Low-level keyboard hook installed for combo hotkey");

    let mut msg = MSG::default();
    while running.load(Ordering::SeqCst) {
        unsafe {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 <= 0 {
                break;
            }
        }
    }

    unsafe {
        let _ = UnhookWindowsHookEx(hook);
    }
    debug!("Keyboard hook removed");

    Ok(())
}

#[cfg(windows)]
fn windows_vk_to_hotkey_key(vk_code: u32) -> Option<HotkeyKey> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_ADD, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1,
        VK_F10, VK_F11, VK_F12, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9,
        VK_HOME, VK_INSERT, VK_LEFT, VK_LWIN, VK_MENU, VK_NEXT, VK_OEM_MINUS, VK_OEM_PLUS,
        VK_PAUSE, VK_PRIOR, VK_RWIN, VK_RETURN, VK_RIGHT, VK_SCROLL, VK_SHIFT, VK_SPACE, VK_SUBTRACT,
        VK_TAB, VK_UP,
    };

    Some(match vk_code {
        x if x == VK_CONTROL.0 as u32 || x == 0xA2 || x == 0xA3 => HotkeyKey::Control,
        x if x == VK_SHIFT.0 as u32 || x == 0xA0 || x == 0xA1 => HotkeyKey::Shift,
        x if x == VK_MENU.0 as u32 || x == 0xA4 || x == 0xA5 => HotkeyKey::Alt,
        x if x == VK_LWIN.0 as u32 || x == VK_RWIN.0 as u32 => HotkeyKey::Meta,
        x if x == VK_CAPITAL.0 as u32 => HotkeyKey::CapsLock,
        x if x == VK_SCROLL.0 as u32 => HotkeyKey::ScrollLock,
        x if x == VK_PAUSE.0 as u32 => HotkeyKey::Pause,
        x if x == VK_INSERT.0 as u32 => HotkeyKey::Insert,
        x if x == VK_RETURN.0 as u32 => HotkeyKey::Enter,
        x if x == VK_BACK.0 as u32 => HotkeyKey::Backspace,
        x if x == VK_DELETE.0 as u32 => HotkeyKey::Delete,
        x if x == VK_HOME.0 as u32 => HotkeyKey::Home,
        x if x == VK_END.0 as u32 => HotkeyKey::End,
        x if x == VK_PRIOR.0 as u32 => HotkeyKey::PageUp,
        x if x == VK_NEXT.0 as u32 => HotkeyKey::PageDown,
        x if x == VK_LEFT.0 as u32 => HotkeyKey::Left,
        x if x == VK_RIGHT.0 as u32 => HotkeyKey::Right,
        x if x == VK_UP.0 as u32 => HotkeyKey::Up,
        x if x == VK_DOWN.0 as u32 => HotkeyKey::Down,
        x if x == VK_SPACE.0 as u32 => HotkeyKey::Space,
        x if x == VK_TAB.0 as u32 => HotkeyKey::Tab,
        x if x == VK_ESCAPE.0 as u32 => HotkeyKey::Escape,
        x if x == VK_OEM_PLUS.0 as u32 || x == VK_ADD.0 as u32 => HotkeyKey::Plus,
        x if x == VK_OEM_MINUS.0 as u32 || x == VK_SUBTRACT.0 as u32 => HotkeyKey::Minus,
        x if x == VK_F1.0 as u32 => HotkeyKey::F(1),
        x if x == VK_F2.0 as u32 => HotkeyKey::F(2),
        x if x == VK_F3.0 as u32 => HotkeyKey::F(3),
        x if x == VK_F4.0 as u32 => HotkeyKey::F(4),
        x if x == VK_F5.0 as u32 => HotkeyKey::F(5),
        x if x == VK_F6.0 as u32 => HotkeyKey::F(6),
        x if x == VK_F7.0 as u32 => HotkeyKey::F(7),
        x if x == VK_F8.0 as u32 => HotkeyKey::F(8),
        x if x == VK_F9.0 as u32 => HotkeyKey::F(9),
        x if x == VK_F10.0 as u32 => HotkeyKey::F(10),
        x if x == VK_F11.0 as u32 => HotkeyKey::F(11),
        x if x == VK_F12.0 as u32 => HotkeyKey::F(12),
        x @ 0x41..=0x5A => HotkeyKey::Letter((b'a' + (x as u8 - 0x41)) as char),
        x @ 0x30..=0x39 => HotkeyKey::Digit((b'0' + (x as u8 - 0x30)) as char),
        _ => return None,
    })
}

#[cfg(not(windows))]
fn run_hook_thread(
    config: HotkeyConfig,
    tx: mpsc::Sender<HotkeyEvent>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    use rdev::{listen, EventType};

    for key in &config.keys {
        if to_rdev_keys(*key).is_none() {
            anyhow::bail!("Unsupported hotkey on this platform");
        }
    }

    let running_flag = running.clone();
    let mut combo_state = ComboState::default();

    info!("Starting global hotkey listener for non-Windows platform");
    listen(move |event| {
        if !running_flag.load(Ordering::SeqCst) {
            return;
        }

        let (pressed, key) = match event.event_type {
            EventType::KeyPress(key) => (true, key),
            EventType::KeyRelease(key) => (false, key),
            _ => return,
        };

        let Some(keys) = from_rdev_key(key) else {
            return;
        };

        for key in keys {
            combo_state.update(&config, key, pressed, &tx);
        }
    })
    .map_err(|e| anyhow::anyhow!("Failed to start non-Windows hotkey listener: {:?}", e))
}

#[cfg(not(windows))]
fn to_rdev_keys(key: HotkeyKey) -> Option<Vec<rdev::Key>> {
    use rdev::Key;

    Some(match key {
        HotkeyKey::Control => vec![Key::ControlLeft, Key::ControlRight],
        HotkeyKey::Shift => vec![Key::ShiftLeft, Key::ShiftRight],
        HotkeyKey::Alt => vec![Key::Alt, Key::AltGr],
        HotkeyKey::Meta => vec![Key::MetaLeft, Key::MetaRight],
        HotkeyKey::CapsLock => vec![Key::CapsLock],
        HotkeyKey::ScrollLock => vec![Key::ScrollLock],
        HotkeyKey::Pause => vec![Key::Pause],
        HotkeyKey::Insert => vec![Key::Insert],
        HotkeyKey::Enter => vec![Key::Return, Key::KpReturn],
        HotkeyKey::Backspace => vec![Key::Backspace],
        HotkeyKey::Delete => vec![Key::Delete, Key::KpDelete],
        HotkeyKey::Home => vec![Key::Home],
        HotkeyKey::End => vec![Key::End],
        HotkeyKey::PageUp => vec![Key::PageUp],
        HotkeyKey::PageDown => vec![Key::PageDown],
        HotkeyKey::Left => vec![Key::LeftArrow],
        HotkeyKey::Right => vec![Key::RightArrow],
        HotkeyKey::Up => vec![Key::UpArrow],
        HotkeyKey::Down => vec![Key::DownArrow],
        HotkeyKey::Space => vec![Key::Space],
        HotkeyKey::Tab => vec![Key::Tab],
        HotkeyKey::Escape => vec![Key::Escape],
        HotkeyKey::Plus => vec![Key::Equal, Key::KpPlus],
        HotkeyKey::Minus => vec![Key::Minus, Key::KpMinus],
        HotkeyKey::F(1) => vec![Key::F1],
        HotkeyKey::F(2) => vec![Key::F2],
        HotkeyKey::F(3) => vec![Key::F3],
        HotkeyKey::F(4) => vec![Key::F4],
        HotkeyKey::F(5) => vec![Key::F5],
        HotkeyKey::F(6) => vec![Key::F6],
        HotkeyKey::F(7) => vec![Key::F7],
        HotkeyKey::F(8) => vec![Key::F8],
        HotkeyKey::F(9) => vec![Key::F9],
        HotkeyKey::F(10) => vec![Key::F10],
        HotkeyKey::F(11) => vec![Key::F11],
        HotkeyKey::F(12) => vec![Key::F12],
        HotkeyKey::F(_) => return None,
        HotkeyKey::Letter('a') => vec![Key::KeyA],
        HotkeyKey::Letter('b') => vec![Key::KeyB],
        HotkeyKey::Letter('c') => vec![Key::KeyC],
        HotkeyKey::Letter('d') => vec![Key::KeyD],
        HotkeyKey::Letter('e') => vec![Key::KeyE],
        HotkeyKey::Letter('f') => vec![Key::KeyF],
        HotkeyKey::Letter('g') => vec![Key::KeyG],
        HotkeyKey::Letter('h') => vec![Key::KeyH],
        HotkeyKey::Letter('i') => vec![Key::KeyI],
        HotkeyKey::Letter('j') => vec![Key::KeyJ],
        HotkeyKey::Letter('k') => vec![Key::KeyK],
        HotkeyKey::Letter('l') => vec![Key::KeyL],
        HotkeyKey::Letter('m') => vec![Key::KeyM],
        HotkeyKey::Letter('n') => vec![Key::KeyN],
        HotkeyKey::Letter('o') => vec![Key::KeyO],
        HotkeyKey::Letter('p') => vec![Key::KeyP],
        HotkeyKey::Letter('q') => vec![Key::KeyQ],
        HotkeyKey::Letter('r') => vec![Key::KeyR],
        HotkeyKey::Letter('s') => vec![Key::KeyS],
        HotkeyKey::Letter('t') => vec![Key::KeyT],
        HotkeyKey::Letter('u') => vec![Key::KeyU],
        HotkeyKey::Letter('v') => vec![Key::KeyV],
        HotkeyKey::Letter('w') => vec![Key::KeyW],
        HotkeyKey::Letter('x') => vec![Key::KeyX],
        HotkeyKey::Letter('y') => vec![Key::KeyY],
        HotkeyKey::Letter('z') => vec![Key::KeyZ],
        HotkeyKey::Letter(_) => return None,
        HotkeyKey::Digit('0') => vec![Key::Num0, Key::Kp0],
        HotkeyKey::Digit('1') => vec![Key::Num1, Key::Kp1],
        HotkeyKey::Digit('2') => vec![Key::Num2, Key::Kp2],
        HotkeyKey::Digit('3') => vec![Key::Num3, Key::Kp3],
        HotkeyKey::Digit('4') => vec![Key::Num4, Key::Kp4],
        HotkeyKey::Digit('5') => vec![Key::Num5, Key::Kp5],
        HotkeyKey::Digit('6') => vec![Key::Num6, Key::Kp6],
        HotkeyKey::Digit('7') => vec![Key::Num7, Key::Kp7],
        HotkeyKey::Digit('8') => vec![Key::Num8, Key::Kp8],
        HotkeyKey::Digit('9') => vec![Key::Num9, Key::Kp9],
        HotkeyKey::Digit(_) => return None,
    })
}

#[cfg(not(windows))]
fn from_rdev_key(key: rdev::Key) -> Option<Vec<HotkeyKey>> {
    use rdev::Key;

    Some(match key {
        Key::ControlLeft | Key::ControlRight => vec![HotkeyKey::Control],
        Key::ShiftLeft | Key::ShiftRight => vec![HotkeyKey::Shift],
        Key::Alt | Key::AltGr => vec![HotkeyKey::Alt],
        Key::MetaLeft | Key::MetaRight => vec![HotkeyKey::Meta],
        Key::CapsLock => vec![HotkeyKey::CapsLock],
        Key::ScrollLock => vec![HotkeyKey::ScrollLock],
        Key::Pause => vec![HotkeyKey::Pause],
        Key::Insert => vec![HotkeyKey::Insert],
        Key::Return | Key::KpReturn => vec![HotkeyKey::Enter],
        Key::Backspace => vec![HotkeyKey::Backspace],
        Key::Delete | Key::KpDelete => vec![HotkeyKey::Delete],
        Key::Home => vec![HotkeyKey::Home],
        Key::End => vec![HotkeyKey::End],
        Key::PageUp => vec![HotkeyKey::PageUp],
        Key::PageDown => vec![HotkeyKey::PageDown],
        Key::LeftArrow => vec![HotkeyKey::Left],
        Key::RightArrow => vec![HotkeyKey::Right],
        Key::UpArrow => vec![HotkeyKey::Up],
        Key::DownArrow => vec![HotkeyKey::Down],
        Key::Space => vec![HotkeyKey::Space],
        Key::Tab => vec![HotkeyKey::Tab],
        Key::Escape => vec![HotkeyKey::Escape],
        Key::Equal | Key::KpPlus => vec![HotkeyKey::Plus],
        Key::Minus | Key::KpMinus => vec![HotkeyKey::Minus],
        Key::F1 => vec![HotkeyKey::F(1)],
        Key::F2 => vec![HotkeyKey::F(2)],
        Key::F3 => vec![HotkeyKey::F(3)],
        Key::F4 => vec![HotkeyKey::F(4)],
        Key::F5 => vec![HotkeyKey::F(5)],
        Key::F6 => vec![HotkeyKey::F(6)],
        Key::F7 => vec![HotkeyKey::F(7)],
        Key::F8 => vec![HotkeyKey::F(8)],
        Key::F9 => vec![HotkeyKey::F(9)],
        Key::F10 => vec![HotkeyKey::F(10)],
        Key::F11 => vec![HotkeyKey::F(11)],
        Key::F12 => vec![HotkeyKey::F(12)],
        Key::KeyA => vec![HotkeyKey::Letter('a')],
        Key::KeyB => vec![HotkeyKey::Letter('b')],
        Key::KeyC => vec![HotkeyKey::Letter('c')],
        Key::KeyD => vec![HotkeyKey::Letter('d')],
        Key::KeyE => vec![HotkeyKey::Letter('e')],
        Key::KeyF => vec![HotkeyKey::Letter('f')],
        Key::KeyG => vec![HotkeyKey::Letter('g')],
        Key::KeyH => vec![HotkeyKey::Letter('h')],
        Key::KeyI => vec![HotkeyKey::Letter('i')],
        Key::KeyJ => vec![HotkeyKey::Letter('j')],
        Key::KeyK => vec![HotkeyKey::Letter('k')],
        Key::KeyL => vec![HotkeyKey::Letter('l')],
        Key::KeyM => vec![HotkeyKey::Letter('m')],
        Key::KeyN => vec![HotkeyKey::Letter('n')],
        Key::KeyO => vec![HotkeyKey::Letter('o')],
        Key::KeyP => vec![HotkeyKey::Letter('p')],
        Key::KeyQ => vec![HotkeyKey::Letter('q')],
        Key::KeyR => vec![HotkeyKey::Letter('r')],
        Key::KeyS => vec![HotkeyKey::Letter('s')],
        Key::KeyT => vec![HotkeyKey::Letter('t')],
        Key::KeyU => vec![HotkeyKey::Letter('u')],
        Key::KeyV => vec![HotkeyKey::Letter('v')],
        Key::KeyW => vec![HotkeyKey::Letter('w')],
        Key::KeyX => vec![HotkeyKey::Letter('x')],
        Key::KeyY => vec![HotkeyKey::Letter('y')],
        Key::KeyZ => vec![HotkeyKey::Letter('z')],
        Key::Num0 | Key::Kp0 => vec![HotkeyKey::Digit('0')],
        Key::Num1 | Key::Kp1 => vec![HotkeyKey::Digit('1')],
        Key::Num2 | Key::Kp2 => vec![HotkeyKey::Digit('2')],
        Key::Num3 | Key::Kp3 => vec![HotkeyKey::Digit('3')],
        Key::Num4 | Key::Kp4 => vec![HotkeyKey::Digit('4')],
        Key::Num5 | Key::Kp5 => vec![HotkeyKey::Digit('5')],
        Key::Num6 | Key::Kp6 => vec![HotkeyKey::Digit('6')],
        Key::Num7 | Key::Kp7 => vec![HotkeyKey::Digit('7')],
        Key::Num8 | Key::Kp8 => vec![HotkeyKey::Digit('8')],
        Key::Num9 | Key::Kp9 => vec![HotkeyKey::Digit('9')],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_capslock() {
        let hk = parse_hotkey("CapsLock").unwrap();
        assert_eq!(hk.keys(), &[HotkeyKey::CapsLock]);
        #[cfg(windows)]
        assert!(hk.suppress_keys.contains(&HotkeyKey::CapsLock));
    }

    #[test]
    fn parse_function_keys() {
        for i in 1..=12 {
            let hk = parse_hotkey(&format!("F{}", i)).unwrap();
            assert_eq!(hk.keys(), &[HotkeyKey::F(i)]);
        }
    }

    #[test]
    fn parse_combo_hotkey() {
        let hk = parse_hotkey("Ctrl+Shift+Space").unwrap();
        assert_eq!(
            hk.keys(),
            &[HotkeyKey::Control, HotkeyKey::Shift, HotkeyKey::Space]
        );
    }

    #[test]
    fn parse_combo_case_and_spacing() {
        let hk = parse_hotkey("  cmd + option + F9 ").unwrap();
        assert_eq!(
            hk.keys(),
            &[HotkeyKey::Meta, HotkeyKey::Alt, HotkeyKey::F(9)]
        );
    }

    #[test]
    fn parse_multi_non_modifier_combo() {
        let hk = parse_hotkey("A+B+F6").unwrap();
        assert_eq!(
            hk.keys(),
            &[HotkeyKey::Letter('a'), HotkeyKey::Letter('b'), HotkeyKey::F(6)]
        );
    }

    #[test]
    fn parse_aliases() {
        assert_eq!(parse_hotkey("esc").unwrap().keys(), &[HotkeyKey::Escape]);
        assert_eq!(parse_hotkey("return").unwrap().keys(), &[HotkeyKey::Enter]);
        assert_eq!(parse_hotkey("pgdn").unwrap().keys(), &[HotkeyKey::PageDown]);
        assert_eq!(parse_hotkey("win").unwrap().keys(), &[HotkeyKey::Meta]);
    }

    #[test]
    fn parse_rejects_empty_and_unknown_tokens() {
        assert!(parse_hotkey("").is_err());
        assert!(parse_hotkey("UnknownKey").is_err());
        assert!(parse_hotkey("Ctrl++A").is_err());
    }

    #[test]
    fn combo_state_emits_pressed_and_released_once() {
        let config = parse_hotkey("Ctrl+Shift+A").unwrap();
        let (tx, rx) = mpsc::channel();
        let mut state = ComboState::default();

        state.update(&config, HotkeyKey::Control, true, &tx);
        state.update(&config, HotkeyKey::Shift, true, &tx);
        state.update(&config, HotkeyKey::Letter('a'), true, &tx);
        state.update(&config, HotkeyKey::Letter('a'), true, &tx);
        state.update(&config, HotkeyKey::Shift, false, &tx);
        state.update(&config, HotkeyKey::Control, false, &tx);

        assert_eq!(rx.recv().unwrap(), HotkeyEvent::Pressed);
        assert_eq!(rx.recv().unwrap(), HotkeyEvent::Released);
        assert!(rx.try_recv().is_err());
    }
}
