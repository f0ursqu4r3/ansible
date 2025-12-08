# Agent Notes
- Entry point: `src/main.rs`. UI built with `raylib` (WeakFont for measurements). Project parsing uses `walkdir` + `syn` (with `proc-macro2` span locations). No regex parsing remains.
- Data flow: `ProjectModel` loads `.rs` files, builds `ParsedFile` (lines, defs, calls) and a `defs` index keyed by name. `AppState` owns windows, search state, and persists layouts to `.trace_viewer_layout.json` in the project root.
- UI: Sidebar lists files (filtered by search) and matching function defs. Code windows are draggable, have a breadcrumb bar (path + module hint), syntax highlighting (keywords/strings/comments + call highlights), and clickable call names that open definitions. Scroll is per-window.
- Parsing: `SyntaxCollector` visits functions, impl fns, modules, `ExprCall`, and method calls. Calls/defs record module paths and line/col via `Span::start()`. Call hits prefer defs in the same module path.
- Persistence: `SavedLayout` serializes window position/size/scroll using relative paths. Saved on exit (`app.save_layout()`), loaded on startup.
- Color/theme: see top constants for palette and geometry. Highlight colors and spacing are centralized near constants.
- Input: `collect_typed_chars` loops `get_char_pressed()`. Sidebar search requires click to focus; typing is only consumed when focused. Backspace handled per-frame.
- Known gaps: no full module/trait resolution; external `mod` files aren’t parsed; no keyboard shortcuts/backstack; highlighting is minimal. Call spans rely on span lengths; unexpected spans fall back to name length.
