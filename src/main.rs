mod audio;
mod config;
mod context;
mod insertion;
mod refinement;
mod transcription;
mod ui;

use anyhow::{Context, Result};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{error, info, warn};

use audio::{AudioCapture, RecordingBuffer};
use config::AppConfig;
use context::capture_context;
use insertion::insert_text;
use refinement::Refiner;
use transcription::Transcriber;
use ui::overlay::RecordingOverlay;
use ui::tray::{SystemTray, TrayCommand};

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("Duper Disper v{} starting", env!("CARGO_PKG_VERSION"));

    // Load config
    let config = AppConfig::load()?;
    info!("Config loaded: stt={:?}, hotkey={}", config.stt_backend, config.hotkey);

    // Initialize transcriber based on configured backend
    let transcriber = match config.stt_backend {
        transcription::SttBackend::Local => {
            let models_dir = AppConfig::models_dir()?;
            let model_path = transcription::ensure_model(&config.whisper_model, &models_dir)?;
            let language = if config.language == "auto" {
                None
            } else {
                Some(config.language.clone())
            };
            Transcriber::new_local(&model_path, language)?
        }
        ref backend @ (transcription::SttBackend::OpenAI
        | transcription::SttBackend::Deepgram
        | transcription::SttBackend::Groq) => {
            info!("Using cloud STT: {:?}", backend);
            Transcriber::new_cloud(
                backend.clone(),
                config.cloud_stt.clone(),
                config.language.clone(),
            )?
        }
    };

    // Initialize audio capture
    let audio = AudioCapture::new()?;
    let recording_buffer = RecordingBuffer::new();
    let _capture_stream = audio.start_stream(recording_buffer.clone())?;

    // Initialize refinement
    let refiner = if config.enable_refinement {
        Some(Refiner::new(config.refinement.clone()))
    } else {
        None
    };

    // Initialize overlay
    let overlay = RecordingOverlay::new();

    // Set up global hotkey
    let hotkey_manager = GlobalHotKeyManager::new()
        .map_err(|e| anyhow::anyhow!("Failed to create hotkey manager: {}", e))?;

    let hotkey = parse_hotkey(&config.hotkey)?;
    hotkey_manager
        .register(hotkey)
        .map_err(|e| anyhow::anyhow!("Failed to register hotkey: {}", e))?;
    info!("Registered hotkey: {}", config.hotkey);

    // Set up system tray
    let tray = SystemTray::new()?;

    // Create tokio runtime for async operations (LLM calls)
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()?;

    let running = Arc::new(AtomicBool::new(true));
    let is_recording = Arc::new(AtomicBool::new(false));

    info!("Ready! Hold {} to record, release to transcribe.", config.hotkey);

    // Main event loop
    while running.load(Ordering::SeqCst) {
        // Check hotkey events
        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            let pressed = event.state == global_hotkey::HotKeyState::Pressed;

            if pressed && !is_recording.load(Ordering::SeqCst) {
                // Start recording
                is_recording.store(true, Ordering::SeqCst);
                recording_buffer.start();
                overlay.show_recording();
                tray.set_recording(true);
                info!("Recording started");
            } else if !pressed && is_recording.load(Ordering::SeqCst) {
                // Stop recording
                is_recording.store(false, Ordering::SeqCst);
                recording_buffer.stop();
                tray.set_recording(false);
                info!("Recording stopped");

                // Process the recording
                overlay.show_transcribing();

                let samples = recording_buffer.take_samples();
                if samples.len() < 1600 {
                    // Less than 0.1s of audio, ignore
                    warn!("Recording too short, ignoring");
                    overlay.hide();
                    continue;
                }

                // Resample to 16kHz for Whisper
                let samples_16k =
                    audio::resample(&samples, audio.sample_rate(), 16000);

                info!("Captured {} samples ({}s at source rate)",
                    samples.len(),
                    samples.len() as f32 / audio.sample_rate() as f32
                );

                // Transcribe
                match transcriber.transcribe(&samples_16k) {
                    Ok(result) => {
                        if result.text.is_empty() {
                            warn!("Empty transcription, skipping");
                            overlay.hide();
                            continue;
                        }

                        info!("Raw transcript: {}", result.text);

                        // Refine if enabled
                        let final_text = if let Some(ref refiner) = refiner {
                            overlay.show_refining();
                            let ctx = capture_context(config.capture_screenshots);

                            match rt.block_on(refiner.refine(&result.text, &ctx)) {
                                Ok(refined) => {
                                    info!("Refined: {}", refined);
                                    refined
                                }
                                Err(e) => {
                                    error!("Refinement failed, using raw transcript: {}", e);
                                    result.text
                                }
                            }
                        } else {
                            result.text
                        };

                        // Insert into active application
                        overlay.hide();
                        if let Err(e) = insert_text(&final_text, config.insertion_method()) {
                            error!("Failed to insert text: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Transcription failed: {}", e);
                        overlay.hide();
                    }
                }
            }
        }

        // Check tray menu events
        if let Some(cmd) = tray.poll_event() {
            match cmd {
                TrayCommand::Quit => {
                    info!("Quit requested");
                    running.store(false, Ordering::SeqCst);
                }
                TrayCommand::Settings => {
                    info!("Settings requested");
                    // TODO: open settings window
                }
                TrayCommand::ToggleRefinement => {
                    info!("Toggle refinement");
                    // TODO: toggle refinement on/off
                }
            }
        }

        // Don't spin the CPU
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    info!("Duper Disper shutting down");
    Ok(())
}

fn parse_hotkey(key_str: &str) -> Result<HotKey> {
    let parts: Vec<&str> = key_str.split('+').map(|s| s.trim()).collect();
    let mut modifiers = Modifiers::empty();
    let mut code = None;

    for part in &parts {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "alt" => modifiers |= Modifiers::ALT,
            "shift" => modifiers |= Modifiers::SHIFT,
            "super" | "win" | "meta" => modifiers |= Modifiers::SUPER,
            key => {
                code = Some(match key {
                    "capslock" | "caps" => Code::CapsLock,
                    "space" => Code::Space,
                    "tab" => Code::Tab,
                    "escape" | "esc" => Code::Escape,
                    "f1" => Code::F1,
                    "f2" => Code::F2,
                    "f3" => Code::F3,
                    "f4" => Code::F4,
                    "f5" => Code::F5,
                    "f6" => Code::F6,
                    "f7" => Code::F7,
                    "f8" => Code::F8,
                    "f9" => Code::F9,
                    "f10" => Code::F10,
                    "f11" => Code::F11,
                    "f12" => Code::F12,
                    "insert" => Code::Insert,
                    "pause" => Code::Pause,
                    "scrolllock" => Code::ScrollLock,
                    s if s.len() == 1 => {
                        let c = s.chars().next().unwrap();
                        match c {
                            'a' => Code::KeyA,
                            'b' => Code::KeyB,
                            'c' => Code::KeyC,
                            'd' => Code::KeyD,
                            'e' => Code::KeyE,
                            'f' => Code::KeyF,
                            'g' => Code::KeyG,
                            'h' => Code::KeyH,
                            'i' => Code::KeyI,
                            'j' => Code::KeyJ,
                            'k' => Code::KeyK,
                            'l' => Code::KeyL,
                            'm' => Code::KeyM,
                            'n' => Code::KeyN,
                            'o' => Code::KeyO,
                            'p' => Code::KeyP,
                            'q' => Code::KeyQ,
                            'r' => Code::KeyR,
                            's' => Code::KeyS,
                            't' => Code::KeyT,
                            'u' => Code::KeyU,
                            'v' => Code::KeyV,
                            'w' => Code::KeyW,
                            'x' => Code::KeyX,
                            'y' => Code::KeyY,
                            'z' => Code::KeyZ,
                            '0' => Code::Digit0,
                            '1' => Code::Digit1,
                            '2' => Code::Digit2,
                            '3' => Code::Digit3,
                            '4' => Code::Digit4,
                            '5' => Code::Digit5,
                            '6' => Code::Digit6,
                            '7' => Code::Digit7,
                            '8' => Code::Digit8,
                            '9' => Code::Digit9,
                            _ => anyhow::bail!("Unsupported key: {}", s),
                        }
                    }
                    _ => anyhow::bail!("Unknown key: {}", key),
                });
            }
        }
    }

    let code = code.context("No key specified in hotkey string")?;
    Ok(HotKey::new(Some(modifiers), code))
}
