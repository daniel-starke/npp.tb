use super::text::testing::FixedMeasure;
use super::*;

fn render_light(source: &str) -> Result<Diagram> {
	render(
		source,
		&Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		&mut FixedMeasure,
	)
}

/// Front matter that selects the `elk` layout.
const ELK_FRONT_MATTER: &str = "---
config:
  layout: elk
---
";

/// The SVG parses and has the natural size.
fn assert_valid_svg(diagram: &Diagram) {
	assert!(
		diagram.svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""),
		"{}",
		diagram.svg
	);
	assert!(diagram.svg.ends_with("</svg>"));
	let (_, width, _) = npp::svg::render_png_scaled(&diagram.svg, 1.0)
		.unwrap_or_else(|| panic!("invalid SVG: {}", diagram.svg));
	assert_eq!(f64::from(width), diagram.width.ceil());
}

#[test]
fn detects_diagram_types_in_mermaid_order() {
	use DiagramKind::*;
	let cases = [
		("pie title x", Some(Pie)),
		("  \n graph TD", Some(Flowchart)),
		("flowchart-elk TD", Some(FlowchartElk)),
		("stateDiagram-v2", Some(State)),
		("sequenceDiagram %% comment", Some(Sequence)),
		("%% only a comment\nclassDiagram", Some(Class)),
		("graph TD\nA[C4Container]", Some(C4)),
		("ISHIKAWA", Some(Ishikawa)),
		("ishikawas", None),
		("cynefin-beta:", Some(Cynefin)),
		("cynefin-betas", None),
		("usecase-beta", Some(Usecase)),
		("error", Some(Error)),
		("--- \nx", Some(FrontMatterOnly)),
		("unknown", None),
		("", None),
	];
	for (code, expected) in cases {
		assert_eq!(DiagramKind::detect(code), expected, "{code}");
	}
}

/// How often `svg` has `needle`.
fn count(svg: &str, needle: &str) -> usize {
	svg.matches(needle).count()
}

#[test]
fn renders_icons_in_labels() {
	let source = "flowchart LR\nA[\"fa:fa-car Car\"] -- \"fab:fa-github\" --> B[\"far:fa-clock fas:fa-user\"]\nC[\"fa:fa-bogus fal:fa-car fak:fa-car Text\"]\nsubgraph S[\"fa:fa-twitter\"]\nD\nend\n";
	for is_dark in [false, true] {
		let options =
			Options { is_dark, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH };
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		let expected = if cfg!(feature = "font-awesome") { 5 } else { 0 };
		assert_eq!(count(&diagram.svg, "class=\"label-icon\""), expected, "{}", diagram.svg);
		assert!(!diagram.svg.contains("fa:fa-"), "{}", diagram.svg);
	}
	// Other diagrams on the labels of HTML, and math, which leaves the reference as text.
	for source in [
		"stateDiagram-v2\nA: fa:fa-car x\n",
		"classDiagram\nclass A[\"fa:fa-car x\"]\n",
		"erDiagram\n\"fa:fa-car x\" {\n}\n",
		"block\na[\"fa:fa-car x\"]\n",
		"kanban\ntodo[Todo]\n  a[fa:fa-car x]\n",
		"mindmap\nroot((fa:fa-car x))\n",
		"swimlane-beta\nsubgraph S\nA[\"fa:fa-car x\"]\nend\n",
		"usecase-beta\nactor A(\"fa:fa-car x\")\n",
		"agentflow-beta\nA[\"fa:fa-car x\"]\n",
	] {
		let diagram = render_light(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
		assert_valid_svg(&diagram);
		let expected = usize::from(cfg!(feature = "font-awesome"));
		assert_eq!(count(&diagram.svg, "class=\"label-icon\""), expected, "{source}");
	}
}

#[test]
fn renders_icon_shapes() {
	for (form, background_paths) in
		[("", 0), (", form: \"square\"", 2), (", form: \"circle\"", 2), (", form: \"rounded\"", 2)]
	{
		for position in ["", ", pos: \"t\""] {
			for label in ["", ", label: \"User\""] {
				let source = format!(
					"flowchart LR\nA@{{ icon: \"fa:user\"{form}{position}{label} }} --> B@{{ icon: \"fa:nonexistent\"{form}, h: 60 }}\n"
				);
				let diagram = render_light(&source).unwrap();
				assert_valid_svg(&diagram);
				let svg = &diagram.svg;
				assert_eq!(count(svg, "class=\"icon-shape default\""), 2, "{svg}");
				assert_eq!(count(svg, "fill=\"transparent\""), 2, "{svg}");
				assert_eq!(
					count(svg, "stroke=\"none\" fill=\""),
					background_paths,
					"{source}: {svg}"
				);
				// The icon, or the unknown icon for both without Font Awesome.
				let (glyphs, unknown) =
					if cfg!(feature = "font-awesome") { (1, 1) } else { (0, 2) };
				assert_eq!(count(svg, "viewBox=\"0 0 448 512\""), glyphs, "{svg}");
				assert_eq!(count(svg, "viewBox=\"0 0 80 80\""), unknown, "{svg}");
				assert_eq!(count(svg, ">?</text>"), unknown, "{svg}");
			}
		}
	}
	// A size of the asset scales the icon.
	let diagram = render_light("flowchart LR\nA@{ icon: \"x:y\", h: 60, w: 30 }\n").unwrap();
	assert!(diagram.svg.contains("width=\"60\" height=\"60\" viewBox=\"0 0 80 80\""));
}

#[cfg(feature = "font-awesome")]
#[test]
fn centers_icon_shapes_on_the_path_of_the_icon() {
	let diagram =
		render_light("flowchart LR\nA@{ icon: \"fa:i\", form: \"circle\", label: \"x\" }\n")
			.unwrap();
	// The icon `i` is 320 wide and its path 56 pixels of the scaled box less wide.
	let svg = &diagram.svg;
	assert!(svg.contains("viewBox=\"0 0 320 512\""), "{svg}");
	let i = super::icons::font_awesome("fa", "i").unwrap().box_in_square(48.0);
	assert!(i.x > 0.0 && i.width < 48.0 && i.height < 48.0);
	let diameter = i.width.max(i.height) * std::f64::consts::SQRT_2 + 40.0;
	assert!(svg.contains(&format!("width=\"{}\"", svg::number(diameter))), "{svg}");
}

#[test]
fn renders_icons_of_other_diagrams() {
	let has_icons = cfg!(feature = "font-awesome");
	let architecture = render_light(
		"architecture-beta\ngroup g(fa:cloud)[G]\nservice a(fab:github)[A] in g\nservice b(database)[B]\nservice c(logos:aws)[C]\nservice d(fa-user)[D]\n",
	)
	.unwrap();
	assert_valid_svg(&architecture);
	assert_eq!(count(&architecture.svg, ">?</text>"), if has_icons { 1 } else { 4 });
	assert_eq!(count(&architecture.svg, "viewBox=\"0 0 496 512\""), usize::from(has_icons));
	let tree = render_light(
		"treeView-beta\nroot/\n  a icon(fa:car)\n  b icon(fab:github)\n  c icon(fa:bogus)\n  d icon(file)\n",
	)
	.unwrap();
	assert_valid_svg(&tree);
	assert_eq!(count(&tree.svg, "viewBox=\"0 0 80 80\""), if has_icons { 1 } else { 3 });
	assert_eq!(count(&tree.svg, "viewBox=\"0 0 24 24\""), 1);
	let usecase = render_light(
		"usecase-beta\nactor A@{ icon: \"fa:user\" }\nactor B@{ icon: \"user\" }\nactor C@{ icon: \"bogus\" }\n",
	)
	.unwrap();
	assert_valid_svg(&usecase);
	assert_eq!(count(&usecase.svg, "usecase-actor-icon-fallback"), if has_icons { 1 } else { 3 });
	assert_eq!(count(&usecase.svg, "viewBox=\"0 0 448 512\""), if has_icons { 2 } else { 0 });
}

#[test]
fn renders_pie_charts() {
	let diagram = render_light(
		"---\ntitle: Key elements\n---\npie showData\n    \"Calcium\" : 42.96\n    \"Potassium\" : 50.05\n    \"Magnesium\" : 10.01\n    \"Iron\" :  5\n    \"Tiny\" : 0.01\n",
	)
	.unwrap();
	assert_valid_svg(&diagram);
	assert!(diagram.svg.contains(">Key elements</text>"));
	assert!(diagram.svg.contains(">Calcium [42.96]</text>"));
	// Slices below 1 % get a legend entry but no slice.
	assert_eq!(diagram.svg.matches("class=\"pieCircle\"").count(), 4);
	assert!(diagram.svg.contains(">46%</text>"));
	// Default theme colors.
	assert!(diagram.svg.contains("fill=\"#ECECFF\""));
	assert_eq!(diagram.height, 450.0);
}

#[test]
fn applies_theme_and_config_of_the_document() {
	let diagram = render_light(
		"%%{init: {'theme': 'forest', 'pie': {'legendPosition': 'top'}, 'themeVariables': {'pie1': '#123456'}}}%%\npie\n\"a\" : 1\n\"b\" : 2",
	)
	.unwrap();
	assert!(diagram.svg.contains("fill=\"#123456\""));
	assert!(diagram.height > 450.0);
	let dark = render(
		"pie\n\"a\" : 1",
		&Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		&mut FixedMeasure,
	)
	.unwrap();
	assert_ne!(dark.svg, render_light("pie\n\"a\" : 1").unwrap().svg);
}

#[test]
fn reports_errors_with_lines_of_the_code_block() {
	let error = render_light("---\ntitle: x\n---\npie\n\"a\" : 1\nnonsense").unwrap_err();
	assert_eq!(error.line, Some(6));
	let error = render_light("pie\n\"a\" : -1").unwrap_err();
	assert!(error.message.contains("Negative values"));
	assert!(render_light("nothing").is_err());
	let limits = Limits { max_text_size: 10, max_edges: 500 };
	let error = render(
		"pie\n\"long label\" : 1",
		&Options { is_dark: false, limits, screen_width: DEFAULT_SCREEN_WIDTH },
		&mut FixedMeasure,
	)
	.unwrap_err();
	assert!(error.message.contains("Maximum text size"));
}

#[test]
fn renders_info() {
	let diagram = render_light("info\nshowInfo").unwrap();
	assert_valid_svg(&diagram);
	assert!(diagram.svg.contains("v12.2.0"));
}

#[test]
fn ids_depend_on_source_and_theme() {
	let first = render_light("pie\n\"a\" : 1").unwrap();
	let second = render_light("pie\n\"b\" : 1").unwrap();
	let id = |diagram: &Diagram| {
		diagram.svg.split("id=\"").nth(1).unwrap().split('"').next().unwrap().to_owned()
	};
	assert_ne!(id(&first), id(&second));
	assert_eq!(id(&first), id(&render_light("pie\n\"a\" : 1").unwrap()));
}

#[test]
fn renders_random_input_without_panicking() {
	const FRAGMENTS: [&str; 40] = [
		"pie",
		" showData",
		"info",
		"\n",
		" ",
		"\"",
		"'",
		":",
		"-",
		"1",
		"0.5",
		".",
		"title",
		" title ",
		"accTitle",
		"accDescr",
		"{",
		"}",
		"%%",
		"%%{",
		"}%%",
		"init",
		"{\"theme\":\"dark\"}",
		"---",
		"\nconfig:\n  theme: forest",
		"\"a\" : 1\n",
		"\\",
		"ä😀",
		"<b>",
		"#",
		"graph",
		"-->",
		"[",
		"]",
		"(",
		")",
		"\t",
		"showInfo",
		"e5",
		"\r\n",
	];
	let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
	let mut next_random = move || {
		state ^= state << 13;
		state ^= state >> 7;
		state ^= state << 17;
		state
	};
	let iterations = std::env::var("RANDOM_RENDER_ITERATIONS")
		.ok()
		.and_then(|value| value.parse().ok())
		.unwrap_or(2000);
	for _ in 0..iterations {
		let length = next_random() % 30;
		let source: String = (0..length)
			.map(|_| FRAGMENTS[(next_random() % FRAGMENTS.len() as u64) as usize])
			.collect();
		if let Ok(diagram) = render_light(&source) {
			assert!(diagram.svg.ends_with("</svg>"));
		}
	}
}

#[test]
fn renders_math_in_labels() {
	for source in [
		r#"flowchart LR
A["$$x^2$$"] -->|"$$\sqrt{x+3}$$"| B("a $$\frac{1}{2}$$ b")
subgraph S["$$\alpha$$"]
C["$$\begin{pmatrix}a&b\\\\c&d\end{pmatrix}$$"]
end"#,
		r#"stateDiagram-v2
state "$$x^2$$" as a
a --> b: $$\frac{1}{2}$$
note right of b: $$y$$"#,
		r#"classDiagram
class A["$$x^2$$"]
A --> B: $$y$$"#,
		r#"erDiagram
A ||--o{ B : "$$\frac{1}{2}$$""#,
		r#"requirementDiagram
requirement r {
id: 1
text: "$$\sqrt{x}$$"
}"#,
		"mindmap\nroot[\"$$x^2$$\"]\n  a",
		"kanban\ntodo[\"$$x^2$$\"]\n  a[\"$$y$$\"]",
		"block-beta\nA[\"$$x^2$$\"]",
		r#"sequenceDiagram
participant a as $$\frac{x}{y}$$
a->>b: Solve $$\sqrt{2}$$<br/>two words
Note over a,b: $$x$$
a->>a: $$y$$
loop $$x^2$$
b->>a: text
end
alt $$a$$
a->>b: x
else $$b$$
b->>a: y
end"#,
	] {
		let diagram = render_light(source).unwrap_or_else(|error| panic!("{source}: {error}"));
		assert_valid_svg(&diagram);
		assert!(!diagram.svg.contains("$$"), "{source}: {}", diagram.svg);
		assert!(diagram.svg.contains("Cambria Math"), "{source}");
	}
	// Mermaid's other diagrams and SVG labels show the text.
	let pie = render_light("pie\n\"$$x^2$$\": 1").unwrap();
	assert!(pie.svg.contains("$$x^2$$"));
	let svg_labels =
		render_light("%%{init: {\"htmlLabels\": false}}%%\nflowchart LR\nA[\"$$x^2$$\"]").unwrap();
	assert!(svg_labels.svg.contains("$$x^2$$"));
	let svg_labels = render_light(
		"%%{init: {\"flowchart\": {\"htmlLabels\": false}}}%%\nflowchart LR\nA[\"$$x^2$$\"]",
	)
	.unwrap();
	assert!(svg_labels.svg.contains("$$x^2$$"));
	// A formula KaTeX rejects fails the diagram like in Mermaid.
	for source in [
		"flowchart LR\nA[\"$$\\foo$$\"]",
		"sequenceDiagram\na->>b: $$\\foo$$",
		"sequenceDiagram\nparticipant a as $$\\foo$$",
		"stateDiagram-v2\na --> b: $$x^$$",
	] {
		let error = render_light(source).err().unwrap_or_else(|| panic!("{source}"));
		assert!(error.message.starts_with("KaTeX parse error: "), "{}", error.message);
		assert_eq!(error.line, None);
	}
}

#[test]
fn renders_random_math_labels_without_panicking() {
	const FRAGMENTS: [&str; 62] = [
		"\\smash",
		"\\hphantom",
		"\\boxed",
		"\\cancel",
		"\\xcancel",
		"\\colorbox{red}",
		"\\fcolorbox{red}{blue}",
		"\\rule",
		"\\raisebox",
		"\\pmb",
		"\\href{x}",
		"\\def\\a#1{#1#1}\\a",
		"\\newcommand",
		"\\tag",
		"\\substack",
		"\\genfrac",
		"\\begin{CD}@>",
		"\\begin{align}",
		"$$",
		"$",
		"x",
		"1",
		" ",
		"\\n",
		"<br/>",
		"<b>",
		"</b>",
		"{",
		"}",
		"^",
		"_",
		"&",
		"\\\\",
		"\\\\\\\\",
		"\\frac",
		"\\sqrt",
		"[3]",
		"\\sum",
		"\\int",
		"\\left",
		"\\right",
		"(",
		")",
		"|",
		"\\begin{matrix}",
		"\\end{matrix}",
		"\\begin{cases}",
		"\\end{cases}",
		"\\hat",
		"\\overline",
		"\\overbrace",
		"\\overrightarrow",
		"\\text",
		"\\mathbb",
		"\\color{red}",
		"\\textcolor",
		"\\phantom",
		"\\binom",
		"\\big",
		"\\foo",
		"\\displaystyle",
		"\\,",
	];
	let mut state = 0x7A3D_11C9_46E0_B285u64;
	let mut next_random = move || {
		state ^= state << 13;
		state ^= state >> 7;
		state ^= state << 17;
		state
	};
	let iterations = std::env::var("RANDOM_RENDER_ITERATIONS")
		.ok()
		.and_then(|iterations| iterations.parse().ok())
		.unwrap_or(2000usize);
	for index in 0..iterations {
		let length = next_random() % 24;
		let text: String = (0..length)
			.map(|_| FRAGMENTS[(next_random() % FRAGMENTS.len() as u64) as usize])
			.collect();
		// A flowchart string cannot hold quotes, the fragments have none.
		let source = match index % 3 {
			0 => format!("flowchart LR\nA[\"$${text}$$\"] -->|\"{text}\"| B"),
			1 => format!("sequenceDiagram\nparticipant a as $${text}$$\na->>b: $${text}$$"),
			_ => format!("classDiagram\nclass A {{\n+$${text}$$ x\n}}\nA --> B: {text}"),
		};
		if let Ok(diagram) = render_light(&source) {
			assert!(diagram.svg.ends_with("</svg>"), "{source}");
		}
	}
}

#[test]
fn renders_random_flowcharts_without_panicking() {
	const FRAGMENTS: [&str; 56] = [
		"fa:fa-car",
		"fab:fa-github ",
		" fal:fa-x",
		"fa:fa-",
		"@{ icon: \"fa:user\", form: \"circle\" }",
		"@{ icon: \"fa-\", form: \"square\", pos: \"t\", h: 0 }",
		"@{ icon: \":\", form: \"rounded\", w: -5 }",
		"@{ icon: \"@a:b:c:d\" }",
		"A",
		"B",
		"c1",
		" ",
		"\n",
		";",
		"-->",
		"---",
		"-.->",
		"==>",
		"--",
		" text ",
		"|",
		"|x|",
		"[",
		"]",
		"(",
		")",
		"{",
		"}",
		"((",
		"))",
		"[(",
		")]",
		"[/",
		"\\]",
		">",
		"subgraph",
		" s1",
		" s2 [Title]",
		"\nend\n",
		"direction LR\n",
		" & ",
		":::cls",
		"classDef cls fill:#f9f\n",
		"style A stroke:#333\n",
		"linkStyle 0 stroke:red\n",
		"click A \"https://x\"\n",
		"@{ shape: hex }",
		"@{ shape: doc, label: \"d\" }",
		"\"q\"",
		"\"`**m**`\"",
		"<br>",
		"~~~",
		"o",
		"x",
		"A --> A\n",
		"e1@-->",
	];
	let mut state: u64 = 0x2545_F491_4F6C_DD1D;
	let mut next_random = move || {
		state ^= state << 13;
		state ^= state >> 7;
		state ^= state << 17;
		state
	};
	let iterations = std::env::var("RANDOM_RENDER_ITERATIONS")
		.ok()
		.and_then(|value| value.parse().ok())
		.unwrap_or(2000);
	for index in 0..iterations {
		let length = next_random() % 40;
		let header = ["flowchart TD\n", "graph LR\n", "flowchart BT\n"][index % 3];
		// Every other pair of headers uses the `elk` layout.
		let layout = if index / 3 % 2 == 1 { ELK_FRONT_MATTER } else { "" };
		let source: String = [layout, header]
			.into_iter()
			.chain(
				(0..length).map(|_| FRAGMENTS[(next_random() % FRAGMENTS.len() as u64) as usize]),
			)
			.collect();
		if let Ok(diagram) = render_light(&source) {
			assert!(diagram.svg.ends_with("</svg>"));
		}
	}
}

#[test]
fn renders_random_sequence_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 52] = [
		"Alice",
		"Bob",
		"A-B",
		" ",
		"
",
		";",
		"->>",
		"-->>",
		"->",
		"-x",
		"-)",
		"<<->>",
		"-|\\",
		"//-",
		"()",
		"+",
		"-",
		":",
		": text",
		": wrap:a long text that wraps around",
		"<br>",
		"participant ",
		"actor ",
		" as ",
		"@{ \"type\": \"queue\" }",
		"@{ \"type\": \"control\" }",
		"@{ \"type\": \"database\" }",
		"create ",
		"destroy ",
		"activate ",
		"deactivate ",
		"note left of ",
		"note right of ",
		"note over ",
		",",
		"loop x
",
		"alt x
",
		"else y
",
		"par
",
		"and z
",
		"critical
",
		"option o
",
		"rect rgb(1,2,3)
",
		"box Purple B
",
		"end
",
		"autonumber
",
		"autonumber 10 5
",
		"links Alice: {\"a\": \"https://x\"}
",
		"title T
",
		"#",
		"%%",
		"#59;",
	];
	let mut state: u64 = 0x1D87_2B41_7C3A_9E55;
	let mut next_random = move || {
		state ^= state << 13;
		state ^= state >> 7;
		state ^= state << 17;
		state
	};
	let iterations = std::env::var("RANDOM_RENDER_ITERATIONS")
		.ok()
		.and_then(|value| value.parse().ok())
		.unwrap_or(2000);
	for index in 0..iterations {
		let length = next_random() % 40;
		let header = [
			"sequenceDiagram
",
			"%%{init: {\"look\": \"classic\"}}%%
sequenceDiagram
",
		][index % 2];
		let source: String = std::iter::once(header)
			.chain(
				(0..length).map(|_| FRAGMENTS[(next_random() % FRAGMENTS.len() as u64) as usize]),
			)
			.collect();
		if let Ok(diagram) = render_light(&source) {
			assert!(diagram.svg.ends_with("</svg>"));
		}
	}
}

#[test]
fn renders_sequence_diagrams() {
	let source = "sequenceDiagram
    title Greetings
    participant A as Alice
    actor B@{ \"type\": \"entity\" }
    A->>+B: Hello<br/>there
    Note over A,B: A note
    loop Every minute
        B-->>-A: Hi #35;1
    end
    autonumber
    A-xB: Bye";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"sequence\""));
		for text in ["Alice", "Hello", "there", "A note", "[Every minute]", "Hi #1", "Greetings"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	let error = render_light(
		"sequenceDiagram
A->>B: x
deactivate B",
	)
	.unwrap_err();
	assert!(error.message.contains("inactive participant"), "{error}");
	let error = render_light(
		"sequenceDiagram
loop x
A->>B: y",
	)
	.unwrap_err();
	assert_eq!(error.line, Some(4));
}

#[test]
fn renders_random_state_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 44] = [
		"A",
		"B",
		"[*]",
		" ",
		"
",
		";",
		"-->",
		"--",
		":",
		": text",
		"::",
		":::cls",
		"state ",
		"state \"Long name\" as ",
		"state X {
",
		"}
",
		"{",
		"<<fork>>",
		"<<join>>",
		"[[choice]]",
		"note left of ",
		"note right of ",
		"note \"floating\" as N
",
		"end note
",
		"classDef cls fill:#f00,color:red
",
		"class A cls
",
		"style B stroke:#0f0;
",
		"direction LR
",
		"direction TB
",
		"click A \"https://x\" \"tip\"
",
		"click B href \"https://y\"
",
		"hide empty description
",
		"scale 100 width
",
		"accTitle: t
",
		"accDescr { d }
",
		"#",
		"%%",
		"%%{",
		"#59;",
		"<br>",
		"**bold**",
		"\"",
		"as ",
		"default",
	];
	let mut state: u64 = 0x5A17_E2D9_04C3_B861;
	let mut next_random = move || {
		state ^= state << 13;
		state ^= state >> 7;
		state ^= state << 17;
		state
	};
	let iterations = std::env::var("RANDOM_RENDER_ITERATIONS")
		.ok()
		.and_then(|value| value.parse().ok())
		.unwrap_or(2000);
	for index in 0..iterations {
		let length = next_random() % 40;
		let header = [
			"stateDiagram-v2
",
			"%%{init: {\"look\": \"classic\"}}%%
stateDiagram
",
		][index % 2];
		// Every other pair of headers uses the `elk` layout.
		let layout = if index / 3 % 2 == 1 { ELK_FRONT_MATTER } else { "" };
		let source: String = [layout, header]
			.into_iter()
			.chain(
				(0..length).map(|_| FRAGMENTS[(next_random() % FRAGMENTS.len() as u64) as usize]),
			)
			.collect();
		if let Ok(diagram) = render_light(&source) {
			assert!(diagram.svg.ends_with("</svg>"));
		}
	}
}

#[test]
fn renders_state_diagrams() {
	let source = "stateDiagram-v2
    [*] --> Still
    Still: A state
    Still: with a description
    Still --> Moving : go
    state Moving {
        [*] --> Slow
        Slow --> Fast
        --
        [*] --> Loud
    }
    state fork <<fork>>
    Moving --> fork
    fork --> [*]
    note right of Still : A note
    classDef hot fill:#f00
    class Moving hot";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"stateDiagram\""));
		for text in ["A state", "with a description", "go", "Moving", "Slow", "A note"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	let error = render_light(
		"stateDiagram-v2
  note right of Q : hi
",
	)
	.unwrap_err();
	assert!(error.message.contains("No such shape"), "{error}");
	let error = render_light(
		"stateDiagram-v2
  A --> B
  C --x D
",
	)
	.unwrap_err();
	assert_eq!(error.line, Some(3));
}

#[test]
fn renders_random_er_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 46] = [
		"A",
		"B",
		"\"Quoted name\"",
		" ",
		"\n",
		";",
		"||--o{",
		"}|..|{",
		"|o--|{",
		"u--o|",
		"one or more to zero or many",
		"1 to 0+",
		"1",
		"1.5",
		":",
		": label",
		": \"two words\"",
		":::cls",
		"[Alias]",
		"[",
		"]",
		"{\n",
		"}\n",
		"{",
		"}",
		"string name PK, FK \"comment\"\n",
		"List~int~ items\n",
		"`odd name` x UK\n",
		"type? y\n",
		"subgraph S [Title]\n",
		"subgraph T\n",
		"end\n",
		"direction LR\n",
		"classDef cls fill:#f00,color:red\n",
		"class A cls\n",
		"style B stroke:#0f0;\n",
		"style S fill:#ffe\n",
		"accTitle: t\n",
		"accDescr { d }\n",
		"#",
		"%%",
		"<br>",
		"**bold**",
		"\"",
		"~",
		"(",
	];
	let mut state: u64 = 0x3E1D_77A0_9B24_C5F1;
	let mut next_random = move || {
		state ^= state << 13;
		state ^= state >> 7;
		state ^= state << 17;
		state
	};
	let iterations = std::env::var("RANDOM_RENDER_ITERATIONS")
		.ok()
		.and_then(|value| value.parse().ok())
		.unwrap_or(2000);
	for index in 0..iterations {
		let length = next_random() % 40;
		let header = ["erDiagram\n", "%%{init: {\"look\": \"classic\"}}%%\nerDiagram\n"][index % 2];
		// Every other pair of headers uses the `elk` layout.
		let layout = if index / 3 % 2 == 1 { ELK_FRONT_MATTER } else { "" };
		let source: String = [layout, header]
			.into_iter()
			.chain(
				(0..length).map(|_| FRAGMENTS[(next_random() % FRAGMENTS.len() as u64) as usize]),
			)
			.collect();
		if let Ok(diagram) = render_light(&source) {
			assert!(diagram.svg.ends_with("</svg>"));
		}
	}
}

#[test]
fn renders_er_diagrams() {
	let source = "---\ntitle: Orders\n---\nerDiagram\n  CUSTOMER ||--o{ ORDER : places\n  CUSTOMER {\n    string name PK \"full name\"\n    List~string~ tags\n  }\n  ORDER }|..|| PRODUCT : contains\n  subgraph S [Store]\n    PRODUCT\n  end\n  classDef hot fill:#f96\n  class ORDER hot";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"er\""));
		assert!(diagram.svg.contains("_er-zeroOrMoreEnd"));
		for text in ["Orders", "CUSTOMER", "full name", "List<string>", "places", "Store"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	// Like jison, the line of the last token before the error.
	let error = render_light("erDiagram\n  A ||--o{ B\n  C\n").unwrap_err();
	assert_eq!(error.line, Some(2));
}

#[test]
fn renders_random_requirement_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 40] = [
		"a",
		"b",
		"\"two words\"",
		" ",
		"
",
		"requirement ",
		"functionalRequirement ",
		"designConstraint ",
		"element ",
		"{
",
		"}
",
		"{",
		"}",
		"id: 1
",
		"text: some *text*
",
		"risk: high
",
		"verifymethod: test
",
		"type: simulation
",
		"docref: doc/ref
",
		"id:",
		" - satisfies -> ",
		" <- contains - ",
		" - traces -> ",
		"-",
		"->",
		"<-",
		":::cls",
		":",
		",",
		"classDef cls fill:#f00,color:red
",
		"class a cls
",
		"style b stroke:#0f0;
",
		"direction LR
",
		"accTitle: t
",
		"accDescr { d }
",
		"#",
		"%%",
		"\"",
		"<br>",
		"%",
	];
	let mut state: u64 = 0x51C7_0E2B_88A3_1F49;
	let mut next_random = move || {
		state ^= state << 13;
		state ^= state >> 7;
		state ^= state << 17;
		state
	};
	let iterations = std::env::var("RANDOM_RENDER_ITERATIONS")
		.ok()
		.and_then(|value| value.parse().ok())
		.unwrap_or(2000);
	for index in 0..iterations {
		let length = next_random() % 40;
		let header = [
			"requirementDiagram
",
			"%%{init: {\"look\": \"classic\"}}%%
requirementDiagram
",
		][index % 2];
		// Every other pair of headers uses the `elk` layout.
		let layout = if index / 3 % 2 == 1 { ELK_FRONT_MATTER } else { "" };
		let source: String = [layout, header]
			.into_iter()
			.chain(
				(0..length).map(|_| FRAGMENTS[(next_random() % FRAGMENTS.len() as u64) as usize]),
			)
			.collect();
		if let Ok(diagram) = render_light(&source) {
			assert!(diagram.svg.ends_with("</svg>"));
		}
	}
}

#[test]
fn renders_requirement_diagrams() {
	let source = "---
title: Requirements
---
requirementDiagram
  requirement test_req {
    id: 1
    text: the **test** text.
    risk: high
    verifymethod: test
  }
  element test_entity:::blue {
    type: simulation
  }
  test_entity - satisfies -> test_req
  test_req <- contains - test_entity
  classDef blue stroke:#00f";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"requirement\""));
		assert!(diagram.svg.contains("_requirement-requirement_arrowEnd"));
		assert!(diagram.svg.contains("_requirement-requirement_containsStart"));
		for text in
			["Requirements", "<<Requirement>>", "test_req", "ID: 1", "test", "<<satisfies>>"]
		{
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	// Like jison, the line of the last token before the error.
	let error = render_light(
		"requirementDiagram
  requirement a {
    risk: big
  }
",
	)
	.unwrap_err();
	assert_eq!(error.line, Some(3));
	assert!(
		render_light(
			"requirementDiagram
  element a {
  }
  a - traces -> b
"
		)
		.is_err()
	);
}

/// Renders random sequences of `fragments` after each of `headers` and checks that nothing
/// panics; `RANDOM_RENDER_ITERATIONS` sets the count.
fn render_random_diagrams(headers: &[&str], fragments: &[&str], seed: u64) {
	let mut state = seed;
	let mut next_random = move || {
		state ^= state << 13;
		state ^= state >> 7;
		state ^= state << 17;
		state
	};
	let iterations = std::env::var("RANDOM_RENDER_ITERATIONS")
		.ok()
		.and_then(|value| value.parse().ok())
		.unwrap_or(2000);
	for index in 0..iterations {
		let length = next_random() % 40;
		let source: String = std::iter::once(headers[index % headers.len()])
			.chain(
				(0..length).map(|_| fragments[(next_random() % fragments.len() as u64) as usize]),
			)
			.collect();
		if let Ok(diagram) = render_light(&source) {
			assert!(diagram.svg.ends_with("</svg>"));
		}
	}
}

#[test]
fn renders_random_packet_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 22] = [
		"0",
		"15",
		"-",
		"+",
		"8",
		":",
		" ",
		"
",
		"\"label\"",
		"'x'",
		"\"a\\\"b\"",
		"title T
",
		"accTitle: t
",
		"accDescr { d }
",
		"%% c
",
		"1.5",
		"0-31: \"w\"
",
		"+1: \"b\"
",
		"
",
		"\"",
		"packet",
		"	",
	];
	render_random_diagrams(
		&[
			"packet
",
			"packet-beta
",
			"%%{init: {\"packet\": {\"bitsPerRow\": 3, \"bitOrder\": \"descending\"}}}%%
packet
",
		],
		&FRAGMENTS,
		0x2C1B_5E77_0D93_4A61,
	);
}

#[test]
fn renders_packet_diagrams() {
	let source = "---
title: TCP
---
packet
0-15: \"Source Port\"
16-31: \"Destination Port\"
32-63: \"Sequence Number\"
+1: \"URG\"
+40: \"Data\"
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"packet\""));
		assert_eq!(diagram.svg.matches("class=\"packetBlock\"").count(), 6);
		for text in ["TCP", "Source Port", "64", "65", "95", "96", "104"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
		// Five rows of 32 + 15 px and the title row.
		assert_eq!(diagram.height, 47.0 * 5.0);
	}
	let error = render_light(
		"packet
0-15: \"a\"
17-20: \"b\"
",
	)
	.unwrap_err();
	assert_eq!(error.line, Some(3));
}

#[test]
fn renders_random_radar_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 30] = [
		"axis ",
		"curve ",
		"a",
		"b",
		"[\"label\"]",
		",",
		"{",
		"}",
		"1",
		"2.5",
		":",
		" ",
		"
",
		"max 3",
		"min 1",
		"ticks 40",
		"showLegend false",
		"graticule polygon",
		"graticule circle",
		"title T
",
		"accTitle: t
",
		"accDescr { d }
",
		"%% c
",
		"-",
		"circle",
		"true",
		"\"",
		"[",
		"]",
		"a: 2",
	];
	render_random_diagrams(
		&[
			"radar-beta
",
			"radar-beta:
",
			"%%{init: {\"theme\": \"dark\"}}%%
radar-beta
",
		],
		&FRAGMENTS,
		0x6A09_E667_F3BC_C908,
	);
}

#[test]
fn renders_radar_diagrams() {
	let source = "---
title: Grades
---
radar-beta
axis m[\"Math\"], s[\"Science\"], e[\"English\"]
curve a[\"Alice\"]{85, 90, 80}
curve b[\"Bob\"]{s: 75, m: 70, e: 85}
max 100
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"radar\""));
		assert_eq!(diagram.svg.matches("class=\"radarGraticule\"").count(), 5);
		assert!(diagram.svg.contains("class=\"radarCurve-1\""));
		assert!(diagram.svg.contains("dominant-baseline=\"hanging\""));
		for text in ["Grades", "Math", "Alice", "Bob"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
		assert_eq!((diagram.width, diagram.height), (700.0, 700.0));
	}
	let error = render_light(
		"radar-beta
axis A
curve c{1}
curve d{x 1, 2}
",
	)
	.unwrap_err();
	assert_eq!(error.line, Some(4));
}

#[test]
fn renders_random_kanban_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 30] = [
		"  ",
		"    ",
		"	",
		"
",
		"Todo",
		"id1",
		"[",
		"]",
		"(",
		")",
		"((",
		"))",
		"{{",
		"}}",
		"-)",
		"(-",
		"\"",
		"\"`",
		"`\"",
		"@{ ",
		"}",
		"assigned: 'a'",
		"ticket: 1",
		"priority: 'High'",
		"\"x
y\"",
		":::cls",
		"::icon(i)",
		"%% c",
		"kanban",
		"^",
	];
	render_random_diagrams(
		&[
			"kanban
",
			"kanban
  Todo
",
			"---
config:
  kanban:
    ticketBaseUrl: 'x/#TICKET#'
---
kanban
",
		],
		&FRAGMENTS,
		0x3C6E_F372_FE94_F82B,
	);
}

#[test]
fn renders_kanban_diagrams() {
	let source = "---
config:
  kanban:
    ticketBaseUrl: 'https://example.com/#TICKET#'
---
kanban
  Todo
    [Create Documentation]
    docs[Blog post]@{ ticket: MC-1, assigned: 'knsv', priority: 'High' }
  id9[Done]
    a[Done item with a long label that wraps into lines]
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"kanban\""));
		assert!(diagram.svg.contains("class=\"cluster undefined section-2\""));
		assert!(diagram.svg.contains("xlink:href=\"https://example.com/MC-1\""));
		assert!(diagram.svg.contains("stroke=\"orange\""));
		for text in ["Todo", "Done", "MC-1", "knsv", "Blog post"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	let error = render_light(
		"kanban
  Todo
    a
b
  c
",
	)
	.unwrap_err();
	assert_eq!(error.line, Some(5));
}

#[test]
fn renders_journey_diagrams() {
	let source = "---
config:
  journey:
    titleFontSize: 2rem
---
journey
  accTitle: Day
  title My working day
  section Go to work
    Make tea: 5: Me
    A task with a rather long name that wraps and overflows the box: 3: Me, Cat
    Supercalifragilisticexpialidociousword: 1
    Odd: x: Me
  section Go home
    Sit down: 5: Me
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"journey\""));
		assert!(diagram.svg.contains("class=\"journey-section section-type-1\""));
		assert!(diagram.svg.contains("clip-path=\"url(#"));
		for text in ["My working day", "Go home", "Make tea", "Cat", "Me"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	for placement in ["tspan", "old"] {
		let source = format!(
			"%%{{init: {{'journey': {{'textPlacement': '{placement}'}}}}}}%%\njourney\n  section S\n    a<br>b: 5: Me\n"
		);
		assert_valid_svg(&render_light(&source).unwrap());
	}
	let error = render_light("journey\n  section S\n    Journey to work: 5: Me\n").unwrap_err();
	assert_eq!(error.line, Some(3));
}

#[test]
fn renders_random_journey_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 22] = [
		" ",
		"\n",
		"  ",
		"title T",
		"section S",
		"Section",
		"task",
		":",
		": 5",
		": 3: Me, Cat",
		": abc",
		",",
		";",
		"#",
		"%%",
		"}",
		"accTitle: a",
		"accDescr {",
		"accDescr: d",
		"journey",
		"<br>",
		"Supercalifragilisticexpialidocious word",
	];
	render_random_diagrams(
		&["journey\n", "journey\n  section A\n", "journey\ntitle X\n  a: 1: P\n"],
		&FRAGMENTS,
		0x5BE0_CD19_137E_2179,
	);
}

#[test]
fn caches_diagrams_and_pictures() {
	let mut renderer = DiagramRenderer::new(
		Box::new(FixedMeasure),
		Box::new(FixedClock { now: 0.0, utc_offset: 0.0 }),
		&NoImageDecoder,
		Limits::default(),
	);
	let Ok(Image::Svg(first)) = renderer.render("pie\n\"a\" : 1", false, Output::Svg) else {
		panic!()
	};
	let Ok(Image::Svg(again)) = renderer.render("pie\n\"a\" : 1", false, Output::Svg) else {
		panic!()
	};
	assert!(Rc::ptr_eq(&first, &again));
	let Ok(Image::Png(png)) = renderer.render("pie\n\"a\" : 1", false, Output::Png { scale: 2.0 })
	else {
		panic!()
	};
	assert!(png.pixel_width.abs_diff(png.width * 2) <= 2);
	assert!(png.bytes.starts_with(b"\x89PNG"));
	let Ok(Image::Png(cached)) =
		renderer.render("pie\n\"a\" : 1", false, Output::Png { scale: 2.0 })
	else {
		panic!()
	};
	assert!(Rc::ptr_eq(&png, &cached));
	assert!(renderer.render("nonsense", false, Output::Svg).is_err());
}

#[test]
fn outlines_all_text_for_pictures() {
	let diagram = render_light("pie title Pets\n\"Dogs\" : 3\n\"Cats\" : 1").unwrap();
	assert_eq!(diagram.texts.len(), 5);
	let outlined = svg::outline_text(&diagram, &mut FixedMeasure);
	assert!(!outlined.contains("<text"));
	assert!(outlined.contains("<path class=\"pieTitleText\""));
}

#[cfg(windows)]
#[test]
#[ignore = "development aid: writes diagrams to MERMAID_DUMP_DIR"]
fn dump_diagrams() {
	let directory = std::env::var("MERMAID_DUMP_DIR").unwrap();
	let scale = std::env::var("MERMAID_DUMP_SCALE").map_or(1.0, |scale| scale.parse().unwrap());
	let mut text = npp::direct_write::TextEngine::new().unwrap();
	let clock = crate::mermaid_clock::system_clock();
	for entry in std::fs::read_dir(&directory).unwrap() {
		let path = entry.unwrap().path();
		if path.extension().is_none_or(|extension| extension != "mmd") {
			continue;
		}
		let source = std::fs::read_to_string(&path).unwrap();
		for is_dark in [false, true] {
			let suffix = if is_dark { "-dark" } else { "" };
			let started = std::time::Instant::now();
			let options =
				Options { is_dark, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH };
			match render_with(
				&source,
				&options,
				&mut text,
				&*clock,
				crate::mermaid_image::decoder(),
			) {
				Ok(diagram) => {
					let rendered = started.elapsed();
					let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
					std::fs::write(
						path.with_file_name(format!("{stem}{suffix}.svg")),
						&diagram.svg,
					)
					.unwrap();
					let started = std::time::Instant::now();
					let outlined = svg::outline_text(&diagram, &mut text);
					let (png, _, _) = npp::svg::render_png_scaled(&outlined, scale).unwrap();
					eprintln!("{stem}{suffix}: SVG {rendered:?}, PNG {:?}", started.elapsed());
					std::fs::write(path.with_file_name(format!("{stem}{suffix}.png")), png)
						.unwrap();
				}
				Err(error) => eprintln!("{}: {error}", path.display()),
			}
		}
	}
}

#[test]
fn renders_timeline_diagrams() {
	let source = "timeline
  title History of things
  section Early
    2002 : LinkedIn
    2004 : Facebook : Google
  section Late
    2010 : A rather long event text that wraps<br>over several lines
         : Supercalifragilisticexpialidociousword
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"timeline\""));
		assert!(diagram.svg.contains("-arrowhead)"));
		for text in ["History of things", "Early", "2004", "Google", "lines"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	for header in [
		"timeline TD",
		"---\nconfig:\n  look: neo\n  theme: dark\n---\ntimeline",
		"---\nconfig:\n  look: neo\n  theme: redux-dark-color\n---\ntimeline TD",
		"%%{init: {'timeline': {'disableMulticolor': true}, 'themeVariables': {'THEME_COLOR_LIMIT': 3}}}%%\ntimeline",
	] {
		let source = format!("{header}\n  a : b : c\n  d : e\n  f\n");
		let diagram = render_light(&source).unwrap();
		assert_valid_svg(&diagram);
	}
	let neo =
		render_light("---\nconfig:\n  look: neo\n  theme: dark\n---\ntimeline\n  a : b\n").unwrap();
	assert!(neo.svg.contains("-gradient-bright)"));
	let error = render_light("timeline\n  section S\n  : event\n").unwrap_err();
	assert_eq!(error.line, Some(3));
}

#[test]
fn renders_random_timeline_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 22] = [
		" ",
		"\n",
		"  ",
		"title T",
		"section S",
		"Section",
		"2002",
		":",
		": ",
		": event",
		":x",
		"#",
		"%%",
		"}",
		"accTitle: a",
		"accDescr {",
		"accDescr: d",
		"timeline",
		"timeline TD",
		"<br>",
		"Supercalifragilisticexpialidocious word",
		"\t",
	];
	render_random_diagrams(
		&["timeline\n", "timeline TD\n  section A\n", "timeline\ntitle X\n  a : b\n"],
		&FRAGMENTS,
		0x1F83_D9AB_FB41_BD6B,
	);
}

#[test]
fn renders_sankey_diagrams() {
	let source = "sankey

%% source,target,value
Agricultural 'waste',Bio-conversion,124.729
Bio-conversion,Liquid,0.597
\"Heating and cooling, \"\"homes\"\"\",Losses,26.862
Bio-conversion,\"Heating and cooling, \"\"homes\"\"\",280.322
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"sankey\""));
		assert!(diagram.svg.contains("-linearGradient-1)"));
		for text in ["Bio-conversion 280.92", "Heating and cooling, \"homes\" 280.32"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	for config in [
		"sankey:\n    showValues: false\n    labelStyle: outlined\n    nodeAlignment: left",
		"sankey:\n    linkColor: source\n    nodeAlignment: right\n    prefix: \"$\"",
		"sankey:\n    linkColor: \"#ff0000\"\n    nodeAlignment: center\n    nodeColors:\n      Liquid: red",
		"theme: forest\n  sankey:\n    linkColor: target\n    width: 800\n    nodeWidth: 30",
	] {
		let diagram = render_light(&format!("---\nconfig:\n  {config}\n---\n{source}")).unwrap();
		assert_valid_svg(&diagram);
	}
	let error = render_light("sankey\nA,B,1\nB,A,1\n").unwrap_err();
	assert_eq!(error.message, "circular link");
	let error = render_light("sankey\nA,B,1\n\nMüll,B,1\n").unwrap_err();
	assert_eq!(error.line, Some(4));
}

#[test]
fn renders_random_sankey_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 20] = [
		"A", "B", "C", ",", ",", "1", "0", "-2.5", "x", "\"", "\"\"", "\n", "\n", "\r\n", " ", "ü",
		"#35;", "<b>", "sankey", "%%",
	];
	render_random_diagrams(
		&["sankey\n", "sankey-beta\nA,B,1\n", "sankey\nA,B,1\nB,C,2\n"],
		&FRAGMENTS,
		0x5A4B_E1C3_9D27_0F61,
	);
}

#[test]
fn renders_quadrant_charts() {
	let source = "quadrantChart
  title Reach and engagement of campaigns
  x-axis Low Reach --> High Reach
  y-axis Low Engagement --> High Engagement
  quadrant-1 We should expand
  quadrant-2 Need to promote
  quadrant-3 Re-evaluate
  quadrant-4 May be improved
  Campaign A:::class1: [0.3, 0.6] radius: 20
  Campaign B: [0.45, 0.23] color: #ff0000, stroke-width: 3px
  classDef class1 color: #908342, radius : 10
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"quadrantChart\""));
		assert!(diagram.svg.contains("transform=\"translate(5, 363) rotate(-90)\""));
		for text in ["Campaign A", "We should expand", "High Engagement"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	for config in [
		"quadrantChart:\n    xAxisPosition: bottom\n    yAxisPosition: right",
		"quadrantChart:\n    chartWidth: 300\n    chartHeight: 200\n    useMaxWidth: false",
		"theme: forest\n  themeVariables:\n    quadrant1Fill: \"#b4dcff\"\n    quadrantPointFill: red",
	] {
		let diagram = render_light(&format!("---\nconfig:\n  {config}\n---\n{source}")).unwrap();
		assert_valid_svg(&diagram);
	}
	let error = render_light("quadrantChart\n  A: [0.5, 0.5] foo: 3").unwrap_err();
	assert_eq!(error.message, "style named foo is not supported.");
	assert_eq!(error.line, Some(2));
}

#[test]
fn renders_random_quadrant_charts_without_panicking() {
	const FRAGMENTS: [&str; 24] = [
		"A",
		"x-axis ",
		"y-axis ",
		" --> ",
		"quadrant-1 ",
		"title ",
		": [",
		"0.5",
		"1",
		", ",
		"]",
		":::",
		"c",
		"classDef ",
		"radius: 3",
		"color: #f00",
		"\"",
		"\"`",
		"`\"",
		"\n",
		" ",
		";",
		"ü",
		"accDescr {",
	];
	render_random_diagrams(
		&["quadrantChart\n", "quadrantChart\nA: [0.1, 0.2]\n"],
		&FRAGMENTS,
		0x1D3E_7A5B_C2F4_0968,
	);
}

#[test]
fn renders_xy_charts() {
	let source = "xychart
  title \"Sales Revenue\"
  x-axis Months [jan, feb, mar, apr, may, jun]
  y-axis \"Revenue (in $)\" 4000 --> 11000
  bar \"Sold\" [5000, 6000, 7500, 8200, 9500, 10500]
  line \"Trend\" [5000 \"low\", 6000, 7500, 8200, 9500, 10500 \"high\"]
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"xychart\""));
		assert!(diagram.svg.contains("class=\"bar-plot-0\""));
		assert!(diagram.svg.contains("dominant-baseline=\"text-before-edge\""));
		for text in ["Sales Revenue", "jan", "10000", "Revenue (in $)", "Trend", "high"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	for config in [
		"xyChart:\n    chartOrientation: horizontal\n    showDataLabel: true",
		"xyChart:\n    showDataLabel: true\n    showDataLabelOutsideBar: true\n    xAxis:\n      labelRotation: 45",
		"xyChart:\n    width: 300\n    height: 200\n    showLegend: false\n    yAxis:\n      showTitle: false",
		"theme: forest\n  themeVariables:\n    xyChart:\n      plotColorPalette: \"#ff0000, #00ff00\"",
	] {
		let diagram = render_light(&format!("---\nconfig:\n  {config}\n---\n{source}")).unwrap();
		assert_valid_svg(&diagram);
	}
	let diagram = render_light("xychart horizontal\n  x-axis 0 --> 10\n  line [1, 3, 2]").unwrap();
	assert_valid_svg(&diagram);
	let error = render_light("xychart\n  title \"x\"").unwrap_err();
	assert_eq!(error.message, "No Plot to render, please provide a plot with some data");
	let error = render_light("xychart\n  bar [1, 2\n  line [3]").unwrap_err();
	assert_eq!(error.line, Some(2));
}

#[test]
fn renders_random_xy_charts_without_panicking() {
	const FRAGMENTS: [&str; 26] = [
		"A",
		"x-axis ",
		"y-axis ",
		" --> ",
		"title ",
		"line ",
		"bar ",
		"[",
		"]",
		"1",
		"-2.5",
		".5",
		"1e309",
		", ",
		"\"",
		"\"q\"",
		"horizontal",
		"xychart ",
		"\n",
		" ",
		";",
		"ü",
		"accDescr {",
		"}",
		"%%",
		"0",
	];
	render_random_diagrams(
		&["xychart\n", "xychart\nbar [1, 2]\n", "xychart horizontal\nx-axis [a, b]\n"],
		&FRAGMENTS,
		0x5E9A_41C7_83D2_6B0F,
	);
}

#[test]
fn renders_treemaps() {
	let source = "treemap-beta
title Budget
\"Critical\":::critical
  \"Server Costs\": 50000
  \"Salaries\": 80,000
\"Normal\"
  \"Office Supplies\": 5000
  \"Marketing\": 15000
classDef critical fill:#e74c3c,color:#fff,stroke:#c0392b,stroke-width:3px
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"treemap\""));
		assert!(diagram.svg.contains("fill:#e74c3c"));
		for text in ["Budget", "Critical", "Salaries", "80,000", "130,000"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	for config in [
		"treemap:\n    valueFormat: \"$0,0\"",
		"treemap:\n    valueFormat: \".1%\"\n    showValues: false",
		"treemap:\n    padding: 0\n    nodeWidth: 30\n    nodeHeight: 20\n    diagramPadding: 0",
		"theme: forest",
	] {
		let diagram = render_light(&format!("---\nconfig:\n  {config}\n---\n{source}")).unwrap();
		assert_valid_svg(&diagram);
	}
	assert_valid_svg(&render_light("treemap").unwrap());
	let error = render_light("treemap\n\"Root\"\n  \"a\" 5").unwrap_err();
	assert_eq!(error.line, Some(3));
	let error = render_light(
		"---\nconfig:\n  treemap:\n    valueFormat: \"$bad!\"\n---\ntreemap\n\"a\": 1",
	)
	.unwrap_err();
	assert_eq!(error.message, "invalid format: bad!");
}

#[test]
fn renders_random_treemaps_without_panicking() {
	const FRAGMENTS: [&str; 22] = [
		"\"A\"",
		"'b'",
		": ",
		", ",
		"1",
		"1,000",
		"0",
		".5",
		":::c1",
		"classDef c1 fill:#f00",
		"title T",
		"\n",
		"  ",
		"\t",
		"    ",
		"%% x",
		"\"",
		"ü",
		"accDescr {",
		"}",
		"_",
		"treemap",
	];
	render_random_diagrams(
		&["treemap\n", "treemap\n\"Root\"\n", "treemap-beta\n\"R\"\n  \"a\": 1\n"],
		&FRAGMENTS,
		0x7A3C_19E5_D4B8_2F61,
	);
}

#[test]
fn renders_gantt_diagrams() {
	let source = "gantt
    title A Gantt Diagram
    dateFormat YYYY-MM-DD
    excludes weekends
    section Section #35;1
        A task          :done, crit, a1, 2014-01-01, 30d
        Another task    :active, after a1, 20d
        Milestone       :milestone, m1, 2014-02-20, 0d
    section Another<br>line
        Task in Another :2014-01-12, 12d
        another task    :24d
        Marker          :vert, v1, 2014-01-15, 0d
    click a1 href \"https://example.com/a1\"
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"gantt\""));
		assert!(diagram.svg.contains("xlink:href=\"https://example.com/a1\""));
		assert!(diagram.svg.contains("rotate(45) scale(0.8)"));
		for text in
			["A Gantt Diagram", "Section #1", "Another", "line", "A task", "Marker", "2014-01-05"]
		{
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	for config in [
		"gantt:\n    displayMode: compact\n    topAxis: true\n    useWidth: 600",
		"gantt:\n    numberSectionStyles: 0\n    barHeight: 0\n    fontSize: 30",
		"theme: forest",
	] {
		let diagram = render_light(&format!("---\nconfig:\n  {config}\n---\n{source}")).unwrap();
		assert_valid_svg(&diagram);
	}
	for valid in [
		"gantt",
		"gantt\ndateFormat x\nA :0, 5000",
		"gantt\ndateFormat YYYY\ntickInterval 1millisecond\nA :2000, 2010",
		"gantt\nA :2014-01-01, 3d\nB :Jan 5 2014, 2d\nC :after A B, 1d",
		"gantt\ntickInterval 99999week\nweekday friday\nA :2014-01-01, 30d",
		"gantt\ndateFormat YYYY-MM-DD\nexcludes weekends\nA :0001-01-01, 9999-12-31",
	] {
		assert_valid_svg(&render_light(valid).unwrap());
	}
	let error = render_light("gantt\n  A :3d").unwrap_err();
	assert_eq!(error.line, Some(2));
	assert_eq!(error.message, "Cannot read properties of undefined (reading 'endTime')");
	let error = render_light("gantt\ndateFormat YYYY-MM-DD\n\nA :nonsense, 3d").unwrap_err();
	assert_eq!((error.line, error.message.as_str()), (Some(4), "Invalid date:nonsense"));
	assert_eq!(render_light("gantt\ntopAxis").unwrap_err().message, "yy.TopAxis is not a function");
	assert_eq!(render_light("gantt\n2014-01-01 :3d").unwrap_err().line, Some(2));
	let error = render_light("gantt\ndateFormat YYYY-MM-DD\nexcludes monday tuesday wednesday thursday friday saturday sunday\nA :2014-01-01, 3d").unwrap_err();
	assert!(error.message.starts_with("Failed to find a valid date"), "{}", error.message);
}

#[test]
fn renders_random_gantt_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 30] = [
		"A ",
		":",
		"a1, ",
		"2014-01-01",
		", ",
		"3d",
		"1.5w",
		"after a1",
		"until a1",
		"done, ",
		"crit, ",
		"milestone, ",
		"vert, ",
		"section S",
		"title T",
		"dateFormat YYYY-MM-DD HH:mm",
		"dateFormat x",
		"axisFormat %a %W",
		"tickInterval 2day",
		"excludes weekends",
		"inclusiveEndDates",
		"click a1 href \"x\"",
		"call f(1)",
		"todayMarker off",
		"weekday monday",
		"\n",
		"  ",
		"#",
		"%",
		"ü",
	];
	render_random_diagrams(
		&["gantt\n", "gantt\ndateFormat YYYY-MM-DD\n", "gantt\nA :a1, 2014-01-01, 2d\n"],
		&FRAGMENTS,
		0x3B71_C09E_5A2D_84F6,
	);
}

#[test]
fn renders_git_graphs() {
	let source = "gitGraph
title Release flow
commit id: \"a\" tag: \"v1\"
branch develop order: 1
commit id: \"b\" type: HIGHLIGHT
checkout main
commit type: REVERSE
merge develop tag: \"m\" type: HIGHLIGHT
branch hotfix
commit
checkout develop
cherry-pick id: \"a\"
commit
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"gitGraph\""));
		for text in
			["Release flow", "main", "develop", "hotfix", "v1", "cherry-pick:a", "2-a08ff49"]
		{
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	for header in ["gitGraph TB:", "gitGraph BT:", "gitGraph LR:"] {
		for config in [
			"gitGraph:\n    parallelCommits: true",
			"gitGraph:\n    rotateCommitLabel: false\n    showBranches: false",
			"theme: redux-color\n  look: neo",
			"theme: neo-dark",
			"theme: redux-dark\n  look: neo",
		] {
			let code = source.replacen("gitGraph", header, 1);
			let diagram = render_light(&format!("---\nconfig:\n  {config}\n---\n{code}")).unwrap();
			assert_valid_svg(&diagram);
		}
	}
	assert_valid_svg(&render_light("gitGraph").unwrap());
	let error = render_light("gitGraph\n commit\n checkout dev\n").unwrap_err();
	assert_eq!(error.line, Some(3));
	assert!(error.message.starts_with("Trying to checkout branch which is not yet created."));
}

#[test]
fn renders_random_git_graphs_without_panicking() {
	const FRAGMENTS: [&str; 30] = [
		"commit",
		" id: \"a\"",
		" id: \"b\"",
		" tag: \"t\"",
		" msg: \"m\"",
		" type: HIGHLIGHT",
		" type: REVERSE",
		"branch b",
		"branch c order: 2",
		"checkout main",
		"checkout b",
		"switch c",
		"merge b",
		"merge c id: \"m\"",
		"cherry-pick id: \"a\"",
		"cherry-pick id: \"m\" parent: \"a\"",
		"title T",
		"accDescr {",
		"}",
		"gitGraph",
		"\n",
		"  ",
		"\t",
		"%% x",
		":",
		"\"",
		"ü",
		"1",
		".",
		"<br>",
	];
	render_random_diagrams(
		&[
			"gitGraph\n",
			"gitGraph TB:\n",
			"gitGraph BT:\ncommit\n",
			"---\nconfig:\n  gitGraph:\n    parallelCommits: true\n---\ngitGraph BT:\n",
		],
		&FRAGMENTS,
		0x91C4_2E7B_D05A_63F8,
	);
}

#[test]
fn renders_block_diagrams() {
	let source = "block-beta
columns 3
a[\"Square\"]:2 b(\"Round\")
block:group:3
  columns 2
  c((\"Circle\")) d{{\"Hexagon\"}}
  e[(\"Database\")] f<[\"Arrow\"]>(right, down)
end
space g>\"Odd\"] h[/\"Lean\"/]
a --> b
b -- \"label\" --> c
c x--x h
classDef blue fill:#66f,stroke:#333,color:#ff6;
class a,group blue
style b fill:#f9F,stroke-width:4px
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"block\""));
		for text in ["Square", "Round", "Circle", "Hexagon", "Database", "Arrow", "label"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	for config in
		["theme: redux-color\n  look: neo", "theme: neo-dark\n  look: neo", "theme: forest"]
	{
		let diagram = render_light(&format!("---\nconfig:\n  {config}\n---\n{source}")).unwrap();
		assert_valid_svg(&diagram);
	}
	// Mermaid's errors.
	for (code, message) in [
		("block\n  style x fill:#f00\n  x\n", "Cannot set properties of undefined"),
		("block\n  columns 0\n  a b\n", "Columns must be an integer"),
		("block\n  a{\"d\"} b\n  a --> b\n", "this.calcIntersect is not a function"),
	] {
		let error = render_light(code).unwrap_err();
		assert!(error.message.contains(message), "{}", error.message);
	}
	assert_eq!(render_light("block\n  a\n  b[ \"x\"]\n").unwrap_err().line, Some(3));
}

#[test]
fn renders_random_block_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 34] = [
		"a",
		"b",
		"[\"x\"]",
		"(\"y\")",
		"((\"z\"))",
		"{{\"h\"}}",
		"[(\"c\")]",
		"[/\"l\"/]",
		"<[\"arrow\"]>(x, down)",
		":2",
		":0",
		" --> ",
		" -- \"label\" --> ",
		" -.- ",
		" ==> ",
		"space",
		"space:3",
		"columns 2",
		"columns auto",
		"block:g",
		"block",
		"end",
		"classDef c fill:#f00",
		"class a c",
		"style a fill:#0f0",
		"\n",
		"  ",
		"\"",
		"`",
		"ü",
		"<br>",
		"%% x",
		"accTitle: t",
		"root",
	];
	render_random_diagrams(
		&["block\n", "block-beta\ncolumns 3\n", "---\nconfig:\n  look: neo\n---\nblock\n"],
		&FRAGMENTS,
		0x5B1C_7A93_E2D4_0F86,
	);
}

#[test]
fn renders_random_c4_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 44] = [
		"Person(p, \"P\", \"descr\")",
		"Person_Ext(e, \"E\")",
		"System(s, \"S\", \"a longer description of the system\")",
		"SystemDb(d, \"D\")",
		"SystemQueue_Ext(q, \"Q\")",
		"Container(c, \"C\", \"Rust\", \"descr\")",
		"ContainerDb(cd, \"CD\")",
		"Component(k, \"K\", \"tech\")",
		"ComponentQueue_Ext(kq, \"KQ\")",
		"Boundary(b, \"B\") {",
		"Enterprise_Boundary(eb, \"EB\") {",
		"System_Boundary(sb, \"SB\", \"type\") {",
		"Container_Boundary(cb, \"CB\") {",
		"Deployment_Node(n, \"N\", \"Ubuntu\") {",
		"Node_L(nl, \"NL\") {",
		"Node_R(nr, \"NR\") {",
		"}",
		"{",
		"Rel(p, s, \"Uses\", \"HTTPS\")",
		"BiRel(s, d, \"Reads\")",
		"Rel_U(c, k, \"up\")",
		"Rel_Back(k, cd, \"back\")",
		"RelIndex(1, p, c, \"first\")",
		"Rel(p, b, \"to boundary\")",
		"Rel(p, s, $label=\"named\")",
		"UpdateElementStyle(p, $fontColor=\"red\", $bgColor=\"#ff0\", $shape=\"EightSidedShape\")",
		"UpdateRelStyle(p, s, $offsetX=\"-40\", $offsetY=\"x\", $lineColor=\"blue\")",
		"UpdateLayoutConfig($c4ShapeInRow=\"1\", $c4BoundaryInRow=\"0\")",
		"title T",
		"accTitle: A",
		"accDescr { multi\nline }",
		"(",
		")",
		",",
		"\"",
		"$descr=\"x\"",
		"$sprite=\"s\", $tags=\"t\", $link=\"https://example.com\"",
		"direction TB",
		"\n",
		"  ",
		"%% comment",
		"ü<br>&amp;",
		"Person(p",
		"System(s, \"S\")",
	];
	render_random_diagrams(
		&[
			"C4Context\n",
			"C4Container\n",
			"C4Component\n",
			"C4Dynamic\n",
			"C4Deployment\n",
			"---\nconfig:\n  wrap: true\n  c4:\n    c4ShapeInRow: 1\n---\nC4Context\n",
		],
		&FRAGMENTS,
		0x3C4D_19E7_A2B5_6F08,
	);
}

#[test]
fn renders_mindmap_diagrams() {
	let source = "mindmap
  root((mindmap))
    Origins
      Long history
      ::icon(fa fa-book)
      Popularisation
        British popular psychology author Tony Buzan
    Research
      On effectiveness<br/>and features
      On Automatic creation
    Tools
      id)I am a cloud(
        id))I am a bang((
      [square]
      (rounded)
      {{hexagon}}
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"mindmap\""));
		assert!(diagram.svg.contains("class=\"node mindmap-node section-root section--1\""));
		assert!(diagram.svg.contains("edge-depth-1"));
		for text in ["mindmap", "Origins", "I am a cloud", "hexagon"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	let neo = render_light(
		"---\nconfig:\n  look: neo\n  theme: neo\n---\nmindmap\n  r\n    a\n    b((c))\n",
	);
	let neo = neo.unwrap();
	assert_valid_svg(&neo);
	assert!(neo.svg.contains("-gradient)"));
	let error = render_light("mindmap\n  root\n  second\n").err().unwrap();
	assert_eq!(error.line, Some(3));
	assert!(render_light("mindmap\n").is_ok());
}

#[test]
fn renders_random_mindmap_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 26] = [
		"  ",
		"    ",
		"	",
		"\n",
		"root",
		"id1",
		"[",
		"]",
		"(",
		")",
		"((",
		"))",
		"{{",
		"}}",
		"-)",
		"(-",
		"\"",
		"\"`",
		"`\"",
		"**b**",
		":::cls",
		"::icon(i)",
		"%% c",
		"mindmap",
		"<br/>",
		"a long label with words",
	];
	render_random_diagrams(
		&[
			"mindmap\n",
			"mindmap\n  root\n",
			"---\nconfig:\n  look: neo\n  mindmap:\n    padding: 0\n    maxNodeWidth: 1\n---\nmindmap\n",
			"%%{init: {\"mindmap\": {\"colorBy\": \"depth\", \"padding\": 1e300}}}%%\nmindmap\n",
		],
		&FRAGMENTS,
		0x5BE0_CD19_137E_2179,
	);
}

#[test]
fn renders_architecture_diagrams() {
	let source = "architecture-beta
  group api(cloud)[API]
  group private[Private] in api
  service db(database)[Database] in private
  service disk1(disk)[Storage] in api
  service server(server)[Server] in api
  service gateway(internet)[Gateway]
  service text \"Icon text\"[Texts]
  service unknown(logos:aws)
  junction mid
  db:L -[reads]- R:server
  disk1:T <--> B:server
  server{group}:T --> B:gateway
  gateway:R -[bent]- T:mid
  mid:B -- T:text
  text:R -- L:unknown
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"architecture\""));
		assert!(diagram.svg.contains("class=\"architecture-service\""));
		assert!(diagram.svg.contains("class=\"architecture-junction\""));
		assert_eq!(diagram.svg.matches("class=\"arrow\"").count(), 3);
		for text in ["Database", "Server", "reads", "bent", "Icon", "?"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content.contains(text)), "{text}");
		}
	}
	let error =
		render_light("architecture-beta\n  service a\n  service b\n  a:R -- L:b\n  a:T -- B:b\n")
			.err()
			.unwrap();
	assert_eq!(error.message, "Can not create second element with ID `a-b`");
	assert_eq!(error.line, Some(5));
	assert!(render_light("architecture-beta\n").is_ok());
	// An alignment that contradicts the edges leaves fcose's grid without a size.
	let contradicting = "architecture-beta\n  service s0\n  service s1\n  service s2\n  s1:T --> B:s2\n  s0:L <-- L:s1\n  align row s1 s0 s2\n";
	let error = render_light(contradicting).err().unwrap();
	assert!(error.message.starts_with("Architecture layout failed"));
}

/// Mermaid's layouts of text-free diagrams (`architecture/fixtures.txt`, from `mermaid@12.2.0`
/// in Edge): node positions, group boxes, edge paths and the view box.
#[test]
fn lays_out_architecture_diagrams_like_mermaid() {
	let fixtures = include_str!("architecture/fixtures.txt");
	let numbers = |text: &str| -> Vec<f64> {
		text.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e'))
			.filter(|part| !part.is_empty() && *part != "-")
			.filter_map(|part| part.parse().ok())
			.collect()
	};
	for fixture in fixtures.split("# ").filter(|fixture| !fixture.trim().is_empty()) {
		let (name, rest) = fixture.split_once('\n').unwrap();
		let source: String = rest
			.lines()
			.filter(|line| !line.starts_with("= "))
			.map(|line| format!("{line}\n"))
			.collect();
		let diagram = render_light(&source).unwrap();
		let svg = &diagram.svg;
		let id = svg.split("id=\"").nth(1).unwrap().split('"').next().unwrap();
		for line in rest.lines().filter_map(|line| line.strip_prefix("= ")) {
			let mut parts = line.splitn(3, ' ');
			let (kind, key) = (parts.next().unwrap(), parts.next().unwrap());
			let expected = numbers(parts.next().unwrap_or(key));
			let actual = match kind {
				"svc" => {
					let at = svg.find(&format!("id=\"{id}-service-{key}\"")).unwrap();
					numbers(
						svg[at..].split("translate(").nth(1).unwrap().split(')').next().unwrap(),
					)
				}
				"junction" => {
					let at = svg.find(&format!("id=\"{id}-node-{key}\"")).unwrap();
					let start = svg[..at].rfind("translate(").unwrap();
					numbers(svg[start + 10..].split(')').next().unwrap())
				}
				"group" => {
					let at = svg.find(&format!("id=\"{id}-group-{key}\"")).unwrap();
					let element = &svg[at..at + svg[at..].find("/>").unwrap()];
					["x", "y", "width", "height"]
						.iter()
						.map(|attribute| {
							let value = element.split(&format!(" {attribute}=\"")).nth(1).unwrap();
							value.split('"').next().unwrap().parse().unwrap()
						})
						.collect()
				}
				"edge" => {
					let at = svg.find(&format!("id=\"{id}-{key}\"")).unwrap();
					let start = svg[..at].rfind(" d=\"").unwrap();
					numbers(svg[start + 4..].split('"').next().unwrap())
				}
				_ => numbers(svg.split("viewBox=\"").nth(1).unwrap().split('"').next().unwrap()),
			};
			let expected = if kind == "viewBox" {
				numbers(key).into_iter().chain(expected).collect()
			} else {
				expected
			};
			assert_eq!(actual.len(), expected.len(), "{name} {line}");
			for (actual, expected) in actual.iter().zip(&expected) {
				assert!((actual - expected).abs() < 0.002, "{name} {line}: {actual} vs {expected}");
			}
		}
	}
}

#[test]
fn renders_random_architecture_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 37] = [
		"(fa:car)",
		"(fab-github)",
		"(a:b:c:d)",
		" ",
		"\n",
		"\n  service a",
		"\n  service b(server)",
		"\n  service c[Title]",
		"\n  service d \"text\"",
		"\n  junction j",
		"\n  group g",
		"\n  group h(cloud)[Group] in g",
		" in g",
		" in h",
		"\n  a",
		"\n  b",
		"\n  j",
		":L",
		":R",
		":T",
		":B",
		"L:",
		"R:",
		"T:",
		"B:",
		" -- ",
		" <--> ",
		" -[label]- ",
		"{group}",
		" a",
		" b",
		" c",
		" j",
		"\n  align row",
		"\n  align column",
		"\n  title T",
		"%% c",
	];
	render_random_diagrams(
		&[
			"architecture-beta",
			"architecture-beta\n  service a\n  service b\n  service c\n  junction j",
			"architecture-beta\n  group g\n  group h in g\n  service a in g\n  service b in h\n  service c in h\n  junction j",
			"%%{init: {\"architecture\": {\"iconSize\": 0, \"padding\": -5, \"numIter\": 1e9, \"seed\": -1, \"idealEdgeLengthMultiplier\": 0}}}%%\narchitecture-beta\n  service a\n  service b\n  service c\n  junction j",
		],
		&FRAGMENTS,
		0x1F83_D9AB_FB41_BD6B,
	);
}

#[test]
fn renders_tree_view_diagrams() {
	let source = "---
config:
  treeView:
    showIcons: true
    extensionIcons:
      .md: none
---
treeView-beta
  title Project
  src/
    index.js :::highlight
    utils/ ## helpers
      helper.js icon(logos:javascript)
  README.md
  \"two  spaces\" icon()
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"treeView\""));
		assert!(diagram.svg.contains("class=\"treeView-highlight-bg\""));
		assert_eq!(diagram.svg.matches("class=\"treeView-node-icon\"").count(), 5);
		for text in ["src", "index.js", "helpers", "?", "two  spaces"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	let box_drawing = "treeView-beta\n├── src\n│   └── a.js\n│\n└──  \n";
	let error = render_light(box_drawing).err().unwrap();
	assert_eq!(error.line, Some(5));
	assert_eq!(render_light("treeView-beta\n  a\n  :::x\n").err(), None);
	assert_eq!(render_light("treeView-beta\n:::x\n").err().and_then(|error| error.line), Some(2));
}

#[test]
fn renders_random_tree_view_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 32] = [
		" icon(fa:car)",
		" icon(fab:)",
		" ",
		"  ",
		"\t",
		"\n",
		"\n  ",
		"\n    ",
		"a",
		"dir/",
		"/",
		"\"quoted name\"",
		"'single'",
		"\"",
		":::highlight",
		" :::x",
		" icon(file)",
		" icon()",
		" icon(none)",
		" icon(pack:name)",
		" ## description",
		"##",
		"title T",
		"accTitle: A",
		"accDescr { D }",
		"├── ",
		"└── ",
		"│   ",
		"┣━━ ",
		"─",
		"%% c",
		"treeView-beta",
	];
	render_random_diagrams(
		&[
			"treeView-beta",
			"treeView-beta\n  a/\n    b\n",
			"%%{init: {\"treeView\": {\"rowIndent\": -5, \"paddingX\": 1e9, \"paddingY\": -1, \"lineThickness\": 0, \"showIcons\": true}}}%%\ntreeView-beta\n",
		],
		&FRAGMENTS,
		0x5C1E_7A90_33D2_14E1,
	);
}

#[test]
fn renders_cynefin_diagrams() {
	let source = "cynefin-beta
  title Incident Response
  accTitle: Cynefin
  complex
    \"Investigate root cause\"
  complicated \"Analyze data\"
  clear
  chaotic \"Page on-call\"
  confusion \"a\" \"b\" \"c\" \"d\"
  complex --> complicated : \"Pattern found\"
  chaotic --> chaotic
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"cynefin\""));
		assert_eq!(diagram.svg.matches("class=\"cynefinItem\"").count(), 6);
		assert_eq!(diagram.svg.matches("class=\"cynefinItemOverflow\"").count(), 1);
		assert_eq!(diagram.svg.matches("class=\"cynefinArrowLine\"").count(), 1);
		for text in ["Incident Response", "Complex", "Disorder", "+1 more", "Pattern found"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	let error = render_light("cynefin-beta\n  complex\n  word\n").err().unwrap();
	assert_eq!(error.line, Some(3));
}

#[test]
fn renders_random_cynefin_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 22] = [
		" ",
		"\n",
		"\n  ",
		"complex",
		"complicated",
		"clear",
		"chaotic",
		"confusion",
		" \"item\"",
		" \"\"",
		"\"",
		" --> ",
		" : ",
		":",
		"title T",
		"accTitle: A",
		"accDescr { D }",
		"%% c",
		"cynefin-beta",
		"cynefin-beta:",
		"word",
		"-->",
	];
	render_random_diagrams(
		&[
			"cynefin-beta",
			"cynefin-beta\n  complex \"a\"\n",
			"%%{init: {\"cynefin\": {\"width\": -5, \"height\": 0, \"padding\": 1e9, \"boundaryAmplitude\": -3, \"seed\": 1e300}, \"themeVariables\": {\"cynefin\": {\"domainFontSize\": \"x\", \"itemFontSize\": -50}}}}%%\ncynefin-beta\n",
		],
		&FRAGMENTS,
		0x3A71_C0DE_9B12_6F45,
	);
}

#[test]
fn renders_ishikawa_diagrams() {
	let source = "ishikawa-beta
  Blurry photo
    Process
      Out of focus
        Autofocus missed
      Shutter too slow
    User
      Shaky hands<br>no tripod
    Equipment
    Environment
      Low light
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"ishikawa\""));
		assert_eq!(diagram.svg.matches("class=\"ishikawa-branch\"").count(), 4);
		assert_eq!(diagram.svg.matches("class=\"ishikawa-sub-branch\"").count(), 5);
		assert_eq!(diagram.svg.matches("class=\"ishikawa-spine\"").count(), 1);
		for text in ["Blurry", "photo", "Autofocus", "missed", "Shaky", "hands", "tripod"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	assert!(render_light("ishikawa\n").is_ok());
	let error = render_light("ishikawa\n\n  Effect\n").err().unwrap();
	assert_eq!(error.line, Some(3));
}

#[test]
fn renders_random_ishikawa_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 16] = [
		" ",
		"  ",
		"\t",
		"\n",
		"\n  ",
		"\n    ",
		"\n      ",
		"Effect",
		"cause",
		"a long cause with many words",
		"<br>",
		"%% c",
		"ishikawa",
		"\n\n",
		"x<br/>y",
		"\r",
	];
	render_random_diagrams(
		&[
			"ishikawa",
			"ishikawa\n  Effect\n",
			"%%{init: {\"fontSize\": -5, \"ishikawa\": {\"diagramPadding\": -1e9}}}%%\nishikawa-beta\n  E\n",
		],
		&FRAGMENTS,
		0x77E1_5A0C_4D2B_19F3,
	);
}

#[test]
fn renders_event_modeling_diagrams() {
	let source = "eventmodeling
tf 01 ui CartScreen
tf 02 cmd AddItem { productId: 7 }
tf 03 evt ItemAdded [[ItemAddedData]]
tf 04 rmo Cart.Items ->> 03
rf 05 pcr Shipping `json` { orderId: 1 }
entity Cart.Items
note 01 {
  shown first
}
gwt 02 given evt ItemAdded then cmd AddItem

data ItemAddedData {
  productId: 7,
  quantity: 1
}
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"eventmodeling\""));
		assert_eq!(diagram.svg.matches("class=\"em-box\"").count(), 5);
		assert_eq!(diagram.svg.matches("class=\"em-relation\"").count(), 3);
		assert_eq!(diagram.svg.matches("class=\"em-swimlane\"").count(), 4);
		for text in ["CartScreen", "C/RM: Cart", "productId:\u{a0}7", "Items"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
		let item = diagram.texts.iter().find(|recorded| recorded.content == "Items").unwrap();
		let is_black = diagram.svg[item.attributes.clone()].contains("fill=\"black\"");
		assert_eq!(is_black, !options.is_dark);
	}
	let error = render_light("eventmodeling\ntf 01 ui A\ntf 01 cmd B\n").err().unwrap();
	assert_eq!(error.line, Some(3));
}

#[test]
fn renders_random_event_modeling_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 30] = [
		" ",
		"\n",
		"tf ",
		"rf ",
		"timeframe ",
		"01 ",
		"02 ",
		"1234 ",
		"ui ",
		"cmd ",
		"evt ",
		"rmo ",
		"pcr ",
		"A ",
		"B.C ",
		"->> 01 ",
		"->> 02 ",
		"[[D]] ",
		"`json` ",
		"{ a: 1 } ",
		"\"text\" ",
		"data D {\n  x: 1\n}\n",
		"entity E ",
		"note 01 {\n n\n}\n",
		"gwt 01 given evt A then cmd B ",
		"%% c",
		"// c\n",
		"/* c */",
		"title T",
		".",
	];
	render_random_diagrams(
		&[
			"eventmodeling\n",
			"eventmodeling\ntf 01 ui A\n",
			"%%{init: {\"eventmodeling\": {\"padding\": -1e9}}}%%\neventmodeling\n",
		],
		&FRAGMENTS,
		0x2E6B_91D4_0AC7_5F13,
	);
}

#[test]
fn renders_wardley_maps() {
	let source = "wardley-beta
title Tea Shop
size [800, 500]
evolution Genesis@0.2 -> Custom@0.5 -> Product@0.8 / Rental -> Commodity@1.0
anchor Business [0.95, 0.63]
component Cup of Tea [0.79, 0.61] label [19, -4] (buy) inertia
component Water [0.38, 0.82] (market)
component Kettle [0.43, 0.35]
pipeline Kettle {
  component Electric [0.62]
  component Campfire [0.35] label [-5, 3]
}
Business -> Cup of Tea
Cup of Tea +'boils'> Kettle
Kettle -.-> Water ; heats
evolve Kettle 0.62
note \"standardising\" [0.75, 0.42]
annotations [0.97, 0.3]
annotation 1,[0.48, 0.85] \"Standardising\"
accelerator Fast [0.5, 0.5]
deaccelerator Slow [0.3, 0.3]
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"wardley-beta\""));
		assert_eq!(diagram.svg.matches("class=\"wardley-node ").count(), 6);
		assert_eq!(diagram.svg.matches("class=\"wardley-link\"").count(), 2);
		assert_eq!(diagram.svg.matches("wardley-link--dashed").count(), 1);
		assert!(diagram.svg.contains("class=\"wardley-pipeline-box\""));
		for text in ["Tea Shop", "Product / Rental", "boils", "heats", "1. Standardising", "Fast"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	let error = render_light("wardley-beta\ncomponent market share [0.5, 0.5]\n").err().unwrap();
	assert_eq!(error.line, Some(2));
}

#[test]
fn renders_random_wardley_maps_without_panicking() {
	const FRAGMENTS: [&str; 34] = [
		" ",
		"\n",
		"component ",
		"anchor ",
		"A ",
		"B C ",
		"\"q\" ",
		"[0.5, 0.5] ",
		"[0.5] ",
		"[1, 0] ",
		"label [-5, 3] ",
		"(build) ",
		"(market) ",
		"inertia ",
		"-> ",
		"-.-> ",
		"+<> ",
		"+'x'> ",
		"; label",
		"pipeline A {\n",
		"}\n",
		"evolve A 0.7",
		"evolution X -> Y@0.5 / Z",
		"size [0, 0]",
		"size [100000, 100000]",
		"note \"n\" [0.2, 0.2]",
		"annotations [0.5, 0.5]",
		"annotation 1,[0.4, 0.4] \"t\"",
		"accelerator Q [0.5, 0.5]",
		"deaccelerator R [0.5, 0.5]",
		"title T",
		"%% c",
		"150.5",
		"@",
	];
	render_random_diagrams(
		&["wardley-beta\n", "wardley-beta\ncomponent A [0.5, 0.5]\ncomponent B [0.6, 0.6]\n"],
		&FRAGMENTS,
		0x6A0D_44C1_B7E2_9F58,
	);
}

#[test]
fn renders_venn_diagrams() {
	let source = "venn-beta
  title Skills
  set Frontend[\"Front end\"]:20
    text React
    text \"CSS\"
  set Backend
  set DevOps:8
  union Frontend, Backend[\"Fullstack\"]
    text REST
  union Frontend, Backend, DevOps
  style Frontend fill:#ff6b6b, color: red
  style Backend, DevOps fill: rgb(10, 20, 30)
";
	for options in [
		Options { is_dark: false, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
		Options { is_dark: true, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH },
	] {
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"venn\""));
		assert_eq!(diagram.svg.matches("venn-area venn-circle").count(), 3);
		assert_eq!(diagram.svg.matches("venn-area venn-intersection").count(), 4);
		for text in ["Skills", "Front end", "Fullstack", "React", "CSS", "REST"] {
			assert!(diagram.texts.iter().any(|recorded| recorded.content == text), "{text}");
		}
	}
	let error = render_light("venn-beta\nset A\nunion A,B\n").err().unwrap();
	assert_eq!((error.line, error.message.as_str()), (Some(3), "unknown set identifier: B"));
	assert!(render_light("venn-beta\n").is_err());
	let many: String = (0..40).map(|index| format!("set S{index}\n")).collect();
	assert!(render_light(&format!("venn-beta\n{many}")).is_err());
}

#[test]
fn renders_random_venn_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 26] = [
		" ",
		"\n",
		"\n  ",
		"set A",
		"set B",
		"set C",
		"set \"q d\"",
		"union A,B",
		"union A,B,C",
		"[\"Label\"]",
		"[x]",
		":5",
		":0",
		":-3",
		":1e9",
		"text A x",
		"text item",
		"style A fill: #f00, stroke-width: 9",
		"style A,B color: red",
		"title T",
		"%% c",
		"settings",
		",",
		":",
		"\"",
		"rgb(1,2,3)",
	];
	render_random_diagrams(
		&[
			"venn-beta\n",
			"venn-beta\nset A\nset B\n",
			"%%{init: {\"venn\": {\"width\": 0, \"height\": -5, \"padding\": 1e9}}}%%\nvenn-beta\nset A\n",
		],
		&FRAGMENTS,
		0x1B9E_5F02_C4D7_6A83,
	);
}

#[test]
fn renders_railroad_diagrams() {
	let sources = [
		(
			"railroad-beta\naccTitle: Numbers\nnumber = sequence(optional(terminal(\"-\")), oneOrMore(nonterminal(\"digit\")), zeroOrMore(choice(special(\"e\"), terminal(\".\")))) ;\ndigit = choice(terminal(\"0\"), terminal(\"1\")) ;\n",
			"railroad",
		),
		(
			"railroad-ebnf-beta\nexpression = term , { ( \"+\" | \"-\" ) , term } ;\nterm ::= [ \"-\" ] , digit+ - \"0\" , ? any ? ;\n",
			"railroadEbnf",
		),
		(
			"railroad-abnf-beta\nscheme = ALPHA *( ALPHA / DIGIT / \"+\" ) [ %x41-5A ] 2DIGIT ;\n; comment\n",
			"railroadAbnf",
		),
		(
			"railroad-peg-beta\nExpr <- Term ((\"+\" / \"-\") Term)* !. ; # comment\nTerm <- &[0-9] Digit+ / \"(\" Expr \")\" ;\n",
			"railroadPeg",
		),
	];
	for (source, role) in sources {
		for options in [
			Options {
				is_dark: false,
				limits: Limits::default(),
				screen_width: DEFAULT_SCREEN_WIDTH,
			},
			Options {
				is_dark: true,
				limits: Limits::default(),
				screen_width: DEFAULT_SCREEN_WIDTH,
			},
		] {
			let result = render(source, &options, &mut FixedMeasure);
			if role == "railroadPeg" {
				// `[` is no PEG token.
				assert_eq!(result.err().map(|error| error.line), Some(Some(3)));
				continue;
			}
			let diagram = result.unwrap_or_else(|error| panic!("{role}: {error:?}"));
			assert_valid_svg(&diagram);
			assert!(diagram.svg.contains(&format!("aria-roledescription=\"{role}\"")), "{role}");
			assert!(diagram.svg.contains("railroad-rule-name"));
		}
	}
	let peg =
		render_light("railroad-peg-beta\nExpr <- Term ((\"+\" / \"-\") Term)* !. ;\n").unwrap();
	assert!(peg.texts.iter().any(|text| text.content == "? !. ?"));
	let themed = render_light(
		"%%{init: {\"theme\": \"forest\", \"railroad\": {\"terminalFill\": \"#123456\", \"padding\": 3}}}%%\nrailroad-beta\na = terminal(\"x\") ;\n",
	)
	.unwrap();
	assert!(themed.svg.contains("fill=\"#123456\""));
	let error = render_light("railroad-beta\na = terminal(\"x\")\n").err().unwrap();
	assert_eq!(error.line, Some(2));
	assert!(render_light("railroad-beta\n").is_ok());
}

#[test]
fn renders_random_railroad_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 34] = [
		" ",
		"\n",
		"a = ",
		"b ::= ",
		"c <- ",
		";",
		"terminal(\"x\")",
		"nonterminal('y')",
		"special(\"s\")",
		"sequence(",
		"choice(",
		"optional(",
		"oneOrMore(",
		"zeroOrMore(",
		"(",
		")",
		",",
		"[",
		"]",
		"{",
		"}",
		"|",
		"/",
		"?",
		"*",
		"+",
		"-",
		"&",
		"!",
		".",
		"\"q\"",
		"2*3",
		"%x41-42",
		"(* c *)",
	];
	render_random_diagrams(
		&[
			"railroad-beta\n",
			"railroad-ebnf-beta\n",
			"railroad-abnf-beta\n",
			"railroad-peg-beta\n",
			"%%{init: {\"railroad\": {\"padding\": 1e9, \"fontSize\": 0, \"strokeWidth\": -1}}}%%\nrailroad-ebnf-beta\n",
		],
		&FRAGMENTS,
		0x5A1D_93C7_2E80_4B61,
	);
}

#[test]
fn renders_usecase_diagrams() {
	let source = "usecase-beta\ndirection LR\naccTitle: Shop\nactor Customer(\"Customer\")\nactor Staff@{ type: hollow, business: true } <<Employee>>\nactor Bot@{ type: awesome }\nactor Icon@{ icon: \"fa:user\" }\nsystemBoundary shop[\"Shop\"]@{ type: package }:::sys\n  Browse(\"Browse\")\n  Pay(\"Pay\")@{ business: true } <<Core>>\n  Ship[Ship order] <<Record>>\nend\njson Order@{ \"items\": [{ \"name\": \"Book\" }], \"total\": 29.95 }:::data\nnote for Browse \"`A **note**`\"\nCustomer starts@-- \"buys\" ---> Pay\nCustomer --> Browse\nStaff --o Ship\nBot --x Browse\nPay ..> : include Browse\nShip ..> : extend Pay\nBot --|> Customer\nShip --> Order\nclassDef sys stroke:#c8a02a\nclassDef data stroke:#3572a5\nstarts@{ animation: fast }\nstyle Pay stroke:#c33,stroke-width:3px\n";
	for is_dark in [false, true] {
		let options =
			Options { is_dark, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH };
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"usecase\""));
		for class in [
			"usecase-actor-stick",
			"usecase-business-marker",
			"boundary-tab",
			"usecase-json-key-cell",
		] {
			assert!(diagram.svg.contains(class), "{class}");
		}
	}
	for config in
		["look: classic\n  theme: default", "theme: neutral\n  usecase:\n    colorScheme: rotate"]
	{
		let source = format!(
			"---\nconfig:\n  {config}\n---\nusecase-beta\nactor A\nsystemBoundary S\n  U(Use)\nend\nA --> U\n"
		);
		assert_valid_svg(&render_light(&source).unwrap());
	}
	let error = render_light("usecase-beta\nactor A\nA ..> : include B\n").unwrap_err();
	assert_eq!(error.line, Some(3));
}

#[test]
fn renders_random_usecase_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 49] = [
		"@{ icon: \"fa:user\" }",
		"@{ icon: \"fa-i\" }",
		"fa:fa-car",
		" ",
		"\n",
		"\n  ",
		"actor ",
		"A",
		"B1",
		"\"Str ing\"",
		"\"`md **x**`\"",
		"(label)",
		"[rect]",
		"-->",
		"<--",
		"---->",
		"--",
		"--o",
		"o--",
		"--x",
		"x--",
		"--|>",
		"..> : include ",
		"..> : extend ",
		"e1@",
		"@{ type: hollow }",
		"@{ business: true }",
		"@{ icon: \"x\" }",
		"@{ type: package }",
		"@{ animate: true }",
		"<<St>>",
		":::c",
		",",
		"systemBoundary ",
		"end",
		"note for ",
		"json J@{ \"a\": [1, {\"b\": null}] }",
		"json K@{ \"a\": ",
		"classDef c ",
		"stroke:#f00",
		"class ",
		"style ",
		"direction TB",
		"direction RL",
		"accTitle: t",
		"accDescr { d }",
		"%% c",
		"{",
		"}",
	];
	render_random_diagrams(
		&[
			"usecase-beta\n",
			"---\nconfig:\n  look: classic\n  usecase:\n    colorScheme: rotate\n    nodeSpacing: -5\n    actorFontSize: 0\n    wrappingWidth: 1e9\n---\nusecase-beta\n",
		],
		&FRAGMENTS,
		0x7E6A_31C4_59D2_0B17,
	);
}

#[test]
fn renders_agentflow_diagrams() {
	let source = "---\ntitle: Agents\n---\nagentflow-beta TB\n  connector github[\"GitHub API\"]\n  github@{ protocol: \"http\", token_required: true }\n  flow orchestrator[\"Orchestrator\"]\n    gather[\"Gather\"]@{ shape: input }\n    plan[\"Plan\"]@{\n      shape: task,\n      label: \"Plan it\",\n    }\n    decide[\"Ready?\"]@{ shape: decision }\n    gather --> plan --> decide\n  end\n  flow workers[\"Workers\"]\n    search[\"web_search\"]@{ shape: tool, connectorRef: \"github.list\" }\n    notes[\"Notes\"]@{ shape: refdoc }\n    search -.- notes\n  end\n  flow archive[\"Archive\"]\n    store[\"Store\"]\n  end\n  archive@{ view: \"collapsed\" }\n  global\n    log\n  end\n  decide --> search\n  notes --> archive\n  decide --x archive\n  log --> github\n  classDef hot fill:#f96,stroke:#333\n  class plan hot\n  linkStyle 0 stroke:#f00\n";
	for is_dark in [false, true] {
		let options =
			Options { is_dark, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH };
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"agentflow\""));
		assert!(diagram.svg.contains("agentflowTitleText"));
	}
	let cases: [(&str, Option<usize>, &str); 4] = [
		// The reserved id `connectors` makes no vertex, so the edge ends at no node.
		("connectors --> y", None, "Cannot read properties of undefined (reading 'shape')"),
		// A flow container that ends up empty is drawn as a node.
		("flow e\n  connectors\nend", None, "No such shape: flowGroup. Please check your syntax."),
		("a@{\n  shape: tool\n  label: \"Multi\" x\n}", Some(5), "Invalid metadata for a"),
		("a@{ shape: Task }", Some(2), "No such shape: Task. Shape names should be lowercase."),
	];
	for (body, line, message) in cases {
		let error = render_light(&format!("agentflow-beta LR\n{body}\n")).unwrap_err();
		assert_eq!((error.line, error.message.as_str()), (line, message), "{body}");
	}
}

#[test]
fn renders_random_agentflow_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 44] = [
		" ",
		"\n",
		"\n  ",
		"A",
		"B1",
		"connectors",
		"[\"Str ing\"]",
		"(round)",
		"{{hex}}",
		"[[sub]]",
		"-->",
		"---",
		"-.-",
		"-.->",
		"==>",
		"--x",
		"--o",
		"<-->",
		"-- text -->",
		"|label|",
		"&",
		"e1@",
		"@{ shape: tool }",
		"@{ shape: input, label: \"In\" }",
		"@{ view: collapsed }",
		"@{ animate: true }",
		"@{\n  shape: task,\n  label: 'x' y\n}",
		"@{ shape: [",
		"flow ",
		"connector ",
		"global",
		"end",
		":::c",
		"classDef c fill:#f00",
		"class ",
		"style ",
		"linkStyle 0 stroke:#0f0",
		"direction TB",
		"click A \"https://x\"",
		"accTitle: t",
		"%% c",
		"{",
		"}",
		"\"",
	];
	render_random_diagrams(
		&[
			"agentflow-beta LR\n",
			"---\nconfig:\n  theme: dark\n  flowchart:\n    inheritDir: true\n  agentflow:\n    nodeSpacing: -5\n    wrappingWidth: 1e9\n---\nagentflow-beta TB\n",
		],
		&FRAGMENTS,
		0x4AF1_0C77_D3B2_6E95,
	);
}

#[test]
fn renders_swimlane_diagrams() {
	let source = "---
title: Lanes
---
swimlane-beta LR
  subgraph Dep1[Sales team]
    Start --> Check{Ok?}
  end
  subgraph Dep2
    Process2 --> Done([Done])
  end
  Check -- yes --> Process2
  Check -- no --> Start
  Loose --> Done
  style Dep1 fill:#fee
";
	for is_dark in [false, true] {
		let options =
			Options { is_dark, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH };
		let diagram = render(source, &options, &mut FixedMeasure).unwrap();
		assert_valid_svg(&diagram);
		assert!(diagram.svg.contains("aria-roledescription=\"swimlane\""));
		assert_eq!(diagram.svg.matches("class=\"cluster swimlane").count(), 3);
		assert!(diagram.svg.contains("__swimlane_default__"));
		assert!(diagram.svg.contains("class=\"label edgeLabel\" id=\"edge-label-Check-Process2-"));
		assert!(diagram.svg.contains("swimlane-title") && diagram.svg.contains("rotate(-90)"));
	}
	for direction in ["TB", "TD", "BT", "RL"] {
		let source = format!(
			"swimlane-beta {direction}
  subgraph A
    a1 --> a2
  end
  subgraph B
    b1
  end
  a2 --> b1 --> a1
"
		);
		assert_valid_svg(&render_light(&source).unwrap());
	}
	// `layout: swimlane` works for flowcharts too, `layout: dagre` turns it off.
	let flowchart = render_light(
		"---
config:
  layout: swimlane
---
flowchart TD
  subgraph L
    A --> B
  end
",
	)
	.unwrap();
	assert!(flowchart.svg.contains("cluster swimlane"));
	let dagre = render_light(
		"---
config:
  layout: dagre
---
swimlane-beta TD
  subgraph L
    A --> B
  end
",
	)
	.unwrap();
	assert!(!dagre.svg.contains("cluster swimlane"));
	let error = render_light(
		"swimlane-beta TB
  subgraph L
    A --> A
  end
",
	)
	.unwrap_err();
	assert_eq!(error.message, "Cannot read properties of undefined (reading 'filter')");
}

#[test]
fn renders_random_swimlane_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 30] = [
		" ",
		"
",
		"
  ",
		"A",
		"B1",
		"__swimlane_default__",
		"[\"Str ing\"]",
		"(round)",
		"{dec}",
		"([stadium])",
		"-->",
		"---",
		"-.->",
		"==>",
		"--x",
		"-- text -->",
		"|label|",
		"&",
		"subgraph ",
		"subgraph L1",
		"end",
		"direction LR",
		":::c",
		"classDef c fill:#f00",
		"style L1 fill:#0f0",
		"linkStyle 0 stroke:#0f0",
		"%% c",
		"\"",
		"[",
		"placeholder-0",
	];
	render_random_diagrams(
		&[
			"swimlane-beta LR
",
			"swimlane-beta TB
",
			"---
config:
  theme: dark
  swimlane:
    lineHops: gap
    automaticLaneOrdering: true
    ignoreCrossLaneEdges: false
  flowchart:
    nodeSpacing: -5
    rankSpacing: 1e9
---
swimlane-beta BT
",
		],
		&FRAGMENTS,
		0x5A1F_E6B2_0D93_4C71,
	);
}

fn render_in_mode(source: &str, is_dark: bool) -> Result<Diagram> {
	let options =
		Options { is_dark, limits: Limits::default(), screen_width: DEFAULT_SCREEN_WIDTH };
	render(source, &options, &mut FixedMeasure)
}

/// A diagram of every type that Mermaid sketches with roughjs.
const HAND_DRAWN_BODIES: [&str; 14] = [
	"flowchart LR\n  A[Rect] --> B(Round) --> C{Diamond}\n  subgraph S\n    D[(Db)] --> E((Circle))\n  end\n  C -.-> D\n  A@{ shape: docs }\n  style A fill:#fdd,stroke:#f00,stroke-width:3px,stroke-dasharray:4 2",
	"stateDiagram-v2\n  [*] --> A\n  state A {\n    a --> b\n    --\n    c --> [*]\n  }\n  A --> F\n  state F <<fork>>\n  state C <<choice>>\n  F --> C\n  note right of C: a note",
	"classDiagram\n  class Animal {\n    +String name\n    +eat() void\n  }\n  Animal <|-- Duck\n  note for Duck \"can fly\"",
	"erDiagram\n  CUSTOMER ||--o{ ORDER : places\n  ORDER {\n    string id PK\n    int count\n  }",
	"requirementDiagram\n  requirement r {\n    id: 1\n    text: t\n  }\n  element e {\n    type: t\n  }\n  e - satisfies -> r",
	"usecase-beta\n  actor A\n  actor H@{ type: hollow }\n  actor B@{ business: true }\n  systemBoundary \"S\"\n    U(\"Use\")\n    R[Rect]\n  end\n  X(\"Biz\")@{ business: true }\n  A --> U\n  H --> R\n  B --> X\n  json J@{\n    \"k\": \"v\"\n  }",
	"swimlane-beta TB\n  subgraph L1\n    a --> b\n  end\n  subgraph L2\n    c\n  end\n  b --> c\n  a --> c",
	"agentflow-beta TB\n  flow F[\"Flow\"]\n    a[\"A\"]@{ shape: tool }\n    b[\"B\"]\n    a --> b\n  end\n  flow G[\"Gone\"]\n    c[\"C\"]\n  end\n  b --> c\n  G@{ view: \"collapsed\" }",
	"C4Context\n  Person(p, \"P\", \"x\")\n  System(s, \"S\")\n  SystemDb(d, \"D\")\n  SystemQueue(q, \"Q\")\n  Rel(p, s, \"uses\")",
	"block-beta\n  columns 3\n  a[\"A\"] b(\"B\") c((\"C\"))\n  d{\"D\"} e[(\"E\")] f>\"F\"]",
	"kanban\n  Todo\n    t1[Task one]\n  Done\n    t2[Task two]",
	"mindmap\n  root((Root))\n    A[Square]\n    B(Round)\n      C{{Hex}}",
	"venn-beta\n  set A\n  set B\n  union A,B\n  style A,B fill:#f00",
	"ishikawa\n  Effect\n    Cause one\n      Sub cause\n    Cause two",
];

fn hand_drawn(body: &str, seed: &str) -> String {
	format!("---\nconfig:\n  look: handDrawn\n  handDrawnSeed: {seed}\n---\n{body}")
}

#[test]
fn renders_hand_drawn_diagrams() {
	for body in HAND_DRAWN_BODIES {
		let classic = format!(
			"---
config:
  look: classic
---
{body}"
		);
		let classic = render_light(&classic).unwrap_or_else(|error| panic!("{body}: {error:?}"));
		for is_dark in [false, true] {
			let source = hand_drawn(body, "7");
			let diagram =
				render_in_mode(&source, is_dark).unwrap_or_else(|error| panic!("{error:?}"));
			assert_valid_svg(&diagram);
			// The hachure lines make the sketch much longer.
			assert!(diagram.svg.len() > classic.svg.len() + 2000, "not sketched: {body}");
			// EdgeHTML draws no line with it.
			assert!(!diagram.svg.contains("stroke-dasharray=\"0 0\""), "{body}");
			let again =
				render_in_mode(&source, is_dark).unwrap_or_else(|error| panic!("{error:?}"));
			assert_eq!(again.svg, diagram.svg);
		}
		// Seed 0 is not random here, and other seeds draw other pictures.
		let unseeded = render_light(&hand_drawn(body, "0")).unwrap_or_else(|e| panic!("{e:?}"));
		let unseeded_again =
			render_light(&hand_drawn(body, "0")).unwrap_or_else(|e| panic!("{e:?}"));
		assert_eq!(unseeded.svg, unseeded_again.svg);
		let seeded = render_light(&hand_drawn(body, "7")).unwrap_or_else(|e| panic!("{e:?}"));
		assert_ne!(unseeded.svg, seeded.svg, "{body}");
	}
}

#[test]
fn hand_drawn_shapes_have_the_class_and_look_of_mermaid() {
	let diagram =
		render_light(&hand_drawn("flowchart LR\n  A --> B((B))\n  C@{ shape: sm-circ }", "7"))
			.unwrap_or_else(|error| panic!("{error:?}"));
	assert_eq!(count(&diagram.svg, "class=\"rough-node default\""), 2);
	assert_eq!(count(&diagram.svg, "class=\"node default\""), 1);
	assert!(diagram.svg.contains("data-look=\"handDrawn\""));
	assert!(diagram.svg.contains(" transition"));
	let er = render_light(&hand_drawn("erDiagram\n  A {\n    int x\n  }", "7"))
		.unwrap_or_else(|error| panic!("{error:?}"));
	assert!(er.svg.contains("-background\""), "{}", er.svg);
}

#[test]
fn renders_hostile_hand_drawn_input() {
	let long_label = "x".repeat(3000);
	for seed in ["-5", "2147483648", "1e30", "-1e300", "3.7", "abc", "null", "[1]"] {
		let diagram = render_light(&hand_drawn("flowchart LR\n  A --> B", seed))
			.unwrap_or_else(|error| panic!("{seed}: {error:?}"));
		assert_valid_svg(&diagram);
	}
	for body in [
		"flowchart LR\n  A --> B\n  style A stroke-width:1e9,stroke-dasharray:abc",
		"flowchart LR\n  A --> B\n  style A stroke-width:-3,stroke-dasharray:1 2 3 4,fill:a:b",
		"flowchart LR\n  A --> B\n  style B stroke-width:NaN,stroke:none,fill:none",
		"flowchart LR\n  A@{ shape: circle, label: \"\" }\n  B@{ shape: text }\n  C@{ shape: bolt }",
		&format!("flowchart LR\n  A[{long_label}] --> B"),
		"venn-beta\n  set A\n  set B\n  union A,B\n  style A stroke-width:abc,fill:notacolor",
		"ishikawa\n  E",
		"usecase-beta\n  actor A\n  style A stroke-width:1e9",
	] {
		let diagram = render_light(&hand_drawn(body, "7"))
			.unwrap_or_else(|error| panic!("{body}: {error:?}"));
		assert!(diagram.svg.ends_with("</svg>"));
	}
}

#[test]
fn renders_random_hand_drawn_diagrams_without_panicking() {
	const FRAGMENTS: [&str; 30] = [
		"A",
		"B",
		" ",
		"\n",
		"-->",
		"-.->",
		"==>",
		"[",
		"]",
		"(",
		")",
		"{",
		"}",
		"((",
		"))",
		"@{ shape: cyl }",
		"@{ shape: docs }",
		"@{ shape: fr-circ }",
		"@{ shape: brace }",
		"subgraph S\n",
		"end\n",
		"style A fill:#f00,stroke-width:9e9,stroke-dasharray:x\n",
		"classDef c stroke:none,fill:none\n",
		":::c",
		"\"",
		"|t|",
		"1e300",
		"-",
		"state",
		"ä😀",
	];
	render_random_diagrams(
		&[
			"---\nconfig:\n  look: handDrawn\n  handDrawnSeed: 7\n---\nflowchart LR\n",
			"---\nconfig:\n  look: handDrawn\n---\nstateDiagram-v2\n",
			"---\nconfig:\n  look: handDrawn\n  handDrawnSeed: -3\n---\nswimlane-beta TB\n",
			"---\nconfig:\n  look: handDrawn\n  theme: dark\n---\nblock-beta\n",
			"---\nconfig:\n  look: handDrawn\n---\nusecase-beta\n",
			"---\nconfig:\n  look: handDrawn\n---\nclassDiagram\n",
		],
		&FRAGMENTS,
		0x7C3D_19A4_E255_0B6F,
	);
}

#[test]
fn renders_elk_layout_diagrams() {
	let bodies = [
		"flowchart TD\n  A --> B & C & D\n  B --> E\n  C -->|label| E\n  D --> E\n  E --> A\n  A --> A",
		"flowchart LR\n  A[Start] --> B{Is it?}\n  B -->|Yes| C((OK))\n  C --> D[(Rethink)]\n  D --> B\n  B ---->|No| E>End]",
		"flowchart BT\n  A --> B\n  subgraph S [Sub]\n    B --> C\n    subgraph T\n      D\n    end\n  end\n  C --> D\n  A --> S\n  S --> E\n  T --> A",
		"flowchart RL\n  A --> B\n  subgraph S\n    direction TB\n    C --> D\n  end\n  B --> S\n  subgraph Empty\n  end\n  S --> S",
		"stateDiagram-v2\n  [*] --> Still\n  Still --> Moving\n  Moving --> Still\n  Moving --> Crash\n  state Crash {\n    [*] --> Burning\n    Burning --> [*]\n  }\n  Crash --> [*]",
		"classDiagram\n  Animal <|-- Duck\n  Animal \"1\" --> \"*\" Fish : eats\n  class Duck {\n    +swim()\n  }",
		"erDiagram\n  CUSTOMER ||--o{ ORDER : places\n  ORDER ||--|{ LINE-ITEM : contains\n  CUSTOMER }|..|{ DELIVERY-ADDRESS : uses",
		"requirementDiagram\n  requirement test_req {\n    id: 1\n    text: the test text.\n    risk: high\n    verifymethod: test\n  }\n  element test_entity {\n    type: simulation\n  }\n  test_entity - satisfies -> test_req",
	];
	for body in bodies {
		for layout in ["elk", "elk.mrtree"] {
			let source = format!("---\nconfig:\n  layout: {layout}\n---\n{body}");
			let first = render_light(&source).unwrap_or_else(|error| panic!("{body}: {error:?}"));
			assert_valid_svg(&first);
			assert_eq!(render_light(&source).unwrap().svg, first.svg, "deterministic");
		}
	}
	// `flowchart-elk` selects the layout, `dagre` keeps the old one.
	let elk = render_light("flowchart-elk TD\n  A --> B & C").unwrap();
	let configured =
		render_light("---\nconfig:\n  layout: elk\n---\nflowchart TD\n  A --> B & C").unwrap();
	let dagre =
		render_light("---\nconfig:\n  layout: dagre\n---\nflowchart TD\n  A --> B & C").unwrap();
	assert!(elk.svg.contains('Q') && configured.svg.contains('Q'), "rounded corners");
	assert_ne!(configured.svg, dagre.svg);
}

#[test]
fn renders_hostile_elk_input() {
	let bodies = [
		"flowchart TD\n  A --> B\n  style A width:1e300px,height:-5px\n  B --> B\n  B --> B\n  B -->|x| B",
		"flowchart TD\n  subgraph S\n    A\n  end\n  S --> A\n  A --> S\n  S --> S",
		"flowchart LR\n  subgraph A\n    subgraph B\n      subgraph C\n        x\n      end\n    end\n  end\n  x --> A\n  C --> B",
		"flowchart TD\n  A --> B\n  A --> B\n  B --> A\n  B --> A\n  C",
	];
	for body in bodies {
		let source = format!("---\nconfig:\n  layout: elk\n---\n{body}");
		if let Ok(diagram) = render_light(&source) {
			assert!(diagram.svg.ends_with("</svg>"));
		}
	}
	// Deep nesting.
	let mut source = String::from("---\nconfig:\n  layout: elk\n---\nflowchart TD\n");
	for level in 0..60 {
		source.push_str(&format!("subgraph s{level}\n"));
		if level % 7 == 0 {
			source.push_str("direction LR\n");
		}
	}
	source.push_str("x --> y\n");
	for _ in 0..60 {
		source.push_str("end\n");
	}
	// Edges crossing every level, to a cluster and from a cluster to a node deep inside it.
	source.push_str("z --> x\ny --> z\ns3 --> z\ns0 --> x\ns5 --> s50\n");
	let diagram = render_light(&source).unwrap();
	assert!(diagram.svg.ends_with("</svg>"));
}
