# Trace Viewer (raylib)

A lightweight, game-like code viewer that lets you explore projects by opening floating windows, tracing function calls/types, and navigating module breadcrumbs.

## Features
- Raylib UI with draggable, closable code windows; multiple files side by side, plus a minimap.
- Click any highlighted call/type name to jump to its definition in a new window (prefers same module when available). Single-function/type windows group Rust structs with their impl blocks.
- Syntax highlighting via `syntect` (keywords/strings/comments + call highlights) plus line numbers.
- Breadcrumb bar per window (relative path + module path hint).
- Sidebar with file list and live search; search results include matching definitions you can jump to.
- Mouse-wheel scrolling per window; layout (position/size/scroll) is persisted to `.trace_viewer_layout.json` in the project root.
- Uses a real monospaced font when found (JetBrains Mono in `assets/`, Consolas on Windows, DejaVu Sans Mono on Linux, Meslo on macOS, or `TRACE_VIEWER_FONT` env override); falls back to raylib’s default if none found.
- Horizontal scrolling for long lines (hold Shift + scroll while hovering a window).
- Zoom (Ctrl + wheel) centers on the cursor; space + drag or middle-drag to pan. Double-click a single-function window to open the full file at that definition.
- Connection lines show call relationships between windows (drawn behind windows); clicking call sites highlights them.

## Controls
- Left click: focus window/title bar; click and drag title to move; click “x” to close.
- Scroll wheel: scroll code in the hovered window. Shift + wheel scrolls horizontally.
- Middle drag or hold Space + drag: pan the world.
- Sidebar: click files to open; type in the search bar (auto-focus by click) to filter files and defs.
- Code: click a highlighted function/method name to open its definition in a new window.
- Double-click a single-function/type window: open the full file scrolled to that definition.

## Running
```bash
cargo run -- .          # or pass another project root
```
Requires a desktop with raylib-compatible graphics. Tested on Rust 1.78+.

## Notes and limits
- Parsing uses a plugin-based Tree-sitter pipeline (Rust, Python, JavaScript, TypeScript/TSX) and a fallback for unknown files. Rust spans include basic module hints; full type/trait resolution is not implemented. If multiple defs share a name, the closest module match is preferred.
- Syntax highlighting is lightweight (keywords/strings/comments + call highlights), not a full lexer.
- Module breadcrumbs use the first definition’s module path as a hint; nested module files declared externally aren’t expanded yet.
- Layout persistence stores relative paths only; deleting/renaming files will drop those windows on next load.

## Next ideas
- Richer analysis (trait/impl resolution, `use` following, cross-language symbol links).
- Better highlight (full lexer), minimap polish, and jump history/back stack.
- Keyboard shortcuts for search, next/prev result, and window focus.
