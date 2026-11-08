//! Architecture diagrams (`architectureDb.ts`, `architectureTypes.ts`, `architectureRenderer.ts`,
//! `svgDraw.ts`, `architectureStyles.ts`, `architectureIcons.ts`, `architectureSeed.ts`), laid
//! out with fcose like Mermaid's cytoscape graph (`architecture/layout.rs`).
//!
//! Icons are Mermaid's built-in architecture icons; other icon packs are not loaded, so their
//! names draw the unknown icon like an unregistered pack does in Mermaid. Icon texts, which
//! Mermaid writes as HTML in a `foreignObject`, are SVG text here, which every renderer shows.

mod db;
mod layout;
mod parser;

use std::collections::HashMap;

use super::formatted_text::{self, Line};
use super::icons::{self, write_icon_tile, write_unknown_icon};
use super::label::{self, encode_entities};
use super::layout::fcose::RelativePlacement;
use super::rendering::{BBox, Point, translate};
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::Font;
use super::{Context, Diagram, Result};
use db::Db;
use layout::{Box as CyBox, EdgePoints, Kind};
use parser::Direction;

/// The font of cytoscape's labels, which size the groups.
const CYTOSCAPE_FONT: &str = "Helvetica Neue, Helvetica, sans-serif";

struct Settings {
	padding: f64,
	icon_size: f64,
	font_size: f64,
	use_max_width: bool,
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let code = encode_entities(code);
	let architecture = parser::parse(&code, context)?;
	let accessibility = architecture.common.accessibility.clone();
	let db = Db::new(architecture, context)?;
	if db.edges.len() > context.limits.max_edges {
		return Err(context.error_at(
			1,
			format!(
				"Too many edges ({} > {}); raise the limit in the Mermaid settings",
				db.edges.len(),
				context.limits.max_edges
			),
		));
	}
	let config = |key: &str, default: f64| context.config_number("architecture", key, default);
	let settings = Settings {
		padding: config("padding", 40.0).clamp(0.0, 10_000.0),
		icon_size: match config("iconSize", 80.0) {
			size if size > 0.0 => size.min(10_000.0),
			_ => 80.0,
		},
		font_size: config("fontSize", 16.0).clamp(0.0, 1_000.0),
		use_max_width: context.config_bool("architecture", "useMaxWidth", true),
	};
	let multiplier = config("idealEdgeLengthMultiplier", 1.5);
	let elasticity = config("edgeElasticity", 0.45);
	let iterations = config("numIter", 2500.0).clamp(0.0, 100_000.0);
	let seed = match js_to_uint32(config("seed", 1.0)) {
		// 0 leaves `Math.random` as it is: any layout of Mermaid's is one of its variants.
		0 => 1,
		seed => seed,
	};

	// The cytoscape elements: groups, services, junctions, edges.
	let group_count = db.groups.len();
	let node_index = |id: &str| {
		db.group_position(id).or_else(|| db.node_position(id).map(|node| group_count + node))
	};
	let mut nodes = Vec::with_capacity(group_count + db.nodes.len());
	for group in &db.groups {
		let parent = group.parent.as_deref().and_then(node_index);
		nodes.push(layout::Node { kind: Kind::Group, parent, label: None });
	}
	let label_font = Font::new(CYTOSCAPE_FONT, settings.font_size);
	for node in &db.nodes {
		let label =
			node.title.as_deref().filter(|title| !title.is_empty()).map(|title| {
				(canvas_width(context, title, &label_font).ceil(), settings.font_size)
			});
		nodes.push(layout::Node {
			kind: if node.is_junction { Kind::Junction } else { Kind::Service },
			parent: node.parent.as_deref().and_then(node_index),
			label,
		});
	}
	let mut ids: HashMap<String, ()> = db
		.groups
		.iter()
		.map(|group| (group.id.clone(), ()))
		.chain(db.nodes.iter().map(|node| (node.id.clone(), ())))
		.collect();
	let edge_font = Font::new(CYTOSCAPE_FONT, 16.0);
	let mut edges = Vec::with_capacity(db.edges.len());
	for edge in &db.edges {
		let id = format!("{}-{}", edge.lhs_id, edge.rhs_id);
		if ids.insert(id.clone(), ()).is_some() {
			return Err(context
				.error_at(edge.line, format!("Can not create second element with ID `{id}`")));
		}
		let (Some(source), Some(target)) = (node_index(&edge.lhs_id), node_index(&edge.rhs_id))
		else {
			continue;
		};
		let label = edge
			.title
			.as_deref()
			.filter(|title| !title.is_empty())
			.map(|title| (canvas_width(context, title, &edge_font).ceil(), 16.0));
		edges.push(layout::Edge {
			source,
			target,
			source_dir: edge.lhs_dir,
			target_dir: edge.rhs_dir,
			is_segments: edge.lhs_dir.is_x() != edge.rhs_dir.is_x(),
			label,
		});
	}
	let constraints = db.constraints(settings.icon_size, multiplier);
	let to_indices = |lists: &[Vec<String>]| -> Vec<Vec<usize>> {
		lists.iter().map(|list| list.iter().filter_map(|id| node_index(id)).collect()).collect()
	};
	let layout_constraints = layout::Constraints {
		horizontal: to_indices(&constraints.horizontal),
		vertical: to_indices(&constraints.vertical),
		relative: constraints
			.relative
			.iter()
			.filter_map(|relative| {
				Some(RelativePlacement {
					first: node_index(&relative.first)?,
					second: node_index(&relative.second)?,
					is_horizontal: relative.is_horizontal,
					gap: relative.gap,
				})
			})
			.collect(),
	};
	let layout_config = layout::Config {
		icon_size: settings.icon_size,
		padding: settings.padding,
		iterations,
		seed,
		same_group_length: multiplier * settings.icon_size,
		cross_group_length: 0.5 * settings.icon_size,
		same_group_elasticity: elasticity,
	};
	let result = layout::layout(&nodes, &edges, &layout_constraints, &layout_config).map_err(
		|_| {
			context.error_at(
				1,
				"Architecture layout failed: a declared `align row|column` directive likely contradicts the edge directions, or two declared alignments overlap on a shared node. Check that the order of members in each `align` chain is consistent with the edges between them, and that no node appears in two `align` directives along the same axis.",
			)
		},
	)?;

	let mut drawing =
		Drawing { context, settings, db: &db, body: Svg::new(), bbox: BBox::default() };
	drawing.body.open("g").end().close_tag("g");
	drawing.draw_edges(&result.edges);
	drawing.draw_services(&result.positions[group_count..]);
	drawing.draw_groups(&result.boxes[..group_count]);
	let Drawing { context, settings, mut body, bbox, .. } = drawing;
	let _ = &mut body;
	let (x, y, width, height) = match bbox.is_empty() {
		true => (0.0, 0.0, 0.0, 0.0),
		false => (bbox.min.x, bbox.min.y, bbox.max.x - bbox.min.x, bbox.max.y - bbox.min.y),
	};
	let frame = Frame {
		x: x - settings.padding,
		y: y - settings.padding,
		width: width + settings.padding * 2.0,
		height: height + settings.padding * 2.0,
		use_max_width: settings.use_max_width,
	};
	Ok(document(context, "architecture", &frame, &accessibility, body))
}

/// `measureText` of a canvas: Blink shapes each word and space on its own there, so there is
/// no kerning across spaces.
fn canvas_width(context: &mut Context, text: &str, font: &Font) -> f64 {
	let words: f64 = text.split(' ').map(|word| context.measure_exact(word, font)).sum();
	words + text.matches(' ').count() as f64 * context.measure_exact(" ", font)
}

/// `value >>> 0`.
fn js_to_uint32(value: f64) -> u32 {
	if !value.is_finite() {
		return 0;
	}
	value.trunc().rem_euclid(4_294_967_296.0) as u32
}

struct Drawing<'c, 'm> {
	context: &'c mut Context<'m>,
	settings: Settings,
	db: &'c Db,
	body: Svg,
	/// `getBBox` of the diagram.
	bbox: BBox,
}

impl Drawing<'_, '_> {
	fn root_font(&self) -> Font {
		Font::new(self.context.font_family(), self.context.font_size())
	}

	fn text_color(&self) -> String {
		self.context.theme.css("textColor")
	}

	/// A `createText` label without HTML at `(x, y)` (the group's translation), rotated by
	/// `rotation` degrees around the group's origin; adds its box to the diagram's.
	fn write_label(
		&mut self,
		text: &str,
		width: f64,
		(anchor, baseline): (Anchor, Baseline),
		transform: &str,
		map: &dyn Fn(Point) -> Point,
	) {
		let font = self.root_font();
		let rows = self.rows(text, width, &font);
		let fill = self.text_color();
		self.body.open("g").attr("transform", transform).end();
		let attributes = |svg: &mut Svg| _ = svg.attr_if("fill", &fill);
		formatted_text::write(
			&mut self.body,
			self.context,
			&rows,
			&font,
			(anchor, baseline),
			&attributes,
		);
		self.body.close_tag("g");
		let bbox = formatted_text::bounding_box(self.context, &rows, &font, (anchor, baseline));
		self.include_box(&bbox, map);
	}

	fn rows(&mut self, text: &str, width: f64, font: &Font) -> Vec<Line> {
		let lines = formatted_text::markdown_lines(text);
		formatted_text::wrap(self.context, &lines, width, font)
	}

	/// Adds the corners of `bbox`, mapped to the diagram, to the diagram's box.
	fn include_box(&mut self, bbox: &BBox, map: &dyn Fn(Point) -> Point) {
		if bbox.is_empty() {
			return;
		}
		for (x, y) in [
			(bbox.min.x, bbox.min.y),
			(bbox.max.x, bbox.min.y),
			(bbox.min.x, bbox.max.y),
			(bbox.max.x, bbox.max.y),
		] {
			self.bbox.include_point(map(Point { x, y }));
		}
	}

	/// `drawEdges`.
	fn draw_edges(&mut self, points: &[EdgePoints]) {
		let db = self.db;
		let padding = self.settings.padding;
		let icon_size = self.settings.icon_size;
		let half_icon_size = icon_size / 2.0;
		let arrow_size = icon_size / 6.0;
		let half_arrow_size = arrow_size / 2.0;
		let stroke = self.context.theme.css("archEdgeColor");
		let stroke_width = self.context.theme.css("archEdgeWidth");
		let arrow_fill = self.context.theme.css("archEdgeArrowColor");
		self.body.open("g").attr("class", "architecture-edges").end();
		for (edge, points) in db.edges.iter().zip(points) {
			let (mut start, mid, mut end) = (points.start, points.mid, points.end);
			let group_shift = padding + 4.0;
			let shift = |point: &mut (f64, f64), direction: Direction, amount: (f64, f64)| {
				match direction {
					Direction::Left => point.0 -= amount.0,
					Direction::Right => point.0 += amount.0,
					Direction::Top => point.1 -= amount.0,
					Direction::Bottom => point.1 += amount.1,
				}
			};
			if edge.lhs_group {
				shift(&mut start, edge.lhs_dir, (group_shift, group_shift + 18.0));
			}
			if edge.rhs_group {
				shift(&mut end, edge.rhs_dir, (group_shift, group_shift + 18.0));
			}
			let is_junction = |id: &str| db.node(id).is_some_and(|node| node.is_junction);
			// Into the junction's center.
			let inward = |point: &mut (f64, f64), direction: Direction| match direction {
				Direction::Left => point.0 += half_icon_size,
				Direction::Right => point.0 -= half_icon_size,
				Direction::Top => point.1 += half_icon_size,
				Direction::Bottom => point.1 -= half_icon_size,
			};
			if !edge.lhs_group && is_junction(&edge.lhs_id) {
				inward(&mut start, edge.lhs_dir);
			}
			if !edge.rhs_group && is_junction(&edge.rhs_id) {
				inward(&mut end, edge.rhs_dir);
			}
			self.body.open("g").end();
			let path = format!(
				"M {},{} L {},{} L{},{} ",
				number(start.0),
				number(start.1),
				number(mid.0),
				number(mid.1),
				number(end.0),
				number(end.1)
			);
			self.body
				.open("path")
				.attr("d", &path)
				.attr("class", "edge")
				.attr("id", &format!("{}-L_{}_{}_0", self.context.id, edge.lhs_id, edge.rhs_id))
				.attr("fill", "none")
				.attr_if("stroke", &stroke)
				.attr_if("stroke-width", &stroke_width)
				.close();
			for point in [start, mid, end] {
				self.bbox.include_point(Point { x: point.0, y: point.1 });
			}
			for (is_arrow, direction, point) in
				[(edge.lhs_into, edge.lhs_dir, start), (edge.rhs_into, edge.rhs_dir, end)]
			{
				if !is_arrow {
					continue;
				}
				let shift_x = match direction {
					Direction::Left => point.0 - arrow_size + 2.0,
					Direction::Right => point.0 - 2.0,
					_ => point.0 - half_arrow_size,
				};
				let shift_y = match direction {
					Direction::Top => point.1 - arrow_size + 2.0,
					Direction::Bottom => point.1 - 2.0,
					_ => point.1 - half_arrow_size,
				};
				let s = arrow_size;
				let corners = match direction {
					Direction::Left => [(s, s / 2.0), (0.0, s), (0.0, 0.0)],
					Direction::Right => [(0.0, s / 2.0), (s, 0.0), (s, s)],
					Direction::Top => [(0.0, 0.0), (s, 0.0), (s / 2.0, s)],
					Direction::Bottom => [(s / 2.0, 0.0), (s, s), (0.0, s)],
				};
				let points: Vec<String> =
					corners.iter().map(|(x, y)| format!("{},{}", number(*x), number(*y))).collect();
				self.body
					.open("polygon")
					.attr("points", &points.join(" "))
					.attr("transform", &translate(shift_x, shift_y))
					.attr("class", "arrow")
					.attr_if("fill", &arrow_fill)
					.close();
				for (x, y) in corners {
					self.bbox.include_point(Point { x: x + shift_x, y: y + shift_y });
				}
			}
			if let Some(title) = edge.title.as_deref().filter(|title| !title.is_empty()) {
				self.draw_edge_label(edge, title, (start, mid, end));
			}
			self.body.close_tag("g");
		}
		self.body.close_tag("g");
	}

	/// The label of an edge, along the edge: rotated on vertical edges, at 45 degrees on bent
	/// ones.
	fn draw_edge_label(
		&mut self,
		edge: &parser::Edge,
		title: &str,
		(start, mid, end): ((f64, f64), (f64, f64), (f64, f64)),
	) {
		let is_bent = edge.lhs_dir.is_x() != edge.rhs_dir.is_x();
		let width = match () {
			_ if is_bent => (start.0 - end.0).abs() / 2.0,
			_ if edge.lhs_dir.is_x() => (start.0 - end.0).abs(),
			_ => (start.1 - end.1).abs() / 1.5,
		};
		let middle = (Anchor::Middle, Baseline::Middle);
		if !is_bent {
			let (transform, angle) = match edge.lhs_dir.is_x() {
				true => (format!("translate({}, {})", number(mid.0), number(mid.1)), 0.0),
				false => {
					(format!("translate({}, {}) rotate(-90)", number(mid.0), number(mid.1)), -90.0)
				}
			};
			let map = move |point: Point| rotate_translate(point, angle, (0.0, 0.0), mid);
			self.write_label(title, width, middle, &transform, &map);
			return;
		}
		// The size of the label with its middle baseline, then rotated with the alphabetic one.
		let font = self.root_font();
		let rows = self.rows(title, width, &font);
		let original = formatted_text::bounding_box(self.context, &rows, &font, middle);
		let auto = (Anchor::Middle, Baseline::Alphabetic);
		let unrotated = formatted_text::bounding_box(self.context, &rows, &font, auto);
		let (x, y) = xy_factors(edge.lhs_dir, edge.rhs_dir);
		let angle = -x * y * 45.0;
		let rotated = rotated_box(&unrotated, angle, (0.0, 0.0));
		let original_height = original.max.y - original.min.y;
		let (rotated_width, rotated_height) =
			(rotated.max.x - rotated.min.x, rotated.max.y - rotated.min.y);
		let offset = (
			mid.0 + x * rotated_width / 2.0,
			mid.1 - original_height / 2.0 + y * rotated_height / 2.0,
		);
		let pivot = (0.0, original_height / 2.0);
		let transform = format!(
			"translate({}, {}) rotate({}, 0, {})",
			number(offset.0),
			number(offset.1),
			number(angle),
			number(pivot.1)
		);
		let fill = self.text_color();
		self.body.open("g").attr("transform", &transform).end();
		let attributes = |svg: &mut Svg| _ = svg.attr_if("fill", &fill);
		formatted_text::write(&mut self.body, self.context, &rows, &font, auto, &attributes);
		self.body.close_tag("g");
		let map = move |point: Point| rotate_translate(point, angle, pivot, offset);
		self.include_box(&unrotated, &map);
	}

	/// `drawServices` and `drawJunctions`, placed like `positionNodes`.
	fn draw_services(&mut self, positions: &[(f64, f64)]) {
		let db = self.db;
		let icon_size = self.settings.icon_size;
		self.body.open("g").attr("class", "architecture-services").end();
		let (services, junctions): (Vec<usize>, Vec<usize>) =
			(0..db.nodes.len()).partition(|&node| !db.nodes[node].is_junction);
		for node in services.into_iter().chain(junctions) {
			let data = &db.nodes[node];
			let (x, y) = positions.get(node).copied().unwrap_or((0.0, 0.0));
			let place = move |point: Point| Point { x: point.x + x, y: point.y + y };
			if data.is_junction {
				self.body
					.open("g")
					.attr("class", "architecture-junction")
					.attr("transform", &translate(x, y))
					.end();
				self.body
					.open("g")
					.end()
					.open("rect")
					.attr("id", &format!("{}-node-{}", self.context.id, data.id))
					.attr("fill-opacity", "0")
					.number_attr("width", icon_size)
					.number_attr("height", icon_size)
					.close()
					.close_tag("g")
					.close_tag("g");
				self.bbox.include_rect(x, y, icon_size, icon_size);
				continue;
			}
			self.body
				.open("g")
				.attr("id", &format!("{}-service-{}", self.context.id, data.id))
				.attr("class", "architecture-service")
				.attr("transform", &translate(x, y))
				.end();
			if let Some(title) = data.title.as_deref().filter(|title| !title.is_empty()) {
				let transform =
					format!("translate({}, {})", number(icon_size / 2.0), number(icon_size));
				let offset = (icon_size / 2.0, icon_size);
				let map = move |point: Point| {
					place(Point { x: point.x + offset.0, y: point.y + offset.1 })
				};
				self.write_label(
					title,
					icon_size * 1.5,
					(Anchor::Middle, Baseline::Middle),
					&transform,
					&map,
				);
			}
			self.body.open("g").end();
			match (&data.icon, &data.icon_text) {
				(Some(icon), _) => self.write_icon(icon, icon_size),
				(None, Some(text)) => {
					self.write_icon("blank", icon_size);
					self.write_icon_text(text);
				}
				(None, None) => {
					let s = icon_size;
					let path = format!(
						"M0,{} V5 Q0,0 5,0 H{} Q{},0 {},5 V{} Z",
						number(s),
						number(s - 5.0),
						number(s),
						number(s),
						number(s)
					);
					let stroke = self.context.theme.css("archGroupBorderColor");
					let stroke_width = self.context.theme.css("archGroupBorderWidth");
					self.body
						.open("path")
						.attr("class", "node-bkg")
						.attr("id", &format!("{}-node-{}", self.context.id, data.id))
						.attr("d", &path)
						.attr("fill", "none")
						.attr_if("stroke", &stroke)
						.attr_if("stroke-width", &stroke_width)
						.attr("stroke-dasharray", "8")
						.close();
				}
			}
			self.bbox.include_rect(x, y, icon_size, icon_size);
			self.body.close_tag("g").close_tag("g");
		}
		self.body.close_tag("g");
	}

	/// The `<svg>` of a built-in icon (`getIconSVG`), the unknown icon for other names.
	fn write_icon(&mut self, name: &str, size: f64) {
		if let Some(glyph) = icons::registered_glyph(name, Some("mermaid-architecture")) {
			self.body.open("g").end().open("g").end();
			// `currentColor`, black on a light page.
			let fill = match self.context.theme.text("darkMode").as_deref() {
				Some("true") => self.context.theme.css("textColor"),
				_ => "#000".to_owned(),
			};
			icons::write_glyph_svg(&mut self.body, &glyph, size, &|svg| {
				svg.attr_if("fill", &fill);
			});
			self.body.close_tag("g").close_tag("g");
			return;
		}
		let name = builtin_icon_name(name).unwrap_or("unknown");
		self.body
			.open("g")
			.end()
			.open("svg")
			.attr("xmlns", "http://www.w3.org/2000/svg")
			.number_attr("width", size)
			.number_attr("height", size)
			.attr("viewBox", "0 0 80 80")
			.end()
			.open("g")
			.end();
		if name == "unknown" {
			write_unknown_icon(&mut self.body);
		} else {
			write_icon_tile(&mut self.body);
			self.body.out.push_str(builtin_icon_body(name));
		}
		self.body.close_tag("g").close_tag("svg").close_tag("g");
	}

	/// The icon text of a service (an HTML flex box in Mermaid): white lines as wide as the text,
	/// at most the icon's width, centered between them, at the left of the icon and in its
	/// middle; at most as many lines as fit (`-webkit-line-clamp`), the last one cut with an
	/// ellipsis.
	fn write_icon_text(&mut self, text: &str) {
		let icon_size = self.settings.icon_size;
		let font = self.root_font();
		let plain = strip_tags(&label::decode_entities_to_text(text));
		let available = icon_size - 2.0;
		let max_lines = (available / font.size).floor().max(0.0) as usize;
		let runs = [label::Run { text: plain.clone(), style: label::Style::default() }];
		let (wrapped, _) = label::wrap_normal(self.context, &font, &runs, available);
		let mut lines: Vec<String> = wrapped
			.iter()
			.map(|line| line.runs.iter().map(|(run, _)| run.text.as_str()).collect::<String>())
			.map(|line| line.trim().to_owned())
			.collect();
		if lines.len() > max_lines {
			lines.truncate(max_lines);
			if let Some(last) = lines.last_mut() {
				last.push('…');
			}
		}
		let content_width = self.context.measure(&plain, &font).0;
		let center = 1.0 + content_width.min(available) / 2.0;
		let line_height = self.context.normal_line_height(&font);
		let (ascent, descent) = self.context.vertical_metrics(&font);
		let half_leading = ((line_height - ascent.round() - descent.round()) / 2.0).floor();
		let top = (icon_size - line_height * lines.len() as f64) / 2.0;
		for (index, line) in lines.iter().enumerate() {
			let run = TextRun {
				x: center,
				y: top + index as f64 * line_height + half_leading + ascent.round(),
				anchor: Anchor::Middle,
				baseline: Baseline::Alphabetic,
				font: &font,
				content: line,
			};
			self.body.text(run, |svg| _ = svg.attr("fill", "#fff"));
		}
	}

	/// `drawGroups`.
	fn draw_groups(&mut self, boxes: &[CyBox]) {
		let db = self.db;
		let padding = self.settings.padding;
		let group_icon_size = padding * 0.75;
		let font_size = self.settings.font_size;
		let half_icon_size = self.settings.icon_size / 2.0;
		let stroke = self.context.theme.css("archGroupBorderColor");
		let stroke_width = self.context.theme.css("archGroupBorderWidth");
		self.body.open("g").attr("class", "architecture-groups").end();
		for (group, bounds) in db.groups.iter().zip(boxes) {
			let (x1, y1, width, height) = (bounds.x1, bounds.y1, bounds.width(), bounds.height());
			self.body
				.open("rect")
				.attr("id", &format!("{}-group-{}", self.context.id, group.id))
				.number_attr("x", x1 + half_icon_size)
				.number_attr("y", y1 + half_icon_size)
				.number_attr("width", width)
				.number_attr("height", height)
				.attr("class", "node-bkg")
				.attr("fill", "none")
				.attr_if("stroke", &stroke)
				.attr_if("stroke-width", &stroke_width)
				.attr("stroke-dasharray", "8")
				.close();
			self.bbox.include_rect(x1 + half_icon_size, y1 + half_icon_size, width, height);
			self.body.open("g").end();
			let (mut shifted_x, mut shifted_y) = (x1, y1);
			if let Some(icon) = &group.icon {
				let (x, y) = (shifted_x + half_icon_size + 1.0, shifted_y + half_icon_size + 1.0);
				self.body
					.open("g")
					.attr("transform", &format!("translate({}, {})", number(x), number(y)))
					.end();
				self.write_icon(icon, group_icon_size);
				self.body.close_tag("g");
				self.bbox.include_rect(x, y, group_icon_size, group_icon_size);
				shifted_x += group_icon_size;
				shifted_y += font_size / 2.0 - 1.0 - 2.0;
			}
			if let Some(title) = group.title.as_deref().filter(|title| !title.is_empty()) {
				let offset = (shifted_x + half_icon_size + 4.0, shifted_y + half_icon_size + 2.0);
				let transform = format!("translate({}, {})", number(offset.0), number(offset.1));
				let map =
					move |point: Point| Point { x: point.x + offset.0, y: point.y + offset.1 };
				self.write_label(
					title,
					width,
					(Anchor::Start, Baseline::Alphabetic),
					&transform,
					&map,
				);
			}
			self.body.close_tag("g");
		}
		self.body.close_tag("g");
	}
}

/// `getArchitectureDirectionXYFactors`.
fn xy_factors(source: Direction, target: Direction) -> (f64, f64) {
	use Direction::{Bottom, Left, Right, Top};
	match (source, target) {
		(Left, Top) | (Top, Left) => (1.0, 1.0),
		(Bottom, Left) | (Left, Bottom) => (1.0, -1.0),
		(Bottom, Right) | (Right, Bottom) => (-1.0, -1.0),
		_ => (-1.0, 1.0),
	}
}

/// `point` rotated by `angle` degrees around `pivot`, then moved by `offset`.
fn rotate_translate(point: Point, angle: f64, pivot: (f64, f64), offset: (f64, f64)) -> Point {
	let radians = angle.to_radians();
	let (sin, cos) = radians.sin_cos();
	let (dx, dy) = (point.x - pivot.0, point.y - pivot.1);
	Point {
		x: pivot.0 + dx * cos - dy * sin + offset.0,
		y: pivot.1 + dx * sin + dy * cos + offset.1,
	}
}

/// The axis-aligned box of `bbox` rotated around `pivot`, like `getBoundingClientRect`.
fn rotated_box(bbox: &BBox, angle: f64, pivot: (f64, f64)) -> BBox {
	let mut rotated = BBox::default();
	for (x, y) in [
		(bbox.min.x, bbox.min.y),
		(bbox.max.x, bbox.min.y),
		(bbox.min.x, bbox.max.y),
		(bbox.max.x, bbox.max.y),
	] {
		rotated.include_point(rotate_translate(Point { x, y }, angle, pivot, (0.0, 0.0)));
	}
	rotated
}

/// Text without its HTML tags.
fn strip_tags(text: &str) -> String {
	let mut output = String::with_capacity(text.len());
	let mut rest = text;
	while let Some(start) = rest.find('<') {
		output.push_str(&rest[..start]);
		match rest[start..].find('>') {
			Some(end) => rest = &rest[start + end + 1..],
			None => {
				rest = &rest[start..];
				break;
			}
		}
	}
	output.push_str(rest);
	output
}

/// The built-in icon an icon name refers to (`stringToIcon` with the architecture icons as
/// fallback prefix): a simple name or `mermaid-architecture:name`.
fn builtin_icon_name(name: &str) -> Option<&'static str> {
	const NAMES: [&str; 7] =
		["database", "server", "disk", "internet", "cloud", "unknown", "blank"];
	let parts: Vec<&str> = name.split(':').collect();
	let icon = match parts.as_slice() {
		[simple] if !simple.contains('-') => *simple,
		[prefix, icon] | [_, prefix, icon] if *prefix == "mermaid-architecture" => *icon,
		_ => return None,
	};
	NAMES.into_iter().find(|&known| known == icon)
}

/// The bodies of the built-in icons inside their blue tile (`architectureIcons.ts`), with
/// presentation attributes.
fn builtin_icon_body(name: &str) -> &'static str {
	match name {
		"database" => concat!(
			r##"<path d="m20,57.86c0,3.94,8.95,7.14,20,7.14s20-3.2,20-7.14" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<path d="m20,45.95c0,3.94,8.95,7.14,20,7.14s20-3.2,20-7.14" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<path d="m20,34.05c0,3.94,8.95,7.14,20,7.14s20-3.2,20-7.14" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<ellipse cx="40" cy="22.14" rx="20" ry="7.14" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<line x1="20" y1="57.86" x2="20" y2="22.14" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<line x1="60" y1="57.86" x2="60" y2="22.14" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
		),
		"server" => concat!(
			r##"<rect x="17.5" y="17.5" width="45" height="45" rx="2" ry="2" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<line x1="17.5" y1="32.5" x2="62.5" y2="32.5" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<line x1="17.5" y1="47.5" x2="62.5" y2="47.5" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<g>"##,
			r##"<path d="m56.25,25c0,.27-.45.5-1,.5h-10.5c-.55,0-1-.23-1-.5s.45-.5,1-.5h10.5c.55,0,1,.23,1,.5Z" fill="#fff" stroke-width="0"/>"##,
			r##"<path d="m56.25,25c0,.27-.45.5-1,.5h-10.5c-.55,0-1-.23-1-.5s.45-.5,1-.5h10.5c.55,0,1,.23,1,.5Z" fill="none" stroke="#fff" stroke-miterlimit="10"/>"##,
			r##"</g>"##,
			r##"<g>"##,
			r##"<path d="m56.25,40c0,.27-.45.5-1,.5h-10.5c-.55,0-1-.23-1-.5s.45-.5,1-.5h10.5c.55,0,1,.23,1,.5Z" fill="#fff" stroke-width="0"/>"##,
			r##"<path d="m56.25,40c0,.27-.45.5-1,.5h-10.5c-.55,0-1-.23-1-.5s.45-.5,1-.5h10.5c.55,0,1,.23,1,.5Z" fill="none" stroke="#fff" stroke-miterlimit="10"/>"##,
			r##"</g>"##,
			r##"<g>"##,
			r##"<path d="m56.25,55c0,.27-.45.5-1,.5h-10.5c-.55,0-1-.23-1-.5s.45-.5,1-.5h10.5c.55,0,1,.23,1,.5Z" fill="#fff" stroke-width="0"/>"##,
			r##"<path d="m56.25,55c0,.27-.45.5-1,.5h-10.5c-.55,0-1-.23-1-.5s.45-.5,1-.5h10.5c.55,0,1,.23,1,.5Z" fill="none" stroke="#fff" stroke-miterlimit="10"/>"##,
			r##"</g>"##,
			r##"<g>"##,
			r##"<circle cx="32.5" cy="25" r=".75" fill="#fff" stroke="#fff" stroke-miterlimit="10"/>"##,
			r##"<circle cx="27.5" cy="25" r=".75" fill="#fff" stroke="#fff" stroke-miterlimit="10"/>"##,
			r##"<circle cx="22.5" cy="25" r=".75" fill="#fff" stroke="#fff" stroke-miterlimit="10"/>"##,
			r##"</g>"##,
			r##"<g>"##,
			r##"<circle cx="32.5" cy="40" r=".75" fill="#fff" stroke="#fff" stroke-miterlimit="10"/>"##,
			r##"<circle cx="27.5" cy="40" r=".75" fill="#fff" stroke="#fff" stroke-miterlimit="10"/>"##,
			r##"<circle cx="22.5" cy="40" r=".75" fill="#fff" stroke="#fff" stroke-miterlimit="10"/>"##,
			r##"</g>"##,
			r##"<g>"##,
			r##"<circle cx="32.5" cy="55" r=".75" fill="#fff" stroke="#fff" stroke-miterlimit="10"/>"##,
			r##"<circle cx="27.5" cy="55" r=".75" fill="#fff" stroke="#fff" stroke-miterlimit="10"/>"##,
			r##"<circle cx="22.5" cy="55" r=".75" fill="#fff" stroke="#fff" stroke-miterlimit="10"/>"##,
			r##"</g>"##,
		),
		"disk" => concat!(
			r##"<rect x="20" y="15" width="40" height="50" rx="1" ry="1" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<ellipse cx="24" cy="19.17" rx=".8" ry=".83" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<ellipse cx="56" cy="19.17" rx=".8" ry=".83" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<ellipse cx="24" cy="60.83" rx=".8" ry=".83" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<ellipse cx="56" cy="60.83" rx=".8" ry=".83" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<ellipse cx="40" cy="33.75" rx="14" ry="14.58" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<ellipse cx="40" cy="33.75" rx="4" ry="4.17" fill="#fff" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<path d="m37.51,42.52l-4.83,13.22c-.26.71-1.1,1.02-1.76.64l-4.18-2.42c-.66-.38-.81-1.26-.33-1.84l9.01-10.8c.88-1.05,2.56-.08,2.09,1.2Z" fill="#fff" stroke-width="0"/>"##,
		),
		"internet" => concat!(
			r##"<circle cx="40" cy="40" r="22.5" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<line x1="40" y1="17.5" x2="40" y2="62.5" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<line x1="17.5" y1="40" x2="62.5" y2="40" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<path d="m39.99,17.51c-15.28,11.1-15.28,33.88,0,44.98" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<path d="m40.01,17.51c15.28,11.1,15.28,33.88,0,44.98" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<line x1="19.75" y1="30.1" x2="60.25" y2="30.1" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
			r##"<line x1="19.75" y1="49.9" x2="60.25" y2="49.9" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##,
		),
		"cloud" => {
			r##"<path d="m65,47.5c0,2.76-2.24,5-5,5H20c-2.76,0-5-2.24-5-5,0-1.87,1.03-3.51,2.56-4.36-.04-.21-.06-.42-.06-.64,0-2.6,2.48-4.74,5.65-4.97,1.65-4.51,6.34-7.76,11.85-7.76.86,0,1.69.08,2.5.23,2.09-1.57,4.69-2.5,7.5-2.5,6.1,0,11.19,4.38,12.28,10.17,2.14.56,3.72,2.51,3.72,4.83,0,.03,0,.07-.01.1,2.29.46,4.01,2.48,4.01,4.9Z" fill="none" stroke="#fff" stroke-miterlimit="10" stroke-width="2"/>"##
		}
		_ => "",
	}
}
