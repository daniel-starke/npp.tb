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
- **Settings**: opens the settings file in Notepad++.

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

Select lines, or place the cursor in a paragraph, and run the command. A dialog asks for the maximum
number of characters per line. The last value is remembered in `<plugins config
dir>/NppToolBox.ini`.

- Kept prefixes: indentation, line comments (`//`, `///`, `//!`, `#`, `--`, `;`, `%`, `'`, `REM`,
  `::`), block comment bodies (` * `), blockquotes (`>`, nested) and their combinations.
- Markdown list items (`-`, `*`, `+`, `1.`, `1)`, task boxes `[ ]`) wrap with a hanging indent.
- Never reflowed: fenced code blocks, indented code (Markdown files), tables, headings, thematic
  breaks, HTML blocks, link reference definitions and block comment delimiter lines (`/*`, `*/`).
- Hard line breaks (trailing `\`, and two trailing spaces in Markdown files) are preserved.
- Files with the extension `.md`, `.markdown`, `.mdown`, `.mkd`, `.mkdn`, `.mdwn` or `.mdtxt` are
  treated as Markdown: comment markers are not recognized and `#` starts a heading. All other files
  are treated as code or plain text.
- Words longer than the line width are not split.

### Render HTML and Copy as HTML

Select Markdown, or select nothing to use the whole document, and run the command. The Markdown
flavor is GitHub's ([GFM](https://github.github.com/gfm/), as implemented by
[cmark-gfm](https://github.com/github/cmark-gfm)):

- CommonMark with tables, task lists, strikethrough, footnotes, autolinks for bare `www.` and
  `http(s)://` addresses and e-mail addresses, and the filter for unsafe raw HTML tags (`<script>`,
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
styles written into every element, which Office and e-mail applications need, and as plain text with
the original Markdown. Applications pick the format they support: Word, Outlook or a rich text
editor paste the formatted HTML, a plain text editor pastes the Markdown.

> [!NOTE] Pasting the HTML can break fonts when the Office application merges formatting, i.e. when
> *Merge Formatting* is the default for pasting (Word and Outlook: *File > Options > Advanced > Cut,
> copy, and paste*). Then the pasted text takes over the fonts of the target document while some
> elements keep GitHub's fonts. Choose *Keep Source Formatting* to keep the GitHub look, or *Keep
> Text Only* for plain text.

### Settings

**Settings** opens `<plugins config dir>/NppToolBox.ini`, usually
`%APPDATA%\Notepad++\plugins\Config\NppToolBox.ini`. A missing file is created with every setting at
its default value and described. Settings missing in an existing file are added. Changes apply the
next time a command runs, no restart is needed.

| Section    | Key                     | Default    | Meaning                                                     |
|------------|-------------------------|------------|-------------------------------------------------------------|
| `[Rewrap]` | `MaxLineWidth`          | `80`       | Maximum characters per line (8 to 1000), set by Rewrap.     |
| `[Html]`   | `EmbedMarkdownImages`   | `1`        | `1`: embed local `![alt](path)` images, `0`: keep the path. |
| `[Html]`   | `EmbedHtmlImages`       | `1`        | `1`: embed local `<img src="path">` images, `0`: keep it.   |
| `[Html]`   | `MaxEmbeddedImageBytes` | `10485760` | Larger images stay linked.                                  |
| `[Html]`   | `HighlightCode`         | `1`        | `1`: color code blocks with Notepad++ lexers, `0`: plain.   |

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
  and `mingw32\bin` on `PATH`, and use a GNU host toolchain (`rustup default
  stable-x86_64-pc-windows-gnu`) so that build scripts need no Visual Studio.
- **macOS:** `brew install mingw-w64`.

### Development Tasks

- `cargo xtask update-npp-interface [--version <x.y.z>]` updates the Notepad++/Scintilla interface
  constants (`crates/npp/src/interface.rs`) from the official headers of the given Notepad++ release
  (default: the newest). It needs `curl`, and warns when struct declarations changed.
- `cargo xtask update-emoji [--revision <revision>]` regenerates the emoji shortcode table
  (`crates/npp-toolbox/src/emoji_table.rs`) from GitHub's [gemoji](https://github.com/github/gemoji)
  database (default: `master`). It needs `curl`.
- `cargo xtask icon-preview` renders all toolbar icons in both themes to `target/icon-preview.png`
  and lists violations of the [icon style](crates/npp-toolbox/icons/README.md).

### Runtime Dependencies

- Notepad++ 8.9.8 or newer (same architecture as the DLL).
- Windows 10 1607 or newer. No runtime DLLs besides Windows system DLLs. The C runtime is linked
  statically.

### Build Dependencies

- Rust 1.85 or newer (edition 2024) with the `x86_64-pc-windows-gnu` and `i686-pc-windows-gnu`
  targets, and MinGW-w64 (`x86_64-w64-mingw32-gcc`, `i686-w64-mingw32-gcc` and `windres` for the
  version resource; set `WINDRES` to use another `windres`).
- Crates: `windows-sys`, `unicode-width` and `pulldown-cmark` (Markdown parser) at runtime. `resvg`
  at build time renders the SVG toolbar icons.
- `curl` (and `git` as fallback) only for `cargo xtask update-npp-interface` and `cargo xtask
  update-emoji`.

## License

MIT, see [LICENSE](LICENSE). The plugin contains third-party works under the MIT license, see
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
