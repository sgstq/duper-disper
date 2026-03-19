use crate::config::AppConfig;
use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{error, info};

#[cfg(target_os = "windows")]
use winit::platform::windows::EventLoopBuilderExtWindows;

/// Launch the settings window on a background thread.
/// Returns a handle that signals when the window is open.
pub fn open_settings_window(is_open: Arc<AtomicBool>) {
    if is_open.load(Ordering::SeqCst) {
        info!("Settings window already open");
        return;
    }

    is_open.store(true, Ordering::SeqCst);
    let open_flag = is_open.clone();

    std::thread::Builder::new()
        .name("settings-ui".into())
        .spawn(move || {
            let config = match AppConfig::load() {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to load config for settings UI: {}", e);
                    open_flag.store(false, Ordering::SeqCst);
                    return;
                }
            };

            let app = SettingsApp::new(config);

            let options = eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_inner_size([520.0, 580.0])
                    .with_min_inner_size([420.0, 400.0])
                    .with_title("Duper Disper - Settings"),
                // Allow eframe to create an event loop on a non-main thread
                #[cfg(target_os = "windows")]
                event_loop_builder: Some(Box::new(|builder| {
                    builder.with_any_thread(true);
                })),
                ..Default::default()
            };

            if let Err(e) = eframe::run_native(
                "Duper Disper Settings",
                options,
                Box::new(|cc| {
                    configure_style(&cc.egui_ctx);
                    Ok(Box::new(app))
                }),
            ) {
                error!("Settings window error: {}", e);
            }

            open_flag.store(false, Ordering::SeqCst);
            info!("Settings window closed");
        })
        .expect("Failed to spawn settings UI thread");
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
            active_tab: Tab::General,
            status_message: None,
            stt_backend_idx,
        }
    }

    fn save_config(&mut self) {
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

    fn render_general_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("General");
        ui.separator();

        egui::Grid::new("general_grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label("Hotkey:");
                ui.text_edit_singleline(&mut self.config.hotkey);
                ui.end_row();

                ui.label("Language:");
                ui.text_edit_singleline(&mut self.config.language);
                ui.end_row();

                ui.label("Insertion method:");
                egui::ComboBox::from_id_salt("insertion_method")
                    .selected_text(&self.config.insertion_method)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.config.insertion_method, "clipboard".into(), "Clipboard (Ctrl+V)");
                        ui.selectable_value(&mut self.config.insertion_method, "typing".into(), "Simulate Typing");
                    });
                ui.end_row();

                ui.label("Sound feedback:");
                ui.checkbox(&mut self.config.sound_feedback, "");
                ui.end_row();

                ui.label("Show overlay:");
                ui.checkbox(&mut self.config.show_overlay, "");
                ui.end_row();
            });
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
            egui::RichText::new("Hold CapsLock (or your configured hotkey) to record, release to transcribe.")
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

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
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
                    Tab::General => self.render_general_tab(ui),
                    Tab::Transcription => self.render_transcription_tab(ui),
                    Tab::Refinement => self.render_refinement_tab(ui),
                    Tab::About => self.render_about_tab(ui),
                }
            });
        });
    }
}
