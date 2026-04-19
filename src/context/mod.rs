#[cfg(windows)]
use anyhow::Result;
#[cfg(windows)]
use tracing::debug;
#[cfg(not(windows))]
use tracing::warn;

/// The detected category of the active application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCategory {
    /// Code editor or IDE (VS Code, IntelliJ, Vim, etc.)
    CodeEditor,
    /// Terminal / shell emulator
    Terminal,
    /// Web browser
    Browser,
    /// Chat / messaging app
    Chat,
    /// Document editor (Word, Google Docs, etc.)
    DocumentEditor,
    /// Unknown / general purpose
    Other,
}

impl AppCategory {
    /// Return an LLM context hint describing the domain of the active application.
    /// This tells the LLM what kind of vocabulary to expect.
    pub fn context_hint(&self) -> &'static str {
        match self {
            AppCategory::CodeEditor => concat!(
                "The user is in a CODE EDITOR / IDE. ",
                "Most dictated words are likely programming identifiers (variable names, function names, ",
                "class names, keywords, file paths, CLI flags). Prefer camelCase, snake_case, PascalCase, ",
                "or SCREAMING_SNAKE_CASE forms when they match something in the surrounding text or window title. ",
                "Common spoken words may actually be code: e.g. \"string\" → String, \"none\" → None, ",
                "\"get user\" → getUser. Preserve exact technical spelling.",
            ),
            AppCategory::Terminal => concat!(
                "The user is in a TERMINAL / SHELL. ",
                "Dictated words are likely shell commands, file paths, flags, or CLI arguments. ",
                "Preserve exact command syntax, flags (--verbose, -rf), pipes, and path separators. ",
                "Do not capitalize or punctuate as natural language.",
            ),
            AppCategory::Browser => concat!(
                "The user is in a WEB BROWSER. ",
                "Content could be anything — check the window title for clues (e.g. GitHub means code context, ",
                "Gmail means email context). Adapt spelling and casing accordingly.",
            ),
            AppCategory::Chat => concat!(
                "The user is in a CHAT / MESSAGING app. ",
                "The tone is likely conversational and informal. Preserve casual phrasing. ",
                "Do not over-correct grammar in casual messages.",
            ),
            AppCategory::DocumentEditor => concat!(
                "The user is in a DOCUMENT EDITOR. ",
                "Text should follow standard prose conventions with proper grammar, punctuation, and capitalization.",
            ),
            AppCategory::Other => "",
        }
    }
}

/// Detect the category of the active application from its process name and window title.
pub fn detect_app_category(app_name: &str, window_title: &str) -> AppCategory {
    let app_lower = app_name.to_lowercase();
    let title_lower = window_title.to_lowercase();

    // Code editors / IDEs
    let code_editors = [
        "code.exe", "code", "code - insiders",           // VS Code
        "devenv.exe", "devenv",                           // Visual Studio
        "idea64.exe", "idea.exe", "idea",                 // IntelliJ
        "pycharm64.exe", "pycharm",                       // PyCharm
        "webstorm64.exe", "webstorm",                     // WebStorm
        "clion64.exe", "clion",                           // CLion
        "goland64.exe", "goland",                         // GoLand
        "rustrover64.exe", "rustrover",                   // RustRover
        "rider64.exe", "rider",                           // Rider
        "sublime_text.exe", "sublime_text",               // Sublime Text
        "atom.exe", "atom",                               // Atom
        "notepad++.exe", "notepad++",                     // Notepad++
        "cursor.exe", "cursor",                           // Cursor
        "zed.exe", "zed",                                 // Zed
        "nvim", "vim", "gvim.exe", "gvim",               // (Neo)Vim
        "emacs", "emacs.exe",                             // Emacs
        "kate", "gedit", "geany",                         // Linux editors
        "android studio",                                 // Android Studio
        "xcode",                                          // Xcode
    ];

    // Terminals
    let terminals = [
        "windowsterminal.exe", "wt.exe",                  // Windows Terminal
        "cmd.exe", "powershell.exe", "pwsh.exe",          // Windows shells
        "conhost.exe",                                    // Console Host
        "alacritty.exe", "alacritty",                     // Alacritty
        "wezterm-gui.exe", "wezterm-gui", "wezterm",      // WezTerm
        "kitty", "gnome-terminal", "konsole",             // Linux terminals
        "iterm2", "terminal",                             // macOS terminals
        "hyper.exe", "hyper",                             // Hyper
        "mintty.exe", "mintty",                           // MinTTY / Git Bash
    ];

    // Browsers
    let browsers = [
        "chrome.exe", "chrome",
        "firefox.exe", "firefox",
        "msedge.exe", "msedge",
        "brave.exe", "brave",
        "opera.exe", "opera",
        "safari",
        "vivaldi.exe", "vivaldi",
        "arc.exe", "arc",
    ];

    // Chat / messaging
    let chat_apps = [
        "slack.exe", "slack",
        "discord.exe", "discord",
        "teams.exe", "ms-teams.exe", "teams",
        "telegram.exe", "telegram",
        "signal.exe", "signal",
        "whatsapp.exe", "whatsapp",
        "element.exe", "element",
    ];

    // Document editors
    let doc_editors = [
        "winword.exe", "winword",                         // MS Word
        "excel.exe", "excel",                             // MS Excel
        "powerpnt.exe", "powerpnt",                       // PowerPoint
        "onenote.exe", "onenote",                         // OneNote
        "soffice.bin", "soffice",                         // LibreOffice
        "notion.exe", "notion",                           // Notion
        "obsidian.exe", "obsidian",                       // Obsidian
        "typora.exe", "typora",                           // Typora
        "wordpad.exe",                                    // WordPad
    ];

    if code_editors.iter().any(|e| app_lower == *e) {
        return AppCategory::CodeEditor;
    }
    if terminals.iter().any(|e| app_lower == *e) {
        return AppCategory::Terminal;
    }
    if browsers.iter().any(|e| app_lower == *e) {
        // Check if the browser is showing a code-related site
        let code_sites = ["github", "gitlab", "bitbucket", "codepen", "codesandbox",
                          "stackblitz", "repl.it", "jsfiddle", "leetcode", "hackerrank",
                          "codeforces", "stackoverflow", "stack overflow"];
        if code_sites.iter().any(|s| title_lower.contains(s)) {
            return AppCategory::CodeEditor;
        }
        return AppCategory::Browser;
    }
    if chat_apps.iter().any(|e| app_lower == *e) {
        return AppCategory::Chat;
    }
    if doc_editors.iter().any(|e| app_lower == *e) {
        return AppCategory::DocumentEditor;
    }

    // Fallback: check window title for clues
    let code_title_hints = [" - visual studio", " - vs code", " - intellij", " - pycharm",
                            " - webstorm", " - clion", " - goland", " - rustrover",
                            " - sublime text", " - cursor", " - zed", ".rs ", ".py ", ".js ",
                            ".ts ", ".go ", ".java ", ".cpp ", ".c ", ".cs ",
                            ".rb ", ".php ", ".swift ", ".kt "];
    if code_title_hints.iter().any(|h| title_lower.contains(h)) {
        return AppCategory::CodeEditor;
    }

    AppCategory::Other
}

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

impl CapturedContext {
    /// Detect the category of the active application.
    pub fn app_category(&self) -> AppCategory {
        detect_app_category(&self.app_name, &self.window_title)
    }

    /// Get the LLM context hint for the active application.
    pub fn app_context_hint(&self) -> &str {
        self.app_category().context_hint()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_context_default_is_empty() {
        let ctx = CapturedContext::default();
        assert!(ctx.app_name.is_empty());
        assert!(ctx.window_title.is_empty());
        assert!(ctx.surrounding_text.is_empty());
        assert!(ctx.screenshot_base64.is_none());
    }

    #[test]
    fn captured_context_clone() {
        let ctx = CapturedContext {
            app_name: "chrome.exe".to_string(),
            window_title: "Google".to_string(),
            surrounding_text: "hello".to_string(),
            screenshot_base64: Some("abc123".to_string()),
        };
        let cloned = ctx.clone();
        assert_eq!(cloned.app_name, "chrome.exe");
        assert_eq!(cloned.window_title, "Google");
        assert_eq!(cloned.surrounding_text, "hello");
        assert_eq!(cloned.screenshot_base64, Some("abc123".to_string()));
    }

    #[test]
    fn captured_context_debug_format() {
        let ctx = CapturedContext::default();
        let debug = format!("{:?}", ctx);
        assert!(debug.contains("CapturedContext"));
    }

    #[cfg(not(windows))]
    #[test]
    fn capture_context_stub_returns_empty() {
        let ctx = capture_context(false);
        assert!(ctx.app_name.is_empty());
        assert!(ctx.window_title.is_empty());
        assert!(ctx.surrounding_text.is_empty());
        assert!(ctx.screenshot_base64.is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn capture_context_stub_ignores_screenshot_flag() {
        let ctx = capture_context(true);
        assert!(ctx.screenshot_base64.is_none(), "Stub should not capture screenshots");
    }

    // ---- App category detection tests ----

    #[test]
    fn detect_vscode_as_code_editor() {
        assert_eq!(detect_app_category("Code.exe", "main.rs - VS Code"), AppCategory::CodeEditor);
        assert_eq!(detect_app_category("code", ""), AppCategory::CodeEditor);
    }

    #[test]
    fn detect_intellij_as_code_editor() {
        assert_eq!(detect_app_category("idea64.exe", "MyProject"), AppCategory::CodeEditor);
    }

    #[test]
    fn detect_terminal_apps() {
        assert_eq!(detect_app_category("WindowsTerminal.exe", "PowerShell"), AppCategory::Terminal);
        assert_eq!(detect_app_category("alacritty", "~"), AppCategory::Terminal);
        assert_eq!(detect_app_category("cmd.exe", ""), AppCategory::Terminal);
    }

    #[test]
    fn detect_browser_apps() {
        assert_eq!(detect_app_category("chrome.exe", "Google Search"), AppCategory::Browser);
        assert_eq!(detect_app_category("firefox.exe", "Reddit"), AppCategory::Browser);
    }

    #[test]
    fn detect_browser_on_code_site_as_code_editor() {
        assert_eq!(detect_app_category("chrome.exe", "sgstq/repo - GitHub"), AppCategory::CodeEditor);
        assert_eq!(detect_app_category("firefox.exe", "How to parse JSON - Stack Overflow"), AppCategory::CodeEditor);
        assert_eq!(detect_app_category("msedge.exe", "CodeSandbox - Online IDE"), AppCategory::CodeEditor);
    }

    #[test]
    fn detect_chat_apps() {
        assert_eq!(detect_app_category("slack.exe", "#general"), AppCategory::Chat);
        assert_eq!(detect_app_category("discord.exe", "Server"), AppCategory::Chat);
        assert_eq!(detect_app_category("teams.exe", "Meeting"), AppCategory::Chat);
    }

    #[test]
    fn detect_document_editors() {
        assert_eq!(detect_app_category("WINWORD.EXE", "Document1"), AppCategory::DocumentEditor);
        assert_eq!(detect_app_category("notion.exe", "Notes"), AppCategory::DocumentEditor);
        assert_eq!(detect_app_category("obsidian.exe", "My Vault"), AppCategory::DocumentEditor);
    }

    #[test]
    fn detect_code_editor_from_window_title() {
        // Unknown app but title contains code file extension
        assert_eq!(detect_app_category("unknown.exe", "main.rs - Something"), AppCategory::CodeEditor);
        assert_eq!(detect_app_category("unknown.exe", "app.py - Editor"), AppCategory::CodeEditor);
        assert_eq!(detect_app_category("unknown.exe", "index.js - Editor"), AppCategory::CodeEditor);
    }

    #[test]
    fn detect_unknown_app_as_other() {
        assert_eq!(detect_app_category("calculator.exe", "Calculator"), AppCategory::Other);
        assert_eq!(detect_app_category("mspaint.exe", "Untitled - Paint"), AppCategory::Other);
    }

    #[test]
    fn app_category_case_insensitive() {
        assert_eq!(detect_app_category("CODE.EXE", ""), AppCategory::CodeEditor);
        assert_eq!(detect_app_category("Chrome.exe", ""), AppCategory::Browser);
        assert_eq!(detect_app_category("SLACK.EXE", ""), AppCategory::Chat);
    }

    #[test]
    fn code_editor_hint_mentions_programming() {
        let hint = AppCategory::CodeEditor.context_hint();
        assert!(hint.contains("CODE EDITOR"));
        assert!(hint.contains("camelCase"));
    }

    #[test]
    fn terminal_hint_mentions_commands() {
        let hint = AppCategory::Terminal.context_hint();
        assert!(hint.contains("TERMINAL"));
        assert!(hint.contains("shell commands"));
    }

    #[test]
    fn other_category_hint_is_empty() {
        assert!(AppCategory::Other.context_hint().is_empty());
    }

    #[test]
    fn captured_context_app_category_method() {
        let ctx = CapturedContext {
            app_name: "Code.exe".to_string(),
            window_title: "main.rs - VS Code".to_string(),
            ..Default::default()
        };
        assert_eq!(ctx.app_category(), AppCategory::CodeEditor);
        assert!(!ctx.app_context_hint().is_empty());
    }

    #[test]
    fn captured_context_other_category_empty_hint() {
        let ctx = CapturedContext {
            app_name: "calc.exe".to_string(),
            window_title: "Calculator".to_string(),
            ..Default::default()
        };
        assert_eq!(ctx.app_category(), AppCategory::Other);
        assert!(ctx.app_context_hint().is_empty());
    }
}

#[cfg(windows)]
fn capture_active_window_screenshot() -> Result<String> {
    use base64::Engine;
    use screenshots::image::ImageOutputFormat;
    use std::io::Cursor;

    let screens = screenshots::Screen::all()?;
    if let Some(screen) = screens.first() {
        let image = screen.capture()?;
        let mut png_bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut png_bytes, ImageOutputFormat::Png)
            .map_err(|e| anyhow::anyhow!("Failed to encode screenshot as PNG: {}", e))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes.into_inner());
        Ok(b64)
    } else {
        anyhow::bail!("No screens found")
    }
}
