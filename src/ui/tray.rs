use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};
use tracing::info;

pub enum TrayCommand {
    OpenWindow,
    ToggleRefinement,
    Quit,
}

pub struct SystemTray {
    tray: TrayIcon,
    settings_id: MenuItem,
    toggle_refinement_id: MenuItem,
    quit_id: MenuItem,
    recording: bool,
}

impl SystemTray {
    pub fn new() -> anyhow::Result<Self> {
        let menu = Menu::new();

        let settings_item = MenuItem::new("Open Window", true, None);
        let toggle_item = MenuItem::new("Disable Refinement", true, None);
        let quit_item = MenuItem::new("Quit", true, None);

        menu.append(&settings_item)?;
        menu.append(&toggle_item)?;
        menu.append(&quit_item)?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Duper Disper - Push to Talk")
            .with_icon(create_icon(false))
            .build()?;

        info!("System tray icon created");

        Ok(Self {
            tray,
            settings_id: settings_item,
            toggle_refinement_id: toggle_item,
            quit_id: quit_item,
            recording: false,
        })
    }

    /// Poll for menu events (non-blocking).
    pub fn poll_event(&self) -> Option<TrayCommand> {
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.quit_id.id() {
                return Some(TrayCommand::Quit);
            }
            if event.id == self.settings_id.id() {
                return Some(TrayCommand::OpenWindow);
            }
            if event.id == self.toggle_refinement_id.id() {
                return Some(TrayCommand::ToggleRefinement);
            }
        }
        None
    }

    /// Reflect the recording state in the tray icon and tooltip.
    /// Red dot = recording, blue dot = idle. This is the primary visual
    /// feedback on platforms without a floating overlay (macOS/Linux).
    pub fn set_recording(&mut self, recording: bool) {
        if recording == self.recording {
            return;
        }
        self.recording = recording;
        if let Err(e) = self.tray.set_icon(Some(create_icon(recording))) {
            tracing::warn!("Failed to update tray icon: {}", e);
        }
        let tooltip = if recording {
            "Duper Disper - Recording..."
        } else {
            "Duper Disper - Push to Talk"
        };
        let _ = self.tray.set_tooltip(Some(tooltip));
    }
}

/// Build a 32x32 RGBA circle icon. Red when recording, blue when idle.
fn create_icon(recording: bool) -> tray_icon::Icon {
    let size = 32;
    let mut rgba = vec![0u8; size * size * 4];
    let center = size as f32 / 2.0;
    let radius = center - 2.0;

    let (r, g, b) = if recording {
        (229u8, 53u8, 53u8) // #E53535 red
    } else {
        (66u8, 133u8, 244u8) // #4285F4 blue
    };

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = (y * size + x) * 4;

            if dist <= radius {
                // Anti-alias the edge for a smoother look.
                let alpha = ((radius - dist).clamp(0.0, 1.0) * 255.0) as u8;
                rgba[idx] = r;
                rgba[idx + 1] = g;
                rgba[idx + 2] = b;
                rgba[idx + 3] = if dist <= radius - 1.0 { 255 } else { alpha };
            }
        }
    }

    tray_icon::Icon::from_rgba(rgba, size as u32, size as u32)
        .expect("valid RGBA tray icon")
}
