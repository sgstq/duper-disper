use anyhow::Result;
use tracing::{debug, info};

/// Strategy for inserting text into the active application.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InsertionMethod {
    /// Paste via clipboard. Works across supported platforms but modifies clipboard.
    Clipboard,
    /// Type characters one by one. Slower but doesn't touch clipboard.
    SimulateTyping,
}

/// Insert text into the currently focused text field.
pub fn insert_text(text: &str, method: InsertionMethod) -> Result<()> {
    info!("Inserting {} chars via {:?}", text.len(), method);

    match method {
        InsertionMethod::Clipboard => insert_via_clipboard(text),
        InsertionMethod::SimulateTyping => insert_via_typing(text),
    }
}

fn insert_via_clipboard(text: &str) -> Result<()> {
    use arboard::Clipboard;

    // Save current clipboard content
    let mut clipboard = Clipboard::new()?;
    let old_content = clipboard.get_text().ok();

    // Set our text
    clipboard.set_text(text)?;

    // Simulate the platform paste shortcut.
    simulate_paste()?;

    // Wait a bit for the paste to complete
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Restore old clipboard content
    if let Some(old) = old_content {
        let _ = clipboard.set_text(old);
    }

    debug!("Clipboard paste complete");
    Ok(())
}

fn insert_via_typing(text: &str) -> Result<()> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        use enigo::{Enigo, Keyboard, Settings};
        let mut enigo = Enigo::new(&Settings::default())?;
        enigo.text(text)?;
        debug!("Simulated typing complete");
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // Fallback: just use clipboard method
        insert_via_clipboard(text)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insertion_method_enum_equality() {
        assert_eq!(InsertionMethod::Clipboard, InsertionMethod::Clipboard);
        assert_eq!(InsertionMethod::SimulateTyping, InsertionMethod::SimulateTyping);
        assert_ne!(InsertionMethod::Clipboard, InsertionMethod::SimulateTyping);
    }

    #[test]
    fn insertion_method_debug_format() {
        let debug = format!("{:?}", InsertionMethod::Clipboard);
        assert!(debug.contains("Clipboard"));
        let debug = format!("{:?}", InsertionMethod::SimulateTyping);
        assert!(debug.contains("SimulateTyping"));
    }

    #[test]
    fn insertion_method_clone() {
        let method = InsertionMethod::Clipboard;
        let cloned = method;
        assert_eq!(method, cloned);
    }
}

fn simulate_paste() -> Result<()> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        use enigo::{Direction, Enigo, Key, Keyboard, Settings};
        let mut enigo = Enigo::new(&Settings::default())?;
        #[cfg(windows)]
        let modifier = Key::Control;
        #[cfg(target_os = "macos")]
        let modifier = Key::Meta;

        enigo.key(modifier, Direction::Press)?;
        enigo.key(Key::Unicode('v'), Direction::Click)?;
        enigo.key(modifier, Direction::Release)?;
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // Linux support still needs a platform-specific paste implementation.
        return Err(anyhow::anyhow!("Paste simulation not implemented for this platform"));
    }

    #[cfg(any(windows, target_os = "macos"))]
    Ok(())
}
