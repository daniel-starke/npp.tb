# Mermaid diagrams

NppToolBox renders ```` ```mermaid ```` code blocks with its own pure Rust renderer
(`crates/npp-toolbox/src/mermaid/`). It follows Mermaid 12.2.0. Its syntax documentation and, in
case of doubt, its JavaScript implementation. The goal is the same picture Mermaid draws in a
browser, so the renderer is checked against Mermaid's own renders in Edge. Positions, sizes and
edge paths of every node, and the computed fill, stroke and width of every shape.

This file states what is supported, where the output differs from Mermaid and which odd
behaviors of Mermaid are reproduced on purpose.

## Output

- **Markdown viewer with WebView2 or EdgeHTML:** inline SVG, with links, tooltips and selectable
  text.
- **Markdown viewer with RichEdit:** PNG picture at the DPI of the view.
- **Copy as HTML:** PNG `<img>`, because Office applications and LibreOffice drop inline SVG.
- **Theme:** Mermaid's defaults of 12.2 in light mode and their dark counterparts in dark mode
  (Copy as HTML is always light). `%%{init}%%` directives and front matter `config:` override
  them.
- **Errors:** a muted error box with Mermaid's message and the line in the code block, followed by
  the code.
- **Settings:** `[Mermaid]` `Enabled`, `MaxTextSize` (bytes, default 50000, Mermaid's
  `maxTextSize`) and `MaxEdges` (default 500, Mermaid's `maxEdges`).

## Support level

| Diagram|State|Notes|
|---|---|---|
|`info`, `pie`|supported||
|`flowchart`, `graph`|supported|All shapes, `@{}` syntax, subgraphs, collapsed subgraphs, edge styles, `classDef`, `linkStyle`, `click`.|
|`sequenceDiagram`|supported|All participant types, blocks, notes, activations, boxes, create/destroy, autonumber, central connections. Actor menus only with `forceMenus`.|
|`stateDiagram`, `stateDiagram-v2`|supported|Composite and concurrent states, notes, forks, choices, `classDef`, `click`.|
|`erDiagram`|supported|Attributes with keys and comments, aliases, all cardinalities, subgraphs, `classDef`, `style`.|
|`requirementDiagram`|supported|All requirement types, elements, relationships, `classDef`, `class`, `:::`, `style`, `direction`.|
|`classDiagram`, `classDiagram-v2`|supported|Members, generics, annotations, all relation types with cardinalities, lollipop interfaces, notes, nested namespaces (`hierarchicalNamespaces`), `classDef`, `cssClass`, `:::`, `style`, `link`, `click`, `direction`, `hideEmptyMembersBox`.|
|`packet`, `packet-beta`|supported|Bit ranges, bit counts (`+8`), `bitsPerRow`, `bitOrder: descending`, `showBits`.|
|`radar-beta`|supported|Axes, curves with values in axis order or by axis name, `circle` and `polygon` graticules, `ticks`, `min`, `max`, `showLegend`.|
|`kanban`|supported|Sections, items, `@{ }` metadata (`label`, `ticket`, `assigned`, `priority`), `ticketBaseUrl` links, `sectionWidth`.|
|`journey`|supported|Sections, tasks with scores and people, the actor legend, `textPlacement` (`fo`, `tspan`, `old`), title font and color.|
|`timeline`|supported|Sections, periods with events, `timeline LR` and `timeline TD`, `disableMulticolor`, the neo look and the redux themes.|
|`sankey`, `sankey-beta`|supported|CSV links, quoted fields, all node alignments, link colors (gradient, source, target, any color), `labelStyle: outlined`, `nodeColors`, values with `prefix` and `suffix`.|
|`quadrantChart`|supported|Axes with one or two labels, quadrant texts, points with `:::class`, `classDef` and point styles, all configuration options and theme variables.|
|`xychart`, `xychart-beta`|supported|Bar and line plots, band and linear axes, `horizontal`, legends, point labels, data labels, label rotation, all configuration options and theme variables.|
|`treemap`, `treemap-beta`|supported|Sections and leaves by indentation, `classDef` and `:::class`, `valueFormat` (all of d3-format), `showValues`, `padding`, `nodeWidth`, `nodeHeight`, `diagramPadding`, title and accessibility.|
|`gantt`|supported|Sections, dates in any `dateFormat` of dayjs, durations, `after` and `until`, the tags `active`, `done`, `crit`, `milestone` and `vert`, `excludes`, `includes`, `weekend`, `inclusiveEndDates`, `axisFormat` (all of d3's time format), `tickInterval`, `weekday`, `todayMarker`, `topAxis`, `displayMode: compact`, `click … href`, title and accessibility.|
|`gitGraph`|supported|Commits with ids, tags and types, branches with `order`, `checkout`/`switch`, merges, cherry-picks, `LR`, `TB` and `BT`, `parallelCommits`, `rotateCommitLabel`, `showBranches`, `showCommitLabel`, `mainBranchName`, `mainBranchOrder`, the neo look and the redux themes, title and accessibility.|
|`block`, `block-beta`|supported|Columns, spans (`:N`), `space`, composite blocks, all block shapes, block arrows in every direction, edges with labels and arrow types, `classDef`, `class`, `style`, `padding`, the neo look and the redux themes.|
|`C4Context`, `C4Container`, `C4Component`, `C4Dynamic`, `C4Deployment`|supported|People, systems, containers and components with their database, queue and external variants, boundaries and deployment nodes, all relationship types, `RelIndex`, `UpdateElementStyle` (also `$shape`), `UpdateRelStyle`, `UpdateLayoutConfig`, the per-type font and color settings, title and accessibility.|
|`mindmap`|supported|All node shapes, `:::class`, `::icon()`, sections by branch or by depth (`colorBy`), `padding`, `maxNodeWidth`, the neo look and the redux themes.|
|`architecture`|supported|Services, junctions, nested groups, edges with directions, arrows, `{group}` ends and titles, `align row`/`align column`, the built-in icons and icon texts, all configuration options except `randomize: true`.|
|`treeView-beta`|supported|Rows by indentation or as `tree` output (`├──`, `└──`, `│` and the heavy variants), directories (`/`), quoted names, `:::highlight`, `icon()`, descriptions (`##`), `showIcons` with `filenameIcons` and `extensionIcons`, the style options of `themeVariables.treeView`, title and accessibility.|
|`cynefin-beta`|supported|The five domains with items (at most three in Confusion, then `+N more`), transitions with labels, wavy boundaries (`boundaryAmplitude`, `seed`), `showDomainDescriptions`, `width`, `height`, `padding`, the `cynefin` theme variables, title and accessibility.|
|`ishikawa`, `ishikawa-beta`|supported|The effect and causes by indentation, nested to any depth, `<br>` line breaks, `diagramPadding`, `useMaxWidth`.|
|`eventmodeling`|supported|Time and reset frames of all entity types, qualified names, `->>` sources, inline data and `[[data]]` references, `data`, `entity`, `note` and `gwt` statements (parsed, not drawn like in Mermaid), the `em*` theme variables, `padding`.|
|`wardley-beta`|supported|Anchors, components with labels, sourcing strategies (`build`, `buy`, `outsource`, `market`) and inertia, links with ports, flows and labels, `evolve`, pipelines, notes, annotations, accelerators and deaccelerators, `size`, evolution stages with boundaries, title and accessibility.|
|`venn-beta`|supported|Sets and unions with labels and sizes, missing pairs of larger unions, text nodes (indented or `text A, B …`), `style` (fill, stroke, stroke-width, fill-opacity, color), the venn palette of the theme, `width`, `height`, `padding`, title.|
|`railroad-beta`, `railroad-ebnf-beta`, `railroad-abnf-beta`, `railroad-peg-beta`|supported|Rules of terminals, non-terminals, sequences, choices, options, repetitions and special sequences in Mermaid's function syntax, EBNF (ISO 14977 and postfix operators, exceptions), ABNF (repetitions, numeric values) and PEG (ordered choice, lookaheads, `.`); colors and font from the theme or the `railroad` options, `padding`, `strokeWidth`, `useMaxWidth`, accessibility.|
|`usecase-beta`|supported|Actors (normal, `hollow`, `awesome`, icon) and use cases (ellipse, `[…]` rectangle) with business markers and stereotypes, `rect` and `package` system boundaries, all association operators with labels and lengths, include, extend and generalization, notes, JSON tables, edge ids with animation, `classDef`, `class`, `style`, `:::`, `direction`, the role and `rotate` color schemes, the font and spacing options, accessibility.|
|`agentflow-beta`|supported|Nodes with the agentflow shapes and their aliases (`task`, `tool`, `decision`, `input`, `refdoc`, …), `@{ }` metadata, `flow` containers (also nested and `view: collapsed`, with edges redirected to the collapsed container), `connector` and `global` blocks, the edges `-->`, `--x`, `-.-` and labeled edges, `classDef`, `class`, `style`, `linkStyle`, `click`, directions, the kind and container palettes of the color themes, title and accessibility.|
|`swimlane-beta`|supported|The flowchart syntax with top-level subgraphs as lanes (`TB`, `TD`, `BT`, `LR`, `RL`), nested subgraphs, a synthesized lane for nodes outside every subgraph, edge labels placed on the routes, Mermaid's swimlane layout (Sugiyama layering per lane, orthogonal router and its clean-up passes), line hops (`lineHops: arc`, `gap`, `false`), `ignoreCrossLaneEdges`, `optimizeRanksByCrossings`, `automaticLaneOrdering`, the lane palettes of the color themes, `layout: dagre` instead of the swimlane layout, and `layout: swimlane` for flowcharts.|
|ZenUML|out of scope|An external plugin of Mermaid, not one of its registered diagrams.|

Features that are not supported yet in any diagram:

- Images from local paths. `data:` images work.
- Icon packs other than Font Awesome Free, such as `logos:`: they draw the unknown icon (`?`), like
  a pack Mermaid has not loaded.
- `htmlLabels: false`: labels are laid out like HTML labels. Math stays text there, like in
  Mermaid, also in class diagrams, where Mermaid switches a label with math back to HTML.

Math (`$$…$$`) works where Mermaid has it: labels of flowcharts, swimlanes, state, class, ER,
requirement, use case and agentflow diagrams, mindmaps, kanban boards and block diagrams, and
participants, messages, notes and block titles of sequence diagrams. All other diagrams show the
text.

The `handDrawn` look sketches the shapes with an own port of roughjs 4.6.6 where Mermaid uses
roughjs: flowcharts, swimlanes, state, class, ER, requirement, use case, agentflow, C4, block,
kanban, mindmap, Venn and Ishikawa diagrams. With a non-zero `handDrawnSeed` the sketch follows
Mermaid's random numbers exactly. All other diagrams ignore the look, like Mermaid; the
documentation names railroad diagrams, Wardley maps and Cynefin frameworks explicitly.

## Icons

Mermaid has no icon data: a page registers icon packs, or loads Font Awesome's style sheet for
`fa:fa-car` in labels. The plugin has Font Awesome Free 6.7.2 built in and behaves as if its packs
were registered:

| Reference                   | Icons   |
|-----------------------------|---------|
| `fa:`, `fas:`, `fa6-solid:` | solid   |
| `far:`, `fa6-regular:`      | regular |
| `fab:`, `fa6-brands:`       | brands  |

- Labels: `fa:fa-car`, `fas:fa-car`, `far:fa-clock`, `fab:fa-github`. `fak:` and `fal:` are Pro
  styles and draw nothing, as do unknown names. `fad:fa-car` stays text.
- `@{ icon: "fa:user", form: "circle" }` in flowcharts, `actor A@{ icon: "fa:user" }` in use case
  diagrams, `service a(fa:database)` and `group g(fa:cloud)` in architecture diagrams,
  `icon(fa:car)` in tree views. Other packs and unknown names draw the unknown icon (`?`).
- `cargo xtask build x64 --no-default-features` leaves Font Awesome out, which makes the DLL about
  0.8 MB smaller. The references then behave like Mermaid on a page without packs and without the
  style sheet: labels draw nothing and the others the unknown icon. The built-in icons of
  architecture diagrams and tree views stay.

## Differences from Mermaid

- **Layout `elk`**, the default of Mermaid 12.2 for flowcharts, state, class, ER, requirement, use
  case and agentflow diagrams, is an own layered layout with ELK's look, not ELK: edges run
  orthogonally with rounded corners from ports spread along the sides of the nodes, turn in tracks
  between the layers, and hop over each other like in Mermaid (`elk.lineHops`, also `gap` and
  `false`). Every subgraph is laid out on its own and is a block in its parent; edges crossing its
  border meet at ports on the frame. A subgraph keeps its `direction` when no edge crosses its
  border. Cycles are broken in the order the edges are declared. Layers, the order within them and
  the positions come from dagre (`minlen` is ignored, like ELK does), so nodes are often in the same
  layers as in Mermaid but in another order, and the routes differ. The other `elk.*` algorithms
  (`elk.stress`, `elk.force`, `elk.mrtree`, `elk.box`, …), the `algorithm` of subgraphs and the
  options of `elk` (`nodePlacementStrategy`, `cycleBreakingStrategy`, `mergeEdges`,
  `considerModelOrder`, …) except `lineHops` are ignored. `layout: dagre` lays out with dagre
  exactly like Mermaid; `cose-bilkent` and unknown names also use dagre, where Mermaid lays a
  flowchart out with cose-bilkent.
- **Icons:** Font Awesome is built in instead of registered by the page, see [Icons](#icons). It
  also finds the aliases and the names of Font Awesome 4 of its `fa:`, `fas:`, `far:` and `fab:`
  packs, in any of these and in the style of the old name (`fa:fa-twitter` draws the brand icon),
  like Mermaid with Font Awesome's style sheet and `v4-shims.css`. A registered pack finds neither.
  Icons in labels are paths in the color of the text. With registered packs the rules of subgraphs,
  ER, requirement, agentflow and mindmap nodes for `path` paint them like the node (a mindmap's
  icons disappear on its background) and the neo look strokes them; with Font Awesome's style sheet
  they have the color of the text. An icon in the 12 pixel label of a use case has the size of that
  text, where Mermaid's is 14 pixels and makes the line a pixel higher. The background of `form:
  circle` is a true circle instead of roughjs's outline, which makes such nodes up to 0.1 pixels
  narrower. In dark mode the icons of architecture diagrams have the text color of the theme, as a
  dark page gives their `currentColor`.
- **Math** is typeset as SVG, because EdgeHTML has no MathML and neither it nor resvg shows
  `foreignObject`. The layout follows Chromium's MathML with the `MATH` table of the same font
  (Cambria Math), so boxes match Edge to 1/64 pixel. Stretched operators are glyph outlines and
  cannot be selected.
- **LaTeX in math:** KaTeX is not embedded. The commands are those of the Markdown viewer's math, a
  subset of KaTeX's. Other commands fail the diagram like an unknown command does in Mermaid, and
  some malformed formulas KaTeX rejects are accepted. `\lcm`, `\tr`, `\rank`, `\arccot`, `\arcsec`,
  `\arccsc`, `\sech`, `\csch`, `\arsinh`, `\arcosh`, `\artanh`, `\iiiint`, `\ce`, `\pu`, `\url`,
  `\includegraphics`, `\htmlClass` and `\htmlId` are among those commands. After a macro (`\def`,
  `\newcommand`) an error gives the position in the expanded formula. Math inside of `\tag{…}` is
  shown as text.
- **`\label`, `\ref` and `\eqref`** fail the diagram as in Mermaid, unlike in the Markdown viewer.
- **Fonts in math:** `\mathbb`, `\mathcal`, `\mathfrak`, `\mathbf`, `\mathsf`, `\mathtt` and
  `\boldsymbol` show their letters. In Chromium Mermaid's `mathvariant` has no effect there.
- **`\overset`, `\underset`, `\stackrel`, `\bmod` and `\mathop`** show their content, which is lost
  in Mermaid's render in Edge, like `\coloneqq`, `\eqqcolon`, `\coloneq`, `\eqcolon`, `\ulcorner`,
  `\urcorner`, `\llcorner`, `\lrcorner`, `\ddddot`, `\varlimsup` and `\varliminf`.
  `\overlinesegment` and `\underlinesegment` show a line without end marks, where Mermaid shows the
  text `undefined`.
- **`\xrightarrow` and the other extensible arrows** stretch over their text, which overflows the
  arrow in Edge. The horizontal arrows of `CD` do the same, with some room around their labels, and
  the labels of its vertical arrows stand next to the arrow instead of on it.
- **`\boxed`, `\fbox`, `\cancel`, `\bcancel`, `\xcancel` and `\sout`** draw their box or line, which
  Chromium ignores in Mermaid's `menclose`. The box takes room, the lines do not.
- **`\colorbox` and `\fcolorbox`** put their background around the text with 3 points of padding. In
  Edge Mermaid's background is a square of 6 points that the text overflows.
- **`\pmb`** draws the shadow of `text-shadow` as a sharp copy.
- **`\href`** is a link around its content. Mermaid does not set KaTeX's `trust`, so it shows
  `\href` in red.
- **`legacyMathML` and `forceLegacyMathML`** are ignored.
- **Formulas in sequence diagrams** have the text color of their element. Mermaid leaves them the
  color of the page, which is black on a dark theme in a page without own colors.
- **The participants at the bottom of a sequence diagram** always show their formulas. Mermaid does
  not wait for them (`byKatex` in `drawActorTypeParticipant`), so that some are missing.
- **Error lines** count from the first line of the code block, including front matter and
  directives, so they point at the right line.
- **Configuration keys:** Mermaid drops every key of a directive or front matter `config:` that its
  default configuration does not have, for example `themeVariables.packet.blockFillColor`. The
  renderer applies such keys.
- **Parse errors** of the Langium grammars (packet, radar) name the expected and the found token
  instead of Langium's message.
- **Edge label backgrounds** are one rectangle in the color of Mermaid's `.edgeLabel` background.
  Mermaid paints an HTML label with two background layers, the inner one only behind the text.
- **Edge animations** are drawn as a static dash pattern.
- **Image sizes:** the natural size of `data:` images in node shapes is read from PNG, GIF, JPEG and
  SVG headers, and for other formats from the installed WIC codecs (not with the `pure-rust`
  feature). Images of unknown size are assumed 100 x 100 pixels; browsers know every format they
  show. In PNG pictures, images are drawn only in formats a WIC codec decodes.
- **Self-loops:** Mermaid's invisible 0.1 px helper rectangles of self-loops are left out.
- **Flowchart:** the left brace shape is about 19 px wider than Mermaid's. An edge into a lined
  cylinder can bend where Mermaid's does not, because Mermaid's cylinder intersection compares
  floating point numbers for equality.
- **ER diagram:** an entity named like a subgraph that is defined later is left out. Mermaid stops
  with "Cannot read properties of undefined (reading 'shape')".
- **Requirement diagram:** a relationship of a name that is no requirement or element is an error
  that names it. Mermaid fails with "Cannot read properties of undefined (reading 'shape')". A
  requirement and an element of the same name are one node, as in Mermaid, but the area of the
  diagram may differ.
- **Class diagram:** a method without a name before its parentheses (`(x)`) and an annotation of an
  unknown class are errors that say so. Mermaid fails with a TypeError. Cardinalities of routes
  shorter than 35 px go to the far end, where Mermaid fails. Static members are underlined with a
  rectangle, so that the PNG output shows the underline too.
- **User journey:** Mermaid writes `NaN` for the face of a score that is no number, which browsers
  draw at 0. The renderer writes 0. The text of a task or section that overflows its box is cut off
  by a clip path, where Mermaid's `<foreignObject>` clips it.
- **Timeline:** Mermaid brightens event boxes with the CSS filter `brightness(120%)`. The renderer
  brightens their colors instead (also of the gradient), which looks the same except for the
  antialiasing of the text. An event before the first time period is an error that says so; Mermaid
  fails with a TypeError. The area of the diagram can differ by up to one pixel where a glyph sticks
  out of its advance.
- **Sankey diagram:** Mermaid sanitizes node names with DOMPurify, which serializes a name that
  contains `<` as HTML. The renderer escapes `&`, `<` and `>` of such names like DOMPurify, but
  keeps tags as written where DOMPurify would also close or drop them. An unknown `nodeAlignment` is
  an error that names it; Mermaid fails with a TypeError. Labels without values are moved down by
  0.35 em in `y` instead of a `dy` attribute.
- **Quadrant chart:** a point whose coordinate is no number is drawn at 0 and its label at the top
  left corner; Mermaid writes `NaN`, which browsers read the same way. Theme colors that are no
  valid color are left out instead of written, which browsers also ignore. A style error names the
  line of the point or `classDef`.
- **XY chart:** Mermaid measures texts in screen pixels and scales them back when the diagram is
  shrunk to the width of the page (a `width` wider than the page), so labels take more room the
  narrower the page is. The renderer lays the chart out at its full size. A chart without plots is
  the error "No Plot to render", where Mermaid fails with a TypeError, and so is a bar data label
  that the first plot has no value for. A line with a missing value ends there, where Mermaid writes
  `NaN`, which browsers draw the same way.
- **Treemap:** Mermaid measures the area of the diagram while it is shown at the width of the page,
  and Chromium's text boxes change slightly with the scale. The area can therefore differ by up to a
  pixel where a text reaches its edge. Parse errors name the expected and the found token instead of
  Langium's message.
- **Gantt chart:** Mermaid makes the chart as wide as the element it renders it in, the renderer
  1200 pixels (Mermaid's width without such an element) unless `useWidth` is set; with `useMaxWidth`
  it still shrinks to the page. Dates are local times like in the browser, but with Windows' time
  zone rules, which apply today's daylight saving time rules to all past years: dates before about
  2000 can be an hour off where the rules changed. The today marker shows the time the diagram was
  drawn, until the code changes. d3 moves the axis lines by half a pixel on displays without high
  DPI only; the renderer always does. `click … href` links are SVG links, where Mermaid opens them
  with a script; callbacks (`call`) never run, as in Mermaid's default security level. A
  `tickInterval` that gives 10,000 ticks or more falls back to the automatic ticks, where Mermaid
  tries to draw them all, and skipping excluded days stops with an error after two million days in
  all.
- **Git graph:** commits without `id` get Mermaid's form of id (`<number>-<7 hex digits>`), but the
  digits are the same in every render, where Mermaid's are random. Parse errors name the expected
  and the found token instead of Langium's message. Errors of the statements name their line.
- **Block diagram:** composite blocks can be nested 12 levels deep; deeper nesting is an error,
  because Mermaid's layout takes twice the time for every level. `space:N` statements may add up to
  100,000 blocks in all. Anonymous composites and spaces get ids like `id:1` instead of random ones.
  A block of zero columns (`a:0`) is drawn without width at the origin, where Mermaid places all
  blocks of its row at infinity.
- **C4 diagram:** Mermaid wraps the rows of elements at the width of the screen
  (`screen.availWidth`). The viewer uses the width of the work area of the monitor it is shown on,
  Copy as HTML that of Notepad++'s monitor, both in CSS pixels; tests use 800, the width of headless
  Edge. Each word of an element label is its own `<text>`, where Mermaid writes a `<tspan>` per
  word; both are drawn without kerning between the words. Parse errors name the expected and the
  found token instead of jison's message. Boundaries can be nested 100 levels deep.
- **Mindmap:** always laid out with cose-bilkent, also where the document names another layout such
  as `layout: elk`, which Mermaid then uses. The layout amplifies differences in the last bit of its
  numbers, so it computes sines, cosines and logarithms correctly rounded like Edge, and the node
  sizes in Blink's single precision (also the box of clouds and bangs, from Skia's conics, and of
  the sketches of the `handDrawn` look, from Skia's cubics and Blink's rounding of moved and united
  boxes). Other browsers can lay the same mindmap out differently. Edge's C runtime rounds the
  angles of a few cloud arcs differently, which can still give such a mindmap another arrangement.
- **Architecture diagram:** `randomize: true` is ignored: the layout starts from the same positions
  as with `false`, without fcose's spectral layout. `seed: 0`, which leaves Mermaid's layout to
  `Math.random`, uses the seed 1. Icon texts are SVG text instead of HTML in a `foreignObject`,
  which EdgeHTML and RichEdit cannot show. Each run of words of a label in the same style is its own
  `<text>`, where Mermaid writes a `<tspan>` per word. Mermaid's breadth-first searches over the
  grid of directions queue a node again from every neighbor before visiting it, which grows with the
  number of paths through a grid of nodes and makes Mermaid hang on large grids; beyond 100,000
  queued nodes or 5,000 relative placement constraints each node is queued once, which can arrange
  such diagrams differently.
- **Tree view:** parse errors name the expected and the found token instead of Langium's message.
- **Cynefin:** with `seed: 0` (the default) Mermaid seeds the wavy boundaries from the id of the SVG
  element, which changes with every render. The seed is taken from a hash of the diagram instead, so
  the boundaries stay the same; any other `seed` gives Mermaid's boundaries. Parse errors name the
  expected and the found token instead of Langium's message.
- **Event model:** the text of a box is SVG text laid out like Mermaid's HTML in a `foreignObject`
  (EdgeHTML and RichEdit draw no `foreignObject`): the bold name centered, the data left-aligned in
  Consolas below an empty line, clipped to the box. HTML in data is shown as text. The HTML takes
  the page's color in Mermaid, black in its renders; dark themes use their text color instead, which
  their boxes need. Parse errors name the expected and the found token instead of Langium's message.
- **Venn diagram:** venn.js starts layouts of eight or more areas (four or more sets) from
  `Math.random` positions, so Mermaid may draw them differently each time; here the random numbers
  are fixed (mulberry32 seeded with 1). Text nodes are SVG text laid out like Mermaid's HTML
  `<span>`s in `foreignObject`s. The layout follows venn.js, but float rounding of the math
  functions (`atan2`, `acos`, `sin`, `cos`) may differ from the browser's in the last bit; in rare
  cases (about 1 in 400 random diagrams) this flips a tie and the circles or an outline are arranged
  slightly differently.
- **Wardley map:** parse errors name the expected and the found token instead of Langium's message.
- **Use case diagram:** parse errors name the expected and the found token instead of Chevrotain's
  message, JSON errors the offset in the JSON object instead of V8's message. Labels are sanitized
  like the other diagrams: DOMPurify would also remove event handler attributes of tags in a label
  and quote the others, so such a label can be wider than in Mermaid. A front matter `title` is not
  drawn, like in Mermaid, whose parser clears it.
- **Agentflow diagram:** the lined document shape (`refdoc`, `lin-doc`, also in flowcharts) is one
  path, where roughjs draws a fill and a stroke path of the same outline. Warnings of Mermaid's
  diagnostics (unknown shapes, containment cycles, …) are not shown, as in Mermaid's render.
- **Swimlane diagram:** ids are ordered like Edge's `localeCompare` for ASCII; other characters sort
  after the ASCII ones by code point, where ICU sorts accented letters with their base letters. The
  neo dash arrays of rounded edges can differ by up to 0.02 px, the length of the rounded corners.
  Large diagrams are slow, like in Mermaid: its clean-up passes try hundreds of routes per crossing
  edge (150 nodes with 220 edges take about 2 s).
- **Railroad diagram:** parse errors name the expected and the found token instead of Langium's
  message; at the end of the diagram they name its last line, where Mermaid says `?`. Expressions
  can be nested 100 levels deep (groups and postfix operators). Texts with tags keep them as written
  where DOMPurify would drop them or their content: a terminal `"<script>x"` shows as written, in
  Mermaid it is empty.
- **Ishikawa diagram:** each line of a label is its own `<text>`, where Mermaid writes a `<tspan>`
  per line. Without an effect Mermaid writes an empty SVG without a size; here it is an empty area
  of twice the padding.
- **Radar chart:** Mermaid sets `overflow: visible` on the diagram, so axis labels can stick out of
  its area. They are cut off at its edge instead.
- **The `handDrawn` look:** `handDrawnSeed: 0` (the default) gives a different picture with every
  render in Mermaid. Here each shape gets its own seed from the diagram and the shape's id, so the
  picture stays the same. Sketched paths are written with three decimals. Where a hachure line ends
  on a rounding tie of Mermaid's coordinates, its end can move by a pixel, because V8's `Math.sin`
  and `Math.cos` differ from the C runtime in the last bit. The background copy Mermaid draws behind
  an ER entity has no labels here.
- **EdgeHTML workarounds,** drawn the same in every engine: the neo look's drop shadow is written
  without `feDropShadow`, sequence markers without `orient="auto-start-reverse"`, and roughjs
  outlines without `stroke-dasharray="0 0"`. EdgeHTML does not support the first two and draws no
  line with the third.

## Original quirks

Mermaid behaves oddly in these cases. The renderer does the same, so that a diagram looks like it
does on GitHub or in the Mermaid Live Editor.

### All diagrams

- Diagram detection follows Mermaid's detector order. The C4 expression
  `/^\s*C4Context|C4Container|…/` lacks parentheses, so `C4Container`, `C4Component`,
  `C4Dynamic` and `C4Deployment` anywhere in the code make it a C4 diagram.
- Labels wrap at spaces like `white-space: break-spaces`: the space after a word has to fit and
  stays at the end of the line. Chromium also breaks after hyphens, so `one-to-zero-or-one`
  wraps as `one-to-zero-or-` and `one`. A word wider than the wrapping width widens the label,
  and all its lines then wrap at the width of that word and its space.
- Pie slices below 1 % are left out.
- Only flowcharts, swimlanes, class diagrams, block diagrams, kanban boards and agentflow diagrams
  have the styles of icons in labels, which lower an icon by 1/8 of its height. In state, ER and
  requirement diagrams, use case diagrams and mindmaps it stands on the baseline.
- A label of only an unknown icon (`fa:fa-bogus`) has no height, but the minimum width of the
  node. The bounding box of a node with `form: circle` and the circle's size follow the path of
  the icon, not its view box, so narrow icons get smaller circles.
- `@{ icon: "fa-user" }` finds `user` in the pack `fa`: a dash separates the pack as well, while
  `fa:fa-user` asks for an icon named `fa-user`, which does not exist.
- `<strong>` inside a bold label is `bolder` (weight 900). With Arial this is Arial Black, whose
  taller metrics make the line one pixel higher.
- A label with `$$…$$` is one row: `<br>` does not break it, and text and formulas follow each
  other without the spaces between them. Markdown stays literal (`**b**`) and `fa:` icons are
  text, while HTML such as `<b>` works.
- In labels other than those of sequence diagrams two backslashes become one before KaTeX sees
  them, so a line break in a matrix needs four: `\\\\`. `#35;` inside a formula fails with KaTeX's
  error about `&`.
- Chromium ignores KaTeX's `columnalign`, `rowspacing` and `columnspacing`: cells of `aligned`,
  `cases` and `array` are centered, with the padding of its style sheet.
- The bars of `\left|` and `vmatrix` do not grow: KaTeX writes `∣`, which the operator dictionary
  does not let stretch. `\boxed` and `\cancel` draw no box or stroke, as Chromium ignores
  `menclose`.
- At 12, 13, 15, 16, 17 and 19 pixels Cambria Math has bitmaps, for which Chromium takes whole
  pixel widths: a formula at 16 pixels is wider in proportion than at 14.
- A formula KaTeX rejects fails the whole diagram, with its message and without a line.

### The `handDrawn` look

- Every roughjs call restarts the random numbers at `handDrawnSeed`, so shapes of the same size
  are sketched alike.
- The sketch overshoots the outline, and node sizes, edge ends and the diagram's area follow the
  sketch: a sketched node is a few pixels larger than in the classic look.
- Small circles (`sm-circ`) and framed circles (`fr-circ`) keep the class `node`, so the
  flowchart rules for `.node path` paint their sketch in the node colors.
- The color slots of the palette themes stroke and fill both paths of a sketched container, the
  hachure lines too, which turns the container into a solid block of the border color.
- A `hachureGap` of 1.5 is rounded to 2, so the hachure lines are 2 pixels apart.
- Style values are cut at their second colon (`fill:url(a:b)` becomes `url(a`).
- ER entities have a copy without roughness behind them, so edge markers do not show through
  the hachure.
- Class notes have the class `undefined`.
- Kanban sections are 600 pixels tall: Mermaid sets the height of a rectangle the sketch does not
  have.
- The slash of a business use case can reach past the sketched ellipse and then widens the
  node.
- Edges with line hops (swimlanes) lose their sketch: the hops replace the sketched path.

### Kanban boards

- A blank line right after `kanban` is a parse error.
- An item whose text starts with `kanban` (in any case) is a parse error.
- `%%` after the text of a node is part of its label, not a comment.
- `:::class` and `::icon()` have no effect, and `b:::red` is the label `b:::red`.
- An item less indented than the sections is put into the last section, unless another node
  follows it ("Items without section detected").
- Only sections 1 to 10 have colors; later ones are black.

### Mindmaps

- A blank line right after `mindmap` is a parse error.
- `::icon()` has no visible effect: the shapes of Mermaid 12.2 draw no icons.
- `id(-text-)` is a default node, `id)text(` and `id(text(` are clouds, and so is `id-)text(-`,
  whose id takes the `-`.
- Labels of square and rounded nodes wrap at `flowchart.wrappingWidth` (120 pixels), the others
  at `maxNodeWidth`.
- `edge-depth-N` counts the parent's indentation in characters, not its level, so the widths of
  edges depend on the indentation. Where the width `17 - 3 (N + 1)` is negative, it falls back
  to 3 pixels.
- Edges start and end 15 pixels from the node centers, inside the nodes: cytoscape clips them at
  its default node, a circle of 30 pixels.

### Architecture diagrams

- Ids cannot start with a capital `L`, `R`, `T` or `B`: the lexer takes the letter for a
  direction, so `service Lambda` is a parse error. `1a` and `true` are ids.
- `service a["text"]` has the title `text`, not an icon text: the bracketed title comes first.
  An icon text needs a quoted string right after the id, like `service a "text"`.
- Groups are created before all services and services before all junctions, so a service or
  edge may name a group or node that is declared later, but a group must come after its parent.
- An edge to a group fails with "Cannot read properties of undefined (reading 'in')".
- Two edges between the same nodes in the same order fail with "Can not create second element
  with ID `a-b`", as does an edge `a`–`b` next to a node `a-b`.
- A node keeps one neighbor per pair of directions: of `a:R -- L:b` and `d:L -- R:a`, only `d`
  places `a`, and `b` starts a separate part of the layout.
- An `align` hint whose order contradicts the edges can leave the layout without a size; this
  fails with Mermaid's "Architecture layout failed" message. Other contradicting hints are laid
  out as well as they go.
- The title (`title`) is not drawn.
- Short icon texts sit at the left of the icon: Mermaid's flex box shrinks to the text.
- Labels on edges that bend are wrapped at half the horizontal distance of the ends, often a
  word or less per line, and drawn at 45 degrees.
- Labels show entities other than `&amp;`, `&lt;`, `&gt;` and `&#39;` as written, and inline
  code is dropped.

### Tree views

- Line ends are hidden from the grammar, so `"a" "b"` on one line are two rows (the blank
  before `"b"` is its indentation), and ` :::x` or ` ## text` at the start of a line belong to
  the row before.
- A name starting with `title`, `accTitle:` or `accDescr:` is taken for that statement: right
  after `treeView-beta`, `titles.txt` is the title `title` and the row `s.txt`; after a row, it is
  a parse error.
- A line of blanks between rows and blanks after `treeView-beta` are parse errors. `%%` after a
  name is part of the name, and `a:::x` without a blank is the name `a:::x`.
- Of repeated annotations the last wins: `a :::x :::y` has the class `y`. Only the class
  `highlight` has a style.
- Labels are black and lines black in every theme, also in dark mode; `themeVariables.treeView`
  sets the colors.
- The tree always starts at a root directory `/`. Converted `tree` output puts entries at the
  column of their parent's name one level below it, so `root/` followed by `├── a` makes `a` a
  child of `root/`.
- The title (`title`) is not drawn.

### Cynefin frameworks

- Domain names need no word end: `complexity` is the domain `complex` followed by an unexpected
  `ity`, a parse error. Other words are parse errors too.
- A second block of the same domain replaces the items of the first.
- Transitions from a domain to itself are left out.
- Themes without a `cynefin` block (`neo`, `redux` and their variants) use the colors of the
  `default` theme.
- A font size given as a string in `themeVariables.cynefin` is appended to: `domainFontSize:
  "20"` makes the title `202px` high (`"20" + 2`), while `itemFontSize: "14"` gives subtitles of
  13 pixels.

### Event models

- `title`, `accTitle` and `accDescr` are parse errors: the name terminal takes these words
  first.
- Names that are keywords of the grammar (`event`, `text`, `data`, `ui`, …) are parse errors.
- Every frame with a namespace (`Cart.Items`) opens a new swimlane, also when an earlier frame
  had the same namespace.
- Quoted inline data (`"text"`) shows an empty data block: only data in braces is shown, and
  the character before the closing brace is dropped.
- Boxes are as wide as Mermaid measures the HTML source of their text, tags and the `style`
  attribute of the data block included, and a third of that with data, so the data often
  wraps or overflows its box.
- The first frame and reset frames without `->>` sources have no incoming arrow; other frames
  get one from the latest earlier frame in another swimlane.

### Venn diagrams

- A diagram without sets fails with "Cannot read properties of undefined (reading 'set')".
- Keywords are case-insensitive (`SET A`). A `title` takes the rest of the line up to `#` or
  `;`.
- Indented `text` lines belong to the latest `set` or `union`; at the start of a line they need
  `text A, B …` with the sets.
- Unions of three or more sets get the missing pairs, sized a quarter of the smaller set (or
  2.5 without sizes), so their circles overlap.
- Sets of size 0 and the areas with them are not drawn, but still take part in placing the
  text nodes.
- Labels of the circles never wrap: venn.js measures them in a detached element.

### Wardley maps

- Mermaid removes the diagram's style sheet before drawing, so the theme's style rules do not
  apply: texts are in the page's default font (Times New Roman), and sourcing strategies keep
  their gray overlays.
- The `wardley-beta` configuration (`width`, `height`, `showGrid`, `nodeRadius`, …) cannot be
  set from a diagram, neither by a directive nor by front matter; `size [w, h]` sets the size.
- Keywords end no word: `components X` is a component named `s X`, and a name starting with
  a lowercase keyword or strategy (`market share`, `notes`) is a parse error.
- Components and anchors need decimal coordinates: `[1, 0]` is a parse error. Values up to 1
  are fractions, larger ones percent.
- `[x, y]` of annotations and accelerators are visibility and evolution like those of
  components, not x and y.

### Agentflow diagrams

- A `flow` container that ends up without children (empty, holding only a `connector` block, or
  whose members `global` moved out) is drawn as a node of the shape `flowGroup`, which does not
  exist: "No such shape: flowGroup. Please check your syntax." Collapsed containers are fine.
- An edge from or to `connectors`, the reserved id of connector blocks, fails with "Cannot read
  properties of undefined (reading 'shape')".
- A node with a `click` link loses the neo look and the kind colors of the color themes: the
  link element takes the node's `data-look`, which these rules are keyed on. This holds for the
  linked nodes of every diagram.
- `classDef` styles of a container's class reach its frame. Its own `style` colors only its
  title; the frame keeps the theme's colors.

### Swimlane diagrams

- A self loop fails with "Cannot read properties of undefined (reading 'filter')": the router
  gives it no points.
- Lane titles show `\n` as written, where node labels break the line.
- Edge labels become nodes of the layout that wrap at the node wrapping width, and are drawn
  without the label background of flowchart edges.

### Use case diagrams

- `colorScheme: rotate` colors only the actors and the boundaries: the role colors of use case
  bodies and business markers have the same specificity as the palette rules and come later in
  the style sheet, so use cases keep their role colors.
- The edge label background has a horizontal padding of 2 pixels, so even an edge without a
  label has a label 4 pixels wide, which the layout keeps room for.
- A style or stroke width of an actor reaches only the fill of its glyph and its business
  marker: the glyph's own rules set the stroke and, in the neo look, the stroke width of its
  lines. An icon actor's question mark is outlined in the actor's border color.
- JSON table cells, stereotypes and boundary titles are Markdown, so `*`, `_` and backticks in
  them format the text; the table title is plain.
- `X@{ … }` alone on a line assigns metadata to `X` and is an error if `X` is declared nowhere;
  `package`, `rectangle`, `skinparam`, `newpage` and `allowmixing` cannot start a statement, but
  may name an actor and take metadata. `actor A:::c, B` gives `A` the classes `c` and `B`.
- `encodeEntities` removes the last `;` of a `style` or `classDef` line with a `#` color before
  parsing, so `classDef x fill:#fff;stroke:#000` is accepted as the style `fill:#fffstroke:#000`;
  any other `;` in a style is an error.
- Metadata separators are decided with three tokens of lookahead: before `}`, a comma followed by
  an empty line or two empty lines without a comma are errors, one empty line is not.
- Edge labels never wrap: the padding of their background keeps the label from reaching its
  `max-width` exactly, which is what switches Mermaid's labels to wrapping, so a long label stays
  on one line and overflows its 200 pixel background.
- Self loops are not merged into one loop like in flowcharts: the three segments dagre routes
  through its helper nodes are drawn one by one, and each is cut at the looped node at both
  ends, so the loop is drawn as two strokes out and back and one around. Requirement diagrams
  draw their self loops the same way.

### Railroad diagrams

- The title is parsed but not drawn. A rule defined twice is drawn twice.
- The options `arcRadius`, `verticalSeparation`, `horizontalSeparation` and `markerRadius`
  cannot be set from a diagram: Mermaid's `sanitizeDirective` drops keys its default
  configuration does not have. `padding`, `strokeWidth`, `fontSize`, `fontFamily` and the
  colors work. `compactMode`, `showMarkers` and the comment colors have no effect.
- Rule names are bold, but measured in the regular weight, so long names come close to the
  start marker.
- The header keywords are case-sensitive, although the diagram is detected in any case:
  `Railroad-beta` is a parse error.
- EBNF: a postfix `?` opens a special sequence when another `?` follows before the next `;`:
  `a = b? c ? ;` is `b` followed by the special sequence `c`. `-` excepts only the next
  primary and is drawn as a sequence with a `-` terminal.
- ABNF: `;` starts a comment that runs to the end of the line, and the rule's own `;` only
  ends it when nothing follows on its line: `a = b ; note` misses its `;`. `0*1x` is drawn as
  an option, any other repetition with a minimum of 0 with a bypass, otherwise as a loop.
- PEG: `&x` and `!x` are drawn as special sequences of their text; a group or repetition
  after them shows as `(...)`.

### Ishikawa diagrams

- A blank line right after `ishikawa` is a parse error, and so is a line starting with
  `ishikawa` (in any case) after it. The effect may follow the keyword on the same line.
- The levels count from the indentation of the first cause, so a cause indented less than the
  first one is still a cause, and the effect may be indented more than the causes.
- `%%` after the text of a node is part of its label.
- `ishikawa` without any node draws nothing.

### User journeys

- A task named like `Title bar: 5: Me` is the title `bar: 5: Me`: `title` followed by a space
  takes the whole line. A task starting with `journey` (in any case) is a parse error.
- `actorColours`, `sectionFills` and `sectionColours` of the configuration have no effect,
  because Mermaid drops arrays from it.
- Scores below 1 or above 5 move the face beyond the task lines, out of the diagram for scores
  below 0. A score that is no number shows a neutral face at the top.
- Consecutive sections of the same name are one section. Tasks before the first section have no
  section.
- With `textPlacement: tspan` or `old` the text of sections takes the fill of their rectangle
  and cannot be seen.
- Task texts are not HTML: `<b>` and `<br>` show as written.

### Timelines

- The height of the period boxes is measured from the text `[object Object]`, because Mermaid
  passes the task itself instead of its text. A period with several lines is taller only from
  where it is drawn on, within its section.
- Text wraps at spaces and `<br>`; `<br/>` and `<BR>` show as written. A first word wider than
  the box sticks out of it and leaves an empty first line.
- Periods before the first section are not drawn when there are sections. A section name used
  twice draws the periods of both under each section.
- `title` followed by a space takes the whole line, colons included. An event needs a space
  after its colon: `2002 :x` is a parse error, and `a:b` inside an event is text.
- `timeline TD`: the time axis and the event lines have no arrowheads (they refer to a marker
  that is not there), the events of a long period overlap the next section, and with the neo
  look the gradient outline is missing, so boxes have no outline.

### Sankey diagrams

- Only printable ASCII is allowed, also in quoted fields: `Müll` or a tab is a parse error. So is
  an entity like `#35;`, which Mermaid encodes into other characters before parsing.
- Spaces before a quoted field (`A, "B"`) and whitespace-only lines are parse errors; empty
  lines are fine. Empty fields (`A,,1`, `A,"",1`) are parse errors, a field of spaces is a node
  with an empty name.
- Line breaks inside quoted fields shrink to one. The value of a label shows on one line with
  the name, because Mermaid separates them with a line break, which SVG text ignores.
- Values are read like `parseFloat`: `12abc` is 12, a value that is no number gives a link
  without width. If all values of every column are 0, nothing but the labels is drawn.
- With values shown, the node padding grows by 15 px, so with more than about 15 nodes in a
  column the nodes and links get no height (the energy example of the documentation).
- A link to its own source, or a cycle, is the error "circular link".

### Quadrant charts

- Points are black without outline unless styled: the default `quadrantPointFill` of every theme
  is the invalid color `hsl(240, 100%, NaN%)`, because Mermaid's theme darkens without amount.
- With points, the x-axis labels are always at the bottom, whatever `xAxisPosition` says.
- Points written later are drawn first, so earlier points lie on top of them.
- `title` takes the rest of the line, semicolons included; `titleX` is ordinary text.
- Coordinates are `1`, `0` or `0` followed by any character and digits, read like JavaScript's
  `Number()`: `0x5` is 5 (far outside the chart), `0a5` is no number. `1.0` is a lexical error.
- A `classDef` applies to all points, also to earlier ones; of two with the same name the last
  wins.
- Texts that contain `<` are sanitized like HTML and shown as such: `A & <b>B</b>` shows as
  `A &amp; <b>B</b>`, and a cut-off tag like `x<y` is dropped (`x`).

### Gantt charts

- The `topAxis` statement fails with "yy.TopAxis is not a function": the grammar calls a function
  the database does not have. Only the configuration option `topAxis: true` works.
- Without `dateFormat`, dates are read by the browser's `Date.parse`: `2014-01-01` is midnight in
  UTC, `2014-01-01 10:00` and `Jan 8 2014` are local times.
- A task whose data are only tags (`:done, crit`) fails with "Cannot read properties of
  undefined (reading 'match')", one with more than three parts with "… (reading 'type')", and a
  first task with only a duration with "… (reading 'endTime')".
- `after` with only unknown ids starts the task at midnight today.
- A line that starts with a date (`2014-01-01 :3d`) and the keyword `accDescription` are parse
  errors. `title` at the end of a line takes the next line as the title.
- Section names that are numbers (`section 10`) come first, in ascending order, in the section
  titles and in compact mode, like the keys of a JavaScript object.

### Block diagrams

- Keywords end at a word boundary (`blockArrow`, `spaces` and `endpoint` are ids), but `~` is
  part of ids: `A ~~~ B` is three blocks. Labels must be quoted and not empty (`a[""]` is a parse
  error), Markdown strings (`` "`…`" ``) are parse errors, and so are `accTitle`, `accDescr`,
  `linkStyle` and `classDef DEFAULT`.
- A chain of edges (`a --> b --> c`) takes the fields of the array of its first edge: `a`, `b`
  and the first edge are dropped, an empty block takes their place and the second edge is not
  drawn. The start of an edge loses its span: `a:2 --> b` spans one column.
- A `class` statement before its block is defined (`class A blue` and then `A`) creates the block,
  which then is not drawn. Ids after a comma and a blank (`class a, b blue`) create a new block
  under the trimmed id: the drawn `b` does not get the class and edges to it end at the origin.
- A `style` statement for a block that is not defined yet, or for a list of ids, fails with
  "Cannot set properties of undefined (setting 'styles')".
- An edge from or to a diamond (`a{"…"}`) fails with "this.calcIntersect is not a function".
- `columns 0` fails with "Columns must be an integer !== 0.".
- The second `block:g … end` of the same id draws none of its blocks.
- Composite blocks and block arrows take the styles of their classes over their own `style`;
  the other shapes take their own.
- A double circle grows in the second drawing: its inner radius becomes half the laid out
  width plus the padding, so it overlaps its neighbors.

### C4 diagrams

- An unquoted argument takes everything up to the next comma, including `)` and line ends:
  `Person(a, b)` and a `Person(c, "C")` in the next line give one person labeled
  `b)⏎Person(c`. A string or `$key="value"` that is still open at the end of the text is a parse
  error, and so is a line that mentions a direction (`Person(a, "direction TB")`).
- A key-value argument in place of the alias makes the alias `[object Object]`, which never
  matches another element; in place of the label it shows no name. A relationship with a
  key-value label (`Rel(a, b, $label="x")`) fails with "t.split is not a function", except in
  `C4Dynamic`, where it is numbered as `1: [object Object]`.
- A relationship to an unknown alias fails with "C4 rel … references an unknown shape or
  boundary", one to the implicit `global` boundary with "C4 element "global" has no intersect
  function".
- `accTitle` is also the visible title; the `title` of the front matter is not shown.
- Element labels wrap with `c4.wrap` (on by default); boundary and relationship labels only with
  the top-level `wrap: true`. Words that are too wide are broken into characters. The first
  relationship is a straight line, all others are curves.
- A per-type font size without a unit (`systemFontSize: "18"`) or an empty font family is
  ignored, and the label takes the diagram's font (16 px), which also scales the type and
  description lines.

### Git graphs

- Langium tries `TITLE` before `REFERENCE` and keywords before both, so branch names cannot be
  keywords (`branch commit`, `branch LR`) or start with `title` (`branch titleX`), and a name that
  starts with a digit is a parse error (`branch 1abc`), unless quoted. Names may not end with `.`
  (`branch a.`). Commit types are case-sensitive (`type: reverse` is an error), `order:` takes
  integers only (`order: 01` is an error) and `gitGraph TB` needs its colon.
- A commit id is also a class of the bullet: an id like `arrow` or `branch` gives the bullet the
  style of arrows or branch lines.
- A commit id used twice replaces the earlier commit in its place; arrows from its children
  start at the later one.
- `cherry-pick` with `tag: ""` shows no tag; without `tag:` it shows `cherry-pick:<id>`.

### XY charts

- Unquoted texts lose their spaces: `title Sales Revenue` shows `SalesRevenue`.
- Markdown strings are not supported: ``"`**x**`"`` shows the backticks and asterisks.
- Keywords need a word boundary: `barn [1, 2]` is a parse error. In the data of an axis, digits
  after a word end the text, so `x-axis Q1 [a, b]` is a parse error too, and so are an empty
  category `""`, numbers with an exponent (`3e5`) and letters beyond ASCII outside quotes.
- Bar data labels (`showDataLabel`) show the values of the first plot for every bar plot, even
  when the first plot is a line.
- A plot with more values than categories loses the rest; one with fewer has no bar there and
  its line ends there. A category listed twice is one position, shared by the values of both.
- Plots before the `x-axis` statement keep the positions 1, 2, … of their values: with a band
  axis after them, all their values go to the first category.

### Treemaps

- Every run of blanks is an indentation token, also inside a line: blanks at the end of a line,
  before `:::` or before a `%%` comment on its own line are parse errors.
- Several roots are allowed, although Mermaid's validator forbids them; leaves at the top level
  and their sections are transparent.
- Values are read like `parseFloat` after removing commas: `1_000` is 1 and `1.5.2` is 1.5. A
  section without leaves is drawn as an empty leaf.
- `classDef` names need two characters at least; `classDef c fill:#f00` is a parse error. A
  `classDef` without styles still adds an (invalid) empty style, and `\,` keeps a comma in a
  style.
- The value of a leaf is shown only when its label fits; labels of treemaps with more than 20
  leaves are smaller and may overflow their cell.

### Radar charts

- Names (of axes and curves) that start with `circle`, `polygon`, `true`, `false`, `title` or a
  digit are parse errors: Langium's lexer takes the first terminal that matches, so `circleA` is
  `circle` followed by `A`.
- Only the first 12 curves have colors; later ones are black.
- With `min` equal to `max` no curve is drawn.

### Sequence diagrams

- Formulas are measured in the default font of the page, 16 pixels, but drawn in the diagram's
  font size. Lines of a message or note with a formula have no line of their own in the height
  of the message.
- Text lines next to formulas in messages, notes and block titles (`$$x$$<br/>two words`) break
  at every space, as Mermaid measures them in an element without a width.
- Text is measured in the page's default font (Times New Roman) and drawn in the theme's font:
  Mermaid's default `fontFamily` ends with `;`, which makes it invalid for measuring. Only a
  top-level `fontFamily` changes that.
- The rule `line { stroke-width: 2px }` hits every line of the diagram, not just the lifelines.
- Notes and label boxes refer to a `drop-shadow` filter that does not exist, so they have no
  shadow.

### State diagrams

- `classDef default` is a parse error.
- `state Foo` followed by `state Bar {` is a parse error.
- A note before its state fails with "No such shape: undefined".
- A `--` with nothing after it leaves a divider without region and fails with "No such shape:
  divider".

### ER diagrams

- A relationship label of several words needs quotes. Without them only the first word is the
  label and the next word becomes a new entity: `A ||--o{ B : places order` adds an entity
  `order`.
- The parts of a style are joined without the blanks between them:
  `style A stroke-dasharray: 5 5` sets `stroke-dasharray:55`.
- A blank at the end of a `classDef` or `style` line makes the line end part of the style, so the
  next line is read as more style text: in `classDef c fill:#f00␣` followed by `A:::c`, the
  entity `A` is lost.
- `direction TB`, `BT`, `LR` or `RL` anywhere in a line takes the whole line, even after a
  relationship: `A ||--|| B : "x" direction LR` only sets the direction.
- `u` before `.`, `-` or `|` is the `md_parent` cardinality, also in names: `u--o{ C : y` is a
  parse error, not an entity `u`.
- `md_parent` has no marker: its end of the line is drawn plain.
- `(` in a style, as in `fill:rgb(1,2,3)`, is a lexical error.
- An attribute type with generics (`List~int~`) loses its Markdown formatting and line breaks:
  Mermaid replaces the cell's content by its text to show the angle brackets.
- Without the top-level `htmlLabels` setting, entities with attributes get 1.25 times the
  configured `diagramPadding` and `entityPadding`; entities without attributes do not.
- In the `default`, `dark`, `forest`, `neutral` and `base` themes, the fill of `classDef` and
  `style` goes to the name and the even attribute rows only. Odd rows keep the theme's row
  color, so white text from `color:#fff` is invisible there. The `redux` themes fill every row.
- Entity names without attributes wrap at `flowchart.wrappingWidth` (120 px), also after hyphens:
  `DELIVERY-ADDRESS` takes two lines.
- Parse errors name the line of the last token before the error, like jison. A missing label
  `A ||--o{ B` followed by a new line is reported on the line of `A`.
- Relationship labels are 14 px, whatever the theme's font size.
- The title has no style rule of its own: it gets the diagram's font and text color.

### Requirement diagrams

- Keywords are matched before names and values, ignoring case: a value that starts with one,
  such as `text: test case` (`test`) or `type: element x`, is a parse error. Quote it.
- Unquoted names and values end at `:`, `,`, `{`, `<`, `>`, `-` and `=`.
- `direction TB`, `BT`, `LR` or `RL` anywhere in a line takes the whole line, also inside a
  body: `text: set direction LR` is a parse error.
- Blanks after `{` swallow its line end, which is a parse error.
- A field given twice keeps its first value.
- The divider below the name is only drawn if a field other than the last one (verification
  method, document reference) has a value.
- The fields start at the left of the box, the type and name are centered.
- `style` with an unknown name stops there: `style x,b fill:#f00` does not style `b`.
- `classDef default` styles only the requirements and elements defined before it.
- `%` in a style is a parse error. The parts of a style are joined without the blanks between
  them.
- Spacing, title margin and `useMaxWidth` come from the `state` configuration, the edge curve
  from `flowchart.curve`.

### Class diagrams

- `classDef` styles only the classes that have the class when it is defined: `classDef red …`
  followed by `cssClass "A" red` or `class A:::red` leaves `A` unstyled.
- `classDef` compares its name with the class list as text, so `classDef def color:#090` styles
  every class (their list contains `default`).
- A linked class (`link`, `click … href`) loses the colors of its color slot and the neo look's
  stroke: its `data-look` attribute moves to the link.
- Notes do not wrap, and their lines are left-aligned.
