# Agent Notes
- Entry point: `src/main.rs`. UI built with `raylib` (WeakFont for measurements).
- Parsing: `ProjectModel` uses a Tree-sitter plugin pipeline (Rust, Python, JS, TS/TSX; fallbacks for others) to build `ParsedFile` (lines, defs, calls) and a `defs` index keyed by name. Syntax highlighting uses `syntect`. Module hints come from file stems; no full resolution.
- Data flow: `AppState` owns windows, search state, and persists layouts to `.trace_viewer_layout.json` in the project root. `find_function_span` groups Rust types with their `impl` blocks.
- UI: Sidebar lists files (filtered by search) and matching defs. Code windows are draggable, have a breadcrumb bar (path + module hint), syntax highlighting (keywords/strings/comments + call highlights), and clickable call/type names that open definitions. Scroll is per-window; zoom is camera-based.
- Input: Mouse wheel scrolls; Shift + wheel scrolls horizontally. Ctrl + wheel zooms around cursor; middle drag or Space + drag pans. Sidebar search requires click to focus; typing is only consumed when focused. Backspace handled per-frame. Double-clicking a SingleFn/Single type window opens the full file at that definition.
- Connections: Windows draw bezier call links behind content; call hits prefer same-module definitions. Windows cache def/call refs for connection resolution. Connections clamp to window bounds and support multiple inbound/outbound links.
- Fonts: `load_monospace_font` tries `TRACE_VIEWER_FONT`, `assets/JetBrainsMono-Regular.ttf`, common OS monospace fonts, then falls back to raylib default. `AppFont` abstracts owned vs default fonts for draw/measure.
- Persistence/theme: Color/theme constants live near the top; spacing/highlight colors centralized. Layout persistence stores relative paths.
- Known gaps: no full module/trait resolution; external `mod` files aren’t parsed; no keyboard shortcuts/backstack; highlighting is minimal.
