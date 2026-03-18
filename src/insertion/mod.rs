use anyhow::Result;
use tracing::{debug, info};

/// Strategy for inserting text into the active application.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InsertionMethod {
    /// Paste via clipboard (Ctrl+V). Works everywhere but modifies clipboard.
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

    // Simulate Ctrl+V
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
    #[cfg(windows)]
    {
        use enigo::{Enigo, Keyboard, Settings};
        let mut enigo = Enigo::new(&Settings::default())?;
        enigo.text(text)?;
        debug!("Simulated typing complete");
    }

    #[cfg(not(windows))]
    {
        // Fallback: just use clipboard method
        insert_via_clipboard(text)?;
    }

    Ok(())
}

fn simulate_paste() -> Result<()> {
    #[cfg(windows)]
    {
        use enigo::{Direction, Enigo, Key, Keyboard, Settings};
        let mut enigo = Enigo::new(&Settings::default())?;
        enigo.key(Key::Control, Direction::Press)?;
        enigo.key(Key::Unicode('v'), Direction::Click)?;
        enigo.key(Key::Control, Direction::Release)?;
    }

    #[cfg(not(windows))]
    {
        // On non-Windows, we'd use different key combos or xdotool
        return Err(anyhow::anyhow!("Paste simulation not implemented for this platform"));
    }

    #[cfg(windows)]
    Ok(())
}
