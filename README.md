# Chameleon

A minimal terminal emulator written in Rust. It runs your shell in a PTY, parses escape sequences with the VTE library, and renders output using crossterm.

## Features

- **PTY + shell** — Spawns your `$SHELL` (or `/bin/sh`) in a pseudo-terminal
- **VTE parsing** — Handles cursor movement, colors (8 standard), bold, erase, scroll, and common CSI/ESC sequences
- **Keyboard input** — Arrow keys, Tab, Enter, Backspace, Ctrl+C/Z/D, and other typical bindings
- **Resize** — Window resize updates the PTY size and redraws the screen
- **Copy** — Select text with the mouse (click and drag), then **Ctrl+Shift+C** to copy to the system clipboard
- **Theme** — Edit text color, background color, opacity, and font size via a config file; **Ctrl+Shift+T** opens the config in `$EDITOR` and reloads the theme on save

## Requirements

- Rust (edition 2021)
- A Unix-like environment (Linux, macOS) for PTY support

## Build & Run

```bash
cargo build --release
cargo run
```

Or run the release binary directly:

```bash
./target/release/minimal-term
```

Exit by closing the shell (e.g. `exit` or Ctrl+D) or terminating the process.

## Copy

- **Select**: Click and drag with the left mouse button to select a rectangular region (shown highlighted).
- **Copy**: Press **Ctrl+Shift+C** to copy the selection to the system clipboard.

## Theme

Theme settings are stored in a config file. If the file does not exist, defaults are used.

- **Config path**: `$XDG_CONFIG_HOME/chameleon/config.toml` (on Linux/macOS typically `~/.config/chameleon/config.toml`).
- **Edit and reload**: Press **Ctrl+Shift+T** to open the config file in your `$EDITOR` (or `$VISUAL`, or `nano`). When you exit the editor, the theme is reloaded and applied immediately.

Example `config.toml`:

```toml
[theme]
# Default text (foreground) color — hex.
default_foreground = "#cccccc"
# Terminal background color — hex.
default_background = "#1e1e1e"
# 0.0 = fully transparent, 1.0 = opaque (stored for terminals that support it).
background_opacity = 0.95
# Font size in points (hint; many terminals ignore app-set font size).
font_size = 14
```

- **Text color** and **background color** are applied by the app (24-bit RGB).
- **Font size** and **background opacity** are stored in the config. Many host terminals do not allow the application to change font size or window transparency; if yours does not, set font size and opacity in your terminal emulator’s own settings.

## Architecture

- **Main thread** — Crossterm raw mode and alternate screen; event loop for keyboard and resize; writes input to the PTY master; redraws from a shared screen buffer when dirty or on timeout.
- **Reader thread** — Reads from the PTY master, feeds bytes into `vte::Parser`, which updates the shared screen buffer via a `Perform` implementation, then triggers a redraw.
- **Resize** — PTY size is updated and the screen buffer is resized and redrawn.

## Dependencies

| Crate          | Role                                   |
| -------------- | -------------------------------------- |
| `crossterm`    | Terminal I/O, raw mode, display, mouse |
| `directories`  | Config path (`~/.config/chameleon`)     |
| `portable-pty` | Cross-platform PTY                     |
| `serde` / `toml` | Theme config parsing                 |
| `vte`          | ANSI/VT escape parsing                 |
| `arboard`      | System clipboard for copy              |

## License

See repository or project root for license information.
