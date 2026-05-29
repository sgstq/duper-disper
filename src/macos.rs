//! macOS platform integration.
//!
//! The app runs a manual main-thread loop (it is not a winit/eframe app in the
//! main process), so on macOS we must:
//!   * become an "accessory" (menu-bar) app so the `tray-icon` status item works
//!     and no Dock icon is shown;
//!   * pump the AppKit event queue every tick so status-item menu clicks are
//!     delivered;
//!   * request the Accessibility permission required by the global hotkey
//!     listener (`rdev`) and the text-insertion backend (`enigo`);
//!   * capture the frontmost app / window / screenshot for refinement context.
//!
//! The AppKit lifecycle uses `objc2`; the CoreFoundation / CoreGraphics /
//! Accessibility bits use direct C FFI (these are extremely stable C ABIs and
//! avoid the friction of the typed CF wrappers).

use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSEventMask, NSWorkspace,
};
use objc2_foundation::{MainThreadMarker, NSDate, NSDefaultRunLoopMode};
use std::ffi::{c_char, c_long, c_void};
use tracing::{debug, warn};

use crate::context::CapturedContext;

// ── AppKit lifecycle ────────────────────────────────────────────────────────

/// Initialise NSApplication as an accessory (menu-bar) app with no Dock icon.
/// Must be called once, on the main thread, before the main loop starts.
pub fn init_menubar_app() {
    let Some(mtm) = MainThreadMarker::new() else {
        warn!("init_menubar_app() called off the main thread; skipping");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
    debug!("NSApplication initialised as accessory app");
}

/// Drain all pending AppKit events. Call once per main-loop tick so the tray
/// icon's menu works. Non-blocking: returns as soon as the queue is empty.
pub fn pump_events() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: on the main thread; `distantPast` => don't block; the run-loop
    // mode static and `nextEvent…` are valid AppKit calls.
    unsafe {
        let past = NSDate::distantPast();
        loop {
            match app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&past),
                NSDefaultRunLoopMode,
                true,
            ) {
                Some(event) => app.sendEvent(&event),
                None => break,
            }
        }
    }
}

// ── Accessibility permission ────────────────────────────────────────────────

/// Returns true if the process is already trusted for Accessibility. If it is
/// not trusted, macOS shows the system prompt directing the user to
/// System Settings > Privacy & Security > Accessibility (shown once per app).
pub fn ensure_accessibility_permission() -> bool {
    #[repr(C)]
    struct CFCallbacks {
        _private: [u8; 0],
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFTypeDictionaryKeyCallBacks: CFCallbacks;
        static kCFTypeDictionaryValueCallBacks: CFCallbacks;
        static kCFBooleanTrue: *const c_void;
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            num_values: c_long,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: *const c_void; // CFStringRef
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    }

    // SAFETY: building a 1-entry CFDictionary { prompt-key: true } and passing
    // it to the Accessibility API. All pointers are valid CF constants.
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            std::ptr::addr_of!(kCFTypeDictionaryKeyCallBacks) as *const c_void,
            std::ptr::addr_of!(kCFTypeDictionaryValueCallBacks) as *const c_void,
        );
        let trusted = AXIsProcessTrustedWithOptions(options);
        if !options.is_null() {
            CFRelease(options);
        }
        trusted
    }
}

// ── Active-window context capture ───────────────────────────────────────────

/// Capture the frontmost application name, window title, and (optionally) a
/// screenshot, for use as LLM refinement context.
pub fn capture_context(include_screenshot: bool) -> CapturedContext {
    let mut ctx = CapturedContext::default();

    if let Some(name) = frontmost_app_name() {
        ctx.app_name = name;
    }

    if let Some((owner, title)) = frontmost_window(&ctx.app_name) {
        if ctx.app_name.is_empty() {
            ctx.app_name = owner;
        }
        if let Some(title) = title {
            ctx.window_title = title;
        }
    }

    if include_screenshot {
        ctx.screenshot_base64 = capture_screenshot();
    }

    debug!(
        "macOS context: app={:?}, title={:?}, screenshot={}",
        ctx.app_name,
        ctx.window_title,
        ctx.screenshot_base64.is_some()
    );
    ctx
}

fn frontmost_app_name() -> Option<String> {
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    app.localizedName().map(|s| s.to_string())
}

// CoreFoundation / CoreGraphics C ABI used for window enumeration.
mod cf {
    use std::ffi::{c_char, c_long, c_void};

    pub type CFTypeRef = *const c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub fn CFArrayGetCount(arr: CFTypeRef) -> c_long;
        pub fn CFArrayGetValueAtIndex(arr: CFTypeRef, idx: c_long) -> CFTypeRef;
        pub fn CFDictionaryGetValue(dict: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
        pub fn CFStringGetCStringPtr(s: CFTypeRef, encoding: u32) -> *const c_char;
        pub fn CFStringGetCString(
            s: CFTypeRef,
            buffer: *mut c_char,
            size: c_long,
            encoding: u32,
        ) -> bool;
        pub fn CFStringGetLength(s: CFTypeRef) -> c_long;
        pub fn CFNumberGetValue(num: CFTypeRef, the_type: c_long, value: *mut c_void) -> bool;
        pub fn CFRelease(cf: CFTypeRef);
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        pub fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFTypeRef;
        pub static kCGWindowOwnerName: CFTypeRef; // CFStringRef
        pub static kCGWindowName: CFTypeRef; // CFStringRef
        pub static kCGWindowLayer: CFTypeRef; // CFStringRef
    }

    pub const UTF8: u32 = 0x0800_0100; // kCFStringEncodingUTF8
    pub const SINT32: c_long = 3; // kCFNumberSInt32Type
    pub const ON_SCREEN_ONLY: u32 = 1 << 0;
    pub const EXCLUDE_DESKTOP: u32 = 1 << 4;
    pub const NULL_WINDOW_ID: u32 = 0;
}

/// Convert a CFStringRef into a Rust String.
unsafe fn cfstring_to_string(s: cf::CFTypeRef) -> Option<String> {
    if s.is_null() {
        return None;
    }
    // Fast path: a directly-usable UTF-8 pointer (not always available).
    let ptr = cf::CFStringGetCStringPtr(s, cf::UTF8);
    if !ptr.is_null() {
        return Some(std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned());
    }
    let len = cf::CFStringGetLength(s);
    if len <= 0 {
        return Some(String::new());
    }
    // Worst case for UTF-8 is 4 bytes per UTF-16 code unit, plus NUL.
    let capacity = (len as usize) * 4 + 1;
    let mut buf = vec![0 as c_char; capacity];
    if cf::CFStringGetCString(s, buf.as_mut_ptr(), capacity as c_long, cf::UTF8) {
        Some(
            std::ffi::CStr::from_ptr(buf.as_ptr())
                .to_string_lossy()
                .into_owned(),
        )
    } else {
        None
    }
}

/// Find the frontmost ordinary (layer 0) window. Prefers a window owned by
/// `preferred_owner` if given; otherwise returns the first on-screen normal
/// window. Returns (owner_name, optional_title).
fn frontmost_window(preferred_owner: &str) -> Option<(String, Option<String>)> {
    // SAFETY: standard CoreGraphics window-list enumeration. The returned array
    // is owned by us (Copy) and released at the end.
    unsafe {
        let list = cf::CGWindowListCopyWindowInfo(
            cf::ON_SCREEN_ONLY | cf::EXCLUDE_DESKTOP,
            cf::NULL_WINDOW_ID,
        );
        if list.is_null() {
            return None;
        }

        let count = cf::CFArrayGetCount(list);
        let mut first_normal: Option<(String, Option<String>)> = None;
        let mut matched: Option<(String, Option<String>)> = None;

        for i in 0..count {
            let dict = cf::CFArrayGetValueAtIndex(list, i);
            if dict.is_null() {
                continue;
            }

            // Only consider layer 0 (ordinary application windows).
            let layer_val = cf::CFDictionaryGetValue(dict, cf::kCGWindowLayer);
            let mut layer: i32 = -1;
            if !layer_val.is_null() {
                cf::CFNumberGetValue(
                    layer_val,
                    cf::SINT32,
                    &mut layer as *mut i32 as *mut c_void,
                );
            }
            if layer != 0 {
                continue;
            }

            let owner = cfstring_to_string(cf::CFDictionaryGetValue(dict, cf::kCGWindowOwnerName))
                .unwrap_or_default();
            // kCGWindowName requires Screen Recording permission; may be absent.
            let title = cfstring_to_string(cf::CFDictionaryGetValue(dict, cf::kCGWindowName))
                .filter(|t| !t.is_empty());

            if first_normal.is_none() {
                first_normal = Some((owner.clone(), title.clone()));
            }
            if !preferred_owner.is_empty() && owner == preferred_owner {
                matched = Some((owner, title));
                break;
            }
        }

        cf::CFRelease(list);
        matched.or(first_normal)
    }
}

/// Capture the screen to a PNG and return it base64-encoded.
/// Uses the `screencapture` CLI (requires Screen Recording permission; if not
/// granted the capture simply returns desktop wallpaper or nothing).
fn capture_screenshot() -> Option<String> {
    use base64::Engine;

    let tmp = std::env::temp_dir().join(format!("duper-disper-shot-{}.png", std::process::id()));
    let status = std::process::Command::new("screencapture")
        .arg("-x") // no shutter sound
        .arg("-t")
        .arg("png")
        .arg(&tmp)
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let bytes = std::fs::read(&tmp).ok()?;
    let _ = std::fs::remove_file(&tmp);
    if bytes.is_empty() {
        return None;
    }
    Some(base64::engine::general_purpose::STANDARD.encode(bytes))
}
