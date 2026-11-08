//! Flowcharts drawn with the swimlane layout (`layout-algorithms/swimlanes/index.ts`,
//! `helpers.ts` `prepareLayoutForSwimlanes`, `edgeLabelNodes.ts`, `common/index.ts`,
//! `adjustLayout.ts`, the swimlane branch of `insertEdge` and `clusters/swimlane.js`).

use super::super::label::LabelBox;
use super::super::layout::swimlane::{self, Direction};
use super::super::rough;
use super::super::svg::Svg;
use super::super::{Context, Error, Look, Result};
use super::edges::{self, EdgeEnd, EdgeRoute};
use super::intersect::Bounds;
use super::line_jumps::{self, EdgeGeometry};
use super::markers::{MarkerColors, Markers};
use super::shapes::{self, Intersect, Shape};
use super::sketch;
use super::{
	BBox, LayoutData, Node, Point, Styles, Theme, label_font, layout_label, node_label_font,
	translate, write_label,
};

/// `DEFAULT_SWIMLANE_ID`: the lane of nodes outside every subgraph.
const DEFAULT_LANE: &str = "__swimlane_default__";

/// The drawing of a node before it is positioned.
enum Drawing {
	Node(Shape, LabelBox),
	/// A label node of an edge: the label and the edge it came from.
	Label(LabelBox, usize),
	Group,
}

/// Renders `data` with the swimlane layout. `is_swimlane_diagram` adds the lane rules of
/// `swimlanes/styles.ts`.
pub fn render(
	context: &mut Context,
	data: &LayoutData,
	is_swimlane_diagram: bool,
) -> Result<(Svg, BBox)> {
	let theme = Theme::new(context, data.style);
	let mut svg = Svg::new();
	svg.open("g").end();
	let colors = MarkerColors {
		is_extension_filled: false,
		line: theme.line_color.clone(),
		edge: theme.edge_stroke.clone(),
		main_bkg: theme.plain_main_bkg.clone(),
		stroke_width: theme.stroke_width.clone(),
	};
	let mut markers =
		Markers::write(&mut svg, &context.id, data.diagram_type, data.markers, colors);
	let direction = Direction::parse(&data.direction);
	let mut nodes = data.nodes.clone();
	let mut edges = data.edges.clone();
	prepare_lanes(&mut nodes, context.look);

	// `createEdgeLabelNodes`: labels become nodes the layering places between the ends.
	let mut layout_edges: Vec<swimlane::Edge> = Vec::with_capacity(edges.len());
	let mut layout_only: Vec<swimlane::Edge> = Vec::new();
	let mut label_texts: Vec<(usize, String)> = Vec::new();
	let last_of = |nodes: &[Node], id: &str| nodes.iter().rposition(|node| node.id == id);
	let original_count = nodes.len();
	for edge in edges.iter_mut() {
		let mut layout_edge = swimlane::Edge {
			id: edge.id.clone(),
			start: edge.start.clone(),
			end: edge.end.clone(),
			curve: edge.curve.clone(),
			..swimlane::Edge::default()
		};
		if !edge.label.is_empty()
			&& let (Some(source), Some(target)) = (
				last_of(&nodes[..original_count], &edge.start),
				last_of(&nodes[..original_count], &edge.end),
			) {
			let id = format!("edge-label-{}-{}-{}", edge.start, edge.end, edge.id);
			let lane = match nodes[source].parent_id != nodes[target].parent_id {
				true => nodes[target].parent_id.clone(),
				false => nodes[source].parent_id.clone(),
			};
			label_texts.push((nodes.len(), std::mem::take(&mut edge.label)));
			nodes.push(Node {
				id: id.clone(),
				dom_id: id.clone(),
				shape: "labelRect".to_owned(),
				parent_id: lane,
				look: edge.look,
				..Node::default()
			});
			for (suffix, start, end) in
				[("to-label", &edge.start, &id), ("from-label", &id, &edge.end)]
			{
				layout_only.push(swimlane::Edge {
					id: format!("{}-{suffix}", edge.id),
					start: start.clone(),
					end: end.clone(),
					is_layout_only: true,
					..swimlane::Edge::default()
				});
			}
			layout_edge.label_node_id = Some(id);
		}
		layout_edges.push(layout_edge);
	}
	layout_edges.extend(layout_only);

	// The measurement of `createGraphWithElements`.
	let is_horizontal = matches!(direction, Direction::LeftRight | Direction::RightLeft);
	let mut drawings: Vec<Drawing> = Vec::with_capacity(nodes.len());
	for (index, node) in nodes.iter().enumerate() {
		if node.is_group {
			drawings.push(Drawing::Group);
			continue;
		}
		if let Some((_, text)) = label_texts.iter().find(|(label, _)| *label == index) {
			let edge = edges
				.iter()
				.position(|edge| {
					node.id == format!("edge-label-{}-{}-{}", edge.start, edge.end, edge.id)
				})
				.unwrap_or(0);
			let styles = edges.get(edge).map(edges::label_styles).unwrap_or_default();
			let (font, _) = node_label_font(context, node, &styles);
			let wrap = context.config_number("flowchart", "wrappingWidth", 120.0);
			let label = layout_label(context, text, false, &font, Some(wrap), 0.0);
			drawings.push(Drawing::Label(label, edge));
			continue;
		}
		let (shape, label) = measure_node(context, &theme, node, is_horizontal, data.style);
		drawings.push(Drawing::Node(shape, label));
	}
	let sizes: Vec<Option<(f64, f64)>> = drawings
		.iter()
		.map(|drawing| match drawing {
			Drawing::Node(shape, label) => {
				let bbox = super::node_box(shape, label);
				Some((bbox.width(), bbox.height()))
			}
			Drawing::Label(label, _) => {
				let mut bbox = BBox::default();
				bbox.include_rect(0.0, 0.0, 0.1, 0.1);
				if !label.is_empty() {
					bbox.include_rect(
						-label.width / 2.0,
						-label.height / 2.0,
						label.width,
						label.height,
					);
				}
				Some((bbox.width(), bbox.height()))
			}
			Drawing::Group => None,
		})
		.collect();

	let mut layout = swimlane::Layout {
		nodes: nodes
			.iter()
			.zip(&sizes)
			.map(|(node, size)| swimlane::Node {
				id: node.id.clone(),
				parent_id: node.parent_id.clone(),
				is_group: node.is_group,
				shape: node.shape.clone(),
				width: size.map(|size| size.0),
				height: size.map(|size| size.1),
				padding: node.is_group.then_some(node.padding),
				is_edge_label: node.shape == "labelRect"
					&& label_texts.iter().any(|(label, _)| nodes[*label].id == node.id),
				..swimlane::Node::default()
			})
			.collect(),
		edges: layout_edges,
		direction,
		config: swimlane::Config {
			node_gap: data.node_spacing,
			layer_gap: data.rank_spacing,
			ignore_cross_lane_edges: context.config_bool("swimlane", "ignoreCrossLaneEdges", true),
			optimize_ranks_by_crossings: context.config_bool(
				"swimlane",
				"optimizeRanksByCrossings",
				true,
			),
			automatic_lane_ordering: context.config_bool(
				"swimlane",
				"automaticLaneOrdering",
				false,
			),
		},
	};
	swimlane::run(&mut layout);
	let placed = &layout.nodes;
	let bounds_of = |index: usize| {
		let node = &placed[index];
		Bounds {
			x: node.x.unwrap_or(0.0),
			y: node.y.unwrap_or(0.0),
			width: node.width.unwrap_or(0.0),
			height: node.height.unwrap_or(0.0),
		}
	};

	let mut bbox = BBox::default();
	let mut clusters = Svg::new();
	let mut nodes_svg = Svg::new();
	for (index, node) in nodes.iter().enumerate() {
		match &drawings[index] {
			Drawing::Group if node.shape == "swimlane" => {
				write_lane(
					&mut clusters,
					&mut bbox,
					context,
					node,
					&placed[index],
					&theme,
					direction,
					is_swimlane_diagram,
				);
			}
			Drawing::Group => {
				super::clusters::write_cluster(
					&mut clusters,
					&mut bbox,
					context,
					node,
					bounds_of(index),
					&theme,
				);
			}
			Drawing::Node(shape, label) => {
				let bounds = bounds_of(index);
				let drawing = super::NodeDrawing { shape, label, extra_labels: &[] };
				super::write_node(
					&mut nodes_svg,
					&mut bbox,
					context,
					&theme,
					data.style,
					node,
					&drawing,
					Point { x: bounds.x, y: bounds.y },
				);
			}
			Drawing::Label(label, edge) => {
				let bounds = bounds_of(index);
				let styles = edges.get(*edge).map(edges::label_styles).unwrap_or_default();
				write_label_node(
					&mut nodes_svg,
					&mut bbox,
					context,
					node,
					label,
					&styles,
					&theme,
					Point { x: bounds.x, y: bounds.y },
				);
			}
		}
	}

	// Edges with the points of the layout, clipped at the ends (`insertEdge`).
	let mut routes: Vec<Option<(usize, Vec<Point>)>> = Vec::with_capacity(edges.len());
	for (index, edge) in edges.iter().enumerate() {
		// The router leaves self loops without points, which `insertEdge` cannot draw.
		let Some(points) = layout
			.edges
			.iter()
			.find(|candidate| candidate.id == edge.id)
			.and_then(|routed| routed.points.clone())
		else {
			return Err(Error::new("Cannot read properties of undefined (reading 'filter')"));
		};
		let end = |id: &str| -> Option<(Bounds, Intersect)> {
			let node = last_of(&nodes, id)?;
			let intersect = match &drawings[node] {
				Drawing::Node(shape, _) => shape.intersect.clone(),
				_ => Intersect::Rect,
			};
			Some((bounds_of(node), intersect))
		};
		let points = match (end(&edge.start), end(&edge.end)) {
			(Some(tail), Some(head)) if points.len() >= 2 => clip_points(&points, &tail, &head),
			_ => points,
		};
		routes.push(Some((index, points)));
	}
	let hops = context.config_value("swimlane", "lineHops").cloned();
	let hops_enabled = !matches!(hops, Some(super::super::value::Value::Bool(false)));
	let use_gaps = hops.as_ref().and_then(|value| value.as_str()) == Some("gap");
	let curve_of = |edge: &super::Edge| match edge.curve.as_deref() {
		None | Some("basis") => "rounded".to_owned(),
		Some(curve) => curve.to_owned(),
	};
	let drawn: Vec<(usize, &Vec<Point>)> = routes
		.iter()
		.flatten()
		.filter(|(_, points)| points.len() >= 2)
		.map(|(index, points)| (*index, points))
		.collect();
	let geometries: Vec<EdgeGeometry> = drawn
		.iter()
		.map(|(index, points)| {
			let edge = &edges[*index];
			let offset = |arrow: &str| edges::marker_offset(arrow).filter(|offset| *offset != 0.0);
			EdgeGeometry {
				points,
				is_rounded: curve_of(edge) == "rounded",
				start_offset: offset(&edge.arrow_type_start),
				end_offset: offset(&edge.arrow_type_end),
			}
		})
		.collect();
	let mut jumps: Vec<Option<line_jumps::JumpPath>> = match hops_enabled {
		true => line_jumps::jump_paths(&geometries, use_gaps),
		false => geometries.iter().map(|_| None).collect(),
	};
	let mut edge_paths = Svg::new();
	for (position, (index, points)) in drawn.iter().enumerate() {
		let mut edge = edges[*index].clone();
		let curve = curve_of(&edge);
		let supports_hops =
			matches!(curve.as_str(), "linear" | "rounded" | "step" | "stepBefore" | "stepAfter");
		edge.curve = Some(curve);
		let route = EdgeRoute {
			points: (*points).clone(),
			label_position: None,
			from_cluster: None,
			to_cluster: None,
			jumps: jumps[position].take().filter(|_| supports_hops),
		};
		let free = |point: Point| Bounds { x: point.x, y: point.y, width: 0.0, height: 0.0 };
		let tail = EdgeEnd { bounds: free(points[0]), intersect: None };
		let head = EdgeEnd { bounds: free(points[points.len() - 1]), intersect: None };
		edges::write_edge(
			&mut edge_paths,
			&mut bbox,
			context,
			&edge,
			&route,
			&tail,
			&head,
			&mut markers,
			&theme,
			"rounded",
		);
	}

	svg.open("g").attr("class", "root").end();
	svg.open("g").attr("class", "clusters").end();
	svg.append(clusters);
	svg.close_tag("g");
	svg.open("g").attr("class", "edges edgePaths").end();
	svg.append(edge_paths);
	svg.close_tag("g");
	svg.open("g").attr("class", "edgeLabels").end().close_tag("g");
	svg.open("g").attr("class", "nodes").end();
	svg.append(nodes_svg);
	svg.close_tag("g");
	svg.close_tag("g");
	markers.write_colored(&mut svg, data.diagram_type);
	svg.close_tag("g");
	Ok((svg, bbox))
}

/// `prepareLayoutForSwimlanes`: top-level subgraphs are lanes, nodes outside every subgraph
/// go to a synthesized lane.
fn prepare_lanes(nodes: &mut Vec<Node>, look: Look) {
	for node in nodes.iter_mut().filter(|node| node.is_group && node.parent_id.is_none()) {
		node.shape = "swimlane".to_owned();
	}
	if !nodes.iter().any(|node| !node.is_group && node.parent_id.is_none()) {
		return;
	}
	match nodes.iter_mut().find(|node| node.id == DEFAULT_LANE) {
		Some(lane) => {
			if lane.is_group {
				lane.shape = "swimlane".to_owned();
			}
		}
		None => {
			// Without an own slot the synthesized lane would take the first lane's colours.
			let next_slot = nodes
				.iter()
				.filter_map(|node| node.color_index)
				.map(|slot| slot + 1)
				.max()
				.unwrap_or(0);
			nodes.push(Node {
				id: DEFAULT_LANE.to_owned(),
				dom_id: DEFAULT_LANE.to_owned(),
				is_group: true,
				shape: "swimlane".to_owned(),
				padding: 20.0,
				look,
				color_index: Some(next_slot),
				..Node::default()
			});
		}
	}
	for node in nodes.iter_mut().filter(|node| !node.is_group && node.parent_id.is_none()) {
		node.parent_id = Some(DEFAULT_LANE.to_owned());
	}
}

/// The shape and label of a flowchart node (`insertNode` during the measurement).
fn measure_node(
	context: &mut Context,
	theme: &Theme,
	node: &Node,
	is_horizontal: bool,
	style: super::DiagramStyle,
) -> (Shape, LabelBox) {
	let styles = Styles::compile(&[&node.css_compiled_styles, &node.css_styles]);
	let (font, _) = node_label_font(context, node, &styles);
	let mut wrap_width = node
		.wrapping_width
		.unwrap_or_else(|| context.config_number("flowchart", "wrappingWidth", 120.0));
	let text = if shapes::has_no_label(&node.shape) { "" } else { node.label.as_str() };
	let mut min_width = match text.is_empty() || shapes::ignores_min_width(&node.shape) {
		true => 0.0,
		false => node.min_width.unwrap_or(0.0),
	};
	if let Some(image) = &node.image {
		wrap_width = wrap_width.max(image.width);
		min_width = 0.0;
	}
	let (shape, label) = super::layout_shape(
		context,
		node,
		(text, &font, &styles),
		(Some(wrap_width), min_width),
		is_horizontal,
		style,
	);
	(sketch::apply(shape, context, node, &styles, theme, style), label)
}

/// The swimlane branch of `insertEdge`: the ends move to the node borders unless the end
/// point duplicates its neighbour (the endpoints prepared for the renderer).
fn clip_points(
	points: &[Point],
	tail: &(Bounds, Intersect),
	head: &(Bounds, Intersect),
) -> Vec<Point> {
	const TOLERANCE: f64 = 0.5;
	if points.len() == 2 {
		return vec![tail.1.point(tail.0, points[0]), head.1.point(head.0, points[1])];
	}
	let inner = &points[1..points.len() - 1];
	let (first, last) = (inner[0], inner[inner.len() - 1]);
	let close = |a: Point, b: Point| (a.x - b.x).abs() < TOLERANCE && (a.y - b.y).abs() < TOLERANCE;
	let new_first = tail.1.point(tail.0, first);
	let new_last = match close(points[points.len() - 1], last) {
		true => last,
		false => head.1.point(head.0, last),
	};
	let mut result = Vec::with_capacity(points.len());
	if !close(new_first, first) {
		result.push(new_first);
	}
	result.extend_from_slice(inner);
	if !close(new_last, last) {
		result.push(new_last);
	}
	result
}

/// A label node (`labelRect`): an invisible 0.1 px rectangle and the label around its centre.
#[allow(clippy::too_many_arguments)]
fn write_label_node(
	svg: &mut Svg,
	bbox: &mut BBox,
	context: &mut Context,
	node: &Node,
	label: &LabelBox,
	styles: &Styles,
	theme: &Theme,
	center: Point,
) {
	svg.open("g")
		.attr("class", "label edgeLabel")
		.attr("id", &node.dom_id)
		.attr("transform", &translate(center.x, center.y))
		.end();
	// `.edgeLabel rect`.
	svg.open("rect")
		.attr("width", "0.1")
		.attr("height", "0.1")
		.attr_if("fill", &theme.edge_label_background)
		.attr("opacity", "0.5")
		.close();
	svg.open("g").attr("class", "label").end();
	let (_, color) = label_font(context, styles);
	let fill = color.unwrap_or_else(|| theme.text_color.clone());
	write_label(svg, context, label, Point::default(), &fill);
	svg.close_tag("g").close_tag("g");
	bbox.include_rect(center.x, center.y, 0.1, 0.1);
	if !label.is_empty() {
		bbox.include_rect(
			center.x - label.width / 2.0,
			center.y - label.height / 2.0,
			label.width,
			label.height,
		);
	}
}

/// `swimlane` of `clusters/swimlane.js`: a title band (left in LR, else on top) and the body.
#[allow(clippy::too_many_arguments)]
fn write_lane(
	svg: &mut Svg,
	bbox: &mut BBox,
	context: &mut Context,
	node: &Node,
	placed: &swimlane::Node,
	theme: &Theme,
	direction: Direction,
	is_swimlane_diagram: bool,
) {
	let styles = Styles::compile(&[&node.css_compiled_styles, &node.css_styles]);
	let (font, color) = label_font(context, &styles);
	let lane_width = placed.width.unwrap_or(0.0);
	// Lane titles show `\n` as written, unlike node labels.
	let text = node.label.replace('\\', "&#92;");
	let label = layout_label(context, &text, node.is_markdown, &font, Some(lane_width), 0.0);
	let padding = node.padding;
	let width =
		if lane_width <= label.width + padding { label.width + padding } else { lane_width };
	let (x, y) = (placed.x.unwrap_or(0.0), placed.y.unwrap_or(0.0));
	let height = placed.height.unwrap_or(0.0);
	let (top, bottom, left) = (y - height / 2.0, y + height / 2.0, x - width / 2.0);
	let content_top = placed.content_top.unwrap_or(top + height / 3.0);
	let slot = theme.color_slot(node);
	let look = node.look.data_name();
	svg.open("g")
		.attr("class", format!("cluster swimlane {}", node.css_classes).trim_end())
		.attr("id", &format!("{}-{}", context.id, node.dom_id))
		.attr("data-id", &node.id)
		.attr("data-et", "cluster")
		.attr("data-look", look);
	if let Some(slot) = slot {
		svg.attr("data-color-id", &format!("color-{slot}"));
	}
	svg.end();
	// `.cluster rect` fills both halves; the neo rule, the lane rules of the swimlane diagram
	// (`stroke: clusterBorder !important` without a slot, `filter: none`) and the slot rule.
	let mut fill = theme.cluster_bkg.clone();
	let mut stroke = theme.cluster_border.clone();
	if node.look == Look::Neo {
		stroke = theme.neo_stroke.clone();
	}
	let mut filter = match node.look == Look::Neo && !is_swimlane_diagram {
		true => theme.neo_filter.clone(),
		false => String::new(),
	};
	match slot {
		Some(slot) => {
			stroke = theme.border_colors[slot].clone();
			if !theme.background_colors.is_empty() {
				fill = theme.background_colors[slot % theme.background_colors.len()].clone();
			}
		}
		None if is_swimlane_diagram => stroke = theme.cluster_border.clone(),
		None => {}
	}
	if is_swimlane_diagram {
		filter.clear();
	}
	let node_css = styles.node_css();
	let hand_drawn = (node.look == Look::HandDrawn).then(|| {
		let mut options =
			sketch::user_node_overrides(&styles, theme, context.sketch_seed(&node.id));
		options.stroke = theme.cluster_border.clone();
		options
	});
	let (border_styles, background_styles) = sketch::border_and_background_styles(&styles);
	let background_styles = background_styles.replacen("fill", "stroke", 1);
	// The lane rules of the color slot: the hachure first, then the outline.
	let slot_strokes = slot.map_or([None, None], |slot| {
		let background = (!theme.background_colors.is_empty())
			.then(|| theme.background_colors[slot % theme.background_colors.len()].as_str());
		[background, Some(theme.border_colors[slot].as_str())]
	});
	let mut rect = |svg: &mut Svg, class: &str, rx: f64, ry: f64, w: f64, h: f64| {
		if let Some(options) = &hand_drawn {
			let is_title = class == "swimlane-title";
			let mut options = options.clone();
			options.fill = Some(match is_title {
				true => theme.cluster_bkg.clone(),
				false => "none".to_owned(),
			});
			if is_title && direction != Direction::LeftRight {
				options.fill_weight = 3.0;
			}
			let sketched = rough::rectangle(rx, ry, w, h, &options);
			let paint = sketch::PathPaint {
				outline_style: if is_title { &border_styles } else { "" },
				fill_style: if is_title { &background_styles } else { "" },
				stroke_by_position: slot_strokes,
				..sketch::PathPaint::default()
			};
			sketch::write_group(svg, &sketched, class, &paint);
			if let Some(ink) = sketch::sketch_box(&sketched) {
				bbox.include(&ink, 0.0, 0.0);
			}
			return;
		}
		svg.open("rect").attr("class", class).attr("style", &node_css);
		svg.number_attr("x", rx)
			.number_attr("y", ry)
			.number_attr("width", w)
			.number_attr("height", h);
		svg.attr_if("fill", &fill)
			.attr_if("stroke", &stroke)
			.attr("stroke-width", "1px")
			.attr_if("filter", &filter);
		svg.close();
		bbox.include_rect(rx, ry, w, h);
	};
	let label_fill = color.unwrap_or_else(|| theme.title_color.clone());
	let center = Point { x: label.width / 2.0, y: label.height / 2.0 };
	if direction == Direction::LeftRight {
		let title_width = label.height + 8.0;
		rect(svg, "swimlane-body", left + title_width, top, (width - title_width).max(0.0), height);
		rect(svg, "swimlane-title", left, top, title_width, height);
		let transform = format!(
			"{} rotate(-90) {}",
			translate(left + title_width / 2.0, y),
			translate(-label.width / 2.0, -label.height / 2.0)
		);
		svg.open("g")
			.attr("class", "cluster-label swimlane-label")
			.attr("transform", &transform)
			.end();
		write_label(svg, context, &label, center, &label_fill);
		svg.close_tag("g");
		// The turned title may reach beyond a short lane.
		if !label.is_empty() {
			bbox.include_rect(
				left + title_width / 2.0 - label.height / 2.0,
				y - label.width / 2.0,
				label.height,
				label.width,
			);
		}
	} else {
		let title_height = label.height.min((content_top - top).max(0.0));
		let body_top = top + title_height;
		rect(svg, "swimlane-body", left, body_top, width, (bottom - body_top).max(0.0));
		rect(svg, "swimlane-title", left, top, width, title_height);
		let (label_left, label_top) =
			(x - label.width / 2.0, top + (title_height - label.height) / 2.0);
		svg.open("g")
			.attr("class", "cluster-label swimlane-label")
			.attr("transform", &translate(label_left, label_top))
			.end();
		write_label(svg, context, &label, center, &label_fill);
		svg.close_tag("g");
		if !label.is_empty() {
			bbox.include_rect(label_left, label_top, label.width, label.height);
		}
	}
	svg.close_tag("g");
}
