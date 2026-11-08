# AGENTS.md

Guidance for AI agents and contributors working on this repository.

## Project

NppToolBox (repository `npp.tb`) is a Notepad++ 8.9.8 plugin for Windows x86 and x64, written in
Rust. MIT licensed, author Daniel Starke.

## Layout

| Path                                    | Purpose                                                                    |
|-----------------------------------------|----------------------------------------------------------------------------|
| `crates/npp`                            | Reusable plugin framework: ABI bindings, Notepad++/Scintilla wrappers, UI. |
| `crates/npp/src/interface.rs`           | **Generated** Notepad++/Scintilla constants and `LangType` names.          |
| `crates/npp-build`                      | Build script helper: icon style check and recoloring, CSS, `windres`.      |
| `crates/npp-toolbox`                    | The plugin (`NppToolBox.dll`): menu definition, commands, editing logic.   |
| `crates/npp-toolbox/icons`              | SVG master icons (one per toolbar button) and the icon style guide.        |
| `crates/npp-toolbox/assets`             | Vendored `github-markdown-light.css`, minified into the DLL at build time. |
| `crates/npp-toolbox/NppToolBox.rc`      | Version resource of the DLL, compiled by MinGW-w64 `windres` in build.rs.  |
| `crates/npp-toolbox/src/emoji_table.rs` | **Generated** GitHub emoji shortcodes (`cargo xtask update-emoji`).        |
| `crates/npp-toolbox/src/mermaid/`       | Own Mermaid renderer (parsers, layouts, SVG), pure Rust.                   |
| `.../mermaid/theme_table.rs`            | **Generated** Mermaid themes as bytecode (`update-mermaid-themes`).        |
| `.../mermaid/icon_table.rs`             | **Generated** Font Awesome icons as bytecode (`update-fontawesome`).       |
| `crates/xtask`                          | Development tasks run with `cargo xtask` (build, interface/emoji, icons).  |

Inside `npp-toolbox`, the editing logic (`markdown_table.rs`, `rewrap.rs`, `text_width.rs`,
`comment_selection.rs`), the Markdown styles of the editor (`markdown_lexer.rs`), the HTML rendering (`markdown_html/`, `code_highlight.rs`,
`image_embedding.rs`, `clipboard_html.rs`, `emoji.rs`, `settings.rs`), the RTF rendering for
RichEdit (`markdown_rtf.rs`), the viewer page, scripts and link mapping (`viewer_page.rs`) and the
SMIL to CSS conversion of SVG images for EdgeHTML (`svg_animation.rs`) and the Mermaid diagrams
(`mermaid.rs`, `mermaid/`) are pure Rust without Win32 code. `commands.rs`, `lexer_highlighter.rs`,
the editor lexer (`editor_lexer.rs`) and the Markdown viewer (`viewer.rs`, `viewer/`)
connect them to the editor and are `#[cfg(windows)]`. The framework crate `npp` provides the
Lexilla lexer interfaces (`lexer.rs`: `ILexer5` in Rust, `IDocument`, foreign lexers), the
WebView2 binding (`webview2.rs`, `webview2/`), the EdgeHTML binding (`edge_html.rs`,
`edge_html/`), the RichEdit control (`rich_edit.rs`), the SVG rendering with `resvg` (`svg.rs`) and
text measurement and glyph outlines with DirectWrite (`direct_write.rs`, `direct_write/`) and
image decoding and PNG encoding with WIC (`wic.rs`, `wic/`).

Icons are SVG and rendered at runtime in the size they are shown with: toolbar icons by
`npp::svg::render_bgra`, the alert icons as SVG `data:` images in HTML and as PNG pictures
(`npp::svg::render_png`) in RTF. RichEdit cannot show SVG. Office applications and LibreOffice drop
inline `<svg>` elements, so the HTML writer does not use them.

`markdown_html` renders GFM with `pulldown-cmark` and its own HTML writer (GitHub extensions, inline
styles for the clipboard). LaTeX math is converted by the own `markdown_html/math.rs` (macros in
`math_macros.rs`, equation numbers and references of a document in `math_equations.rs`);
`pulldown-latex` was rejected because malformed formulas panic or overflow the stack in it. Code
blocks are colored with Notepad++'s Lexilla lexers in a hidden Scintilla editor.

Mermaid diagrams are rendered by the own `mermaid` module, following Mermaid 12.2.0 (its syntax
documentation, and its JavaScript implementation in case of doubt). The SVG uses presentation
attributes instead of a style sheet, so that browsers and resvg draw it alike: inline in the viewer,
as PNG in RichEdit and in Copy as HTML (Office drops inline SVG). Text is measured through the
`TextMeasure` trait with DirectWrite, the text engine of WebView2 and EdgeHTML, so labels fit the
text the browsers draw. resvg is built without text support: for PNGs, every `<text>` written by
`mermaid::svg::Svg::text` is replaced by DirectWrite's glyph outlines (`TextOutline`). Themes run Mermaid's own theme code, compiled to
bytecode by `cargo xtask update-mermaid-themes`, so the colors match Mermaid exactly.

## Build and test

```sh
cargo test --workspace                                   # any host; tests the pure logic
cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
cargo build-x64                                          # or build-x86; runs `cargo xtask build`
cargo deploy-x64                                         # or deploy-x86; build, import check, package
```

The GNU toolchain (MinGW-w64, `*-pc-windows-gnu` targets) is the only supported toolchain, on every
host and in CI. Do not add MSVC specific configuration, flags or instructions. Linkers and the
static CRT are configured in `.cargo/config.toml`, targets in `rust-toolchain.toml`. Without
`<arch>-w64-mingw32-gcc`, `cargo xtask build` links with a multilib MinGW-w64 `gcc -m64`/`-m32`
(`crates/xtask/src/plugin_build.rs`), including the fixes for its SJLJ 32-bit `libgcc`. Install by
copying the DLL to `<Notepad++>/plugins/NppToolBox/NppToolBox.dll`; the folder name must match the
DLL name. The architecture must match the Notepad++ build.

Development tasks:

```sh
cargo xtask build <x64|x86>                          # release DLL, see linker detection above
cargo xtask deploy <x64|x86>                         # build + import check -> target/<target>/package
cargo xtask update-npp-interface [--version 8.9.8]   # default: newest Notepad++ release
cargo xtask update-emoji [--revision master]         # gemoji shortcodes -> emoji_table.rs
cargo xtask update-mermaid-themes [--version 12.2.0] # Mermaid themes -> mermaid/theme_table.rs
cargo xtask update-fontawesome [--version 6.7.2]     # Font Awesome Free -> mermaid/icon_table.rs
cargo xtask icon-preview                             # target/icon-preview.png + style violations
```

Before finishing a change run `cargo fmt --all`, `cargo test --workspace` and clippy for both GNU
targets, also with `--features npp-toolbox/pure-rust` and with `-p npp-toolbox
--no-default-features` (the feature `font-awesome`, on by default, holds the icon data: `cargo xtask
build x64 --no-default-features` leaves it out). Keep all of them warning free.

## Hard rules

- **Dependencies:** keep runtime dependencies minimal (currently `windows-sys`, `windows-core` for
  the COM interfaces of WebView2 and EdgeHTML, `unicode-width`, `pulldown-cmark` and `resvg` for
  icons and diagrams, without default features). Raster images are decoded and PNG is encoded
  with the Windows Imaging Component (`npp/src/wic.rs`); `npp/src/svg/wic_images.rs` draws the
  image nodes resvg skips without its `raster-images` feature. The feature `pure-rust` restores
  resvg's PNG, JPEG, GIF and WebP decoders and tiny-skia's PNG encoder instead; keep both
  configurations building, tested and clippy clean. The main goal is a small DLL: prefer Windows
  components over crates. Text is measured and outlined with DirectWrite, not with resvg's text
  features. Build
  dependencies may be chosen more freely. Enable only the `windows-sys` features that are actually
  used. Items missing in `windows-sys` (e.g. RichEdit) are declared next to their use with the name
  of their SDK header.
- **No unwinding, no backtraces:** all profiles use `panic = "abort"`. Do not add `catch_unwind`,
  `std::backtrace`, backtrace crates or unwinding based error handling. The panic hook in
  `npp/src/panic_message.rs` only shows the message and location before the process aborts.
- **No panics across the ABI:** handle editor states (read-only document, no table, empty selection,
  …) with early returns or user messages, not `unwrap`. Parsers of user input (Markdown, LaTeX, raw
  HTML, XML configuration) must accept any input without panicking and without unbounded recursion;
  keep the random input tests in `markdown_html/tests.rs` passing (`RANDOM_RENDER_ITERATIONS=1000000
  cargo test --release -p npp-toolbox renders_random`).
- **Incremental Markdown lexer:** `markdown_lexer/incremental.rs` parses only from the restart
  line before a change to the one after the requested range, and must style exactly like parsing
  the whole document. Its module doc lists what crosses the parsed part (definitions, unclosed
  blocks, metadata blocks) and the `pulldown-cmark` quirks it avoids. Keep the random edit test
  passing (`RANDOM_LEXER_ITERATIONS=1000000 cargo test --release -p npp-toolbox
  styles_like_a_full_lexing`); a failure prints the edits that reproduce it.
- **Icons:** all toolbar icons follow the common style `npp_build::icon_style::DEFAULT` (Notepad++
  Fluent outline). See `crates/npp-toolbox/icons/README.md`. The build rejects violations. Do not
  loosen the style to make an icon pass; fix the icon or ask first. Never commit generated bitmaps.
- **Generated interface:** never edit `crates/npp/src/interface.rs` by hand. Regenerate it with
  `cargo xtask update-npp-interface`. `ffi.rs` refers to it by the official header names, so a
  removed or renamed constant fails to compile. If the tool warns about changed struct declarations,
  review the hand written `#[repr(C)]` structs in `ffi.rs`. Only names and values are taken from the
  GPL licensed Notepad++ headers and `ScintillaEditView.cpp` (the `langs.xml` names of `LangType`);
  do not commit these sources themselves.
- **Version resource:** `NppToolBox.rc` gets its version numbers from `Cargo.toml` through the
  generated `version.h`. Keep its copyright line in sync with the About text in `commands.rs`.
- **Static CRT:** the DLL must not depend on non-system runtime DLLs such as `libgcc_s_*.dll` or
  `libwinpthread-1.dll` (see `.cargo/config.toml`; `cargo xtask deploy`, run by CI, checks the
  imports against `SYSTEM_DLLS` in `crates/xtask/src/deploy.rs`). `bcryptprimitives.dll` (`std`'s
  `HashMap` seed in `pulldown-cmark`), `advapi32.dll` (WebView2 runtime lookup),
  `ole32.dll`/`oleaut32.dll` (COM), `shell32.dll` (opening links), `dwmapi.dll` (dark title bar)
  and `dwrite.dll` (text of diagrams) are system DLLs and fine.
  `WebView2Loader.dll` is only loaded dynamically when present, never linked. `RoActivateInstance`
  is loaded from `combase.dll` at runtime, so the plugin does not import the WinRT API set.
- **Generated emoji table:** never edit `emoji_table.rs` by hand; run `cargo xtask update-emoji`.
- **Generated Mermaid themes:** never edit `mermaid/theme_table.rs` by hand; run `cargo xtask
  update-mermaid-themes`. The compiler rejects JavaScript it does not know instead of guessing.
- **Generated Font Awesome icons:** never edit `mermaid/icon_table.rs` by hand; run `cargo xtask
  update-fontawesome`. The tool fails on metadata it does not know. Only `mermaid/icons.rs` reads
  the table, behind the feature `font-awesome`.
- **Mermaid parsers** follow the robustness rule of user input above and keep the random input
  test in `mermaid/tests.rs` passing.
- **`MERMAID.md`** states the support level, the differences from Mermaid and the odd behaviors
  of Mermaid that are reproduced on purpose. Update it with every Mermaid change; list a quirk
  only after checking it against Mermaid's own render.
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
- Sort with `npp::sort::sort_by`/`sort_by_key`, not `slice::sort_by`/`sort_by_key`: the standard
  sort is compiled again (about 6 KB) for every element type and closure. `sort()` and
  `sort_unstable()` of numbers and strings are shared and fine.
- Check the DLL size after structural changes. Under fat LTO a single `dyn` trait object changed
  LLVM's inlining across the whole plugin (+400 KB); the flowchart parser dispatches its two
  databases with an enum instead.
- `CHANGELOG`: one block per release, newest first, headed `M.N.P (YYYY-MM-DD)` with the version of
  `Cargo.toml`. Below it one line per change, short and from the user's point of view, grouped in
  this order: ` - added: `, ` - changed: `, ` - fixed: `, ` - removed: `. New features raise the
  minor version, bug fixes alone the patch version.
- Never name a file `mod.rs`. A module with submodules is `name.rs` next to a `name/` directory,
  so that editor tabs stay distinguishable.

## Adding a Mermaid diagram

1. Read `MERMAID.md` first: the support level, the differences from Mermaid and the reproduced
   quirks of the diagrams already done.
2. Port parser, db, layout and renderer into `mermaid/<name>.rs` (+ `mermaid/<name>/`); the module
   doc names the Mermaid files it follows. Reuse `jison.rs`/`langium.rs` (lexer helpers),
   `common.rs`, `label.rs` (HTML labels), `rendering/` (shapes, edges, markers, clusters, `BBox`
   as Blink's `getBBox`) and `layout/` (graphlib, dagre). Add `mod` and a dispatch arm in
   `mermaid.rs::render`.
3. Tests: unit tests (`text::testing::FixedMeasure`), `renders_<name>_diagrams` and
   `renders_random_<name>_diagrams_without_panicking` in `mermaid/tests.rs`.
4. Verify against Mermaid's own render: Mermaid's e2e inputs (`e2e/diagrams/<type>/*.mmd` at tag
   `mermaid@12.2.0`) rendered with `mermaid.min.js` in headless Edge (`msedge --headless
   --dump-dom`, one page per diagram where Mermaid keeps module state), compared with own SVGs from
   `MERMAID_DUMP_DIR=<dir> cargo test --release -p npp-toolbox --lib dump_diagrams -- --ignored`
   (geometry, viewBox, computed paint). Numerical layout ports are checked against the real JS
   libraries run by Node on random inputs (JSONL fixtures, see `layout/dagre/fixtures.jsonl`).
5. Update `MERMAID.md`; smoke-test in Notepad++ with WebView2, EdgeHTML and RichEdit.

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
  two icons serve the Fluent sets (light and dark mode). Notepad++ keeps the handles. All three
  are rendered from the themed SVG in the DPI scaled toolbar size.
- Scintilla positions are byte offsets in the document encoding (UTF-8 or ANSI code page).
  `npp::Scintilla` converts text at the boundary. Use character offsets (`character_column`,
  `position_at_character_column`) when mapping carets from pure logic.
- The plugin is also a Lexilla lexer library (`GetLexerCount`, `GetLexerName`, `CreateLexer`,
  `__stdcall`). Because it exports them, Notepad++ refuses to load the plugin without
  `plugins/Config/NppToolBox.xml` (program directory or `NPPM_GETPLUGINSCONFIGDIR`), so
  `editor_lexer::lexers` creates it. Notepad++ applies only that file's styles to an external
  lexer (after `SCI_STYLECLEARALL`); the code block styles from 40 on are set by the plugin after
  each `NPPN_BUFFERACTIVATED`, `NPPN_LANGCHANGED`, `NPPN_WORDSTYLESUPDATED` and
  `NPPN_DARKMODECHANGED`, deferred with a thread timer until Notepad++ has finished.
- Scintilla calls `ILexer5::Lex` from the start of the first line that changed, so line states
  before it are valid; the Markdown lexer keeps its restart lines there (bit 1). It reads text
  with `IDocument::GetCharRange`, not `BufferPointer`, which would move the gap of Scintilla's
  buffer to the end and back on every edit. `IDocument::ChangeLexerState` makes Scintilla style
  everything again when it is shown (used when link or footnote definitions change).
- Dialogs are built from in-memory `DLGTEMPLATE`s (`npp/src/dialog_template.rs`). The only resource
  script is the version information in `NppToolBox.rc`; keep it that way. Call
  `Notepad::apply_theme_to_dialog` in `WM_INITDIALOG` for dark mode.
- Docking panels (`NPPM_DMMREGASDCKDLG`): `DockedWidgetData::dialog_id` is the menu index that
  Notepad++ runs on startup to restore a panel left open (not with `-nosession`). Closing a panel
  sends `WM_NOTIFY`/`DMN_CLOSE` to the client; a non-zero result vetoes it. The viewer sets
  `DWS_USEOWNDARKMODE` and themes its host window itself, because the host moves between the panel
  and the separate window.
- The viewer state is a thread local `RefCell`. Win32 and Notepad++ calls (creating, showing or
  reparenting windows, docking messages, opening files, modal menus) send messages back to the
  viewer windows, so never hold the borrow across them. Window procedures use `try_borrow_mut`
  (`with_viewer`) and defer or skip work when it is busy; a borrow panic aborts Notepad++.

## WebView2, EdgeHTML and RichEdit facts

- The WebView2 interfaces are declared in `npp/src/webview2/interfaces.rs` with the `windows-core`
  macros, in vtable order of `WebView2.h`. Hand-counted vtable offsets exclude the three `IUnknown`
  methods.
- All engines share the viewer's own context menu (`viewer/window.rs`). WebView2 hands its menu
  over as `webview2::ContextMenuRequest` (`Handled`, a deferral and `SelectedCommandId`), so Copy
  and Select All still run Chromium's commands.
- Without `WebView2Loader.dll` (not part of Windows, its static library is MSVC only) the installed
  runtime is loaded like the loader does: EdgeUpdate `ClientState` key, value `EBWebView`, then
  `EBWebView\<arch>\EmbeddedBrowserWebView.dll!CreateWebViewEnvironmentWithOptionsInternal`.
- `IsScriptEnabled = FALSE` blocks page scripts only; `ExecuteScript` still runs. A page from
  `NavigateToString` cannot navigate to `file:` URLs, WebView2 blocks them without
  `NavigationStarting`. Relative links therefore resolve against `http://npp-toolbox.invalid/<dir>/`.
  Cancelled navigations raise `NavigationCompleted` too.
- EdgeHTML is `Windows.Web.UI.Interop.WebViewControl` (out of process in `Win32WebViewHost`, no
  XAML). `Windows.UI.Xaml.Controls.WebView` needs XAML Islands, which refuse to start without
  `maxversiontested` in the manifest of the exe; Notepad++ has none. The `windows-core` interface
  macro cannot derive from `IInspectable`, so the WinRT interfaces in `npp/src/edge_html/interfaces.rs`
  derive from `IUnknown` and list the three `IInspectable` methods first. IIDs of parameterized
  interfaces are computed from their WinRT signature and documented next to them.
- EdgeHTML's `InvokeScriptAsync("eval", ...)` needs JavaScript enabled, but the page's content
  security policy (`default-src 'none'`) blocks the page's own scripts and handlers and does not
  block the host's `eval`. `Bounds` are pixels of the host window. The control applies the DPI of
  its host window itself, so `Scale` stays 1. The control follows its host window when that is
  reparented. It works in DPI-unaware processes. With "System (Enhanced)" DPI scaling
  (`GdiDpiScaling`) its content is drawn at half its position and size while its input window
  stays in place, and every clip affects both. There the control lives in a per-monitor DPI aware
  top-level window (`edge_html/overlay.rs`) kept over the host. The control moves its window in
  the plugin's process with the caller's thread DPI context, so call it in the per-monitor context
  there. Calls into the control pump messages and can nest window event callbacks, so set the
  thread DPI context explicitly and serialize the overlay updates.
- Creating the EdgeHTML control while the UI thread handles a message sent by another thread or
  process fails fast in `EdgeManager.dll` (`RPC_E_CANTCALLOUT_ININPUTSYNCCALL`), so
  `create_edge_view` calls `ReplyMessage` first.
- The EdgeHTML control shows no context menu. A `contextmenu` listener added by the host's script
  (`viewer_page::CONTEXT_MENU_SCRIPT`) reports the selection and link with
  `window.external.notify` (`ScriptNotify`), and the viewer shows its own menu. EdgeHTML plays CSS
  animations in SVG `<img>` images but no SMIL, hence `svg_animation.rs`.
- `Win32WebViewHost` is one DCOM server per client process, shared by its controls. It never exits
  by itself: the app lifecycle manager suspends it once it shows no control, and `Terminate` only
  ends the content process `WWAHost`. `edge_html/host_process.rs` finds it as the owner of the
  foreign `CoreWindow` below the control's `CoreApplicationBridgeWindow` and terminates it with
  the last view.
- RichEdit (`msftedit.dll`) cannot import HTML (`tomHTML` is `E_NOTIMPL`), hence `markdown_rtf.rs`.
  A read-only RichEdit drops the pictures of inserted RTF, `\picw`/`\pich` must be HIMETRIC, and
  paragraph borders are not drawn (rules are bordered table cells). `EN_LINK` reports the URL inside
  the hidden field instruction, not the link text. It reports hovering a link as `EN_LINK` with
  `WM_SETCURSOR`, and never the mouse leaving it. The character counting of `markdown_rtf.rs` is
  documented there and checked by its tests.
- The hovered link is shown in the viewer's own link bar. Notepad++ draws its status bar itself
  and ignores `SB_SIMPLE`, and its only free section (`STATUSBAR_DOC_TYPE`) gets just the width
  the fixed sections leave. WebView2 raises `StatusBarTextChanged` despite
  `IsStatusBarEnabled = FALSE`; its text may lack the `http://` of the address. EdgeHTML reports
  the link with `viewer_page::LINK_HOVER_SCRIPT`.

## Adding a command

1. Implement the logic in a pure module with tests.
2. Add a `pub fn name(notepad: &Notepad)` in `commands.rs`.
3. Add a `MenuEntry::Command` to `PLUGIN` in `lib.rs` (menu name, shortcut, optional icon).
4. For a toolbar icon add `icons/<name>.svg` following the icon style and check it with `cargo xtask
   icon-preview`. It is available as `toolbar_icons::<NAME>`. Add its `[Toolbar]` setting to
   `settings::TOOLBAR_SETTINGS`; the key is the menu name in camel case (`settings::toolbar_key`).
5. Document the feature in `README.md`. New settings go into `settings.rs` (`settings::ALL`), which
   also writes the commented settings file of the Settings command.
