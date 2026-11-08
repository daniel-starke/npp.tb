//! Clusters (subgraphs) drawn around their laid out children (`rendering-elements/clusters.js`).

use super::super::label::LabelBox;
use super::super::rough::{self, FillStyle};
use super::super::svg::Svg;
use super::super::{Context, Look};
use super::intersect::Bounds;
use super::sketch::{self, PathPaint};
use super::{BBox, Node, Point, Styles, Theme, label_font, layout_label, write_label};

/// The label of a cluster: unwrapped unless it is Markdown, which wraps at the cluster width.
pub fn layout_cluster_label(
	context: &mut Context,
	node: &Node,
	styles: &Styles,
	width: Option<f64>,
) -> LabelBox {
	let (font, _) = label_font(context, styles);
	let wrap_width = if node.is_markdown { Some(width.unwrap_or(200.0)) } else { None };
	layout_label(context, &node.label, node.is_markdown, &font, wrap_width, 0.0)
}

/// Draws the `rect` cluster `node` laid out at `bounds`. Returns the drawn bounds, wider if the
/// label needs it, and the `diff` the recursive layout uses.
pub fn write_cluster(
	svg: &mut Svg,
	bbox: &mut BBox,
	context: &mut Context,
	node: &Node,
	bounds: Bounds,
	theme: &Theme,
) -> (Bounds, f64) {
	match node.shape.as_str() {
		"roundedWithTitle" => return rounded_with_title(svg, bbox, context, node, bounds, theme),
		"divider" => return divider(svg, bbox, context, node, bounds, theme),
		"usecaseSystemBoundary" => {
			return super::shapes::usecase::write_boundary(svg, bbox, context, node, bounds, theme);
		}
		"noteGroup" => {
			let (x, y) = (bounds.x - bounds.width / 2.0, bounds.y - bounds.height / 2.0);
			svg.open("g")
				.attr("class", "note-cluster")
				.attr("id", &format!("{}-{}", context.id, node.dom_id))
				.end();
			svg.open("rect")
				.number_attr("x", x)
				.number_attr("y", y)
				.number_attr("width", bounds.width)
				.number_attr("height", bounds.height)
				.attr("fill", "none")
				.close();
			svg.close_tag("g");
			bbox.include_rect(x, y, bounds.width, bounds.height);
			// Mermaid leaves its `diff` undefined, which counts as 0.
			return (bounds, 0.0);
		}
		_ => {}
	}
	let styles = Styles::compile(&[&node.css_compiled_styles, &node.css_styles]);
	let label = layout_cluster_label(context, node, &styles, Some(bounds.width));
	let label_width = label.width;
	let (width, diff) = match bounds.width <= label_width + node.padding {
		true => (
			label_width + node.padding,
			(label_width + node.padding - bounds.width) / 2.0 - node.padding,
		),
		false => (bounds.width, -node.padding),
	};
	let (x, y) = (bounds.x - width / 2.0, bounds.y - bounds.height / 2.0);
	let slot = theme.color_slot(node);
	// `flowGroup`: an agentflow container, `.flow-cluster rect` outranks its attributes.
	let is_flow = node.shape == "flowGroup";
	let class = match is_flow {
		true => format!("cluster flow-cluster {}", node.css_classes),
		false => format!("cluster {}", node.css_classes),
	};
	svg.open("g")
		.attr("class", class.trim_end())
		.attr("id", &format!("{}-{}", context.id, node.dom_id))
		.attr("data-look", node.look.data_name());
	if let Some(slot) = slot {
		svg.attr("data-color-id", &format!("color-{slot}"));
	}
	svg.end();
	// `.cluster rect`, then the neo rule and the color slot rule, which wins.
	let mut fill = if is_flow { "none".to_owned() } else { theme.cluster_bkg.clone() };
	let mut stroke = theme.cluster_border.clone();
	if node.look == Look::Neo {
		stroke = theme.neo_stroke.clone();
	}
	if let Some(slot) = slot {
		stroke = theme.border_colors[slot].clone();
		if !theme.background_colors.is_empty() {
			fill = theme.background_colors[slot % theme.background_colors.len()].clone();
		}
	}
	if node.look == Look::HandDrawn {
		let rect = [x, y, width, bounds.height];
		let sketched = hand_drawn_rect(svg, context, (node, is_flow), &styles, theme, slot, rect);
		let height = sketched.map_or(bounds.height, |sketched| sketched.height());
		let width = sketched.map_or(width, |sketched| sketched.width());
		if let Some(sketched) = sketched {
			bbox.include(&sketched, 0.0, 0.0);
		}
		write_rect_label(svg, bbox, context, &styles, theme, &label, bounds);
		return (Bounds { x: bounds.x, y: bounds.y, width, height }, diff);
	}
	let stroke_width = if is_flow { "0.75px" } else { "1px" };
	svg.open("rect")
		.attr_if("fill", &fill)
		.attr_if("stroke", &stroke)
		.attr("stroke-width", stroke_width);
	if node.look == Look::Neo {
		svg.attr_if("filter", &theme.neo_filter);
	}
	match is_flow {
		// Only the `classDef` rules (`.class > *`, `!important`) reach the rectangle.
		true => svg
			.attr("rx", "10")
			.attr("ry", "10")
			.attr_if("style", &Styles::compile(&[&node.css_compiled_styles]).node_css()),
		false => svg.attr_if("style", &styles.node_css()),
	};
	svg.number_attr("x", x)
		.number_attr("y", y)
		.number_attr("width", width)
		.number_attr("height", bounds.height)
		.close();
	bbox.include_rect(x, y, width, bounds.height);
	write_rect_label(svg, bbox, context, &styles, theme, &label, bounds);
	(Bounds { x: bounds.x, y: bounds.y, width, height: bounds.height }, diff)
}

/// The label of a `rect` cluster, and the end of its group.
fn write_rect_label(
	svg: &mut Svg,
	bbox: &mut BBox,
	context: &mut Context,
	styles: &Styles,
	theme: &Theme,
	label: &LabelBox,
	bounds: Bounds,
) {
	let (top_margin, _) = super::title_margins(context);
	let label_left = bounds.x - label.width / 2.0;
	let label_top = bounds.y - bounds.height / 2.0 + top_margin;
	svg.open("g")
		.attr("class", "cluster-label ")
		.attr("transform", &super::translate(label_left, label_top))
		.end();
	let (_, color) = label_font(context, styles);
	let fill = color.unwrap_or_else(|| theme.title_color.clone());
	write_label(svg, context, label, Point { x: label.width / 2.0, y: label.height / 2.0 }, &fill);
	svg.close_tag("g").close_tag("g");
	if !label.is_empty() {
		bbox.include_rect(label_left, label_top, label.width, label.height);
	}
}

/// The rectangle of a `rect` cluster in the `handDrawn` look; returns its box, which becomes
/// the size of the cluster.
fn hand_drawn_rect(
	svg: &mut Svg,
	context: &Context,
	(node, is_flow): (&Node, bool),
	styles: &Styles,
	theme: &Theme,
	slot: Option<usize>,
	[x, y, width, height]: [f64; 4],
) -> Option<BBox> {
	let mut options = sketch::user_node_overrides(styles, theme, context.sketch_seed(&node.id));
	options.roughness = 0.7;
	options.fill = Some(theme.cluster_bkg.clone());
	options.stroke = theme.cluster_border.clone();
	options.fill_weight = 3.0;
	if is_flow {
		sketch::flow_container_options(&mut options, theme);
	}
	let radius = if is_flow { 10.0 } else { 0.0 };
	let d = sketch::rounded_rect_path(x, y, width, height, radius);
	let sketched = rough::path(&d, &options);
	let (mut border, background) = sketch::border_and_background_styles(styles);
	let background = background.replacen("fill", "stroke", 1);
	// `path:nth-child(2)` finds no outline when the outline is the only path.
	if is_flow {
		border.clear();
	}
	// `[data-color-id].cluster:not(.swimlane) path` paints both paths.
	let slot_border = slot.map(|slot| theme.border_colors[slot].as_str());
	let slot_fill = slot
		.filter(|_| !theme.background_colors.is_empty())
		.map(|slot| theme.background_colors[slot % theme.background_colors.len()].as_str());
	let paint = PathPaint {
		stroke: slot_border,
		fill: slot_fill,
		outline_style: &border,
		fill_style: &background,
		..PathPaint::default()
	};
	sketch::write_group(svg, &sketched, "", &paint);
	sketch::sketch_box(&sketched)
}

/// The opening tag of a state cluster: classes, ids, look and color slot.
fn open_state_cluster(
	svg: &mut Svg,
	context: &Context,
	node: &Node,
	slot: Option<usize>,
	data_id: bool,
) {
	svg.open("g")
		.attr("class", node.css_classes.trim())
		.attr("id", &format!("{}-{}", context.id, node.dom_id));
	if data_id {
		svg.attr("data-id", &node.id);
	}
	svg.attr("data-look", node.look.data_name());
	if let Some(slot) = slot {
		svg.attr("data-color-id", &format!("color-{slot}"));
	}
	svg.end();
}

/// The stroke and its width of state cluster rectangles: `.statediagram-cluster rect` and its
/// neo rule.
fn state_cluster_stroke(node: &Node, theme: &Theme) -> (String, String) {
	match node.look == Look::Neo {
		true => {
			let stroke = match theme.neo_stroke.starts_with("url(") {
				true => theme.neo_stroke.clone(),
				false => theme.node_border.clone(),
			};
			(stroke, theme.stroke_width.clone())
		}
		false => (theme.node_border.clone(), theme.node_stroke_width.clone()),
	}
}

/// `roundedWithTitle`: a composite state, its title strip above its body.
fn rounded_with_title(
	svg: &mut Svg,
	bbox: &mut BBox,
	context: &mut Context,
	node: &Node,
	bounds: Bounds,
	theme: &Theme,
) -> (Bounds, f64) {
	// User classes reach the body and the label (`.class > *`), `style` statements nothing.
	let styles = Styles::compile(&[&node.css_compiled_styles]);
	let (font, color) = label_font(context, &styles);
	let label = layout_label(context, &node.label, false, &font, None, 0.0);
	let (label_width, label_height) = (label.width, label.height);
	let (width, diff) = match bounds.width <= label_width + node.padding {
		true => (
			label_width + node.padding,
			(label_width + node.padding - bounds.width) / 2.0 - node.padding,
		),
		false => (bounds.width, -node.padding),
	};
	let height = bounds.height;
	let (x, y) = (bounds.x - width / 2.0, bounds.y - height / 2.0);
	let inner_y = bounds.y - bounds.height / 2.0 + label_height + 2.0;
	let inner_height = bounds.height - label_height - 6.0;
	let slot = theme.color_slot(node);
	let slot_border = slot.map(|slot| theme.border_colors[slot].clone());
	let slot_fill = slot
		.filter(|_| !theme.background_colors.is_empty())
		.map(|slot| theme.background_colors[slot % theme.background_colors.len()].clone());
	let is_neo = node.look == Look::Neo;
	let (stroke, stroke_width) = state_cluster_stroke(node, theme);
	open_state_cluster(svg, context, node, slot, true);
	if node.look == Look::HandDrawn {
		let seed = context.sketch_seed(&node.id);
		let raw = |key: &str| context.theme.text(key);
		let outer = match node.corner_radius.is_some_and(|radius| radius != 0.0) {
			true => {
				let options = rough::Options {
					seed,
					roughness: 0.7,
					fill: raw("compositeTitleBackground"),
					fill_style: FillStyle::Solid,
					stroke: theme.plain_node_border.clone(),
					..rough::Options::default()
				};
				rough::path(&sketch::rounded_rect_path(x, y, width, height, 10.0), &options)
			}
			false => rough::rectangle(
				x,
				y,
				width,
				height,
				&rough::Options { seed, ..Default::default() },
			),
		};
		let is_alt = node.css_classes.contains("statediagram-cluster-alt");
		let inner_options = rough::Options {
			seed,
			fill: raw(if is_alt { "altBackground" } else { "compositeBackground" }),
			fill_style: if is_alt { FillStyle::Hachure } else { FillStyle::Solid },
			stroke: theme.plain_node_border.clone(),
			..rough::Options::default()
		};
		let inner = rough::rectangle(x, inner_y, width, inner_height, &inner_options);
		let outer_paint = PathPaint {
			fill_where_no_stroke: slot_fill.as_deref(),
			stroke_where_no_fill: slot_border.as_deref(),
			..PathPaint::default()
		};
		sketch::write_group(svg, &outer, "outer", &outer_paint);
		svg.open("g").end().close_tag("g");
		let label_left = bounds.x - label_width / 2.0;
		svg.open("g")
			.attr("class", "cluster-label")
			.attr("transform", &super::translate(label_left, y + 1.0))
			.end();
		let fill = color.unwrap_or_else(|| theme.title_color.clone());
		write_label(
			svg,
			context,
			&label,
			Point { x: label_width / 2.0, y: label_height / 2.0 },
			&fill,
		);
		svg.close_tag("g");
		sketch::write_group(svg, &inner, "inner", &PathPaint::default());
		svg.close_tag("g");
		let outer_box = sketch::sketch_box(&outer);
		for sketched in [outer_box, sketch::sketch_box(&inner)].into_iter().flatten() {
			bbox.include(&sketched, 0.0, 0.0);
		}
		if !label.is_empty() {
			bbox.include_rect(label_left, y + 1.0, label_width, label_height);
		}
		let height = outer_box.map_or(height, |outer| outer.height());
		return (Bounds { x: bounds.x, y: bounds.y, width, height }, diff);
	}
	let outer_fill = match is_neo {
		true => theme.plain_main_bkg.clone(),
		false => theme.state.composite_title_background.clone(),
	};
	let radius = match is_neo {
		true => context.theme.text("radius").unwrap_or_else(|| "5".to_owned()),
		false => "5".to_owned(),
	};
	svg.open("g").end();
	svg.open("rect")
		.attr("class", "outer")
		.attr_if("fill", slot_fill.as_deref().unwrap_or(&outer_fill))
		.attr_if("stroke", slot_border.as_deref().unwrap_or(&stroke))
		.attr_if("stroke-width", &stroke_width)
		.attr_if("filter", if is_neo { &theme.neo_filter } else { "" })
		.attr("rx", &radius)
		.attr("ry", &radius)
		.number_attr("x", x)
		.number_attr("y", y)
		.number_attr("width", width)
		.number_attr("height", height)
		.close();
	svg.close_tag("g");
	let label_left = bounds.x - label_width / 2.0;
	svg.open("g")
		.attr("class", "cluster-label")
		.attr("transform", &super::translate(label_left, y + 1.0))
		.end();
	let fill = color.unwrap_or_else(|| theme.title_color.clone());
	write_label(svg, context, &label, Point { x: label_width / 2.0, y: label_height / 2.0 }, &fill);
	svg.close_tag("g");
	let inner_fill = match node.css_classes.contains("statediagram-cluster-alt") {
		true => &theme.state.alt_background,
		false => &theme.state.composite_background,
	};
	svg.open("rect")
		.attr("class", "inner")
		.attr_if("fill", inner_fill)
		.attr_if("stroke", slot_border.as_deref().unwrap_or(&stroke))
		.attr_if("stroke-width", &stroke_width)
		.attr_if("style", &styles.node_css())
		.attr("rx", "0")
		.attr("ry", "0")
		.number_attr("x", x)
		.number_attr("y", inner_y)
		.number_attr("width", width)
		.number_attr("height", inner_height)
		.close();
	svg.close_tag("g");
	bbox.include_rect(x, y, width, height);
	if !label.is_empty() {
		bbox.include_rect(label_left, y + 1.0, label_width, label_height);
	}
	(Bounds { x: bounds.x, y: bounds.y, width, height }, diff)
}

/// `divider`: a concurrency region of a composite state.
fn divider(
	svg: &mut Svg,
	bbox: &mut BBox,
	context: &mut Context,
	node: &Node,
	bounds: Bounds,
	theme: &Theme,
) -> (Bounds, f64) {
	let (x, y) = (bounds.x - bounds.width / 2.0, bounds.y - bounds.height / 2.0);
	let slot = theme.color_slot(node);
	let slot_border = slot.map(|slot| theme.border_colors[slot].clone());
	let slot_fill = slot
		.filter(|_| !theme.background_colors.is_empty())
		.map(|slot| theme.background_colors[slot % theme.background_colors.len()].clone());
	let (stroke, stroke_width) = state_cluster_stroke(node, theme);
	let fill = match node.look == Look::Neo {
		true => &theme.plain_main_bkg,
		false => &theme.state.alt_background,
	};
	open_state_cluster(svg, context, node, slot, false);
	if node.look == Look::HandDrawn {
		let options = rough::Options {
			seed: context.sketch_seed(&node.id),
			fill: Some(theme.state.alt_background.clone()),
			fill_style: FillStyle::Solid,
			roughness: 0.5,
			stroke_line_dash: Some("5".to_owned()),
			stroke: theme.plain_node_border.clone(),
			..rough::Options::default()
		};
		let sketched = rough::rectangle(x, y, bounds.width, bounds.height, &options);
		let paint = PathPaint {
			fill_where_no_stroke: slot_fill.as_deref(),
			stroke_where_no_fill: slot_border.as_deref(),
			..PathPaint::default()
		};
		sketch::write_group(svg, &sketched, "divider", &paint);
		svg.open("g").end().close_tag("g").close_tag("g");
		let sketched = sketch::sketch_box(&sketched);
		if let Some(sketched) = sketched {
			bbox.include(&sketched, 0.0, 0.0);
		}
		let height = sketched.map_or(bounds.height, |sketched| sketched.height());
		return (Bounds { height, ..bounds }, -node.padding);
	}
	svg.open("g").end();
	svg.open("rect")
		.attr("class", "divider")
		.attr_if("fill", slot_fill.as_deref().unwrap_or(fill))
		.attr_if("stroke", slot_border.as_deref().unwrap_or(&stroke))
		.attr_if("stroke-width", &stroke_width)
		.attr("stroke-dasharray", "10,10")
		.number_attr("x", x)
		.number_attr("y", y)
		.number_attr("width", bounds.width)
		.number_attr("height", bounds.height)
		.close();
	svg.close_tag("g").close_tag("g");
	bbox.include_rect(x, y, bounds.width, bounds.height);
	(bounds, -node.padding)
}
