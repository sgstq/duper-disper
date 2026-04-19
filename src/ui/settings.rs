use crate::config::AppConfig;
use crate::hotkey;
use eframe::egui;
use std::sync::Mutex;
use tracing::{error, info};

#[cfg(target_os = "windows")]
use winit::platform::windows::EventLoopBuilderExtWindows;

/// Launch the settings window as a child process.
/// Uses a subprocess so the winit event loop can be created fresh each time.
pub fn open_settings_window(child: &Mutex<Option<std::process::Child>>) {
    let mut guard = child.lock().unwrap();

    // Check if a settings process is already running
    if let Some(ref mut proc) = *guard {
        match proc.try_wait() {
            Ok(Some(_)) => {
                // Process has exited, allow reopening
            }
            Ok(None) => {
                info!("Settings window already open");
                return;
            }
            Err(e) => {
                error!("Failed to check settings process: {}", e);
            }
        }
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            error!("Cannot determine current exe path: {}", e);
            return;
        }
    };

    match std::process::Command::new(exe).arg("--settings").spawn() {
        Ok(proc) => {
            info!("Settings subprocess started (pid={})", proc.id());
            *guard = Some(proc);
        }
        Err(e) => {
            error!("Failed to spawn settings subprocess: {}", e);
        }
    }
}

/// Run the settings window in the current process (used by `--settings` subprocess).
pub fn run_settings_window() -> Result<(), eframe::Error> {
    let config = AppConfig::load().unwrap_or_default();
    let app = SettingsApp::new(config);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 580.0])
            .with_min_inner_size([420.0, 400.0])
            .with_title("Duper Disper"),
        #[cfg(target_os = "windows")]
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_any_thread(true);
        })),
        ..Default::default()
    };

    eframe::run_native(
        "Duper Disper",
        options,
        Box::new(|cc| {
            configure_style(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(12.0, 4.0);
    ctx.set_style(style);
}

/// Which tab is currently selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Home,
    General,
    Transcription,
    Refinement,
    About,
}

struct SettingsApp {
    config: AppConfig,
    active_tab: Tab,
    status_message: Option<(String, bool)>, // (message, is_error)
    stt_backend_idx: usize,
    capturing_hotkey: bool,
    hotkey_capture_preview: Option<String>,
}

impl SettingsApp {
    fn new(config: AppConfig) -> Self {
        let stt_backend_idx = match config.stt_backend {
            crate::transcription::SttBackend::Local => 0,
            crate::transcription::SttBackend::OpenAI => 1,
            crate::transcription::SttBackend::Deepgram => 2,
            crate::transcription::SttBackend::Groq => 3,
        };
        Self {
            config,
            active_tab: Tab::Home,
            status_message: None,
            stt_backend_idx,
            capturing_hotkey: false,
            hotkey_capture_preview: None,
        }
    }

    fn save_config(&mut self) {
        if let Err(e) = hotkey::parse_hotkey(&self.config.hotkey) {
            self.status_message = Some((format!("Invalid shortcut: {}", e), true));
            return;
        }

        self.config.stt_backend = match self.stt_backend_idx {
            0 => crate::transcription::SttBackend::Local,
            1 => crate::transcription::SttBackend::OpenAI,
            2 => crate::transcription::SttBackend::Deepgram,
            3 => crate::transcription::SttBackend::Groq,
            _ => crate::transcription::SttBackend::Local,
        };

        match self.config.save() {
            Ok(()) => {
                info!("Settings saved");
                self.status_message = Some(("Settings saved! Restart to apply changes.".into(), false));
            }
            Err(e) => {
                error!("Failed to save settings: {}", e);
                self.status_message = Some((format!("Failed to save: {}", e), true));
            }
        }
    }

    fn backend_name(&self) -> &'static str {
        STT_BACKENDS[self.stt_backend_idx]
    }

    fn capture_hotkey_from_input(&mut self, ctx: &egui::Context) {
        if !self.capturing_hotkey {
            return;
        }

        self.hotkey_capture_preview = current_hotkey_preview(ctx);

        let events = ctx.input(|i| i.events.clone());
        for event in events {
            if let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                repeat: false,
                ..
            } = event
            {
                if let Some(hotkey) = egui_combo_to_hotkey_string(ctx, key, modifiers) {
                    match hotkey::parse_hotkey(&hotkey) {
                        Ok(_) => {
                            self.config.hotkey = hotkey;
                            self.capturing_hotkey = false;
                            self.hotkey_capture_preview = None;
                            self.status_message = Some((
                                format!("Hotkey set to {}. Save to apply.", self.config.hotkey),
                                false,
                            ));
                            break;
                        }
                        Err(e) => {
                            self.status_message = Some((format!("Invalid shortcut: {}", e), true));
                        }
                    }
                }
            }
        }
    }

    fn render_home_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Duper Disper");
        ui.label(
            egui::RichText::new("A desktop voice transcription app for fast dictation into any focused text field.")
                .weak(),
        );
        ui.add_space(12.0);

        ui.group(|ui| {
            ui.heading("Current Setup");
            ui.add_space(6.0);

            egui::Grid::new("home_status_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Hotkey:");
                    ui.monospace(&self.config.hotkey);
                    ui.end_row();

                    ui.label("Speech backend:");
                    ui.label(self.backend_name());
                    ui.end_row();

                    ui.label("Insertion:");
                    ui.label(match self.config.insertion_method() {
                        crate::insertion::InsertionMethod::Clipboard => "Clipboard Paste",
                        crate::insertion::InsertionMethod::SimulateTyping => "Simulate Typing",
                    });
                    ui.end_row();

                    ui.label("Refinement:");
                    ui.label(if self.config.enable_refinement { "Enabled" } else { "Disabled" });
                    ui.end_row();
                });
        });

        ui.add_space(10.0);

        ui.group(|ui| {
            ui.heading("Quick Actions");
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                if ui.button("Save Settings").clicked() {
                    self.save_config();
                }

                if let Ok(config_path) = AppConfig::config_path() {
                    if ui.button("Show Config File").clicked() {
                        let _ = std::process::Command::new("open")
                            .arg("-R")
                            .arg(config_path)
                            .spawn();
                    }
                }
            });
        });

        ui.add_space(10.0);

        ui.group(|ui| {
            ui.heading("How It Works");
            ui.add_space(6.0);
            ui.label(format!(
                "1. Hold {} to record\n2. Release to transcribe\n3. Duper Disper inserts the text into the focused app",
                self.config.hotkey
            ));
        });

        #[cfg(target_os = "macos")]
        {
            ui.add_space(10.0);
            ui.group(|ui| {
                ui.heading("macOS Permissions");
                ui.add_space(6.0);
                ui.label("Grant Accessibility so the app can detect the hotkey and send paste or typing events.");
                ui.label("Grant Microphone access so recording works.");
            });
        }
    }

    fn render_general_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("General");
        ui.separator();

        egui::Grid::new("general_grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label("Hotkey:");
                ui.horizontal(|ui| {
                    let mut hotkey_display = self.config.hotkey.clone();
                    ui.add_enabled(
                        false,
                        egui::TextEdit::singleline(&mut hotkey_display).desired_width(120.0),
                    );

                    if self.capturing_hotkey {
                        let preview = self
                            .hotkey_capture_preview
                            .as_deref()
                            .unwrap_or("Press shortcut...");
                        ui.label(egui::RichText::new(preview).weak());
                        if ui.button("Cancel").clicked() {
                            self.capturing_hotkey = false;
                            self.hotkey_capture_preview = None;
                        }
                    } else if ui.button("Record Shortcut").clicked() {
                        self.capturing_hotkey = true;
                        self.hotkey_capture_preview = None;
                        self.status_message = Some((
                            "Press your shortcut combination, then save to apply.".into(),
                            false,
                        ));
                    }

                    if ui.button("Reset Default").clicked() {
                        self.config.hotkey = crate::config::default_hotkey().to_string();
                        self.capturing_hotkey = false;
                        self.hotkey_capture_preview = None;
                        self.status_message = Some((
                            format!("Shortcut reset to {}. Save to apply.", self.config.hotkey),
                            false,
                        ));
                    }
                });
                ui.end_row();

                ui.label("Language:");
                ui.text_edit_singleline(&mut self.config.language);
                ui.end_row();

                ui.label("Insertion method:");
                egui::ComboBox::from_id_salt("insertion_method")
                    .selected_text(&self.config.insertion_method)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.config.insertion_method, "clipboard".into(), "Clipboard Paste");
                        ui.selectable_value(&mut self.config.insertion_method, "typing".into(), "Simulate Typing");
                    });
                ui.end_row();

                ui.label("Sound feedback:");
                ui.checkbox(&mut self.config.sound_feedback, "");
                ui.end_row();

                ui.label("Show overlay:");
                ui.checkbox(&mut self.config.show_overlay, "");
                ui.end_row();

                #[cfg(windows)]
                {
                    ui.label("Auto-start on login:");
                    ui.checkbox(&mut self.config.auto_start, "");
                    ui.end_row();
                }

                ui.label("Developer mode:");
                ui.checkbox(&mut self.config.developer_mode, "");
                ui.end_row();
            });

        if self.config.developer_mode {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Debug tracing enabled. Logs written with TRACE level detail.")
                    .small()
                    .weak(),
            );
        }
    }

    fn render_transcription_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Speech-to-Text");
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Backend:");
            egui::ComboBox::from_id_salt("stt_backend")
                .selected_text(STT_BACKENDS[self.stt_backend_idx])
                .show_ui(ui, |ui| {
                    for (i, name) in STT_BACKENDS.iter().enumerate() {
                        ui.selectable_value(&mut self.stt_backend_idx, i, *name);
                    }
                });
        });

        ui.add_space(8.0);

        match self.stt_backend_idx {
            0 => {
                // Local
                ui.group(|ui| {
                    ui.label("Local Whisper (whisper.cpp)");
                    ui.add_space(4.0);
                    egui::Grid::new("local_grid")
                        .num_columns(2)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Model:");
                            egui::ComboBox::from_id_salt("whisper_model")
                                .selected_text(&self.config.whisper_model)
                                .show_ui(ui, |ui| {
                                    for m in WHISPER_MODELS {
                                        ui.selectable_value(&mut self.config.whisper_model, m.to_string(), *m);
                                    }
                                });
                            ui.end_row();
                        });
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("Runs locally on CPU. Larger models are slower but more accurate.")
                            .small()
                            .weak(),
                    );
                });
            }
            _ => {
                // Cloud backends
                let backend_name = STT_BACKENDS[self.stt_backend_idx];
                ui.group(|ui| {
                    ui.label(format!("{} Cloud API", backend_name));
                    ui.add_space(4.0);
                    egui::Grid::new("cloud_grid")
                        .num_columns(2)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("API Key:");
                            ui.add(egui::TextEdit::singleline(&mut self.config.cloud_stt.api_key).password(true));
                            ui.end_row();

                            ui.label("API URL:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.config.cloud_stt.api_url)
                                    .hint_text("Leave empty for default"),
                            );
                            ui.end_row();

                            ui.label("Model:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.config.cloud_stt.model)
                                    .hint_text("Leave empty for default"),
                            );
                            ui.end_row();
                        });
                    ui.add_space(2.0);
                    let hint = match self.stt_backend_idx {
                        1 => "Default model: whisper-1",
                        2 => "Default model: nova-2",
                        3 => "Default model: whisper-large-v3-turbo (fastest)",
                        _ => "",
                    };
                    ui.label(egui::RichText::new(hint).small().weak());
                });
            }
        }
    }

    fn render_refinement_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("LLM Refinement");
        ui.separator();

        ui.checkbox(&mut self.config.enable_refinement, "Enable refinement");

        if !self.config.enable_refinement {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Raw transcription will be inserted without LLM processing.")
                    .weak(),
            );
            return;
        }

        ui.add_space(8.0);

        egui::Grid::new("refinement_grid")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("API URL:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.config.refinement.api_url)
                        .hint_text("http://localhost:11434/v1/chat/completions"),
                );
                ui.end_row();

                ui.label("API Key:");
                ui.add(egui::TextEdit::singleline(&mut self.config.refinement.api_key).password(true));
                ui.end_row();

                ui.label("Model:");
                ui.text_edit_singleline(&mut self.config.refinement.model);
                ui.end_row();

                ui.label("Max tokens:");
                ui.add(egui::DragValue::new(&mut self.config.refinement.max_tokens).range(64..=8192));
                ui.end_row();

                ui.label("Use screenshot:");
                ui.checkbox(&mut self.config.refinement.use_screenshot, "Send screenshot for context");
                ui.end_row();
            });

        ui.add_space(8.0);

        ui.collapsing("System Prompt", |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.config.refinement.system_prompt)
                    .desired_rows(10)
                    .desired_width(f32::INFINITY)
                    .code_editor(),
            );
            if ui.button("Reset to default").clicked() {
                self.config.refinement.system_prompt = crate::refinement::DEFAULT_SYSTEM_PROMPT.to_string();
            }
        });
    }

    fn render_about_tab(&self, ui: &mut egui::Ui) {
        ui.heading("About");
        ui.separator();
        ui.add_space(12.0);

        ui.label(
            egui::RichText::new(format!("Duper Disper v{}", env!("CARGO_PKG_VERSION")))
                .size(18.0)
                .strong(),
        );
        ui.add_space(4.0);
        ui.label("Fast push-to-talk voice transcription and refinement tool.");
        ui.add_space(12.0);

        ui.label(
            egui::RichText::new(format!(
                "Hold {} to record, release to transcribe.",
                self.config.hotkey
            ))
            .weak(),
        );

        ui.add_space(20.0);
        if let Ok(config_path) = AppConfig::config_path() {
            ui.horizontal(|ui| {
                ui.label("Config file:");
                ui.monospace(config_path.display().to_string());
            });
        }
    }
}

const STT_BACKENDS: &[&str] = &["Local", "OpenAI", "Deepgram", "Groq"];
const WHISPER_MODELS: &[&str] = &[
    "tiny.en", "tiny", "base.en", "base", "small.en", "small", "medium.en", "medium",
    "large-v2", "large-v3", "large-v3-turbo",
];

fn egui_combo_to_hotkey_string(
    ctx: &egui::Context,
    key: egui::Key,
    modifiers: egui::Modifiers,
) -> Option<String> {
    let mut parts = Vec::new();

    if modifiers.ctrl {
        parts.push("Ctrl");
    }
    if modifiers.shift {
        parts.push("Shift");
    }
    if modifiers.alt {
        parts.push("Alt");
    }
    if modifiers.mac_cmd {
        parts.push("Cmd");
    }

    let keys_down = ctx.input(|i| i.keys_down.iter().copied().collect::<Vec<_>>());
    let mut extra_keys = keys_down
        .into_iter()
        .filter(|down_key| *down_key != key)
        .filter_map(egui_key_to_hotkey)
        .filter(|part| !matches!(*part, "Ctrl" | "Shift" | "Alt" | "Cmd"))
        .collect::<Vec<_>>();
    extra_keys.sort_unstable();
    extra_keys.dedup();

    for extra_key in extra_keys {
        parts.push(extra_key);
    }

    parts.push(egui_key_to_hotkey(key)?);

    let mut unique_parts = Vec::new();
    for part in parts {
        if !unique_parts.contains(&part) {
            unique_parts.push(part);
        }
    }

    Some(unique_parts.join("+"))
}

fn current_hotkey_preview(ctx: &egui::Context) -> Option<String> {
    let modifiers = ctx.input(|i| i.modifiers);
    let keys_down = ctx.input(|i| i.keys_down.iter().copied().collect::<Vec<_>>());

    let mut parts = Vec::new();

    if modifiers.ctrl {
        parts.push("Ctrl");
    }
    if modifiers.shift {
        parts.push("Shift");
    }
    if modifiers.alt {
        parts.push("Alt");
    }
    if modifiers.mac_cmd {
        parts.push("Cmd");
    }

    let mut keys = keys_down
        .into_iter()
        .filter_map(egui_key_to_hotkey)
        .filter(|part| !matches!(*part, "Ctrl" | "Shift" | "Alt" | "Cmd"))
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys.dedup();

    for key in keys {
        parts.push(key);
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("+"))
    }
}

fn egui_key_to_hotkey(key: egui::Key) -> Option<&'static str> {
    Some(match key {
        egui::Key::Escape => "Escape",
        egui::Key::Tab => "Tab",
        egui::Key::Backspace => "Backspace",
        egui::Key::Enter => "Enter",
        egui::Key::Space => "Space",
        egui::Key::Insert => "Insert",
        egui::Key::Delete => "Delete",
        egui::Key::Home => "Home",
        egui::Key::End => "End",
        egui::Key::PageUp => "PageUp",
        egui::Key::PageDown => "PageDown",
        egui::Key::ArrowLeft => "Left",
        egui::Key::ArrowRight => "Right",
        egui::Key::ArrowUp => "Up",
        egui::Key::ArrowDown => "Down",
        egui::Key::Minus => "Minus",
        egui::Key::Plus => "Plus",
        egui::Key::Num0 => "0",
        egui::Key::Num1 => "1",
        egui::Key::Num2 => "2",
        egui::Key::Num3 => "3",
        egui::Key::Num4 => "4",
        egui::Key::Num5 => "5",
        egui::Key::Num6 => "6",
        egui::Key::Num7 => "7",
        egui::Key::Num8 => "8",
        egui::Key::Num9 => "9",
        egui::Key::A => "A",
        egui::Key::B => "B",
        egui::Key::C => "C",
        egui::Key::D => "D",
        egui::Key::E => "E",
        egui::Key::F => "F",
        egui::Key::G => "G",
        egui::Key::H => "H",
        egui::Key::I => "I",
        egui::Key::J => "J",
        egui::Key::K => "K",
        egui::Key::L => "L",
        egui::Key::M => "M",
        egui::Key::N => "N",
        egui::Key::O => "O",
        egui::Key::P => "P",
        egui::Key::Q => "Q",
        egui::Key::R => "R",
        egui::Key::S => "S",
        egui::Key::T => "T",
        egui::Key::U => "U",
        egui::Key::V => "V",
        egui::Key::W => "W",
        egui::Key::X => "X",
        egui::Key::Y => "Y",
        egui::Key::Z => "Z",
        egui::Key::F1 => "F1",
        egui::Key::F2 => "F2",
        egui::Key::F3 => "F3",
        egui::Key::F4 => "F4",
        egui::Key::F5 => "F5",
        egui::Key::F6 => "F6",
        egui::Key::F7 => "F7",
        egui::Key::F8 => "F8",
        egui::Key::F9 => "F9",
        egui::Key::F10 => "F10",
        egui::Key::F11 => "F11",
        egui::Key::F12 => "F12",
        _ => return None,
    })
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.capture_hotkey_from_input(ctx);

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, Tab::Home, "Home");
                ui.selectable_value(&mut self.active_tab, Tab::General, "General");
                ui.selectable_value(&mut self.active_tab, Tab::Transcription, "Transcription");
                ui.selectable_value(&mut self.active_tab, Tab::Refinement, "Refinement");
                ui.selectable_value(&mut self.active_tab, Tab::About, "About");
            });
        });

        egui::TopBottomPanel::bottom("actions").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    self.save_config();
                }

                if ui.button("Close Window").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }

                if let Some((ref msg, is_error)) = self.status_message {
                    let color = if is_error {
                        egui::Color32::from_rgb(220, 50, 50)
                    } else {
                        egui::Color32::from_rgb(50, 180, 50)
                    };
                    ui.label(egui::RichText::new(msg).color(color));
                }
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                match self.active_tab {
                    Tab::Home => self.render_home_tab(ui),
                    Tab::General => self.render_general_tab(ui),
                    Tab::Transcription => self.render_transcription_tab(ui),
                    Tab::Refinement => self.render_refinement_tab(ui),
                    Tab::About => self.render_about_tab(ui),
                }
            });
        });
    }
}
