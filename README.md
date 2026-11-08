# NPP.TB

Notepad++ ToolBox (`NppToolBox`). A plugin with helper functions for Notepad++ 8.9.8 or newer, for
x86 and x64.

- **Realign Markdown Table** (`Ctrl+Alt+T`, toolbar): pads all cells of the table at the cursor to
  equal column widths and honors the `:--`, `--:` and `:-:` alignment markers.
- **Compact Markdown Table** (`Ctrl+Alt+P`, toolbar icon off by default): the opposite of Realign
  Markdown Table, removes all padding from the cells of the table at the cursor.
- **Rewrap…** (`Ctrl+Alt+W`, toolbar): reflows the selected lines, or the paragraph at the cursor,
  to a maximum number of characters per line. The common line prefix is kept.
- **Select Comments** (`Ctrl+Alt+N`, toolbar icon off by default): selects all comments in the
  selection, or in the whole document, as Notepad++'s syntax highlighting recognizes them.
- **Render HTML** (`Ctrl+Alt+H`, toolbar icon off by default): renders the selected Markdown, or
  the whole document, into a new unsaved HTML document that looks like GitHub. Code in another
  language becomes a highlighted code block.
- **Copy as HTML** (`Ctrl+Alt+Shift+H`, toolbar): like Render HTML, but copies the result to the
  clipboard as HTML, together with the Markdown as plain text.
- **Markdown Viewer** (`Ctrl+Alt+M`, toolbar): shows or hides a live preview of the active document,
  docked as side panel or in a separate window, optionally following the cursor.
- **Settings**: opens the settings file in Notepad++.
- **Markdown (NppToolBox)** (`Language` menu): syntax highlighting of Markdown in the editor, with
  the Markdown parser of the plugin. Code blocks are colored by the Notepad++ lexer of their
  language.

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

### Compact Markdown Table

Place the cursor anywhere in a GitHub Flavored Markdown table and run the command. It removes the
spaces around the cell contents and shortens delimiter cells to their colons and one dash, e.g.
`| apple |   3 |` becomes `|apple|3|` and `|:------|----:|` becomes `|:-|-:|`. Missing cells, outer
pipes, prefixes and escaped pipes are treated like in Realign Markdown Table.

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

### Select Comments

Run the command to select all comments of the document, or only those inside the selection. Each
comment becomes one of multiple selections, so they can be edited, deleted or copied together.

- Comments are found with the lexer that colors the document, so they are recognized in every
  language Notepad++ highlights, including embedded scripts and external lexers.
- A style counts as comment when its name in the Style Configurator contains 'comment', or when the
  lexer reports it as comment. In User Defined Languages these are the comment and comment line
  styles.
- The ranges are taken exactly as the lexer styles them. Depending on the lexer, a line comment
  includes its line ending or not.
- Comments partly inside the selection are selected up to the selection boundary.
- Without comments, the selection stays unchanged.

### Render HTML and Copy as HTML

Select Markdown, or select nothing to use the whole document and run the command. A document with
a syntax highlighting language other than Normal Text and Markdown (e.g. C++ or Python) is rendered
as if it were a Markdown code block of that language, colored like one (setting `[Html]
CodeAsCodeBlock`). Documents with a Markdown file extension and user defined languages or lexers
with "Markdown" in their name stay Markdown. The Markdown flavor is GitHub's
([GFM](https://github.github.com/gfm/), as implemented by
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
  matrices, `cases` and `aligned`, the symbols and commands of KaTeX with few exceptions, macro
  definitions (`\def`, `\newcommand`, valid within their formula) and links (`\href`). Unknown
  commands are shown in red.
- Equation numbers in math: `\tag{…}` shows a number at the right end of the line. `\label{name}`
  makes a formula the target of `[text](#name)` and `\ref{name}` and `\eqref{name}` show its number
  as a link, also before the label. A labelled display formula without `\tag` gets the next number
  (1), (2), …; `\nonumber` and starred environments such as `align*` opt out. Other formulas have no
  number, as on GitHub.
- Code blocks with a language (` ```rust `) are colored with the Notepad++ lexer of that language
  and the keywords of your Notepad++ configuration, in GitHub's colors. About 70 languages and their
  GitHub names are known (`c`, `cpp`, `cs`, `java`, `js`, `ts`, `go`, `rust`, `python`, `bash`,
  `powershell`, `bat`, `html`, `xml`, `css`, `sql`, `json`, `yaml`, `toml`, `ini`, `diff`, …). Other
  code blocks are plain.
- Mermaid diagrams in ` ```mermaid ` blocks are drawn by the plugin itself, see [Mermaid
  Diagrams](#mermaid-diagrams).
- Not rendered: GeoJSON, TopoJSON, STL and the other maps and models GitHub draws with JavaScript.
  They stay code blocks. Raw HTML is passed through except for the unsafe tags, while GitHub
  removes much more.

Local images (relative to the document, absolute paths and `file:` URLs) are embedded as Base64
`data:` URIs by default, both Markdown images (`![alt](image.png)`) and HTML `<img>` tags. Internet
images (`http:`, `https:`, `//…`) are never downloaded. The settings switch embedding off per syntax
and limit the image size. Images that are not embedded keep their path as written, which does not
resolve in the unsaved HTML document or after pasting. Embedded are the extensions `png`, `jpg`,
`jpeg`, `jfif`, `gif`, `webp`, `svg`, `bmp`, `ico`, `avif`, `tif`, `tiff`, `jxl`, `heic`, `heif`,
`jxr`, `wdp`, `hdp` and `dds`.

### Mermaid Diagrams

` ```mermaid ` code blocks are drawn as diagrams by an own renderer in the plugin, without
JavaScript and without internet access. It follows [Mermaid](https://mermaid.js.org) 12.2.0 and
draws all of its diagram types, including the beta ones: flowchart, sequence, class, state, ER,
requirement, gantt, pie, journey, timeline, gitGraph, mindmap, kanban, quadrant, XY chart, sankey,
packet, radar, treemap, block, C4, architecture, treeView, cynefin, Ishikawa, event modeling,
Wardley, Venn, railroad (also EBNF, ABNF and PEG), use case, agentflow, swimlane and `info`.

- The look and layout are those of Mermaid 12.2: the themes `redux-color` (flowchart, class, ER,
  state, sequence, requirement and a few more) and `default` in light mode, their dark variants in
  Notepad++'s dark mode, the `neo` look and the `elk` layout. `%%{init: …}%%` directives and the
  front matter `config:` select every other theme (`base`, `forest`, `neutral`, `neo`, `redux`,
  …), the looks `classic`, `neo` and `handDrawn` and the layouts `dagre` and `elk`.
- Math in labels (`$$…$$`), Font Awesome icons (`fa:fa-user`, `icon: "fa:…"`) and the built-in
  architecture icons are drawn as well.
- The Markdown Viewer with WebView2 or EdgeHTML shows the diagrams as SVG, with links, tooltips and
  selectable text. The RichEdit viewer and Copy as HTML get PNG pictures, because RichEdit, Office
  applications and LibreOffice cannot show SVG in place.
- A diagram with an error shows Mermaid's message and its line, followed by the code. Diagrams
  larger than `MaxTextSize` or with more than `MaxEdges` edges are not drawn, so that Notepad++ does
  not block.

The `elk` layout is an own layered layout with orthogonal edges, not a port of ELK, so its pictures
can differ from Mermaid's. [MERMAID.md](MERMAID.md) lists the support level per diagram type, the
differences from Mermaid and the odd behaviors of Mermaid that are reproduced on purpose.

### Image Formats

Where the plugin decodes images itself, it uses the Windows Imaging Component (WIC): images in the
RichEdit viewer, image nodes of diagrams in PNG pictures (RichEdit viewer, Copy as HTML) and the
natural size of diagram images. The formats are those of the WIC codecs installed: PNG, JPEG, GIF,
BMP, ICO, TIFF, JPEG XR and DDS with Windows, WebP, HEIF, AVIF and JPEG XL with the extensions from
the Microsoft Store. [fiwic](https://github.com/daniel-starke/fiwic/) adds the formats of FreeImage
(Targa, Photoshop, OpenEXR, Radiance HDR, JPEG 2000, Netpbm, PCX, SGI and more) to WIC. Images in
formats without a codec are not drawn and the RichEdit viewer shows their alternative text. The
browser engines show the formats the browser supports.

**Render HTML** opens a new tab with a complete HTML page. The GitHub style sheet
([github-markdown-css](https://github.com/sindresorhus/github-markdown-css), light theme) is
embedded, so the page has no external resources. Save it and open it in a browser.

**Copy as HTML** puts the same content on the clipboard twice: as HTML ("HTML Format") with the
styles written into every element, which Office and e-mail applications need and as plain text with
the original Markdown or code. Applications pick the format they support: Word, Outlook or a rich text
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
  converts the simple ones of local images into CSS animations. The others show a still image.
- **RichEdit** (part of Windows) is used when the selected browser is not available or `Engine=1`
  is set. It shows a simpler rendering in GitHub's colors: no math formulas (their LaTeX source is
  shown), no block quote bars and no SVG images (Mermaid diagrams are shown as PNG). Other image
  formats are converted to PNG with WIC (see [Image Formats](#image-formats)).

The preview shows only content of the local computer. Page scripts are disabled (WebView2) or
blocked by a content security policy (both browsers), so `<script>` and event handlers in raw HTML
never run. The policy also blocks all resources except inline styles and the embedded images. Local
images are always embedded in the preview, internet images are never downloaded.

### Markdown Language

The plugin adds the language **Markdown (NppToolBox)** to Notepad++ (`Language` > `M`), which colors
Markdown with the same parser as Render HTML, so the editor shows the document structure as the
rendering sees it: headings, emphasis, strong emphasis, strikethrough, code spans, code blocks,
quotes, list and task markers, links, images and their destinations, raw HTML, rules, tables,
footnotes, math and YAML front matter. Blocks are restyled as a whole while you type, so a line
becomes a heading as soon as you underline it with `===`.

Code blocks with a language (` ```rust `) are colored by the Notepad++ lexer of that language, with
its keywords and its colors in the current theme (light or dark mode), on the font and background of
the `CODE BLOCK` style. The languages are the ones of Render HTML. Other code blocks take the `CODE
BLOCK` style.

Notepad++ uses the language for the extensions `md`, `markdown`, `mdown`, `mkd`, `mkdn`, `mdwn` and
`mdtxt` unless a user defined language claims them, such as the preinstalled "Markdown
(preinstalled)". Select the language in the `Language` menu, or set `[Lexer] Markdown=2` to use it
for every Markdown file the first time it is shown, also over a user defined language. The colors of
the Markdown styles are set in `Settings` > `Style Configurator…` > `Markdown (NppToolBox)`. Their
defaults are mid-tones that read in light and dark themes, on the background of the theme.

Notepad++ keeps these styles in `<plugins config dir>/NppToolBox.xml`, which the plugin creates when
it is missing. Notepad++ refuses to load the plugin without this file. `[Lexer] Markdown=0` removes
the language from Notepad++ after a restart.

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
| `[Html]`    | `CodeAsCodeBlock`       | `1`        | `1`: render code documents as a code block, `0`: as Markdown.     |
| `[Lexer]`   | `Markdown`              | `1`        | `0`: off, `1`: Markdown language, `2`: for all Markdown files.    |
| `[Mermaid]` | `Enabled`               | `1`        | `1`: draw ` ```mermaid ` blocks as diagrams, `0`: show the code.  |
| `[Mermaid]` | `MaxTextSize`           | `50000`    | Larger diagrams (bytes) are not drawn (Mermaid's `maxTextSize`).  |
| `[Mermaid]` | `MaxEdges`              | `500`      | Diagrams with more edges are not drawn (Mermaid's `maxEdges`).    |
| `[Viewer]`  | `Engine`                | `0`        | `0`: WebView2, `1`: RichEdit, `2`: EdgeHTML (fallback: RichEdit). |
| `[Viewer]`  | `FollowCaret`           | `1`        | `1`: follow the cursor and mark the selection, set by the viewer. |
| `[Viewer]`  | `Detached`              | `0`        | `0`: side panel, `1`: separate window, set by the viewer.         |
| `[Viewer]`  | `UpdateDelay`           | `300`      | Milliseconds after the last change before the preview updates.    |
| `[Toolbar]` | `RealignMarkdownTable`  | `1`        | `1`: show the toolbar button of the command, `0`: hide it.        |
| `[Toolbar]` | `CompactMarkdownTable`  | `0`        | As above.                                                         |
| `[Toolbar]` | `Rewrap`                | `1`        | As above.                                                         |
| `[Toolbar]` | `SelectComments`        | `0`        | As above.                                                         |
| `[Toolbar]` | `RenderHtml`            | `0`        | As above.                                                         |
| `[Toolbar]` | `CopyAsHtml`            | `1`        | As above.                                                         |
| `[Toolbar]` | `MarkdownViewer`        | `1`        | As above.                                                         |

The `[Toolbar]` settings, `[Lexer] Markdown` and `Engine` apply after restarting Notepad++ (`Engine`: the next time the
viewer is created).

## Build Instruction

The plugin is built with the GNU toolchain (MinGW-w64) on every host. MSVC is not used.

```sh
cargo build-x64    # 64-bit Notepad++ (x86_64-pc-windows-gnu)
cargo build-x86    # 32-bit Notepad++ (i686-pc-windows-gnu)
cargo deploy-x64   # build-x64, then check the DLL and write the release package
cargo deploy-x86   # build-x86, then check the DLL and write the release package
```

`build-<arch>` writes `target/<target>/release/NppToolBox.dll`. `deploy-<arch>` builds it as well,
fails if the DLL imports anything but Windows system DLLs (e.g. a MinGW runtime DLL) and copies it
with `LICENSE`, `README.md` and `THIRD-PARTY-NOTICES.md` to `target/<target>/package/NppToolBox/`.
Copy the DLL to `<Notepad++>/plugins/NppToolBox/NppToolBox.dll` and restart Notepad++. Run the
unit tests on any host with `cargo test --workspace`.

The feature `pure-rust` (`cargo build-x64 --features pure-rust`) decodes images with Rust crates
instead of WIC, as before: PNG, JPEG, GIF and WebP only, and the RichEdit viewer shows only PNG and
JPEG images. The DLL grows by about 500 KiB.

Toolchain setup (`rust-toolchain.toml` installs the targets and components automatically):

- **Linux:** install MinGW-w64, e.g. `apt install gcc-mingw-w64`.
- **Windows:** install MSYS2 with `mingw-w64-x86_64-gcc` and `mingw-w64-i686-gcc`, put `mingw64\bin`
  and `mingw32\bin` on `PATH` and use a GNU host toolchain (`rustup default
  stable-x86_64-pc-windows-gnu`) so that build scripts need no Visual Studio. A single multilib
  MinGW-w64 GCC whose `gcc` supports `-m32` and `-m64` works as well: `cargo build-<arch>` and
  `cargo deploy-<arch>` link with it when `x86_64-w64-mingw32-gcc` or `i686-w64-mingw32-gcc` is
  missing.
- **macOS:** `brew install mingw-w64`.

### Development Tasks

- `cargo xtask update-npp-interface [--version <x.y.z>]` updates the Notepad++/Scintilla interface
  constants and language names (`crates/npp/src/interface.rs`) from the official headers and
  `ScintillaEditView.cpp` of the given Notepad++ release (default: the newest). It needs `curl` and
  warns when struct declarations changed.
- `cargo xtask update-emoji [--revision <revision>]` regenerates the emoji shortcode table
  (`crates/npp-toolbox/src/emoji_table.rs`) from GitHub's [gemoji](https://github.com/github/gemoji)
  database (default: `master`). It needs `curl`.
- `cargo xtask update-mermaid-themes [--version <x.y.z>]` compiles the theme code of Mermaid
  (default: 12.2.0) into the theme table (`crates/npp-toolbox/src/mermaid/theme_table.rs`). It
  needs `curl`.
- `cargo xtask update-fontawesome [--version <x.y.z>]` regenerates the Font Awesome Free icon table
  of the diagrams (`crates/npp-toolbox/src/mermaid/icon_table.rs`, default: 6.7.2). It needs `curl`.
  The icons are part of the default feature `font-awesome`; `--no-default-features` leaves them
  out.
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
- Crates: `windows-sys`, `windows-core` (COM support for WebView2, EdgeHTML, DirectWrite and
  WIC), `unicode-width`, `pulldown-cmark` (Markdown parser) and `resvg` (renders the SVG icons in
  the needed size and the PNG pictures of diagrams) at runtime. `resvg` also checks the toolbar
  icon style at build time.
- `curl` (and `git` as fallback) only for `cargo xtask update-npp-interface`, `update-emoji`,
  `update-mermaid-themes` and `update-fontawesome`.

## License

MIT, see [LICENSE](LICENSE). The plugin contains third-party works under the MIT, BSD and ISC
licenses and the icons of Font Awesome Free under CC BY 4.0, see
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## AI Assistance Disclaimer

This software was developed with the assistance of AI-based tools.
All design decisions and final content remain the responsibility of the human author.

Users should be aware that AI-assisted code generation may introduce subtle errors,
security vulnerabilities, or suboptimal patterns that standard review processes
might not catch. Independent review and thorough testing before use in production
environments is strongly recommended.
