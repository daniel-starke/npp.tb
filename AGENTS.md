# AGENTS.md

Guidance for AI agents and contributors working on this repository.

## Project

NppToolBox (repository `npp.tb`) is a Notepad++ 8.9.8 plugin for Windows x86 and x64, written in
Rust. MIT licensed, author Daniel Starke.

## Layout

| Path                                    | Purpose                                                                    |
|-----------------------------------------|----------------------------------------------------------------------------|
| `crates/npp`                            | Reusable plugin framework: ABI bindings, Notepad++/Scintilla wrappers, UI. |
| `crates/npp/src/interface.rs`           | **Generated** Notepad++/Scintilla constants of one Notepad++ release.      |
| `crates/npp-build`                      | Build script helper: icon style check, SVG icon rendering, CSS, `windres`. |
| `crates/npp-toolbox`                    | The plugin (`NppToolBox.dll`): menu definition, commands, editing logic.   |
| `crates/npp-toolbox/icons`              | SVG master icons (one per toolbar button) and the icon style guide.        |
| `crates/npp-toolbox/assets`             | Vendored `github-markdown-light.css`, minified into the DLL at build time. |
| `crates/npp-toolbox/NppToolBox.rc`      | Version resource of the DLL, compiled by MinGW-w64 `windres` in build.rs.  |
| `crates/npp-toolbox/src/emoji_table.rs` | **Generated** GitHub emoji shortcodes (`cargo xtask update-emoji`).        |
| `crates/xtask`                          | Development tasks run with `cargo xtask` (interface/emoji update, icons).  |

Inside `npp-toolbox`, the editing logic (`markdown_table.rs`, `rewrap.rs`, `text_width.rs`) and the
HTML rendering (`markdown_html/`, `code_highlight.rs`, `image_embedding.rs`, `clipboard_html.rs`,
`emoji.rs`, `settings.rs`) are pure Rust without Win32 code. `commands.rs` and
`lexer_highlighter.rs` connect them to the editor and are `#[cfg(windows)]`.

`markdown_html` renders GFM with `pulldown-cmark` and its own HTML writer (GitHub extensions, inline
styles for the clipboard). LaTeX math is converted by the own `markdown_html/math.rs`;
`pulldown-latex` was rejected because malformed formulas panic or overflow the stack in it. Code
blocks are colored with Notepad++'s Lexilla lexers in a hidden Scintilla editor.

## Build and test

```sh
cargo test --workspace                                   # any host; tests the pure logic
cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
cargo build-x64                                          # or build-x86; aliases in .cargo/config.toml
```

The GNU toolchain (MinGW-w64, `*-pc-windows-gnu` targets) is the only supported toolchain, on every
host and in CI. Do not add MSVC specific configuration, flags or instructions. Linkers and the
static CRT are configured in `.cargo/config.toml`, targets in `rust-toolchain.toml`. Install by
copying the DLL to `<Notepad++>/plugins/NppToolBox/NppToolBox.dll`; the folder name must match the
DLL name. The architecture must match the Notepad++ build.

Development tasks:

```sh
cargo xtask update-npp-interface [--version 8.9.8]   # default: newest Notepad++ release
cargo xtask update-emoji [--revision master]         # gemoji shortcodes -> emoji_table.rs
cargo xtask icon-preview                             # target/icon-preview.png + style violations
```

Before finishing a change run `cargo fmt --all`, `cargo test --workspace` and clippy for both GNU
targets. Keep all of them warning free.

## Hard rules

- **Dependencies:** keep runtime dependencies minimal (currently `windows-sys`, `unicode-width` and
  `pulldown-cmark` without default features). Build dependencies (`resvg` via `npp-build`) may be
  chosen more freely. Enable only the `windows-sys` features that are actually used.
- **No unwinding, no backtraces:** all profiles use `panic = "abort"`. Do not add `catch_unwind`,
  `std::backtrace`, backtrace crates or unwinding based error handling. The panic hook in
  `npp/src/panic_message.rs` only shows the message and location before the process aborts.
- **No panics across the ABI:** handle editor states (read-only document, no table, empty selection,
  …) with early returns or user messages, not `unwrap`. Parsers of user input (Markdown, LaTeX, raw
  HTML, XML configuration) must accept any input without panicking and without unbounded recursion;
  keep the random input tests in `markdown_html/tests.rs` passing (`RANDOM_RENDER_ITERATIONS=1000000
  cargo test --release -p npp-toolbox renders_random`).
- **Icons:** all toolbar icons follow the common style `npp_build::icon_style::DEFAULT` (Notepad++
  Fluent outline). See `crates/npp-toolbox/icons/README.md`. The build rejects violations. Do not
  loosen the style to make an icon pass; fix the icon or ask first. Never commit generated bitmaps.
- **Generated interface:** never edit `crates/npp/src/interface.rs` by hand. Regenerate it with
  `cargo xtask update-npp-interface`. `ffi.rs` refers to it by the official header names, so a
  removed or renamed constant fails to compile. If the tool warns about changed struct declarations,
  review the hand written `#[repr(C)]` structs in `ffi.rs`. Only names and values are taken from the
  GPL licensed Notepad++ headers; do not commit the headers themselves.
- **Version resource:** `NppToolBox.rc` gets its version numbers from `Cargo.toml` through the
  generated `version.h`. Keep its copyright line in sync with the About text in `commands.rs`.
- **Static CRT:** the DLL must not depend on non-system runtime DLLs such as `libgcc_s_*.dll` or
  `libwinpthread-1.dll` (see `.cargo/config.toml`; CI checks the imports). `bcryptprimitives.dll`
  (system, used by `std`'s `HashMap` seed in `pulldown-cmark`) is fine.
- **Generated emoji table:** never edit `emoji_table.rs` by hand; run `cargo xtask update-emoji`.
- **Third-party notices:** list embedded third-party data and runtime crates in
  `THIRD-PARTY-NOTICES.md`.

## Code style

- Indent with tabs (`rustfmt.toml` has `hard_tabs = true`).
- Use speaking names so the code documents itself. Prefer `line_of_position` over `lfp`, and
  `MenuEntry::Separator` over magic values.
- Comment only what the code cannot say: non-obvious reasons, Win32 or Notepad++ contracts, memory
  layout requirements and every `unsafe` block (`// SAFETY: ...`).
- Match the surrounding code's idioms and comment density.
- Markdown tables (docs and the table realign output): no spaces in delimiter rows, e.g.
  `|---|:--:|` instead of `| --- | :--: |`.
- Put new logic in pure modules with unit tests whenever it does not need Win32.

## Notepad++ / Win32 contracts worth knowing

- New Notepad++ or Scintilla messages: add a speaking-name constant to `ffi.rs` that refers to the
  generated official constant (e.g. `npp::NPPM_GETEXTPART`); never hard code numbers.

- Notepad++ calls all exported functions and menu commands on its UI thread.
- `getFuncsArray` must return an array that stays alive, at a fixed address, for the DLL lifetime.
  Notepad++ writes the command IDs into it. Shortcut keys are kept by pointer as well.
- `FuncItem` callbacks carry no context. `npp::plugin` maps each menu index to a monomorphized
  trampoline (at most `MAX_MENU_ENTRIES`).
- Toolbar icons are registered on `NPPN_TBMODIFICATION` with `NPPM_ADDTOOLBARICON_FORDARKMODE`. The
  bitmap is used by the "Standard icons" set and is pre-composited onto the button face color. The
  two icons serve the Fluent sets (light and dark mode). Notepad++ keeps the handles.
- Scintilla positions are byte offsets in the document encoding (UTF-8 or ANSI code page).
  `npp::Scintilla` converts text at the boundary. Use character offsets (`character_column`,
  `position_at_character_column`) when mapping carets from pure logic.
- Dialogs are built from in-memory `DLGTEMPLATE`s (`npp/src/dialog_template.rs`). The only resource
  script is the version information in `NppToolBox.rc`; keep it that way. Call
  `Notepad::apply_theme_to_dialog` in `WM_INITDIALOG` for dark mode.

## Adding a command

1. Implement the logic in a pure module with tests.
2. Add a `pub fn name(notepad: &Notepad)` in `commands.rs`.
3. Add a `MenuEntry::Command` to `PLUGIN` in `lib.rs` (menu name, shortcut, optional icon).
4. For a toolbar icon add `icons/<name>.svg` following the icon style and check it with `cargo xtask
   icon-preview`. It is available as `toolbar_icons::<NAME>`.
5. Document the feature in `README.md`. New settings go into `settings.rs` (`settings::ALL`), which
   also writes the commented settings file of the Settings command.
