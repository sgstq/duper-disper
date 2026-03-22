// Hide the console window on Windows release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};

use duper_disper::audio::{self, AudioCapture, RecordingBuffer};
use duper_disper::config::AppConfig;
use duper_disper::context::capture_context;
use duper_disper::insertion::insert_text;
use duper_disper::refinement::Refiner;
use duper_disper::transcription::{self, Transcriber};
use duper_disper::hotkey;
use duper_disper::ui::overlay::RecordingOverlay;
use duper_disper::ui::settings;
use duper_disper::ui::tray::{SystemTray, TrayCommand};

fn main() -> Result<()> {
    // If launched as settings subprocess, just run the settings UI and exit
    if std::env::args().any(|a| a == "--settings") {
        return duper_disper::ui::settings::run_settings_window()
            .map_err(|e| anyhow::anyhow!("Settings window error: {}", e));
    }

    // Ensure only one instance of the main app runs at a time
    let _instance_lock = acquire_single_instance_lock()?;

    // Load config early to check developer_mode for log level
    let config = AppConfig::load()?;

    // Initialize logging — use trace level in developer mode, info otherwise
    let default_level = if config.developer_mode { "trace" } else { "info" };
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level));

    if cfg!(not(debug_assertions)) {
        // Release: log to file since there's no console
        if let Ok(log_dir) = AppConfig::config_dir() {
            let log_file = std::fs::File::create(log_dir.join("duper-disper.log"))
                .unwrap_or_else(|_| {
                    // Fall back to temp dir
                    let tmp = std::env::temp_dir().join("duper-disper.log");
                    std::fs::File::create(tmp).expect("Cannot create log file")
                });
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_writer(log_file)
                .with_ansi(false)
                .init();
        } else {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .init();
        }
    } else {
        // Debug: log to stderr (console)
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .init();
    }

    info!("Duper Disper v{} starting", env!("CARGO_PKG_VERSION"));
    info!("Config loaded: stt={:?}, hotkey={}, developer_mode={}", config.stt_backend, config.hotkey, config.developer_mode);

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
    let mut overlay = RecordingOverlay::new(config.show_overlay);

    // Set up global hotkey via low-level keyboard hook
    let hotkey_config = hotkey::parse_hotkey(&config.hotkey)?;
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
    let settings_child: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));

    // If launched with --show-settings, open settings window on startup
    // (unlike --settings, the main app keeps running)
    if std::env::args().any(|a| a == "--show-settings") {
        settings::open_settings_window(&settings_child);
    }

    let hotkey_rx = hotkey::start_listener(hotkey_config, running.clone())?;

    info!("Ready! Hold {} to record, release to transcribe.", config.hotkey);

    // Main event loop
    while running.load(Ordering::SeqCst) {
        // Check hotkey events
        if let Ok(event) = hotkey_rx.try_recv() {
            let pressed = event == hotkey::HotkeyEvent::Pressed;

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

                // Check if audio is essentially silence (Whisper hallucinates on silence)
                let rms = audio::rms_energy(&samples);
                if rms < 0.005 {
                    info!("Audio is silence (RMS={:.6}), skipping transcription", rms);
                    overlay.hide();
                    continue;
                }

                // Resample to 16kHz for Whisper
                let samples_16k =
                    audio::resample(&samples, audio.sample_rate(), 16000);

                info!("Captured {} samples ({}s at source rate, RMS={:.4})",
                    samples.len(),
                    samples.len() as f32 / audio.sample_rate() as f32,
                    rms
                );

                // Transcribe
                match transcriber.transcribe(&samples_16k) {
                    Ok(result) => {
                        if result.text.is_empty() {
                            warn!("Empty transcription, skipping");
                            overlay.hide();
                            continue;
                        }

                        if transcription::is_hallucination(&result.text) {
                            warn!("Detected Whisper hallucination: {:?}, skipping", result.text);
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
                    settings::open_settings_window(&settings_child);
                }
                TrayCommand::ToggleRefinement => {
                    info!("Toggle refinement");
                    // TODO: toggle refinement on/off
                }
            }
        }

        // Update overlay animation (pulsing during recording)
        overlay.tick();

        // Pump Win32 messages (required for tray icon menu to work) and avoid spinning CPU
        pump_messages();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    info!("Duper Disper shutting down");
    Ok(())
}

/// Acquire a system-wide named mutex to prevent multiple instances.
/// Returns the mutex handle (must be kept alive for the process lifetime).
#[cfg(windows)]
fn acquire_single_instance_lock() -> Result<windows::Win32::Foundation::HANDLE> {
    use windows::core::w;
    use windows::Win32::Foundation::GetLastError;
    use windows::Win32::System::Threading::CreateMutexW;

    let handle = unsafe { CreateMutexW(None, true, w!("Global\\DuperDisper_SingleInstance"))? };

    if unsafe { GetLastError() } == windows::Win32::Foundation::ERROR_ALREADY_EXISTS {
        anyhow::bail!("Duper Disper is already running.");
    }

    Ok(handle)
}

#[cfg(not(windows))]
fn acquire_single_instance_lock() -> Result<()> {
    Ok(())
}

/// Process pending Win32 messages. Required for the system tray context menu
/// to appear when right-clicking the tray icon.
#[cfg(windows)]
fn pump_messages() {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(windows))]
fn pump_messages() {
    // No-op on non-Windows platforms
}

