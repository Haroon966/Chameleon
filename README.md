<div align="center">

```
 ██████╗██╗  ██╗ █████╗ ███╗   ███╗███████╗██╗     ███████╗ ██████╗ ███╗   ██╗
██╔════╝██║  ██║██╔══██╗████╗ ████║██╔════╝██║     ██╔════╝██╔═══██╗████╗  ██║
██║     ███████║███████║██╔████╔██║█████╗  ██║     █████╗  ██║   ██║██╔██╗ ██║
██║     ██╔══██║██╔══██║██║╚██╔╝██║██╔══╝  ██║     ██╔══╝  ██║   ██║██║╚██╗██║
╚██████╗██║  ██║██║  ██║██║ ╚═╝ ██║███████╗███████╗███████╗╚██████╔╝██║ ╚████║
 ╚═════╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝     ╚═╝╚══════╝╚══════╝╚══════╝ ╚═════╝ ╚═╝  ╚═══╝
```

### 🦎 A minimal, AI-powered terminal emulator written in Rust

---

![Version](https://img.shields.io/badge/version-0.1.1-00aaff?style=for-the-badge)
![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)
![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20macOS-00e5a0?style=for-the-badge&logo=linux&logoColor=white)
![License](https://img.shields.io/badge/License-Personal%20Use-c084fc?style=for-the-badge)
![AI](https://img.shields.io/badge/AI-Ollama%20%7C%20OpenAI%20%7C%20Gemini%20%7C%20Groq-ffd93d?style=for-the-badge&logo=openai&logoColor=black)
![PTY](https://img.shields.io/badge/PTY-Powered-00aaff?style=for-the-badge)

<img src="images/Chameleon-version-0.1.1.png" width="280" alt="Chameleon v0.1.1" />

---

**Chameleon** runs your shell in a PTY · parses escape sequences with VTE · renders via crossterm · brings **AI command suggestions** with `Ctrl+K`

[📦 Install](#-install) · [✨ Features](#-features) · [⌨️ Key Bindings](#️-key-bindings) · [⚙️ Configuration](#️-configuration) · [🏗️ Architecture](#️-architecture--dependencies) · [📄 License](#-license)

</div>

---

## 📦 Install

### Prebuilt Binary _(recommended)_

> No Rust or package managers required.

1. Download the archive for your platform from [Releases](https://github.com/<owner>/chameleon/releases) (e.g. `chameleon-linux-x86_64.tar.gz`).
2. Extract and move the binary:

```bash
tar -xzf chameleon-*.tar.gz
mv chameleon-*/chameleon ~/bin/

# Or system-wide:
mv chameleon-*/chameleon /usr/local/bin/
```

Ensure `~/bin` is in your `PATH` (or use `/usr/local/bin`).

### Build from Source

> Requires Rust edition 2021

```bash
git clone https://github.com/<owner>/chameleon.git
cd chameleon
cargo build --release

# Run
./target/release/chameleon
# or
cargo run
```

> Replace `<owner>` with the actual GitHub org or username if cloning from elsewhere.

---

## ✨ Features

|     | Feature            | Description                                                                                    |
| --- | ------------------ | ---------------------------------------------------------------------------------------------- |
| 🔌  | **PTY + Shell**    | Spawns your `$SHELL` (or `/bin/sh`) in a real pseudo-terminal with full signal support         |
| 🎨  | **VTE Parsing**    | Cursor movement, 8 standard colors, bold, erase, scroll, CSI/ESC sequences                     |
| 🤖  | **AI Command Bar** | Press `Ctrl+K` — type English, get a shell command. Powered by Ollama, OpenAI, Gemini, or Groq |
| 📐  | **Dynamic Resize** | Window resize updates PTY size and redraws the screen seamlessly                               |
| 📋  | **Mouse Copy**     | Click-drag to select · double-click word · triple-click line · copies to clipboard             |
| 🌈  | **Live Theming**   | Edit `config.toml` and theme reloads instantly via `Ctrl+Shift+T`                              |

---

## ⌨️ Key Bindings

### Chameleon Shortcuts

| Shortcut               | Action                                               |
| ---------------------- | ---------------------------------------------------- |
| `Ctrl` + `K`           | 🤖 Open AI command bar                               |
| `Ctrl` + `Shift` + `T` | 🎨 Open config in `$EDITOR` and reload theme on save |
| `Ctrl` + `Shift` + `C` | 📋 Copy selection to system clipboard                |

### Shell Signals

| Shortcut     | Action                         |
| ------------ | ------------------------------ |
| `Ctrl` + `C` | Send `SIGINT` (interrupt)      |
| `Ctrl` + `Z` | Send `SIGTSTP` (suspend)       |
| `Ctrl` + `D` | Send `EOF` (often exits shell) |
| `Ctrl` + `\` | Send `SIGQUIT`                 |
| `Esc`        | Dismiss AI bar / pickers       |

> **Note:** Standard keys — arrows, Tab, Enter, Backspace, Home, End, Page Up/Down, Delete, Insert — are all passed through to the shell unchanged.

### Mouse

| Action                  | Result                 |
| ----------------------- | ---------------------- |
| Click + drag            | Stream selection       |
| Double-click            | Select word            |
| Triple-click            | Select whole line      |
| Release after selecting | Auto-copy to clipboard |

---

## 🤖 AI Command Bar

```
Ctrl+K  →  type your prompt  →  Enter to run  ·  Esc to dismiss
```

**Example flow:**

```
❯ ~/projects  ^K
┌─────────────────────────────────────────────────┐
│ 🤖  find all rust files modified in last 7 days │
└─────────────────────────────────────────────────┘
  ➜  find . -name "*.rs" -mtime -7 -type f
     [Enter to run · Esc to dismiss]
```

**Switching models:** Type `/model` or `/models` in the AI bar to open the backend picker:

```
/model  →  choose backend (Ollama · OpenAI · Gemini · Groq)  →  pick a model
```

**Managing API keys** from inside the AI bar:

- **Configure API** — add or change an API key
- **Remove API** — remove a provider's key

---

## ⚙️ Configuration

**Location:** `~/.config/chameleon/config.toml`
_(or `$XDG_CONFIG_HOME/chameleon/config.toml` if set)_

The config file is created automatically the first time you press `Ctrl+Shift+T` (config directory and a default `config.toml` are created if missing).

> **Quick edit:** Press `Ctrl+Shift+T` inside Chameleon to open the config in your `$EDITOR` (or `$VISUAL`, or `nano` if unset). Save and exit — theme reloads immediately.

```toml
# ──────────────────────────────────────
# THEME
# ──────────────────────────────────────
[theme]
default_foreground = "#cccccc"     # text color
default_background = "#1e1e1e"     # terminal background
background_opacity = 0.95          # 0.0 transparent → 1.0 opaque
font_size          = 14            # points (hint; host may override)

# ──────────────────────────────────────
# AI
# ──────────────────────────────────────
[ai]
backend  = "ollama"                         # ollama | openai | gemini | groq
base_url = "http://127.0.0.1:11434"        # Ollama server URL
model    = "llama3.2:latest"

# API keys — env vars are preferred over storing here
# OPENAI_API_KEY / GEMINI_API_KEY / GROQ_API_KEY
[ai.providers.openai]
api_key = "sk-..."

[ai.providers.gemini]
api_key = "..."

[ai.providers.groq]
api_key = "..."
```

### Theme Options

| Option               | Description                          | Default     |
| -------------------- | ------------------------------------ | ----------- |
| `default_foreground` | Text color (hex)                     | `"#cccccc"` |
| `default_background` | Background color (hex)               | `"#1e1e1e"` |
| `background_opacity` | `0.0` = transparent · `1.0` = opaque | `0.95`      |
| `font_size`          | Font size in points (clamped 6–72)   | `14`        |

### AI Options

| Option     | Description         | Example                    |
| ---------- | ------------------- | -------------------------- |
| `backend`  | Default AI provider | `"ollama"`                 |
| `base_url` | Ollama server URL   | `"http://127.0.0.1:11434"` |
| `model`    | Default model       | `"llama3.2:latest"`        |

### API Key Environment Variables

| Provider | Environment Variable |
| -------- | -------------------- |
| OpenAI   | `OPENAI_API_KEY`     |
| Gemini   | `GEMINI_API_KEY`     |
| Groq     | `GROQ_API_KEY`       |

---

## 🏗️ Architecture & Dependencies

### Thread Model

```
┌─────────────────────────────────────────────────────────────────┐
│                        CHAMELEON PROCESS                        │
│                                                                 │
│  ┌──────────────────────┐      ┌──────────────────────────┐    │
│  │     MAIN THREAD      │      │      READER THREAD        │    │
│  │                      │      │                           │    │
│  │  • Crossterm raw     │      │  • Reads PTY master       │    │
│  │    mode + alt screen │      │  • Feeds vte::Parser      │    │
│  │  • Keyboard & resize │◄────►│  • Updates screen buffer  │    │
│  │    event loop        │      │    via Perform impl       │    │
│  │  • Writes input to   │      │  • Triggers redraw        │    │
│  │    PTY master        │      │                           │    │
│  │  • Redraws on dirty  │      └──────────────────────────┘    │
│  └──────────────────────┘                                       │
│                                                                 │
│  RESIZE: PTY size updated → buffer resized → full redraw        │
└─────────────────────────────────────────────────────────────────┘
```

### Dependencies

| Crate                 | Role                                          |
| --------------------- | --------------------------------------------- |
| `crossterm`           | Terminal I/O, raw mode, display, mouse        |
| `portable-pty`        | Cross-platform PTY support                    |
| `vte`                 | ANSI/VT100 escape sequence parsing            |
| `arboard`             | System clipboard (copy)                       |
| `ureq` + `serde_json` | HTTP calls to Ollama / OpenAI / Gemini / Groq |
| `serde` + `toml`      | Config file parsing                           |
| `directories`         | XDG-aware config path (`~/.config/chameleon`) |

---

## 📋 Requirements

| Requirement              | Details                                                                                 |
| ------------------------ | --------------------------------------------------------------------------------------- |
| 🐧 **Platform**          | Linux or macOS (PTY requires Unix-like environment)                                     |
| 🤖 **AI** _(optional)_   | [Ollama](https://ollama.ai) with ≥1 model **or** an API key for OpenAI, Gemini, or Groq |
| 🦀 **Build from source** | Rust edition 2021                                                                       |

---

## 📄 License

Personal and non-commercial use is **free**.
Modification, rebranding, and resale require **written permission** from the copyright holder.

See [`LICENSE`](LICENSE) in the project root.

---

<div align="center">

**🦎 Chameleon — A terminal that adapts to you**

_Built with ♥ in Rust · PTY + VTE + crossterm_

</div>
