# Duper Disper

A fast, native push-to-talk voice transcription tool built in Rust. Inspired by SuperWhisper.

## Installation

Download the latest release from the [Releases page](https://github.com/sgstq/duper-disper/releases/latest):

- **Installer** — `duper-disper-x.x.x-setup.exe` — guided setup with Start Menu shortcut and uninstaller
- **Portable** — `duper-disper-x.x.x-portable.zip` — standalone executable, no installation needed

## Disklamer 
<p align="center">
  <b>ALL CODE AND SCRIPTS IN THIS REPOSITORY—EVEN THOSE BASED ON REAL DOCUMENTATION—ARE ENTIRELY EXPERIMENTAL. ALL LOGIC WAS HALLUCINATED BY MATRIX MULTIPLICATIONS….. HAPHAZARDLY. THE FOLLOWING REPOSITORY CONTAINS UNTESTED CODE AND DUE TO ITS CONTENT IT SHOULD NOT BE USED ANYWHERE BY ANYONE ■</b>
</p>

---
## Features

- **Push-to-talk** — Hold a configurable hotkey to record, release to transcribe and insert
- **Universal text insertion** — Transcribed text is inserted into any focused text field in any application (via clipboard paste or simulated typing)
- **Local-first** — Runs Whisper.cpp locally for transcription, no cloud required
- **LLM refinement** — Optionally refines raw transcripts using an LLM (local via Ollama/LM Studio, or cloud APIs)
- **Context-aware** — Captures active application name, window title, surrounding text, and optionally a screenshot to improve refinement quality
- **System tray** — Runs quietly in the system tray with a minimal recording overlay

## Architecture

```
src/
├── main.rs            # App entry point, event loop, hotkey handling
├── audio/             # Microphone capture via CPAL, resampling
├── transcription/     # Whisper.cpp integration via whisper-rs
├── refinement/        # LLM refinement via OpenAI-compatible API
├── context/           # Active window, surrounding text, screenshot capture
├── insertion/         # Text insertion via clipboard or simulated typing
├── config/            # TOML-based configuration
└── ui/
    ├── tray.rs        # System tray icon and menu
    └── overlay.rs     # Recording status overlay
```

## Building

```bash
# Install Rust if needed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build (release for best performance)
cargo build --release
```

On Windows, no additional system dependencies are needed.
On Linux (for development), install: `libgtk-3-dev libasound2-dev libxdo-dev cmake clang`

## Configuration

On first run, a default config is created at:
- Windows: `%APPDATA%\duper-disper\config.toml`
- Linux: `~/.config/duper-disper/config.toml`

Key settings:

```toml
hotkey = "CapsLock"              # Push-to-talk key
whisper_model = "base.en"        # Whisper model (auto-downloaded)
language = "en"                  # Language code or "auto"
insertion_method = "clipboard"   # "clipboard" or "typing"
enable_refinement = true         # Use LLM to clean up transcripts

[refinement]
api_url = "http://localhost:11434/v1/chat/completions"  # Ollama default
model = "llama3"
```

## Whisper Models

Models are automatically downloaded on first use. Available models:
- `tiny.en` / `tiny` — Fastest, lowest quality
- `base.en` / `base` — Good balance (default)
- `small.en` / `small` — Better quality
- `medium.en` / `medium` — High quality
- `large-v3` — Best quality, slowest

## Roadmap

- [x] Settings GUI window
- [ ] macOS support
- [ ] UI Automation for better surrounding text capture
- [ ] Voice activity detection (auto-stop)
- [ ] Multiple recording modes (push-to-talk, toggle, voice-activated)
- [ ] Cloud sync for settings and history
- [ ] Transcript history with search
