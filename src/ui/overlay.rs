/// Floating recording indicator bubble.
///
/// On Windows this creates a small, always-on-top, click-through pill-shaped
/// window that shows the current recording state with a colored indicator dot
/// and status text.  The bubble pulses gently while recording.
///
/// On other platforms the overlay is a no-op stub.

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum OverlayState {
    Hidden = 0,
    Recording = 1,
    Transcribing = 2,
    Refining = 3,
}

impl OverlayState {
    fn label(self) -> &'static str {
        match self {
            Self::Recording => "Recording...",
            Self::Transcribing => "Transcribing...",
            Self::Refining => "Refining...",
            Self::Hidden => "",
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Recording,
            2 => Self::Transcribing,
            3 => Self::Refining,
            _ => Self::Hidden,
        }
    }
}

// ── Windows implementation ──────────────────────────────────────────────────

#[cfg(windows)]
mod platform {
    use super::OverlayState;
    use std::sync::atomic::{AtomicU8, Ordering};
    use windows::core::*;
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// Render state shared between the main thread and the WNDPROC callback.
    static RENDER_STATE: AtomicU8 = AtomicU8::new(0);

    // Layout
    const BUBBLE_W: i32 = 180;
    const BUBBLE_H: i32 = 40;
    const DOT_RADIUS: i32 = 7;
    const DOT_CX: i32 = 20;
    const DOT_CY: i32 = BUBBLE_H / 2;
    const TEXT_X: i32 = 36;
    const TEXT_Y: i32 = 12;
    const SCREEN_TOP_MARGIN: i32 = 24;

    // Colors  (COLORREF = 0x00BBGGRR)
    const COLOR_BG: COLORREF = COLORREF(0x00302020);    // #202030 dark
    const COLOR_RED: COLORREF = COLORREF(0x003535E5);    // #E53535
    const COLOR_AMBER: COLORREF = COLORREF(0x0000B8FB);  // #FBB800
    const COLOR_BLUE: COLORREF = COLORREF(0x00E5881E);   // #1E88E5
    const COLOR_WHITE: COLORREF = COLORREF(0x00FFFFFF);

    // ── Window procedure ────────────────────────────────────────────────────

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);

                let state = OverlayState::from_u8(RENDER_STATE.load(Ordering::Relaxed));

                // Background
                let bg = CreateSolidBrush(COLOR_BG);
                let rc = RECT {
                    left: 0,
                    top: 0,
                    right: BUBBLE_W,
                    bottom: BUBBLE_H,
                };
                FillRect(hdc, &rc, bg);
                let _ = DeleteObject(bg);

                // Colored indicator dot
                let dot_color = match state {
                    OverlayState::Recording => COLOR_RED,
                    OverlayState::Transcribing => COLOR_AMBER,
                    OverlayState::Refining => COLOR_BLUE,
                    OverlayState::Hidden => COLORREF(0x00808080),
                };
                let brush = CreateSolidBrush(dot_color);
                let old_br = SelectObject(hdc, brush);
                let null_pen = GetStockObject(NULL_PEN);
                let old_pen = SelectObject(hdc, null_pen);
                let _ = Ellipse(
                    hdc,
                    DOT_CX - DOT_RADIUS,
                    DOT_CY - DOT_RADIUS,
                    DOT_CX + DOT_RADIUS,
                    DOT_CY + DOT_RADIUS,
                );
                SelectObject(hdc, old_pen);
                SelectObject(hdc, old_br);
                let _ = DeleteObject(brush);

                // Status text
                let label = state.label();
                if !label.is_empty() {
                    let gui_font = GetStockObject(DEFAULT_GUI_FONT);
                    let old_font = SelectObject(hdc, gui_font);
                    SetBkMode(hdc, TRANSPARENT);
                    SetTextColor(hdc, COLOR_WHITE);
                    let wide: Vec<u16> = label.encode_utf16().collect();
                    let _ = TextOutW(hdc, TEXT_X, TEXT_Y, &wide);
                    SelectObject(hdc, old_font);
                }

                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_NCHITTEST => LRESULT(-1), // HTTRANSPARENT – click-through
            WM_ERASEBKGND => LRESULT(1), // handled
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────────────

    pub(super) fn create_window() -> Option<HWND> {
        unsafe {
            let class_name = w!("DuperDisperOverlay");

            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                lpszClassName: class_name,
                ..Default::default()
            };

            if RegisterClassW(&wc) == 0 {
                tracing::error!("Failed to register overlay window class");
                return None;
            }

            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let x = (screen_w - BUBBLE_W) / 2;

            let ex_style = WS_EX_LAYERED
                | WS_EX_TOPMOST
                | WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE
                | WS_EX_TRANSPARENT;

            let hwnd = CreateWindowExW(
                ex_style,
                class_name,
                w!(""),
                WS_POPUP,
                x,
                SCREEN_TOP_MARGIN,
                BUBBLE_W,
                BUBBLE_H,
                None,
                None,
                None,
                None,
            )
            .ok()?;

            // Pill-shaped clipping region
            let rgn = CreateRoundRectRgn(0, 0, BUBBLE_W + 1, BUBBLE_H + 1, BUBBLE_H, BUBBLE_H);
            let _ = SetWindowRgn(hwnd, rgn, true);

            // Semi-transparent
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 230, LWA_ALPHA);

            Some(hwnd)
        }
    }

    pub(super) fn set_state(state: OverlayState) {
        RENDER_STATE.store(state as u8, Ordering::Relaxed);
    }

    pub(super) fn show(hwnd: HWND) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }

    pub(super) fn hide(hwnd: HWND) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }

    pub(super) fn set_alpha(hwnd: HWND, alpha: u8) {
        unsafe {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        }
    }

    pub(super) fn invalidate(hwnd: HWND) {
        unsafe {
            let _ = InvalidateRect(hwnd, None, false);
        }
    }

    pub(super) fn destroy(hwnd: HWND) {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

// ── Public overlay API ──────────────────────────────────────────────────────

pub struct RecordingOverlay {
    state: OverlayState,
    #[cfg(windows)]
    hwnd: Option<windows::Win32::Foundation::HWND>,
    #[cfg(windows)]
    pulse_start: std::time::Instant,
}

impl RecordingOverlay {
    pub fn new(enabled: bool) -> Self {
        #[cfg(windows)]
        {
            let hwnd = if enabled {
                platform::create_window()
            } else {
                None
            };
            if enabled && hwnd.is_none() {
                tracing::warn!("Overlay window creation failed; overlay disabled");
            }
            Self {
                state: OverlayState::Hidden,
                hwnd,
                pulse_start: std::time::Instant::now(),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = enabled;
            Self {
                state: OverlayState::Hidden,
            }
        }
    }

    pub fn show_recording(&mut self) {
        #[cfg(windows)]
        {
            self.pulse_start = std::time::Instant::now();
        }
        self.transition(OverlayState::Recording);
    }

    pub fn show_transcribing(&mut self) {
        self.transition(OverlayState::Transcribing);
    }

    pub fn show_refining(&mut self) {
        self.transition(OverlayState::Refining);
    }

    pub fn hide(&mut self) {
        self.transition(OverlayState::Hidden);
    }

    /// Drive the pulsing animation.  Call once per main-loop tick.
    pub fn tick(&self) {
        #[cfg(windows)]
        if self.state == OverlayState::Recording {
            if let Some(hwnd) = self.hwnd {
                let t = self.pulse_start.elapsed().as_secs_f32();
                // Smooth sine-wave pulse: alpha oscillates 170 ↔ 255
                let alpha = 170.0 + 85.0 * (t * 3.5).sin();
                platform::set_alpha(hwnd, alpha as u8);
            }
        }
    }

    pub fn is_visible(&self) -> bool {
        self.state != OverlayState::Hidden
    }

    pub fn get_status(&self) -> String {
        self.state.label().to_string()
    }

    fn transition(&mut self, new_state: OverlayState) {
        self.state = new_state;
        #[cfg(windows)]
        if let Some(hwnd) = self.hwnd {
            platform::set_state(new_state);
            platform::invalidate(hwnd);
            if new_state == OverlayState::Hidden {
                platform::hide(hwnd);
            } else {
                platform::set_alpha(hwnd, 230);
                platform::show(hwnd);
            }
        }
    }
}

#[cfg(windows)]
impl Drop for RecordingOverlay {
    fn drop(&mut self) {
        if let Some(hwnd) = self.hwnd {
            platform::destroy(hwnd);
        }
    }
}
