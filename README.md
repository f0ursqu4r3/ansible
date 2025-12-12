# Trace Viewer (raylib)

A game-like code explorer with floating windows, clickable call graphs, and a HUD-style sidebar/minimap for navigating large projects.

## Features
- Raylib UI with draggable, closable code windows; multiple files side by side, plus a minimap that fills the available height and sits beside scrollbars.
- Click any highlighted call/type name to jump to its definition in a new window (prefers same module when available). Single-function/type windows include associated `impl` blocks and leading docs/attributes.
- Syntax highlighting via tree-sitter highlight queries (keywords/strings/comments + call highlights) plus line numbers. Breadcrumb and gutter draw above code for clarity.
- Breadcrumb bar per window (relative path + module path hint).
- Sidebar with file list and live search; search results include matching definitions you can jump to.
- Mouse-wheel scrolling per window; layout (position/size/scroll) is persisted to `.trace_viewer_layout.json` in the project root.
- Uses a real monospaced font when found (`TRACE_VIEWER_FONT` env override, bundled fonts under `data/fonts/`, Consolas/DejaVu/Meslo); falls back to raylib's default if none found.
- Horizontal scrolling for long lines (hold Shift + scroll while hovering a window).
- Zoom (Ctrl + wheel) centers on the cursor; space + drag or middle-drag to pan. Double-click a single-function window to open the full file at that definition.
- Connection lines show call relationships between windows, drawn behind windows with a bias to exit/enter from window edges; links are hoverable/clickable only when not covered by a window.

## Controls
- Left click: focus window/title bar; drag title to move; click close icon to close.
- Scroll wheel: scroll code in the hovered window. Shift + wheel scrolls horizontally.
- Middle drag or hold Space + drag: pan the world.
- Sidebar: click files to open; type in the search bar (click to focus) to filter files and defs.
- Code: click a highlighted function/method/type name to open its definition in a new window.
- Double-click a single-function/type window: open the full file scrolled to that definition.

## Running
```bash
cargo run -- .          # or pass another project root
```
Requires a desktop with raylib-compatible graphics. Tested on Rust 1.78+.

## Notes and limits
- Parsing uses a plugin-based Tree-sitter pipeline (Rust, Python, JavaScript, TypeScript/TSX) and a fallback for unknown files. Rust spans include basic module hints; full type/trait resolution is not implemented. If multiple defs share a name, the closest module match is preferred.
- Syntax highlighting is lightweight (keywords/strings/comments + call highlights), not a full lexer.
- Module breadcrumbs use the first definition's module path as a hint; nested module files declared externally aren't expanded yet.
- Layout persistence stores relative paths only; deleting/renaming files will drop those windows on next load.

## Next ideas
- Richer analysis (trait/impl resolution, `use` following, cross-language symbol links).
- Better highlight (full lexer), minimap polish, and jump history/back stack.
- Keyboard shortcuts for search, next/prev result, and window focus.
