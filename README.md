# Chameleon

A minimal terminal emulator written in Rust. It runs your shell in a PTY, parses escape sequences with the VTE library, and renders output using crossterm.

## Features

- **PTY + shell** — Spawns your `$SHELL` (or `/bin/sh`) in a pseudo-terminal
- **VTE parsing** — Handles cursor movement, colors (8 standard), bold, erase, scroll, and common CSI/ESC sequences
- **Keyboard input** — Arrow keys, Tab, Enter, Backspace, Ctrl+C/Z/D, and other typical bindings
- **Resize** — Window resize updates the PTY size and redraws the screen
- **Copy** — Select text with the mouse (click and drag), then **Ctrl+Shift+C** to copy to the system clipboard

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

## Architecture

- **Main thread** — Crossterm raw mode and alternate screen; event loop for keyboard and resize; writes input to the PTY master; redraws from a shared screen buffer when dirty or on timeout.
- **Reader thread** — Reads from the PTY master, feeds bytes into `vte::Parser`, which updates the shared screen buffer via a `Perform` implementation, then triggers a redraw.
- **Resize** — PTY size is updated and the screen buffer is resized and redrawn.

## Dependencies

| Crate          | Role                                   |
| -------------- | -------------------------------------- |
| `crossterm`    | Terminal I/O, raw mode, display, mouse |
| `portable-pty` | Cross-platform PTY                     |
| `vte`          | ANSI/VT escape parsing                 |
| `arboard`      | System clipboard for copy              |

## License

See repository or project root for license information.
