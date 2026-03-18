use anyhow::Result;
#[cfg(windows)]
use tracing::debug;
#[cfg(not(windows))]
use tracing::warn;

/// Captured context about the active application and insertion point.
#[derive(Debug, Clone, Default)]
pub struct CapturedContext {
    /// Name of the active application (e.g., "chrome.exe", "Code.exe").
    pub app_name: String,
    /// Title of the active window.
    pub window_title: String,
    /// Text surrounding the cursor/insertion point.
    pub surrounding_text: String,
    /// Base64-encoded screenshot of the active window (optional).
    pub screenshot_base64: Option<String>,
}

/// Capture context from the currently active application.
/// This is platform-specific; the implementation below is for Windows.
pub fn capture_context(include_screenshot: bool) -> CapturedContext {
    #[cfg(windows)]
    {
        capture_context_windows(include_screenshot)
    }
    #[cfg(not(windows))]
    {
        capture_context_stub(include_screenshot)
    }
}

#[cfg(not(windows))]
fn capture_context_stub(_include_screenshot: bool) -> CapturedContext {
    warn!("Context capture not implemented for this platform");
    CapturedContext::default()
}

#[cfg(windows)]
fn capture_context_windows(include_screenshot: bool) -> CapturedContext {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    let mut ctx = CapturedContext::default();

    unsafe {
        // Get foreground window
        let hwnd = GetForegroundWindow();
        if hwnd.0 == std::ptr::null_mut() {
            return ctx;
        }

        // Get window title
        let mut title_buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title_buf);
        if len > 0 {
            ctx.window_title = String::from_utf16_lossy(&title_buf[..len as usize]);
        }

        // Get process name
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 {
            if let Ok(process) = OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
                false,
                pid,
            ) {
                // Get process name via QueryFullProcessImageNameW or similar
                ctx.app_name = get_process_name(process).unwrap_or_default();
                let _ = windows::Win32::Foundation::CloseHandle(process);
            }
        }

        // Try to get surrounding text via UI Automation
        ctx.surrounding_text = get_surrounding_text_uia().unwrap_or_default();

        // Capture screenshot if requested
        if include_screenshot {
            ctx.screenshot_base64 = capture_active_window_screenshot().ok();
        }
    }

    debug!("Captured context: app={}, title={}", ctx.app_name, ctx.window_title);
    ctx
}

#[cfg(windows)]
unsafe fn get_process_name(process: windows::Win32::Foundation::HANDLE) -> Option<String> {
    use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;

    let mut buf = [0u16; 260];
    let len = GetModuleBaseNameW(process, None, &mut buf);
    if len > 0 {
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    } else {
        None
    }
}

#[cfg(windows)]
fn get_surrounding_text_uia() -> Option<String> {
    // Use UI Automation to get text around the caret in the focused element.
    // This is a simplified approach — full UIA integration would use
    // IUIAutomation::GetFocusedElement + IUIAutomationTextPattern.
    //
    // For now, we try clipboard-based extraction as a fallback:
    // 1. Send Ctrl+A to select all (or Ctrl+Shift+Home/End for surrounding)
    // 2. Copy to clipboard
    // 3. Restore original clipboard
    //
    // This is invasive, so we keep it minimal. A proper implementation
    // would use the UIA COM interfaces directly.

    // For v0.1, return empty - we'll implement proper UIA in the next iteration
    None
}

#[cfg(windows)]
fn capture_active_window_screenshot() -> Result<String> {
    use base64::Engine;
    use std::io::Cursor;

    let screens = screenshots::Screen::all()?;
    if let Some(screen) = screens.first() {
        let image = screen.capture()?;
        // Encode ImageBuffer to PNG bytes
        let mut png_bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut png_bytes, image::ImageFormat::Png)
            .map_err(|e| anyhow::anyhow!("Failed to encode screenshot as PNG: {}", e))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes.into_inner());
        Ok(b64)
    } else {
        anyhow::bail!("No screens found")
    }
}
