# Toolbar Icon Style

All toolbar icons follow one common style: `npp_build::icon_style::DEFAULT`, the
**Notepad++ Fluent outline** style. The build checks every SVG in this directory against it and
fails with a list of violations. Preview the result with:

```sh
cargo xtask icon-preview   # writes target/icon-preview.png, prints violations
```

## Rules

| Rule            | Value                                                                    | Enforced |
|-----------------|--------------------------------------------------------------------------|----------|
| Canvas          | `viewBox="0 0 16 16"`, one SVG per command, file name = icon name        | yes      |
| Foreground      | `currentColor`, becomes `#212121` (light) / `#DEDEDE` (dark theme)       | yes      |
| Accent          | `#0078D4`, becomes `#4CC2FF` in the dark theme                           | yes      |
| Other colors    | none; no gradients, patterns, images, text, filters, masks or opacity    | yes      |
| Strokes         | 1 px wide, `fill="none"` (SVG fills black by default)                    | yes      |
| Foreground fill | not allowed, the foreground is outline only; fills are for the accent    | yes      |
| Pixel grid      | 1 px strokes on half-pixel coordinates (`x.5`), fills on whole pixels    | review   |
| Accent use      | exactly one accent element, marking what the command acts on or adds     | review   |
| Distinctness    | must not resemble a native Notepad++ toolbar icon                        | review   |
| Shapes          | rounded corners `rx="1"`, 1 px margin to the canvas edge if possible     | review   |

The colors were measured from the Notepad++ 8.9.8 Fluent UI icons, so plugin buttons blend in
with the native toolbar in both themes. The "review" rules are checked in the magnified 16 px
column of the preview: blurry, gray lines mean a stroke is off the half-pixel grid.
