//! Block diagrams (`block.jison`, `blockDB.ts`, `layout.ts`, `renderHelpers.ts`,
//! `blockRenderer.ts`, `styles.ts`): blocks in a grid of columns, drawn with the shapes and
//! edges of the flowchart.

mod db;
mod parser;

use super::color::js_number;
use super::label::encode_entities;
use super::rendering::edges::{self, EdgeEnd, EdgeRoute};
use super::rendering::intersect::Bounds;
use super::rendering::markers::{MarkerColors, Markers};
use super::rendering::{
	self, BBox, DiagramStyle, Edge, Node, NodeDrawing, Pattern, Point, Styles, Theme, Thickness,
	shapes,
};
use super::svg::{self, Frame, Svg};
use super::{Context, Diagram, Error, Result};
use db::{Block, BlockDb, ROOT, Size};
use parser::BlockType;

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let code = encode_entities(code) + "\n";
	let statements =
		parser::parse(&code).map_err(|error| context.error_at(error.line, error.message))?;
	let mut db = BlockDb::new(statements).map_err(Error::new)?;
	if db.edges.len() > context.limits.max_edges {
		return Err(Error::new(format!(
			"Edge limit exceeded. {} edges found, but the limit is {}.",
			db.edges.len(),
			context.limits.max_edges
		)));
	}
	let padding = context.config_number("block", "padding", 8.0);
	let theme = Theme::new(context, DiagramStyle::Flowchart);
	for index in hierarchy(&db) {
		measure_block(context, &theme, &mut db, index, padding);
	}
	set_block_sizes(&mut db, ROOT, 0.0, 0.0, padding);
	layout_blocks(&mut db, ROOT, padding).map_err(Error::new)?;

	let mut body = Svg::new();
	let colors = MarkerColors {
		is_extension_filled: false,
		line: theme.line_color.clone(),
		edge: theme.edge_stroke.clone(),
		main_bkg: theme.plain_main_bkg.clone(),
		stroke_width: theme.stroke_width.clone(),
	};
	let mut markers =
		Markers::write(&mut body, &context.id, "block", &["point", "circle", "cross"], colors);
	let mut bbox = BBox::default();
	body.open("g").attr("class", "block").end();
	for index in hierarchy(&db) {
		write_block(&mut body, &mut bbox, context, &theme, &mut db, index, padding);
	}
	write_edges(&mut body, &mut bbox, context, &theme, &db, &mut markers)?;
	body.close_tag("g");
	markers.write_colored(&mut body, "block");
	rendering::write_look_defs(&mut body, context);
	let (x, y) = if bbox.is_empty() { (0.0, 0.0) } else { (bbox.min.x, bbox.min.y) };
	let frame = Frame {
		x: x - 5.0,
		y: y - 5.0,
		width: bbox.width() + 10.0,
		height: bbox.height() + 10.0,
		use_max_width: context.config_bool("block", "useMaxWidth", true),
	};
	Ok(svg::document(context, "block", &frame, &Default::default(), body))
}

/// `performOperations`: the blocks of the hierarchy, parents before their children.
fn hierarchy(db: &BlockDb) -> Vec<usize> {
	let mut order = Vec::new();
	let mut stack: Vec<usize> = db.blocks[ROOT].children.iter().rev().copied().collect();
	while let Some(index) = stack.pop() {
		order.push(index);
		stack.extend(db.blocks[index].children.iter().rev());
	}
	order
}

/// A number JavaScript takes as true.
fn is_truthy(value: f64) -> bool {
	value != 0.0 && !value.is_nan()
}

/// `getNodeFromBlock`: the flowchart node that draws `block`.
fn node_of(
	context: &Context,
	db: &BlockDb,
	block: &Block,
	padding: f64,
	is_positioned: bool,
) -> Node {
	let classes = match block.classes.is_empty() {
		true => "default".to_owned(),
		false => block.classes.join(" "),
	};
	let (shape, radius) = match block.kind {
		Some(BlockType::Round) => ("roundedRect", None),
		Some(BlockType::Composite) => ("composite", Some(0.0)),
		Some(BlockType::Diamond) => ("question", None),
		Some(BlockType::Hexagon) => ("hexagon", None),
		Some(BlockType::BlockArrow) => ("block_arrow", None),
		Some(BlockType::LeanRight) => ("lean_right", None),
		Some(BlockType::LeanLeft) => ("lean_left", None),
		Some(BlockType::Trapezoid) => ("trapezoid", None),
		Some(BlockType::InvTrapezoid) => ("inv_trapezoid", None),
		Some(BlockType::RectLeftInvArrow) => ("rect_left_inv_arrow", None),
		Some(BlockType::Circle) => ("circle", None),
		Some(BlockType::Stadium) => ("stadium", None),
		Some(BlockType::Subroutine) => ("subroutine", None),
		Some(BlockType::Cylinder) => ("cylinder", None),
		Some(BlockType::DoubleCircle) => ("doublecircle", None),
		_ => ("squareRect", None),
	};
	let id = block.id.clone().unwrap_or_else(|| "undefined".to_owned());
	let size = block.size.unwrap_or_default();
	let is_composite = block.is(BlockType::Composite);
	Node {
		dom_id: id.clone(),
		id,
		label: block.label.clone().unwrap_or_default(),
		padding: if is_composite { 0.0 } else { padding },
		css_styles: block.styles.clone().unwrap_or_default(),
		css_compiled_styles: db.class_styles(block),
		css_classes: format!("{classes} flowchart-label"),
		look: context.look,
		shape: shape.to_owned(),
		color_index: block.color_index,
		corner_radius: radius,
		width: Some(size.width).filter(|width| is_truthy(*width)),
		height: Some(size.height).filter(|height| is_truthy(*height)),
		is_positioned,
		directions: block.directions.clone().unwrap_or_default(),
		width_in_columns: block.columns_spanned(),
		..Node::default()
	}
}

/// The shape and label of `node`: the label wraps at the node's width, else at the laid out
/// width of the block, else not at all.
fn layout_node(
	context: &mut Context,
	theme: &Theme,
	node: &Node,
	block_width: f64,
) -> (shapes::Shape, super::label::LabelBox) {
	let styles = Styles::compile(&[&node.css_compiled_styles, &node.css_styles]);
	let (font, _) = rendering::label_font(context, &styles);
	let wrap_width =
		shapes::label_width(node).or_else(|| Some(block_width).filter(|width| is_truthy(*width)));
	let (shape, label) = rendering::layout_shape(
		context,
		node,
		(&node.label, &font, &styles),
		(wrap_width, 0.0),
		false,
		DiagramStyle::Flowchart,
	);
	let shape =
		rendering::sketch::apply(shape, context, node, &styles, theme, DiagramStyle::Flowchart);
	(shape, label)
}

/// `calculateBlockSize`: the size of the block drawn around its label. The size goes to the
/// block of the id map.
fn measure_block(
	context: &mut Context,
	theme: &Theme,
	db: &mut BlockDb,
	index: usize,
	padding: f64,
) {
	let block = &db.blocks[index];
	// Spaces get the size of their row anyway.
	if block.is(BlockType::Space) {
		return;
	}
	let node = node_of(context, db, block, padding, false);
	let width = block.size.map_or(0.0, |size| size.width);
	let (shape, label) = layout_node(context, theme, &node, width);
	let bbox = rendering::node_box(&shape, &label);
	if let Some(target) = db.get(&db.blocks[index].id) {
		db.blocks[target].size =
			Some(Size { width: bbox.width(), height: bbox.height(), x: 0.0, y: 0.0 });
	}
}

/// `getMaxChildSize`: the widest column and the highest block of `children`.
fn max_child_size(db: &BlockDb, children: &[usize]) -> (f64, f64) {
	let (mut max_width, mut max_height) = (0.0, 0.0);
	for &child in children {
		let block = &db.blocks[child];
		if block.is(BlockType::Space) {
			continue;
		}
		let size = block.size.unwrap_or_default();
		let normalized = size.width / block.columns_spanned();
		if normalized > max_width {
			max_width = normalized;
		}
		if size.height > max_height {
			max_height = size.height;
		}
	}
	(max_width, max_height)
}

/// `setBlockSizes`: every child as wide as the widest column, the block around its grid.
fn set_block_sizes(
	db: &mut BlockDb,
	index: usize,
	sibling_width: f64,
	sibling_height: f64,
	padding: f64,
) {
	if !db.blocks[index].size.is_some_and(|size| is_truthy(size.width)) {
		db.blocks[index].size =
			Some(Size { width: sibling_width, height: sibling_height, x: 0.0, y: 0.0 });
	}
	let children = db.blocks[index].children.clone();
	if children.is_empty() {
		return;
	}
	for &child in &children {
		set_block_sizes(db, child, 0.0, 0.0, padding);
	}
	let (max_width, max_height) = max_child_size(db, &children);
	for &child in &children {
		let spanned = db.blocks[child].columns_spanned();
		if let Some(size) = &mut db.blocks[child].size {
			*size = Size {
				width: max_width * spanned + padding * (spanned - 1.0),
				height: max_height,
				x: 0.0,
				y: 0.0,
			};
		}
	}
	for &child in &children {
		set_block_sizes(db, child, max_width, max_height, padding);
	}
	let columns = db.blocks[index].columns.unwrap_or(-1.0);
	let items: f64 = children.iter().map(|&child| db.blocks[child].columns_spanned()).sum();
	let mut x_size = children.len() as f64;
	if columns > 0.0 && columns < items {
		x_size = columns;
	}
	let y_size = (items / x_size).ceil();
	let mut width = x_size * (max_width + padding) + padding;
	let mut height = y_size * (max_height + padding) + padding;
	if width < sibling_width {
		width = sibling_width;
		height = sibling_height;
		let child_width = (sibling_width - x_size * padding - padding) / x_size;
		let child_height = (sibling_height - y_size * padding - padding) / y_size;
		for &child in &children {
			if let Some(size) = &mut db.blocks[child].size {
				*size = Size { width: child_width, height: child_height, x: 0.0, y: 0.0 };
			}
		}
	}
	let own_width = db.blocks[index].size.map_or(0.0, |size| size.width);
	let own_width = if is_truthy(own_width) { own_width } else { 0.0 };
	if width < own_width {
		width = own_width;
		// Grow the children to fit.
		let count = children.len() as f64;
		let count = if columns > 0.0 { count.min(columns) } else { count };
		if count > 0.0 {
			let child_width = (width - count * padding - padding) / count;
			for &child in &children {
				if let Some(size) = &mut db.blocks[child].size {
					size.width = child_width;
				}
			}
		}
	}
	db.blocks[index].size = Some(Size { width, height, x: 0.0, y: 0.0 });
}

fn is_integer(value: f64) -> bool {
	value.is_finite() && value.fract() == 0.0
}

/// `calculateBlockPosition`: the column and row of the grid position.
fn block_position(columns: f64, position: f64) -> std::result::Result<(f64, f64), String> {
	if columns == 0.0 || !is_integer(columns) {
		return Err("Columns must be an integer !== 0.".to_owned());
	}
	if position < 0.0 || !is_integer(position) {
		return Err(format!("Position must be a non-negative integer.{}", js_number(position)));
	}
	if columns < 0.0 {
		return Ok((position, 0.0));
	}
	if columns == 1.0 {
		return Ok((0.0, position));
	}
	Ok((position % columns, (position / columns).floor()))
}

/// The columns a child fills: at most up to the end of its row.
fn columns_filled(block: &Block, columns: f64, position: f64) -> f64 {
	let filled = block.columns_spanned();
	match columns > 0.0 {
		true => filled.min(columns - position % columns),
		false => filled,
	}
}

/// `layoutBlocks`: the centers of the children of a block, row by row.
fn layout_blocks(db: &mut BlockDb, index: usize, padding: f64) -> std::result::Result<(), String> {
	let columns = db.blocks[index].columns.unwrap_or(-1.0);
	let children = db.blocks[index].children.clone();
	if children.is_empty() {
		return Ok(());
	}
	// The highest block of each row, so that rows of different heights do not overlap.
	let mut row_heights: Vec<(f64, f64)> = Vec::new();
	let mut position = 0.0;
	for &child in &children {
		let Some(size) = db.blocks[child].size else { continue };
		let (_, row) = block_position(columns, position)?;
		match row_heights.iter_mut().find(|(known, _)| *known == row) {
			Some(entry) => {
				if size.height > entry.1 {
					entry.1 = size.height;
				}
			}
			None if size.height > 0.0 => row_heights.push((row, size.height)),
			None => {}
		}
		position += columns_filled(&db.blocks[child], columns, position);
	}
	npp::sort::sort_by(&mut row_heights[..], |a, b| a.0.total_cmp(&b.0));
	let mut row_offsets: Vec<(f64, f64)> = Vec::with_capacity(row_heights.len());
	let mut offset = 0.0;
	for &(row, height) in &row_heights {
		row_offsets.push((row, offset));
		offset += height + padding;
	}
	let parent = db.blocks[index].size.unwrap_or_default();
	let row_start = || match is_truthy(parent.x) {
		true => {
			let half = -parent.width / 2.0;
			parent.x + if is_truthy(half) { half } else { 0.0 }
		}
		false => -padding,
	};
	let mut start_x = row_start();
	let mut current_row = 0.0;
	let mut position = 0.0;
	for &child in &children {
		let Some(size) = db.blocks[child].size else { continue };
		let (_, row) = block_position(columns, position)?;
		if row != current_row {
			current_row = row;
			start_x = row_start();
		}
		let half_width = size.width / 2.0;
		let x = start_x + padding + half_width;
		start_x = x + half_width;
		let row_offset = row_offsets.iter().find(|(known, _)| *known == row).map_or(0.0, |e| e.1);
		let row_height =
			row_heights.iter().find(|(known, _)| *known == row).map_or(size.height, |e| e.1);
		let y = parent.y - parent.height / 2.0 + row_offset + row_height / 2.0 + padding;
		if let Some(size) = &mut db.blocks[child].size {
			size.x = x;
			size.y = y;
		}
		layout_blocks(db, child, padding)?;
		position += columns_filled(&db.blocks[child], columns, position);
	}
	Ok(())
}

/// `insertBlockPositioned`: draws the block in its laid out size and keeps where edges end on
/// it.
fn write_block(
	svg: &mut Svg,
	bbox: &mut BBox,
	context: &mut Context,
	theme: &Theme,
	db: &mut BlockDb,
	index: usize,
	padding: f64,
) {
	let is_space =
		db.get(&db.blocks[index].id).is_some_and(|target| db.blocks[target].is(BlockType::Space));
	if is_space {
		return;
	}
	let block = &db.blocks[index];
	let node = node_of(context, db, block, padding, true);
	let size = block.size.unwrap_or_default();
	let (shape, label) = layout_node(context, theme, &node, size.width);
	let center = Point { x: size.x, y: size.y };
	let drawing = NodeDrawing { shape: &shape, label: &label, extra_labels: &[] };
	rendering::write_node(
		svg,
		bbox,
		context,
		theme,
		DiagramStyle::Flowchart,
		&node,
		&drawing,
		center,
	);
	// `updateNodeBounds`: edges meet the box of the drawn shape.
	let (width, height) = (shape.bbox.width(), shape.bbox.height());
	db.blocks[index].intersect = Some((shape.intersect, center, width, height));
}

/// `insertEdges`: straight edges between the centers of their blocks, cut at their shapes.
fn write_edges(
	svg: &mut Svg,
	bbox: &mut BBox,
	context: &mut Context,
	theme: &Theme,
	db: &BlockDb,
	markers: &mut Markers,
) -> Result<()> {
	let default_curve =
		context.config_text("flowchart", "curve").unwrap_or_else(|| "basis".to_owned());
	for edge in &db.edges {
		let Some(start) = edge.start.as_ref().filter(|start| !start.is_empty()) else { continue };
		if edge.end.is_empty() {
			continue;
		}
		let (Some(tail), Some(head)) =
			(db.get(&Some(start.clone())), db.get(&Some(edge.end.clone())))
		else {
			continue;
		};
		let (Some(from), Some(to)) = (db.blocks[tail].size, db.blocks[head].size) else {
			continue;
		};
		let middle = Point { x: from.x + (to.x - from.x) / 2.0, y: from.y + (to.y - from.y) / 2.0 };
		let points = vec![Point { x: from.x, y: from.y }, middle, Point { x: to.x, y: to.y }];
		let thickness =
			if edge.is_thick { "edge-thickness-thick" } else { "edge-thickness-normal" };
		let pattern = if edge.is_dotted { "edge-pattern-dotted" } else { "edge-pattern-solid" };
		let flowchart_edge = Edge {
			id: format!("{}-{}", context.id, edge.id),
			start: start.clone(),
			end: edge.end.clone(),
			label: edge.label.clone(),
			thickness: if edge.is_thick { Thickness::Thick } else { Thickness::Normal },
			pattern: if edge.is_dotted { Pattern::Dotted } else { Pattern::Solid },
			arrow_type_start: edge.arrow_type_start.to_owned(),
			arrow_type_end: edge.arrow_type_end.to_owned(),
			classes: format!("{thickness} {pattern} flowchart-link LS-a1 LE-b1"),
			..Edge::default()
		};
		let end_of = |index: usize| {
			let block = &db.blocks[index];
			let size = block.size.unwrap_or_default();
			match &block.intersect {
				Some((intersect, center, width, height)) => (
					Bounds { x: center.x, y: center.y, width: *width, height: *height },
					Some(intersect),
				),
				None => {
					(Bounds { x: size.x, y: size.y, width: size.width, height: size.height }, None)
				}
			}
		};
		let (tail_bounds, tail_intersect) = end_of(tail);
		let (head_bounds, head_intersect) = end_of(head);
		// The intersection of the diamond calls `this.calcIntersect`, which the graph node
		// it is called on does not have.
		let has_diamond = [tail, head].iter().any(|&index| db.blocks[index].is(BlockType::Diamond));
		if tail_intersect.is_some() && head_intersect.is_some() && has_diamond {
			return Err(Error::new("this.calcIntersect is not a function"));
		}
		let route = EdgeRoute {
			jumps: None,
			points: points.clone(),
			label_position: Some(middle),
			from_cluster: None,
			to_cluster: None,
		};
		edges::write_edge(
			svg,
			bbox,
			context,
			&flowchart_edge,
			&route,
			&EdgeEnd { bounds: tail_bounds, intersect: tail_intersect },
			&EdgeEnd { bounds: head_bounds, intersect: head_intersect },
			markers,
			theme,
			&default_curve,
		);
		if !edge.label.is_empty() {
			let label = edges::layout_edge_label(context, &flowchart_edge, theme);
			edges::write_edge_label(svg, bbox, context, &flowchart_edge, &label, middle, theme);
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn positions_blocks_in_columns() {
		assert_eq!(block_position(-1.0, 3.0), Ok((3.0, 0.0)));
		assert_eq!(block_position(1.0, 3.0), Ok((0.0, 3.0)));
		assert_eq!(block_position(3.0, 7.0), Ok((1.0, 2.0)));
		assert!(block_position(0.0, 1.0).is_err());
		assert_eq!(
			block_position(2.0, f64::INFINITY),
			Err("Position must be a non-negative integer.Infinity".to_owned())
		);
	}
}
