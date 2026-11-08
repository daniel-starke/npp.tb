//! Edges between laid out nodes (`rendering-elements/edges.js`, `utils/lineWithOffset.ts`).

use super::super::label::LabelBox;
use super::super::rough;
use super::super::svg::{Svg, number};
use super::super::{Context, Look};
use super::curves::{self, Curve};
use super::intersect::Bounds;
use super::markers::Markers;
use super::shapes::Intersect;
use super::{
	BBox, Edge, Pattern, Point, Styles, Theme, Thickness, label_font, layout_label, write_label,
};

/// `markerOffsets`: how far the line ends before the tip of a marker.
pub(super) fn marker_offset(arrow: &str) -> Option<f64> {
	match arrow {
		"aggregation" | "extension" | "composition" => Some(17.25),
		"dependency" => Some(6.0),
		"lollipop" => Some(13.5),
		"arrow_point" => Some(4.0),
		"arrow_barb" => Some(0.0),
		"arrow_barb_neo" => Some(5.5),
		_ => None,
	}
}

/// `markerOffsets2`: the gap the neo look leaves for a marker.
fn marker_gap(arrow: &str) -> f64 {
	match arrow {
		"arrow_point" => 4.0,
		"arrow_cross" | "arrow_circle" => 12.5,
		_ => 0.0,
	}
}

/// An end of an edge: the laid out node and its shape's intersection.
pub struct EdgeEnd<'a> {
	pub bounds: Bounds,
	pub intersect: Option<&'a Intersect>,
}

/// The laid out route of an edge.
pub struct EdgeRoute {
	pub points: Vec<Point>,
	/// The center of the label as the layout placed it.
	pub label_position: Option<Point>,
	pub from_cluster: Option<Bounds>,
	pub to_cluster: Option<Bounds>,
	/// The path with line hops that replaces the drawn one (swimlanes).
	pub jumps: Option<super::line_jumps::JumpPath>,
}

fn is_outside(node: Bounds, point: Point) -> bool {
	(point.x - node.x).abs() >= node.width / 2.0 || (point.y - node.y).abs() >= node.height / 2.0
}

/// `intersection`: where the segment from `outside` to `inside` crosses the border of `node`.
fn intersection(node: Bounds, outside: Point, inside: Point) -> Point {
	let (x, y) = (node.x, node.y);
	let w = node.width / 2.0;
	let h = node.height / 2.0;
	let q_total = (outside.y - inside.y).abs();
	let r_total = (outside.x - inside.x).abs();
	if (y - outside.y).abs() * w > (x - outside.x).abs() * h {
		let q = if inside.y < outside.y { outside.y - h - y } else { y - h - outside.y };
		let r = r_total * q / q_total;
		let mut result = Point {
			x: if inside.x < outside.x { inside.x + r } else { inside.x - r_total + r },
			y: if inside.y < outside.y { inside.y + q_total - q } else { inside.y - q_total + q },
		};
		if r == 0.0 {
			result = outside;
		}
		if r_total == 0.0 {
			result.x = outside.x;
		}
		if q_total == 0.0 {
			result.y = outside.y;
		}
		return result;
	}
	let r = if inside.x < outside.x { outside.x - w - x } else { x - w - outside.x };
	let q = q_total * r / r_total;
	let mut result = Point {
		x: if inside.x < outside.x { inside.x + r_total - r } else { inside.x - r_total + r },
		y: if inside.y < outside.y { inside.y + q } else { inside.y - q },
	};
	if r == 0.0 {
		result = outside;
	}
	if r_total == 0.0 {
		result.x = outside.x;
	}
	if q_total == 0.0 {
		result.y = outside.y;
	}
	result
}

/// `cutPathAtIntersect`: the route up to where it enters `boundary`.
fn cut_path_at_intersect(points: &[Point], boundary: Bounds) -> Vec<Point> {
	let mut result: Vec<Point> = Vec::new();
	let Some(&first) = points.first() else { return result };
	let mut last_outside = first;
	let mut is_inside = false;
	for &point in points {
		if !is_outside(boundary, point) && !is_inside {
			let crossing = intersection(boundary, last_outside, point);
			if !result.contains(&crossing) {
				result.push(crossing);
			}
			is_inside = true;
		} else {
			last_outside = point;
			if !is_inside {
				result.push(point);
			}
		}
	}
	result
}

/// `fixCorners`: right angles of orthogonal routes get a small diagonal.
fn fix_corners(points: &[Point]) -> Vec<Point> {
	let mut corners = Vec::new();
	for index in 1..points.len().saturating_sub(1) {
		let (previous, current, next) = (points[index - 1], points[index], points[index + 1]);
		let is_corner = (previous.x == current.x
			&& current.y == next.y
			&& (current.x - next.x).abs() > 5.0
			&& (current.y - previous.y).abs() > 5.0)
			|| (previous.y == current.y
				&& current.x == next.x
				&& (current.x - previous.x).abs() > 5.0
				&& (current.y - next.y).abs() > 5.0);
		if is_corner {
			corners.push(index);
		}
	}
	let adjacent = |a: Point, b: Point, distance: f64| {
		let (dx, dy) = (b.x - a.x, b.y - a.y);
		let ratio = distance / dx.hypot(dy);
		Point { x: b.x - ratio * dx, y: b.y - ratio * dy }
	};
	let mut result = Vec::with_capacity(points.len() + corners.len() * 2);
	for (index, &corner) in points.iter().enumerate() {
		if !corners.contains(&index) {
			result.push(corner);
			continue;
		}
		let (previous, next) = (points[index - 1], points[index + 1]);
		let new_previous = adjacent(previous, corner, 5.0);
		let new_next = adjacent(next, corner, 5.0);
		let (dx, dy) = (new_next.x - new_previous.x, new_next.y - new_previous.y);
		result.push(new_previous);
		let a = 2.0f64.sqrt() * 2.0;
		let mut new_corner = corner;
		if (next.x - previous.x).abs() > 10.0 && (next.y - previous.y).abs() >= 10.0 {
			let r = 5.0;
			new_corner = if corner.x == new_previous.x {
				Point {
					x: if dx < 0.0 { new_previous.x - r + a } else { new_previous.x + r - a },
					y: if dy < 0.0 { new_previous.y - a } else { new_previous.y + a },
				}
			} else {
				Point {
					x: if dx < 0.0 { new_previous.x - a } else { new_previous.x + a },
					y: if dy < 0.0 { new_previous.y - r + a } else { new_previous.y + r - a },
				}
			};
		}
		result.push(new_corner);
		result.push(new_next);
	}
	result
}

/// `getLineFunctionsWithOffset`: the ends move back from the tips of their markers.
fn offset_points(points: &[Point], edge: &Edge) -> Vec<Point> {
	let n = points.len();
	if n == 0 {
		return Vec::new();
	}
	let (first, last) = (points[0], points[n - 1]);
	let start_height = marker_offset(&edge.arrow_type_start);
	let end_height = marker_offset(&edge.arrow_type_end);
	// `Math.atan(dy / dx)` of `calculateDeltaAndAngle`, with its deltas.
	let delta_angle = |a: Option<&Point>, b: Option<&Point>| match (a, b) {
		(Some(a), Some(b)) => {
			let (dx, dy) = (b.x - a.x, b.y - a.y);
			((dy / dx).atan(), dx, dy)
		}
		_ => (0.0, 0.0, 0.0),
	};
	let goes_right = first.x < last.x;
	let goes_down = first.y < last.y;
	points
		.iter()
		.enumerate()
		.map(|(index, d)| {
			let mut offset_x = 0.0;
			let mut offset_y = 0.0;
			if index == 0
				&& let Some(height) = start_height
			{
				let (angle, dx, dy) = delta_angle(points.first(), points.get(1));
				offset_x = height * angle.cos() * if dx >= 0.0 { 1.0 } else { -1.0 };
				offset_y = height * angle.sin().abs() * if dy >= 0.0 { 1.0 } else { -1.0 };
			} else if index == n - 1
				&& let Some(height) = end_height
			{
				let (angle, dx, dy) = delta_angle(points.last(), points.get(n.wrapping_sub(2)));
				offset_x = height * angle.cos() * if dx >= 0.0 { 1.0 } else { -1.0 };
				offset_y = height * angle.sin().abs() * if dy >= 0.0 { 1.0 } else { -1.0 };
			}
			// Comparisons with `undefined` marker heights are false.
			let (to_end_x, to_end_y) = ((d.x - last.x).abs(), (d.y - last.y).abs());
			let (to_start_x, to_start_y) = ((d.x - first.x).abs(), (d.y - first.y).abs());
			if let Some(height) = end_height {
				if to_end_x < height && to_end_x > 0.0 && to_end_y < height {
					let adjustment =
						(height + 1.0 - to_end_x) * if goes_right { 1.0 } else { -1.0 };
					offset_x -= adjustment;
				}
				if to_end_y < height && to_end_y > 0.0 && to_end_x < height {
					let adjustment = (height + 1.0 - to_end_y) * if goes_down { 1.0 } else { -1.0 };
					offset_y -= adjustment;
				}
			}
			if let Some(height) = start_height {
				if to_start_x < height && to_start_x > 0.0 && to_start_y < height {
					let adjustment =
						(height + 1.0 - to_start_x) * if goes_right { 1.0 } else { -1.0 };
					offset_x += adjustment;
				}
				if to_start_y < height && to_start_y > 0.0 && to_start_x < height {
					let adjustment =
						(height + 1.0 - to_start_y) * if goes_down { 1.0 } else { -1.0 };
					offset_y += adjustment;
				}
			}
			Point { x: d.x + offset_x, y: d.y + offset_y }
		})
		.collect()
}

/// `applyMarkerOffsetsToPoints` of rounded edges.
fn apply_marker_offsets(points: &[Point], edge: &Edge) -> Vec<Point> {
	let mut result = points.to_vec();
	let n = points.len();
	if n < 2 {
		return result;
	}
	// A zero offset counts as none.
	let marker_offset = |arrow: &str| marker_offset(arrow).filter(|offset| *offset != 0.0);
	if let Some(offset) = marker_offset(&edge.arrow_type_start) {
		let angle = (points[1].y - points[0].y).atan2(points[1].x - points[0].x);
		result[0] =
			Point { x: points[0].x + offset * angle.cos(), y: points[0].y + offset * angle.sin() };
	}
	if let Some(offset) = marker_offset(&edge.arrow_type_end) {
		let angle = (points[n - 1].y - points[n - 2].y).atan2(points[n - 1].x - points[n - 2].x);
		result[n - 1] = Point {
			x: points[n - 1].x - offset * angle.cos(),
			y: points[n - 1].y - offset * angle.sin(),
		};
	}
	result
}

/// `generateDashArray` of dotted and dashed neo edges.
fn dash_array(length: f64, start_gap: f64, end_gap: f64) -> String {
	let middle = length - start_gap - end_gap;
	let pairs = (middle / 4.0).floor();
	let pairs = if pairs.is_finite() { pairs.max(0.0) as usize } else { 0 };
	let pattern = vec!["2 2"; pairs].join(" ");
	format!("0 {} {pattern} {}", number(start_gap), number(end_gap))
}

/// `traverseEdge`: the point halfway along `points`.
pub fn label_position(points: &[Point]) -> Option<Point> {
	if points.len() == 1 {
		return points.first().copied();
	}
	let distance = |a: Point, b: Point| (b.x - a.x).hypot(b.y - a.y);
	let total: f64 = points.windows(2).map(|pair| distance(pair[0], pair[1])).sum();
	let mut remaining = total / 2.0;
	let round5 = |value: f64| (value * 1e5).round() / 1e5;
	let mut previous: Option<Point> = None;
	for &point in points {
		if let Some(previous) = previous {
			let segment = distance(point, previous);
			if segment == 0.0 {
				return Some(previous);
			}
			if segment < remaining {
				remaining -= segment;
			} else {
				let ratio = remaining / segment;
				if ratio <= 0.0 {
					return Some(previous);
				}
				if ratio >= 1.0 {
					return Some(point);
				}
				return Some(Point {
					x: round5((1.0 - ratio) * previous.x + ratio * point.x),
					y: round5((1.0 - ratio) * previous.y + ratio * point.y),
				});
			}
		}
		previous = Some(point);
	}
	previous
}

/// `isLabelCoordinateInPath`: whether a rounded coordinate of `point` appears in `d`.
fn is_label_coordinate_in_path(point: Point, d: &str) -> bool {
	let mut sanitized = String::with_capacity(d.len());
	let mut rest = d;
	// Numbers with a fraction are rounded, like `/(\d+\.\d+)/g`.
	while let Some(start) = rest.find(|c: char| c.is_ascii_digit()) {
		sanitized.push_str(&rest[..start]);
		let digits = &rest[start..];
		let integer = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
		let after = &digits[integer..];
		let fraction = after
			.strip_prefix('.')
			.map(|f| f.find(|c: char| !c.is_ascii_digit()).unwrap_or(f.len()))
			.unwrap_or(0);
		if fraction > 0 {
			let text = &digits[..integer + 1 + fraction];
			let value: f64 = text.parse().unwrap_or(0.0);
			sanitized.push_str(&super::super::color::js_number((value + 0.5).floor()));
			rest = &digits[integer + 1 + fraction..];
		} else {
			sanitized.push_str(&digits[..integer]);
			rest = after;
		}
	}
	sanitized.push_str(rest);
	let rounded = |value: f64| super::super::color::js_number((value + 0.5).floor());
	sanitized.contains(&rounded(point.x)) || sanitized.contains(&rounded(point.y))
}

/// The paint of the edge style rules: stroke, width and dashes.
/// The paint of an edge. A sketched edge keeps the attributes roughjs gives its path where no
/// style sheet rule overrides them.
fn write_edge_paint(
	svg: &mut Svg,
	edge: &Edge,
	theme: &Theme,
	is_flowchart_link: bool,
	sketch: Option<&rough::Sketch>,
) {
	let (stroke, stroke_width, fill, dashes): (Option<&str>, Option<&str>, _, _) = match &edge.paint
	{
		Some((stroke, width)) => {
			(Some(stroke.as_str()), Some(width.as_str()), Some("none"), edge.dasharray)
		}
		None => {
			let (stroke, fill) = match is_flowchart_link {
				true => (Some(theme.edge_stroke.as_str()), Some("none")),
				false => (None, None),
			};
			let (stroke_width, fill) = match edge.thickness {
				Thickness::Thick => (Some("3.5px"), fill),
				Thickness::Invisible => (Some("0"), Some("none")),
				Thickness::Normal => (Some(theme.edge_stroke_width.as_str()), fill),
			};
			// `.note-edge` comes after `.edge-pattern-dashed`.
			let dashes = match edge.pattern {
				Pattern::Dotted => Some("2"),
				Pattern::Dashed if edge.classes.contains("note-edge") => Some("5"),
				Pattern::Dashed => Some(theme.dashed_edge),
				Pattern::Solid => None,
			};
			(stroke, stroke_width, fill, dashes)
		}
	};
	let mut stroke = stroke.filter(|value| !value.is_empty());
	let mut stroke_width = stroke_width.filter(|value| !value.is_empty());
	let mut fill = fill;
	if let Some(sketch) = sketch {
		stroke = stroke.or(Some(sketch.options.stroke.as_str()));
		stroke_width = stroke_width.or(Some(sketch.options.stroke_width.as_str()));
		fill = fill.or(Some("none"));
	}
	svg.attr_if("stroke", stroke.unwrap_or(""));
	if let Some(fill) = fill {
		svg.attr("fill", fill);
	}
	svg.attr_if("stroke-width", stroke_width.unwrap_or(""));
	if let Some(dashes) = dashes {
		svg.attr("stroke-dasharray", dashes);
	}
}

/// Where the labels of a written edge go.
pub struct EdgePlacement {
	/// The center of the label.
	pub label: Option<Point>,
	/// The route the terminal labels follow: the clipped one if it changed, else the laid out
	/// one.
	pub path: Vec<Point>,
}

/// Writes the path of `edge` (`insertEdge`) and returns where its labels go.
#[allow(clippy::too_many_arguments)]
pub fn write_edge(
	svg: &mut Svg,
	bbox: &mut BBox,
	context: &Context,
	edge: &Edge,
	route: &EdgeRoute,
	tail: &EdgeEnd,
	head: &EdgeEnd,
	markers: &mut Markers,
	theme: &Theme,
	default_curve: &str,
) -> EdgePlacement {
	let mut points = route.points.clone();
	let mut points_changed = false;
	if let (Some(tail_intersect), Some(head_intersect)) = (tail.intersect, head.intersect)
		&& route.points.len() >= 3
	{
		let inner = &route.points[1..route.points.len() - 1];
		let mut clipped = Vec::with_capacity(inner.len() + 2);
		clipped.push(tail_intersect.point(tail.bounds, inner[0]));
		clipped.extend_from_slice(inner);
		clipped.push(head_intersect.point(head.bounds, inner[inner.len() - 1]));
		points = clipped;
	}
	if let Some(cluster) = route.to_cluster {
		points = cut_path_at_intersect(&route.points, cluster);
		points_changed = true;
	}
	if let Some(cluster) = route.from_cluster {
		points.reverse();
		points = cut_path_at_intersect(&points, cluster);
		points.reverse();
		points_changed = true;
	}
	let mut line_data: Vec<Point> = points.iter().copied().filter(|p| !p.y.is_nan()).collect();
	let curve = Curve::parse(edge.curve.as_deref().unwrap_or(default_curve));
	if curve != Curve::Rounded {
		line_data = fix_corners(&line_data);
	}
	let path = match curve {
		Curve::Rounded => curves::rounded(&apply_marker_offsets(&line_data, edge), 5.0),
		_ => curves::line(&offset_points(&line_data, edge), curve),
	};
	// The path `insertEdge` writes; `applyLineJumpsToSvg` replaces it by the path with the
	// hops afterwards.
	let mut inserted_d = path.to_svg();
	// `rc.path(linePath, { roughness: 0.3, seed })`; only its outline is kept.
	let sketch = (edge.look == Look::HandDrawn).then(|| {
		let line_path = match curve {
			// `generateRoundedPath` writes its numbers in full, d3's line with 3 decimals.
			Curve::Rounded => path.to_full_svg(),
			_ => inserted_d.clone(),
		};
		let options = rough::Options {
			roughness: 0.3,
			seed: context.sketch_seed(&edge.id),
			..Default::default()
		};
		rough::path(&line_path, &options)
	});
	if let Some(outline) = sketch.as_ref().and_then(|sketch| sketch.paths.first()) {
		inserted_d = outline.d.clone();
	}
	let d = match &route.jumps {
		Some(jumps) => jumps.d.clone(),
		None => inserted_d.clone(),
	};
	let mut stroke_classes = match edge.thickness {
		Thickness::Normal => "edge-thickness-normal",
		Thickness::Thick => "edge-thickness-thick",
		Thickness::Invisible => "edge-thickness-invisible",
	}
	.to_owned();
	stroke_classes.push_str(match edge.pattern {
		Pattern::Solid => " edge-pattern-solid",
		Pattern::Dotted => " edge-pattern-dotted",
		Pattern::Dashed => " edge-pattern-dashed",
	});
	if sketch.is_some() {
		stroke_classes.push_str(" transition");
	}
	let animation_class = match (&edge.animation, edge.animate) {
		(Some(animation), _) => format!("edge-animation-{animation}"),
		(None, true) => "edge-animation-fast".to_owned(),
		(None, false) => String::new(),
	};
	let mut class = format!(" {stroke_classes}");
	if !edge.classes.is_empty() {
		class.push(' ');
		class.push_str(&edge.classes);
	}
	if !animation_class.is_empty() {
		class.push(' ');
		class.push_str(&animation_class);
	}
	let class_styles = edge.css_compiled_styles.join(";");
	let edge_styles = edge.style.join(";");
	let mut style = match sketch {
		// `edgeStyles.reduce((acc, style) => acc + ';' + style, '')`, without the class styles.
		Some(_) => edge.style.iter().fold(String::new(), |style, part| style + ";" + part),
		None => [class_styles.as_str(), edge_styles.as_str()]
			.into_iter()
			.filter(|part| !part.is_empty())
			.collect::<Vec<_>>()
			.join(";"),
	};
	let stroke_color = Styles::compile(&[&edge.css_compiled_styles, &edge.style])
		.entries
		.into_iter()
		.find(|(key, _)| key == "stroke")
		.map(|(_, value)| value);
	let is_animated =
		edge.animate || edge.animation.is_some() || class_styles.contains("animation");
	if edge.look == Look::Neo && !is_animated {
		let length = path.length();
		let start_gap = marker_gap(&edge.arrow_type_start);
		let end_gap = marker_gap(&edge.arrow_type_end);
		let dashes = match edge.pattern {
			Pattern::Dotted | Pattern::Dashed => dash_array(length, start_gap, end_gap),
			Pattern::Solid => format!(
				"0 {} {} {}",
				number(start_gap),
				number(length - start_gap - end_gap),
				number(end_gap)
			),
		};
		// `applyLineJumpsToSvg` keeps the first gap and the fourth number of the dash array
		// (`/0\s+([\d.]+)\s+[\d.]+\s+([\d.]+)/`) around the length of the new path.
		let dashes = match &route.jumps {
			Some(jumps) => {
				let numbers: Vec<&str> = dashes.split_whitespace().collect();
				match numbers.as_slice() {
					["0", start, _, end, ..] => {
						let (start, end) = (
							start.parse::<f64>().unwrap_or(0.0),
							end.parse::<f64>().unwrap_or(0.0),
						);
						format!(
							"0 {} {} {}",
							number(start),
							number((jumps.length - start - end).max(0.0)),
							number(end)
						)
					}
					_ => dashes,
				}
			}
			None => dashes,
		};
		style = format!("stroke-dasharray: {dashes}; stroke-dashoffset: 0;{style}");
	}
	let id = match &edge.dom_id {
		Some(dom_id) => dom_id.replace("{}", &context.id),
		None => format!("{}-{}", context.id, edge.id),
	};
	svg.open("path").attr("d", &d).attr("id", &id).attr("class", &class);
	let is_link = ["flowchart-link", "transition", "relation"]
		.iter()
		.any(|class| edge.classes.contains(class));
	write_edge_paint(svg, edge, theme, is_link, sketch.as_ref());
	if is_animated {
		svg.attr("stroke-dasharray", "9,5").attr("stroke-linecap", "round");
	}
	svg.attr_if("style", &style)
		.attr("data-edge", "true")
		.attr("data-et", "edge")
		.attr("data-id", &edge.id);
	let use_margin = !is_animated && edge.look == Look::Neo;
	if let Some(marker) =
		markers.reference(&edge.arrow_type_start, true, use_margin, stroke_color.as_deref())
	{
		svg.attr("marker-start", &marker);
	}
	if let Some(marker) =
		markers.reference(&edge.arrow_type_end, false, use_margin, stroke_color.as_deref())
	{
		svg.attr("marker-end", &marker);
	}
	for (name, value) in &edge.attributes {
		svg.attr(name, value);
	}
	svg.close();
	let bounds = match (&sketch, &route.jumps) {
		(_, Some(jumps)) => Some(jumps.bounds),
		(Some(sketch), None) => sketch.bounds().map(|bounds| {
			let drawn = super::sketch::bounds_box(bounds);
			(drawn.min, drawn.max)
		}),
		(None, None) => path.bounds(),
	};
	if let Some((min, max)) = bounds {
		bbox.include_point(min);
		bbox.include_point(max);
	}
	if let Some(&middle) = points.get(points.len() / 2)
		&& !is_label_coordinate_in_path(middle, &inserted_d)
	{
		points_changed = true;
	}
	// `resolveEdgeLabelPosition`.
	let original = route.label_position;
	let label = match (points_changed, original) {
		(true, Some(original)) => {
			label_position(&points).zip(label_position(&route.points)).map(|(updated, before)| {
				Point { x: original.x + updated.x - before.x, y: original.y + updated.y - before.y }
			})
		}
		(true, None) => label_position(&points),
		(false, original) => original,
	};
	let path = if points_changed { points } else { route.points.clone() };
	EdgePlacement { label, path }
}

/// The ends of an edge that terminal labels go to (`calcTerminalLabelPosition`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terminal {
	StartRight,
	EndLeft,
}

/// `calculatePoint`: the point `distance` along `points`, `None` if the route is shorter.
fn point_at_distance(points: &[Point], distance: f64) -> Option<Point> {
	let round5 = |value: f64| (value * 1e5).round() / 1e5;
	let mut remaining = distance;
	let mut previous: Option<Point> = None;
	for &point in points {
		if let Some(previous) = previous {
			let segment = (point.x - previous.x).hypot(point.y - previous.y);
			if segment == 0.0 {
				return Some(previous);
			}
			if segment < remaining {
				remaining -= segment;
			} else {
				let ratio = remaining / segment;
				if ratio <= 0.0 {
					return Some(previous);
				}
				if ratio >= 1.0 {
					return Some(point);
				}
				return Some(Point {
					x: round5((1.0 - ratio) * previous.x + ratio * point.x),
					y: round5((1.0 - ratio) * previous.y + ratio * point.y),
				});
			}
		}
		previous = Some(point);
	}
	None
}

/// `calcTerminalLabelPosition`: beside the route, 25 pixels and the marker size from its end.
/// Mermaid fails on routes shorter than that; their labels go to the far end.
pub fn terminal_label_position(
	path: &[Point],
	terminal: Terminal,
	marker_size: f64,
) -> Option<Point> {
	let mut points = path.to_vec();
	if terminal == Terminal::EndLeft {
		points.reverse();
	}
	let first = *points.first()?;
	let distance = 25.0 + marker_size;
	let center = point_at_distance(&points, distance).or_else(|| points.last().copied())?;
	let offset = 10.0 + marker_size * 0.5;
	let angle = (first.y - center.y).atan2(first.x - center.x);
	// Both ends' labels are on the left of the route as seen from the end.
	Some(Point {
		x: angle.sin() * offset + (first.x + center.x) / 2.0,
		y: -angle.cos() * offset + (first.y + center.y) / 2.0,
	})
}

/// Lays out a terminal label (`createLabel`): plain text in the 11 pixel font of
/// `.edgeTerminals`, not wrapped.
pub fn layout_terminal_label(context: &mut Context, edge: &Edge, text: &str) -> LabelBox {
	let (mut font, _) = label_font(context, &label_styles(edge));
	font.size = 11.0;
	layout_label(context, text, false, &font, None, 0.0)
}

/// Writes a terminal label centered on `position`.
pub fn write_terminal_label(
	svg: &mut Svg,
	bbox: &mut BBox,
	context: &mut Context,
	edge: &Edge,
	label: &LabelBox,
	position: Point,
	theme: &Theme,
) {
	let (_, color) = label_font(context, &label_styles(edge));
	svg.open("g")
		.attr("class", "edgeTerminals")
		.attr("transform", &super::translate(position.x, position.y))
		.end();
	svg.open("g")
		.attr("class", "inner")
		.attr("transform", &super::translate(-label.width / 2.0, -label.height / 2.0))
		.end();
	let fill = color.unwrap_or_else(|| theme.edge_label_color.clone());
	write_label(svg, context, label, Point { x: label.width / 2.0, y: label.height / 2.0 }, &fill);
	bbox.include_rect(
		position.x - label.width / 2.0,
		position.y - label.height / 2.0,
		label.width,
		label.height,
	);
	svg.close_tag("g").close_tag("g");
}

/// The styles of an edge's label.
pub fn label_styles(edge: &Edge) -> Styles {
	Styles::compile(&[&edge.css_compiled_styles, &edge.style])
}

/// Lays out the label of `edge` (`insertEdgeLabel`): wrapped at 200 pixels.
pub fn layout_edge_label(context: &mut Context, edge: &Edge, theme: &Theme) -> LabelBox {
	let styles = label_styles(edge);
	let (mut font, _) = label_font(context, &styles);
	if let Some(size) = theme.edge_label_font_size.filter(|_| styles.get("font-size").is_none()) {
		font.size = size;
	}
	if theme.edge_label_padding == 0.0 {
		return layout_label(context, &edge.label, edge.is_markdown, &font, Some(200.0), 0.0);
	}
	// With a padding the `<div>` never gets exactly as wide as its `max-width`, which is what
	// makes `createText` switch to wrapping: the label stays on one line and overflows.
	let mut label = layout_label(context, &edge.label, edge.is_markdown, &font, None, 0.0);
	if label.width > 200.0 {
		label.width = 200.0;
		label.is_left_aligned = true;
	}
	// The padding widens the label, also an empty one.
	label.width += theme.edge_label_padding * 2.0;
	label
}

/// Writes the label of `edge` centered on `position`, on its background.
pub fn write_edge_label(
	svg: &mut Svg,
	bbox: &mut BBox,
	context: &mut Context,
	edge: &Edge,
	label: &LabelBox,
	position: Point,
	theme: &Theme,
) {
	let (_, color) = label_font(context, &label_styles(edge));
	svg.open("g")
		.attr("class", "edgeLabel")
		.attr("transform", &super::translate(position.x, position.y))
		.end();
	svg.open("g")
		.attr("class", "label")
		.attr("data-id", &edge.id)
		.attr("transform", &super::translate(-label.width / 2.0, -label.height / 2.0))
		.end();
	if !label.is_empty() {
		svg.open("rect")
			.number_attr("width", label.width)
			.number_attr("height", label.height)
			.attr_if("fill", &theme.edge_label_background)
			.close();
		let fill = color.unwrap_or_else(|| theme.edge_label_color.clone());
		write_label(
			svg,
			context,
			label,
			Point { x: label.width / 2.0, y: label.height / 2.0 },
			&fill,
		);
		bbox.include_rect(
			position.x - label.width / 2.0,
			position.y - label.height / 2.0,
			label.width,
			label.height,
		);
	}
	svg.close_tag("g").close_tag("g");
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn cuts_and_offsets_routes() {
		let cluster = Bounds { x: 0.0, y: 100.0, width: 100.0, height: 100.0 };
		let route =
			[Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: 40.0 }, Point { x: 0.0, y: 100.0 }];
		// A vertical route has `r === 0`, where `intersection` returns the outside point.
		assert_eq!(cut_path_at_intersect(&route, cluster), [route[0], route[1]]);
		let diagonal = [Point { x: -40.0, y: 0.0 }, Point { x: 20.0, y: 120.0 }];
		assert_eq!(
			cut_path_at_intersect(&diagonal, cluster),
			[diagonal[0], Point { x: -15.0, y: 50.0 }]
		);
		let edge = Edge { arrow_type_end: "arrow_point".to_owned(), ..Edge::default() };
		let points = offset_points(
			&[
				Point { x: 195.0, y: 53.0 },
				Point { x: 195.0, y: 78.0 },
				Point { x: 195.0, y: 103.0 },
			],
			&edge,
		);
		assert_eq!(points[2], Point { x: 195.0, y: 99.0 });
		assert_eq!(label_position(&route), Some(Point { x: 0.0, y: 50.0 }));
		assert!(is_label_coordinate_in_path(Point { x: 84.0, y: 309.5 }, "M84,390L84,395.917"));
		assert_eq!(dash_array(20.0, 0.0, 4.0), "0 0 2 2 2 2 2 2 2 2 4");
	}

	#[test]
	fn rounds_corners_of_orthogonal_routes() {
		let route =
			[Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: 50.0 }, Point { x: 50.0, y: 50.0 }];
		let fixed = fix_corners(&route);
		assert_eq!(fixed.len(), 5);
		assert_eq!(fixed[1], Point { x: 0.0, y: 45.0 });
		assert_eq!(fixed[3], Point { x: 5.0, y: 50.0 });
	}
}
