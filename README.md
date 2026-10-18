# NPP.TB

Notepad++ ToolBox (`NppToolBox`). A plugin with helper functions for Notepad++ 8.9.8 or newer, for
x86 and x64.

- **Realign Markdown Table** (`Ctrl+Alt+T`, toolbar): pads all cells of the table at the cursor to
  equal column widths and honors the `:--`, `--:` and `:-:` alignment markers.
- **Rewrap…** (`Ctrl+Alt+W`, toolbar): reflows the selected lines, or the paragraph at the cursor,
  to a maximum number of characters per line. The common line prefix is kept.
- **Render HTML** (`Ctrl+Alt+H`, toolbar): renders the selected Markdown, or the whole document,
  into a new unsaved HTML document that looks like GitHub.
- **Copy as HTML** (`Ctrl+Alt+Shift+H`, toolbar): like Render HTML, but copies the result to the
  clipboard as HTML, together with the Markdown as plain text.
- **Markdown Viewer** (`Ctrl+Alt+M`, toolbar): shows or hides a live preview of the active document,
  docked as side panel or in a separate window, optionally following the cursor.
- **Settings**: opens the settings file in Notepad++.

## Installation

1. Close Notepad++.
2. Get `NppToolBox.dll` for the architecture of your Notepad++ (x64 or x86, see `?` > `About
   Notepad++`), either as a prebuilt package or built from source (see [Build
   Instruction](#build-instruction)).
3. Copy it to `<Notepad++>/plugins/NppToolBox/NppToolBox.dll`. The folder name must match the DLL
   name.
4. Start Notepad++. The commands are in the `Plugins` > `NppToolBox` menu.

## Usage Instruction

### Realign Markdown Table

Place the cursor anywhere in a GitHub Flavored Markdown table and run the command.

- Column widths use the display width, so CJK characters count as two columns.
- Delimiter rows are written without spaces, e.g. `|:----|----:|`.
- Leading and trailing pipes are kept as in the header row. They are added only when needed: for
  empty first or last cells and for single column tables.
- Indentation and blockquote prefixes (`> | a | b |`) are kept.
- Escaped pipes (`\|`) stay inside their cell. Like GFM, an unescaped pipe in a code span splits the
  cell.

### Rewrap

Select lines, or place the cursor in a paragraph and run the command. A dialog asks for the maximum
number of characters per line. The last value is remembered in `<plugins config
dir>/NppToolBox.ini`.

- Kept prefixes: indentation, line comments (`//`, `///`, `//!`, `#`, `--`, `;`, `%`, `'`, `REM`,
  `::`), block comment bodies (` * `), blockquotes (`>`, nested) and their combinations.
- Markdown list items (`-`, `*`, `+`, `1.`, `1)`, task boxes `[ ]`) wrap with a hanging indent.
- Never reflowed: fenced code blocks, indented code (Markdown files), tables, headings, thematic
  breaks, HTML blocks, link reference definitions and block comment delimiter lines (`/*`, `*/`).
- Hard line breaks (trailing `\` and two trailing spaces in Markdown files) are preserved.
- Files with the extension `.md`, `.markdown`, `.mdown`, `.mkd`, `.mkdn`, `.mdwn` or `.mdtxt` are
  treated as Markdown: comment markers are not recognized and `#` starts a heading. All other files
  are treated as code or plain text.
- Words longer than the line width are not split.

### Render HTML and Copy as HTML

Select Markdown, or select nothing to use the whole document and run the command. The Markdown
flavor is GitHub's ([GFM](https://github.github.com/gfm/), as implemented by
[cmark-gfm](https://github.com/github/cmark-gfm)):

- CommonMark with tables, task lists, strikethrough, footnotes, autolinks for bare `www.` and
  `http(s)://` addresses and e-mail addresses and the filter for unsafe raw HTML tags (`<script>`,
  `<style>`, `<iframe>`, …).
- GitHub additions: alerts (`> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`),
  heading anchors (`[link](#heading-text)` works), emoji shortcodes (`:tada:`), YAML front matter
  (shown as a table) and math.
- Math in `$…$`, `$$…$$`, `` $`…`$ `` and ` ```math ` blocks is converted to MathML, so no
  JavaScript is needed. The usual LaTeX math is supported: fractions, roots, scripts and limits,
  Greek letters and symbols, accents, fonts (`\mathbb`, `\mathbf`, …), `\text`, `\left`/`\right`,
  matrices, `cases` and `aligned`. Unknown commands are shown in red. Macro definitions (`\def`,
  `\newcommand`) are not supported.
- Code blocks with a language (` ```rust `) are colored with the Notepad++ lexer of that language
  and the keywords of your Notepad++ configuration, in GitHub's colors. About 70 languages and their
  GitHub names are known (`c`, `cpp`, `cs`, `java`, `js`, `ts`, `go`, `rust`, `python`, `bash`,
  `powershell`, `bat`, `html`, `xml`, `css`, `sql`, `json`, `yaml`, `toml`, `ini`, `diff`, …). Other
  code blocks are plain.
- Not rendered: Mermaid, GeoJSON and other diagrams GitHub draws with JavaScript. They stay code
  blocks. Raw HTML is passed through except for the unsafe tags, while GitHub removes much more.

Local images (relative to the document, absolute paths and `file:` URLs) are embedded as Base64
`data:` URIs by default, both Markdown images (`![alt](image.png)`) and HTML `<img>` tags. Internet
images (`http:`, `https:`, `//…`) are never downloaded. The settings switch embedding off per syntax
and limit the image size. Images that are not embedded keep their path as written, which does not
resolve in the unsaved HTML document or after pasting.

**Render HTML** opens a new tab with a complete HTML page. The GitHub style sheet
([github-markdown-css](https://github.com/sindresorhus/github-markdown-css), light theme) is
embedded, so the page has no external resources. Save it and open it in a browser.

**Copy as HTML** puts the same content on the clipboard twice: as HTML ("HTML Format") with the
styles written into every element, which Office and e-mail applications need and as plain text with
the original Markdown. Applications pick the format they support: Word, Outlook or a rich text
editor paste the formatted HTML, a plain text editor pastes the Markdown.

> [!NOTE]
> Pasting the HTML can break fonts when the Office application merges formatting, i.e. when *Merge
> Formatting* is the default for pasting (Word and Outlook: *File > Options > Advanced > Cut, copy
> and paste*). Then the pasted text takes over the fonts of the target document while some elements
> keep GitHub's fonts. Choose *Keep Source Formatting* to keep the GitHub look, or *Keep Text Only*
> for plain text.

### Markdown Viewer

Run the command, or click its toolbar button, to show the preview of the active document. Running it
again hides it. The preview is rendered like Render HTML and follows every change after a short
delay (`UpdateDelay`), switching documents and Notepad++'s dark mode.

- **Follow caret** scrolls the preview to the block at the cursor and marks the blocks of the
  selection. Without it, the preview keeps its scroll position while you edit.
- **Detach** moves the preview into a separate window that can be maximized, e.g. on a second
  screen. **Dock** in that window, or closing it, moves the preview back to the side panel.
- Links open in the default browser (`http`, `https`, `mailto`), in Notepad++ (local files, also
  relative to the document) or in the default application (images, PDF and other non-text files).
  Links to headings (`#heading-text`) scroll the preview. While the mouse is over a link, a bar
  below the preview shows its web address, file path or `#heading`.
- Copy the selection with `Ctrl+C` or the context menu, which every engine shows the same: Copy,
  Copy Link (the web address, file path or `#heading` of the link under the mouse) and Select All.

The preview is shown with one of three engines:

- **Microsoft Edge WebView2** (default) shows the page like Render HTML, with GitHub's light or dark
  style sheet. It needs the WebView2 runtime, which Windows 11 and current Windows 10 installations
  include. The plugin uses `WebView2Loader.dll` when it is placed next to `NppToolBox.dll`, otherwise
  it loads the installed runtime itself. Its temporary browser profile is kept in
  `%TEMP%\NppToolBox.WebView2`.
- **EdgeHTML** (`Engine=2`), the engine of the legacy Microsoft Edge, shows the same page without the
  WebView2 runtime. It is part of Windows 10 1803 and newer, including Windows 11, and runs in the
  Windows process `Win32WebViewHost`. Microsoft no longer develops it, so newer CSS features and
  math formulas (MathML) render differently. With the compatibility setting "Override high DPI
  scaling behavior: System (Enhanced)" of `notepad++.exe`, it is shown in a separate window kept
  over the viewer, so clicking into the viewer deactivates the Notepad++ window. EdgeHTML cannot
  play SMIL animations (`<animate>`, `<set>`, `<animateTransform>`) of SVG images, so the viewer
  converts the simple ones of local images into CSS animations; the others show a still image.
- **RichEdit** (part of Windows) is used when the selected browser is not available or `Engine=1`
  is set. It shows a simpler rendering in GitHub's colors: no math formulas (their LaTeX source is
  shown), no block quote bars and only PNG and JPEG images.

The preview shows only content of the local computer. Page scripts are disabled (WebView2) or blocked
by a content security policy (both browsers), so `<script>` and event handlers in raw HTML never run.
The policy also blocks all resources except inline styles and the embedded images. Local images are always embedded in the preview, internet
images are never downloaded.

### Settings

**Settings** opens `<plugins config dir>/NppToolBox.ini`, usually
`%APPDATA%\Notepad++\plugins\Config\NppToolBox.ini`. A missing file is created with every setting at
its default value and described. Settings missing in an existing file are added. Changes apply the
next time a command runs, no restart is needed.

| Section     | Key                     | Default    | Meaning                                                           |
|-------------|-------------------------|------------|-------------------------------------------------------------------|
| `[Rewrap]`  | `MaxLineWidth`          | `80`       | Maximum characters per line (8 to 1000), set by Rewrap.           |
| `[Html]`    | `EmbedMarkdownImages`   | `1`        | `1`: embed local `![alt](path)` images, `0`: keep the path.       |
| `[Html]`    | `EmbedHtmlImages`       | `1`        | `1`: embed local `<img src="path">` images, `0`: keep it.         |
| `[Html]`    | `MaxEmbeddedImageBytes` | `10485760` | Larger images stay linked (not shown in the Markdown Viewer).     |
| `[Html]`    | `HighlightCode`         | `1`        | `1`: color code blocks with Notepad++ lexers, `0`: plain.         |
| `[Viewer]`  | `Engine`                | `0`        | `0`: WebView2, `1`: RichEdit, `2`: EdgeHTML (fallback: RichEdit). |
| `[Viewer]`  | `FollowCaret`           | `1`        | `1`: follow the cursor and mark the selection, set by the viewer. |
| `[Viewer]`  | `Detached`              | `0`        | `0`: side panel, `1`: separate window, set by the viewer.         |
| `[Viewer]`  | `UpdateDelay`           | `300`      | Milliseconds after the last change before the preview updates.    |
| `[Toolbar]` | `RealignMarkdownTable`  | `1`        | `1`: show the toolbar button of the command, `0`: hide it.        |
| `[Toolbar]` | `Rewrap`                | `1`        | As above.                                                         |
| `[Toolbar]` | `RenderHtml`            | `1`        | As above.                                                         |
| `[Toolbar]` | `CopyAsHtml`            | `1`        | As above.                                                         |
| `[Toolbar]` | `MarkdownViewer`        | `1`        | As above.                                                         |

The `[Toolbar]` settings and `Engine` apply after restarting Notepad++ (`Engine`: the next time the
viewer is created).

## Build Instruction

The plugin is built with the GNU toolchain (MinGW-w64) on every host. MSVC is not used.

```sh
cargo build-x64   # 64-bit Notepad++ (x86_64-pc-windows-gnu)
cargo build-x86   # 32-bit Notepad++ (i686-pc-windows-gnu)
```

Copy `target/<target>/release/NppToolBox.dll` to `<Notepad++>/plugins/NppToolBox/NppToolBox.dll` and
restart Notepad++. Run the unit tests on any host with `cargo test --workspace`.

Toolchain setup (`rust-toolchain.toml` installs the targets and components automatically):

- **Linux:** install MinGW-w64, e.g. `apt install gcc-mingw-w64`.
- **Windows:** install MSYS2 with `mingw-w64-x86_64-gcc` and `mingw-w64-i686-gcc`, put `mingw64\bin`
  and `mingw32\bin` on `PATH` and use a GNU host toolchain (`rustup default
  stable-x86_64-pc-windows-gnu`) so that build scripts need no Visual Studio. A single multilib
  MinGW-w64 GCC whose `gcc` supports `-m32` and `-m64` works as well: `cargo build-x64`/`build-x86`
  link with it when `x86_64-w64-mingw32-gcc` or `i686-w64-mingw32-gcc` is missing.
- **macOS:** `brew install mingw-w64`.

### Development Tasks

- `cargo xtask update-npp-interface [--version <x.y.z>]` updates the Notepad++/Scintilla interface
  constants (`crates/npp/src/interface.rs`) from the official headers of the given Notepad++ release
  (default: the newest). It needs `curl` and warns when struct declarations changed.
- `cargo xtask update-emoji [--revision <revision>]` regenerates the emoji shortcode table
  (`crates/npp-toolbox/src/emoji_table.rs`) from GitHub's [gemoji](https://github.com/github/gemoji)
  database (default: `master`). It needs `curl`.
- `cargo xtask icon-preview` renders all toolbar icons in both themes to `target/icon-preview.png`
  and lists violations of the [icon style](crates/npp-toolbox/icons/README.md).

### Runtime Dependencies

- Notepad++ 8.9.8 or newer (same architecture as the DLL).
- Windows 10 1607 or newer. No runtime DLLs besides Windows system DLLs. The C runtime is linked
  statically.
- Markdown Viewer: The Microsoft Edge WebView2 runtime (Evergreen) or `WebView2Loader.dll` from the
  [WebView2 SDK](https://www.nuget.org/packages/Microsoft.Web.WebView2) next to `NppToolBox.dll`.
  Without them the viewer uses RichEdit. EdgeHTML (`Engine=2`) needs Windows 10 1803 or newer.

### Build Dependencies

- Rust 1.95 or newer (edition 2024) with the `x86_64-pc-windows-gnu` and `i686-pc-windows-gnu`
  targets and MinGW-w64 (`x86_64-w64-mingw32-gcc` and `i686-w64-mingw32-gcc`, or a multilib `gcc`
  with `-m64`/`-m32` and `windres` for the version resource. Set `WINDRES` to use another
  `windres`).
- Crates: `windows-sys`, `windows-core` (COM support for WebView2 and EdgeHTML), `unicode-width`,
  `pulldown-cmark` (Markdown parser) and `resvg` (renders the SVG icons in the needed size) at
  runtime. `resvg` also checks the toolbar icon style at build time.
- `curl` (and `git` as fallback) only for `cargo xtask update-npp-interface` and `cargo xtask
  update-emoji`.

## License

MIT, see [LICENSE](LICENSE). The plugin contains third-party works under the MIT license, see
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## AI Assistance Disclaimer

This software was developed with the assistance of AI-based tools.
All design decisions and final content remain the responsibility of the human author.

Users should be aware that AI-assisted code generation may introduce subtle errors,
security vulnerabilities, or suboptimal patterns that standard review processes
might not catch. Independent review and thorough testing before use in production
environments is strongly recommended.
