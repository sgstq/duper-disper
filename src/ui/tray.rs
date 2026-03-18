use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};
use tracing::info;

pub enum TrayCommand {
    Settings,
    ToggleRefinement,
    Quit,
}

pub struct SystemTray {
    _tray: TrayIcon,
    settings_id: MenuItem,
    toggle_refinement_id: MenuItem,
    quit_id: MenuItem,
}

impl SystemTray {
    pub fn new() -> anyhow::Result<Self> {
        let menu = Menu::new();

        let settings_item = MenuItem::new("Settings...", true, None);
        let toggle_item = MenuItem::new("Disable Refinement", true, None);
        let quit_item = MenuItem::new("Quit", true, None);

        menu.append(&settings_item)?;
        menu.append(&toggle_item)?;
        menu.append(&quit_item)?;

        // Create a simple icon (red circle to indicate "recording ready")
        let icon = create_default_icon();

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Duper Disper - Push to Talk")
            .with_icon(icon)
            .build()?;

        info!("System tray icon created");

        Ok(Self {
            _tray: tray,
            settings_id: settings_item,
            toggle_refinement_id: toggle_item,
            quit_id: quit_item,
        })
    }

    /// Poll for menu events (non-blocking).
    pub fn poll_event(&self) -> Option<TrayCommand> {
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.quit_id.id() {
                return Some(TrayCommand::Quit);
            }
            if event.id == self.settings_id.id() {
                return Some(TrayCommand::Settings);
            }
            if event.id == self.toggle_refinement_id.id() {
                return Some(TrayCommand::ToggleRefinement);
            }
        }
        None
    }

    pub fn set_recording(&self, recording: bool) {
        // Could update icon color here (green = recording, red = idle)
        let _ = recording;
    }
}

fn create_default_icon() -> tray_icon::Icon {
    // Create a simple 32x32 RGBA icon (blue circle)
    let size = 32;
    let mut rgba = vec![0u8; size * size * 4];
    let center = size as f32 / 2.0;
    let radius = center - 2.0;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = (y * size + x) * 4;

            if dist <= radius {
                rgba[idx] = 66;      // R
                rgba[idx + 1] = 133; // G
                rgba[idx + 2] = 244; // B
                rgba[idx + 3] = 255; // A
            }
        }
    }

    tray_icon::Icon::from_rgba(rgba, size as u32, size as u32).unwrap()
}
