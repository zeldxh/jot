# jot

A tiny, native notepad for Windows. Minimal, translucent, Markdown-friendly, styled with the
[Alacritty](https://alacritty.org) palette. Written in Rust with [egui](https://github.com/emilk/egui).

- Borderless window with a thin title bar and adjustable **translucency**
- **Line numbers**, **word wrap** (on by default, toggleable), any installed **font** at any size
- **Live Markdown**: `# Title` is a big heading as you type, plus bold, italic, code, quotes, lists, tasks, links
- **Focus mode**: chrome hidden, centered column, everything but the current paragraph dimmed
- Find, open/save, drag and drop, unsaved-changes prompt, CRLF and UTF-8 handled

## Download

Grab `jot.exe` (or the zip) from the [latest release](https://github.com/zeldxh/jot/releases/latest) and run it.
It is a single file, no installer needed. Windows may show a SmartScreen warning because the exe is unsigned:
choose **More info**, then **Run anyway**.

## Keys

| Key | Action |
|---|---|
| `Ctrl+O` / `Ctrl+S` / `Ctrl+Shift+S` / `Ctrl+N` | Open, save, save as, new |
| `Ctrl+F`, `F3` / `Shift+F3` | Find, next / previous match |
| `Alt+Z` | Toggle word wrap |
| `Ctrl+Shift+F` (`Esc` to leave) | Focus mode |
| `F11` | Fullscreen |
| `Ctrl+Scroll`, `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Font size, reset |
| `Ctrl+Alt+Up` / `Ctrl+Alt+Down` | Opacity |
| `Ctrl+Shift+L` | Toggle line numbers |
| `Ctrl+Shift+M` | Toggle Markdown styling |

Markdown styling is on for `.md` files and for new, unsaved documents.

## Config

`~/.config/jot/config.toml`, created when you change a setting and reloaded when edited:

```toml
font = "IosevkaTerm Nerd Font Mono"   # installed family name, or a path to a .ttf/.otf
font_size = 18.0
opacity = 0.92                        # 0.3 to 1.0
word_wrap = true
line_numbers = true
focus_column = 72.0                   # focus mode width in characters
```

## Build

Needs Rust (MSVC toolchain) and the Visual Studio Build Tools.

```powershell
cargo run --release -- notes.md
pwsh ./scripts/install.ps1     # release build -> %LOCALAPPDATA%\Programs\jot + Start Menu shortcut
```

## Performance

Measured on a release build (RTX 4060 Ti, Windows 10):

| | Small file | 3 MB Markdown file |
|---|---|---|
| Window ready | ~0.15 to 0.35 s | ~0.15 s |
| Memory | ~225 MB | ~790 MB |
| Idle CPU | ~0% | ~3% |

Most of the memory is the GPU driver and wgpu, not the text. The renderer is wgpu on DirectX 12:
the OpenGL renderer is smaller but cannot do a translucent window on Windows.
Files over 1 MB open with Markdown styling off.

## Notes

- The document lives in a plain `String` inside egui's text widget, which is very fast for notes but
  lays the whole text out each edit, so multi-megabyte files will feel slower.
- Markdown markers (`#`, `**`) are dimmed, not hidden.

MIT licensed.
