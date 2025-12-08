# Rust Trace Viewer (raylib)

A lightweight, game-like Rust code viewer that lets you explore projects by opening floating windows, tracing function calls, and navigating module breadcrumbs.

## Features
- Raylib UI with draggable, closable code windows; multiple files side by side.
- Click any highlighted call to jump to its definition (prefers same module when available).
- Syntax highlighting (keywords/strings/comments) plus line numbers.
- Breadcrumb bar per window (relative path + module path hint).
- Sidebar with file list and live search; search results include matching function definitions you can jump to.
- Mouse-wheel scrolling per window; layout (position/size/scroll) is persisted to `.trace_viewer_layout.json` in the project root.
- Uses a real monospaced font when found (JetBrains Mono in `assets/`, Consolas on Windows, DejaVu Sans Mono on Linux, Meslo on macOS, or `TRACE_VIEWER_FONT` env override); falls back to raylib’s default if none found.
- Horizontal scrolling for long lines (hold Shift + scroll while hovering a window).

## Controls
- Left click: focus window/title bar; click and drag title to move; click “x” to close.
- Scroll wheel: scroll code in the hovered window.
- Sidebar: click files to open; type in the search bar (auto-focus by click) to filter files and defs.
- Code: click a highlighted function/method name to open its definition in a new window.

## Running
```bash
cargo run -- .          # or pass another project root
```
Requires a desktop with raylib-compatible graphics. Tested on Rust 1.78+.

## Notes and limits
- Parsing uses `syn` with span locations. It resolves functions (free + impl) and method calls but does not perform full type/trait resolution; if multiple defs share a name, the first in that module is preferred.
- Syntax highlighting is lightweight (keywords/strings/comments + call highlights), not a full lexer.
- Module breadcrumbs use the first definition’s module path as a hint; nested module files declared externally aren’t expanded yet.
- Layout persistence stores relative paths only; deleting/renaming files will drop those windows on next load.

## Next ideas
- Richer Rust analysis (trait impl resolution, `use` following, module file loading).
- Better highlight (full lexer), minimap, and jump history/back stack.
- Keyboard shortcuts for search, next/prev result, and window focus.
