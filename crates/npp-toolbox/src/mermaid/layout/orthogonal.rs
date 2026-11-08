//! An orthogonal edge router for layered layouts, which gives the `elk` layout its look on top of
//! dagre's layering, ordering and placement: edges leave and enter nodes on the side facing the
//! next layer, at ports spread along that side, run straight across the layers and turn in tracks
//! between them. Gaps between layers grow to fit their tracks. Own design, not a port of ELK.
//!
//! The router works in an internal frame where the layers run from top to bottom; the direction of
//! the layout maps it to the final coordinates.

use std::collections::BTreeMap;

use super::dagre::Point;
use super::graph::RankDir;

/// The distance between tracks, and between a track and the layers above and below it.
pub const TRACK_SPACING: f64 = 20.0;
/// How far a self loop reaches out of its node, per loop.
pub const SELF_LOOP_REACH: f64 = 20.0;
/// The distance of the outermost ports from the corners of their side.
const PORT_MARGIN: f64 = 10.0;
/// The smallest distance between two ports when a port moves to straighten its edge.
const MIN_PORT_GAP: f64 = 10.0;
/// Half the distance between the two ends of a self loop.
const SELF_LOOP_HALF_HEIGHT: f64 = 3.5;
/// Positions closer than this are the same.
const EPSILON: f64 = 1e-6;
/// The cost of a track order that makes two edges overlap, compared with one crossing.
const OVERLAP_COST: u32 = 100;

/// A rectangle given by its center, like dagre's nodes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
	pub x: f64,
	pub y: f64,
	pub width: f64,
	pub height: f64,
}

impl Rect {
	fn top(&self) -> f64 {
		self.y - self.height / 2.0
	}

	fn bottom(&self) -> f64 {
		self.y + self.height / 2.0
	}

	fn left(&self) -> f64 {
		self.x - self.width / 2.0
	}

	fn right(&self) -> f64 {
		self.x + self.width / 2.0
	}
}

/// A node on a rank.
#[derive(Clone, Debug)]
pub struct Node {
	pub bounds: Rect,
	pub rank: i64,
}

/// An edge laid out by dagre between two nodes.
#[derive(Clone, Debug, Default)]
pub struct Edge {
	pub source: usize,
	pub target: usize,
	/// dagre's points between the ends, one per rank between the ranks of the ends.
	pub waypoints: Vec<Point>,
	/// The label and its center.
	pub label: Option<Rect>,
	/// Fixed ports instead of ports chosen by the router: the offsets of the ends of the edge
	/// from the centers of its source and target. They must lie on the side the edge leaves or
	/// enters on.
	pub source_port: Option<Point>,
	pub target_port: Option<Point>,
}

/// The graph to route; the router moves nodes and labels to make room for tracks.
#[derive(Clone, Debug, Default)]
pub struct Layout {
	pub direction: RankDir,
	/// The room between levels without tracks.
	pub layer_spacing: f64,
	pub nodes: Vec<Node>,
	pub edges: Vec<Edge>,
}

/// The route of an edge from the border of its start to the border of its end, and the center
/// of its label.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Route {
	pub points: Vec<Point>,
	pub label: Option<Point>,
}

/// The side of a node in the internal frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
	Top,
	Bottom,
}

/// The ends of edges on each side of each node: where the edge goes, the edge and the index of
/// the end in its stations.
type PortSides = BTreeMap<(usize, Side), Vec<(f64, usize, usize)>>;

/// The jogs of the gap below a level with their tracks, and the number of tracks.
type GapTracks = (i64, Vec<(Jog, usize)>, usize);

#[derive(Clone, Copy, Debug)]
enum StationKind {
	/// A port of a node; its position is chosen by the router.
	Port { node: usize },
	/// A point the edge passes through: a dummy of dagre.
	Pass,
}

/// A point of an edge on a rank.
#[derive(Clone, Copy, Debug)]
struct Station {
	kind: StationKind,
	rank: i64,
	x: f64,
}

/// The vertical extent of a rank that holds nodes or labels.
#[derive(Clone, Copy, Debug)]
struct Level {
	top: f64,
	bottom: f64,
	/// Added to everything on the level to make room for the tracks above it.
	shift: f64,
}

/// A turn of an edge between two levels: the horizontal run in a track.
#[derive(Clone, Copy, Debug)]
struct Jog {
	edge: usize,
	/// The index of the upper station in the edge's stations.
	station: usize,
	x_top: f64,
	x_bottom: f64,
}

impl Jog {
	fn low(&self) -> f64 {
		self.x_top.min(self.x_bottom)
	}

	fn high(&self) -> f64 {
		self.x_top.max(self.x_bottom)
	}

	fn overlaps(&self, other: &Jog) -> bool {
		self.low() <= other.high() + EPSILON && other.low() <= self.high() + EPSILON
	}

	/// The crossings and overlaps when `self` runs in a track above `other`.
	fn cost_above(&self, other: &Jog) -> u32 {
		let strictly_inside =
			|x: f64, jog: &Jog| x > jog.low() + EPSILON && x < jog.high() - EPSILON;
		let mut cost = 0;
		// The upper vertical of `other` reaches down past the track of `self`.
		if strictly_inside(other.x_top, self) {
			cost += 1;
		}
		// The lower vertical of `self` reaches down past the track of `other`.
		if strictly_inside(self.x_bottom, other) {
			cost += 1;
		}
		if (self.x_bottom - other.x_top).abs() <= EPSILON {
			cost += OVERLAP_COST;
		}
		cost
	}
}

/// Maps points between the final coordinates and the internal frame.
#[derive(Clone, Copy)]
struct Frame(RankDir);

impl Frame {
	fn point_in(self, point: Point) -> Point {
		match self.0 {
			RankDir::TopBottom => point,
			RankDir::BottomTop => Point { x: point.x, y: -point.y },
			RankDir::LeftRight => Point { x: point.y, y: point.x },
			RankDir::RightLeft => Point { x: point.y, y: -point.x },
		}
	}

	fn point_out(self, point: Point) -> Point {
		match self.0 {
			RankDir::TopBottom => point,
			RankDir::BottomTop => Point { x: point.x, y: -point.y },
			RankDir::LeftRight => Point { x: point.y, y: point.x },
			RankDir::RightLeft => Point { x: -point.y, y: point.x },
		}
	}

	fn rect_in(self, rect: Rect) -> Rect {
		let center = self.point_in(Point { x: rect.x, y: rect.y });
		let (width, height) = match self.0.is_horizontal() {
			true => (rect.height, rect.width),
			false => (rect.width, rect.height),
		};
		Rect { x: center.x, y: center.y, width, height }
	}

	fn rect_out(self, rect: Rect) -> Rect {
		let center = self.point_out(Point { x: rect.x, y: rect.y });
		let (width, height) = match self.0.is_horizontal() {
			true => (rect.height, rect.width),
			false => (rect.width, rect.height),
		};
		Rect { x: center.x, y: center.y, width, height }
	}
}

/// The room a node needs besides its own size for `loops` self loops with labels of the given
/// widths (in the internal frame: across the layers), on the side the loops go out to.
pub fn self_loop_room(loops: usize, label_widths: impl IntoIterator<Item = f64>) -> f64 {
	if loops == 0 {
		return 0.0;
	}
	let widest_label =
		label_widths.into_iter().filter(|width| width.is_finite()).fold(0.0, f64::max);
	SELF_LOOP_REACH * loops as f64 + widest_label
}

/// Routes the edges of `layout`, moving its nodes and labels apart where tracks need
/// room. Returns the route of every edge, empty where an edge cannot be routed.
pub fn route(layout: &mut Layout) -> Vec<Route> {
	let frame = Frame(layout.direction);
	let mut nodes: Vec<Rect> = layout.nodes.iter().map(|node| frame.rect_in(node.bounds)).collect();
	let mut labels: Vec<Option<Rect>> =
		layout.edges.iter().map(|edge| edge.label.map(|label| frame.rect_in(label))).collect();
	let node_rank = |index: usize| layout.nodes.get(index).map(|node| node.rank);

	// The stations of every edge on the ranks between its ends.
	let mut stations: Vec<Option<Vec<Station>>> = Vec::with_capacity(layout.edges.len());
	let mut label_ranks: Vec<Option<i64>> = vec![None; layout.edges.len()];
	for (index, edge) in layout.edges.iter().enumerate() {
		let (Some(source_rank), Some(target_rank)) =
			(node_rank(edge.source), node_rank(edge.target))
		else {
			stations.push(None);
			continue;
		};
		if edge.source == edge.target
			|| source_rank == target_rank
			|| edge.waypoints.len() as u64 + 1 != source_rank.abs_diff(target_rank)
		{
			stations.push(None);
			continue;
		}
		let step = (target_rank - source_rank).signum();
		let mut list = Vec::with_capacity(edge.waypoints.len() + 2);
		list.push(Station {
			kind: StationKind::Port { node: edge.source },
			rank: source_rank,
			x: 0.0,
		});
		let mut rank = source_rank;
		for waypoint in &edge.waypoints {
			rank += step;
			let point = frame.point_in(*waypoint);
			list.push(Station { kind: StationKind::Pass, rank, x: point.x });
		}
		list.push(Station {
			kind: StationKind::Port { node: edge.target },
			rank: target_rank,
			x: 0.0,
		});
		if let Some(label) = labels[index] {
			// The label sits on the dummy closest to its center.
			label_ranks[index] = list[1..list.len() - 1]
				.iter()
				.zip(&edge.waypoints)
				.map(|(station, waypoint)| {
					let point = frame.point_in(*waypoint);
					(station.rank, (point.x - label.x).abs() + (point.y - label.y).abs())
				})
				.min_by(|a, b| a.1.total_cmp(&b.1))
				.map(|(rank, _)| rank);
		}
		stations.push(Some(list));
	}

	// Levels: the ranks with nodes or labels.
	let mut levels: BTreeMap<i64, Level> = BTreeMap::new();
	let mut extend = |rank: i64, top: f64, bottom: f64| {
		let level = levels.entry(rank).or_insert(Level { top, bottom, shift: 0.0 });
		level.top = level.top.min(top);
		level.bottom = level.bottom.max(bottom);
	};
	for (node, bounds) in layout.nodes.iter().zip(&nodes) {
		extend(node.rank, bounds.top(), bounds.bottom());
	}
	for (rank, label) in label_ranks.iter().zip(&labels) {
		if let (Some(rank), Some(label)) = (rank, label) {
			extend(*rank, label.top(), label.bottom());
		}
	}

	for list in stations.iter_mut().flatten() {
		// Points on ranks without a level are free: the edge turns in the gap around them.
		let last = list.len() - 1;
		let mut position = 0;
		list.retain(|station| {
			let keep = position == 0 || position == last || levels.contains_key(&station.rank);
			position += 1;
			keep
		});
	}

	// Ports: the ends on each side of a node, ordered by where the edges go.
	let mut sides: PortSides = BTreeMap::new();
	for (index, list) in stations.iter_mut().enumerate() {
		let Some(list) = list else { continue };
		for (position, neighbor) in [(0, 1), (list.len() - 1, list.len() - 2)] {
			let station = list[position];
			let StationKind::Port { node } = station.kind else { continue };
			let edge = &layout.edges[index];
			let fixed = if position == 0 { edge.source_port } else { edge.target_port };
			if let Some(offset) =
				fixed.filter(|offset| offset.x.is_finite() && offset.y.is_finite())
			{
				list[position].x = nodes[node].x + frame.point_in(offset).x;
				continue;
			}
			let next = list[neighbor];
			let side = if next.rank > station.rank { Side::Bottom } else { Side::Top };
			let toward = match next.kind {
				StationKind::Port { node } => nodes[node].x,
				_ => next.x,
			};
			sides.entry((node, side)).or_default().push((toward, index, position));
		}
	}
	for ((node, _), ends) in sides.iter_mut() {
		npp::sort::sort_by(&mut ends[..], |a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
		let bounds = nodes[*node];
		let count = ends.len() as f64;
		let usable = bounds.width - 2.0 * PORT_MARGIN;
		for (order, &(_, edge, position)) in ends.iter().enumerate() {
			let x = match usable > 0.0 {
				true => bounds.left() + PORT_MARGIN + usable * (order + 1) as f64 / (count + 1.0),
				false => bounds.x,
			};
			if let Some(list) = stations[edge].as_mut() {
				list[position].x = x;
			}
		}
	}
	straighten_ports(&mut stations, &sides, &nodes);

	// Jogs between the levels, grouped by the gap below the upper level.
	let mut gaps: BTreeMap<i64, Vec<Jog>> = BTreeMap::new();
	for (index, list) in stations.iter().enumerate() {
		let Some(list) = list else { continue };
		for station in 0..list.len() - 1 {
			let (a, b) = (list[station], list[station + 1]);
			if (a.x - b.x).abs() <= EPSILON {
				continue;
			}
			let (upper, lower) = if a.rank < b.rank { (a, b) } else { (b, a) };
			gaps.entry(upper.rank).or_default().push(Jog {
				edge: index,
				station,
				x_top: upper.x,
				x_bottom: lower.x,
			});
		}
	}
	let mut tracks: Vec<Vec<f64>> = stations
		.iter()
		.map(|list| list.as_ref().map_or_else(Vec::new, |list| vec![f64::NAN; list.len()]))
		.collect();
	let ranks: Vec<i64> = levels.keys().copied().collect();
	// The levels are stacked anew: each gap is the layer spacing, or what its tracks need.
	let layer_spacing = finite_or_zero(layout.layer_spacing).max(0.0);
	let mut shift = 0.0;
	let mut gap_tracks: Vec<GapTracks> = Vec::new();
	for (position, &rank) in ranks.iter().enumerate() {
		if let Some(level) = levels.get_mut(&rank) {
			level.shift = shift;
		}
		let Some(&next_rank) = ranks.get(position + 1) else { break };
		let jogs = gaps.remove(&rank).unwrap_or_default();
		let assigned = assign_tracks(jogs);
		let count = assigned.iter().map(|(_, track)| track + 1).max().unwrap_or(0);
		let (Some(level), Some(next)) = (levels.get(&rank), levels.get(&next_rank)) else {
			continue;
		};
		let available = next.top - level.bottom;
		let tracks_need = if count > 0 { (count + 1) as f64 * TRACK_SPACING } else { 0.0 };
		let needed = tracks_need.max(layer_spacing);
		if available.is_finite() {
			shift += needed - available;
		}
		gap_tracks.push((rank, assigned, count));
	}
	let shift_of = |rank: i64| levels.get(&rank).map_or(0.0, |level| level.shift);
	for (rank, assigned, count) in gap_tracks {
		let Some(level) = levels.get(&rank) else { continue };
		let top = level.bottom + level.shift;
		let bottom = levels.range(rank + 1..).next().map_or(top, |(_, next)| next.top + next.shift);
		let used = (count as f64 - 1.0) * TRACK_SPACING;
		let first = top + ((bottom - top) - used) / 2.0;
		for (jog, track) in assigned {
			tracks[jog.edge][jog.station] = first + track as f64 * TRACK_SPACING;
		}
	}

	// Move everything below grown gaps.
	for (node, bounds) in layout.nodes.iter().zip(nodes.iter_mut()) {
		bounds.y += shift_of(node.rank);
	}
	for (rank, label) in label_ranks.iter().zip(labels.iter_mut()) {
		if let (Some(rank), Some(label)) = (rank, label) {
			label.y += shift_of(*rank);
		}
	}

	// The points of the routes.
	let mut routes = Vec::with_capacity(layout.edges.len());
	for (index, edge) in layout.edges.iter().enumerate() {
		let Some(list) = &stations[index] else {
			if edge.source != edge.target {
				routes.push(Route::default());
				continue;
			}
			let points = self_loop(layout, &nodes, index);
			// The label of a self loop sits outside the loop.
			let reach = points.iter().map(|point| point.x).fold(f64::INFINITY, f64::min);
			let label = labels[index].zip(nodes.get(edge.source)).map(|(label, node)| {
				let center = Point { x: reach - label.width / 2.0, y: node.y };
				labels[index] = Some(Rect { x: center.x, y: center.y, ..label });
				frame.point_out(center)
			});
			routes.push(Route {
				points: points.into_iter().map(|point| frame.point_out(point)).collect(),
				label,
			});
			continue;
		};
		let label = labels[index].map(|label| frame.point_out(Point { x: label.x, y: label.y }));
		let attach = |station: &Station, toward: &Station| -> f64 {
			let below = toward.rank > station.rank;
			match station.kind {
				StationKind::Port { node } => {
					let bounds = nodes[node];
					if below { bounds.bottom() } else { bounds.top() }
				}
				StationKind::Pass => levels
					.get(&station.rank)
					.map_or(0.0, |level| (level.top + level.bottom) / 2.0 + level.shift),
			}
		};
		let mut points = Vec::with_capacity(list.len() * 2);
		points.push(Point { x: list[0].x, y: attach(&list[0], &list[1]) });
		for station in 0..list.len() - 1 {
			let (a, b) = (list[station], list[station + 1]);
			let track = tracks[index][station];
			if track.is_finite() {
				points.push(Point { x: a.x, y: track });
				points.push(Point { x: b.x, y: track });
			}
		}
		let last = list.len() - 1;
		points.push(Point { x: list[last].x, y: attach(&list[last], &list[last - 1]) });
		routes.push(Route {
			points: simplify(points).into_iter().map(|point| frame.point_out(point)).collect(),
			label,
		});
	}

	for (node, bounds) in layout.nodes.iter_mut().zip(&nodes) {
		node.bounds = frame.rect_out(*bounds);
	}
	for (edge, label) in layout.edges.iter_mut().zip(&labels) {
		edge.label = label.map(|label| frame.rect_out(label));
	}
	routes
}

fn finite_or_zero(value: f64) -> f64 {
	if value.is_finite() { value } else { 0.0 }
}

/// Moves ports onto the line of the station next to them where the order and spacing of the
/// ports on their side allow it, so that short jogs become straight edges.
fn straighten_ports(stations: &mut [Option<Vec<Station>>], sides: &PortSides, nodes: &[Rect]) {
	for ((node, _), ends) in sides {
		let bounds = nodes[*node];
		for (order, &(_, edge, position)) in ends.iter().enumerate() {
			let Some(list) = stations[edge].as_ref() else { continue };
			let neighbor = if position == 0 { 1 } else { list.len() - 2 };
			let target = list[neighbor];
			// A port facing a port of another node moves only if that one stays.
			if matches!(target.kind, StationKind::Port { .. }) && target.x != list[position].x {
				let other_fits = |x: f64| {
					let StationKind::Port { node: other } = target.kind else { return false };
					let other = nodes[other];
					x >= other.left() + PORT_MARGIN && x <= other.right() - PORT_MARGIN
				};
				if other_fits(list[position].x) {
					continue;
				}
			}
			let x = target.x;
			if x < bounds.left() + PORT_MARGIN.min(bounds.width / 2.0)
				|| x > bounds.right() - PORT_MARGIN.min(bounds.width / 2.0)
			{
				continue;
			}
			let port_x = |order: usize| -> Option<f64> {
				let &(_, edge, position) = ends.get(order)?;
				stations[edge].as_ref().map(|list| list[position].x)
			};
			let fits_before =
				order == 0 || port_x(order - 1).is_none_or(|before| x - before >= MIN_PORT_GAP);
			let fits_after = port_x(order + 1).is_none_or(|after| after - x >= MIN_PORT_GAP);
			if fits_before
				&& fits_after
				&& let Some(list) = stations[edge].as_mut()
			{
				list[position].x = x;
			}
		}
	}
	// Ports facing ports that did not move: align the other end.
	for ((node, _), ends) in sides {
		let bounds = nodes[*node];
		for (order, &(_, edge, position)) in ends.iter().enumerate() {
			let Some(list) = stations[edge].as_ref() else { continue };
			let neighbor = if position == 0 { 1 } else { list.len() - 2 };
			let target = list[neighbor];
			let StationKind::Port { .. } = target.kind else { continue };
			let x = target.x;
			if (x - list[position].x).abs() <= EPSILON
				|| x < bounds.left() + PORT_MARGIN.min(bounds.width / 2.0)
				|| x > bounds.right() - PORT_MARGIN.min(bounds.width / 2.0)
			{
				continue;
			}
			let port_x = |order: usize| -> Option<f64> {
				let &(_, edge, position) = ends.get(order)?;
				stations[edge].as_ref().map(|list| list[position].x)
			};
			let fits_before =
				order == 0 || port_x(order - 1).is_none_or(|before| x - before >= MIN_PORT_GAP);
			let fits_after = port_x(order + 1).is_none_or(|after| after - x >= MIN_PORT_GAP);
			if fits_before
				&& fits_after
				&& let Some(list) = stations[edge].as_mut()
			{
				list[position].x = x;
			}
		}
	}
}

/// Orders the jogs of a gap into tracks: the top track is 0. Jogs whose runs overlap get
/// different tracks, in the order with the fewest crossings found.
fn assign_tracks(mut jogs: Vec<Jog>) -> Vec<(Jog, usize)> {
	// Rightward jogs with the larger x on top, leftward ones with the smaller x: then parallel
	// jogs do not cross.
	npp::sort::sort_by(&mut jogs[..], |a, b| {
		let key = |jog: &Jog| match jog.x_bottom > jog.x_top {
			true => -jog.x_top,
			false => jog.x_top,
		};
		key(a).total_cmp(&key(b)).then(a.edge.cmp(&b.edge))
	});
	// Swap neighbors in the order while that saves crossings, a bounded number of times.
	let rounds = jogs.len().min(32);
	for _ in 0..rounds {
		let mut changed = false;
		for position in 1..jogs.len() {
			let (above, below) = (&jogs[position - 1], &jogs[position]);
			if above.overlaps(below) && below.cost_above(above) < above.cost_above(below) {
				jogs.swap(position - 1, position);
				changed = true;
			}
		}
		if !changed {
			break;
		}
	}
	let mut assigned: Vec<(Jog, usize)> = Vec::with_capacity(jogs.len());
	for jog in jogs {
		let track = assigned
			.iter()
			.filter(|(other, _)| other.overlaps(&jog))
			.map(|(_, track)| track + 1)
			.max()
			.unwrap_or(0);
		assigned.push((jog, track));
	}
	assigned
}

/// The route of a self loop in the internal frame: out of the left side of its node and back.
fn self_loop(layout: &Layout, nodes: &[Rect], edge: usize) -> Vec<Point> {
	let Some(node) = layout.edges.get(edge).map(|edge| edge.source) else { return Vec::new() };
	let Some(bounds) = nodes.get(node) else { return Vec::new() };
	// Loops of the same node nest: the later ones reach further.
	let order = layout.edges[..edge]
		.iter()
		.filter(|other| other.source == node && other.target == node)
		.count();
	let reach = SELF_LOOP_REACH * (order + 1) as f64;
	let half_height = SELF_LOOP_HALF_HEIGHT * (order + 1) as f64;
	let left = bounds.left();
	vec![
		Point { x: left, y: bounds.y - half_height },
		Point { x: left - reach, y: bounds.y - half_height },
		Point { x: left - reach, y: bounds.y + half_height },
		Point { x: left, y: bounds.y + half_height },
	]
}

/// Drops repeated points and the middle of three points on one line.
fn simplify(points: Vec<Point>) -> Vec<Point> {
	let mut result: Vec<Point> = Vec::with_capacity(points.len());
	for point in points {
		if result.last().is_some_and(|last| {
			(last.x - point.x).abs() <= EPSILON && (last.y - point.y).abs() <= EPSILON
		}) {
			continue;
		}
		if result.len() >= 2 {
			let (a, b) = (result[result.len() - 2], result[result.len() - 1]);
			let collinear_x = (a.x - b.x).abs() <= EPSILON && (b.x - point.x).abs() <= EPSILON;
			let collinear_y = (a.y - b.y).abs() <= EPSILON && (b.y - point.y).abs() <= EPSILON;
			if collinear_x || collinear_y {
				result.pop();
			}
		}
		result.push(point);
	}
	result
}

#[cfg(test)]
mod tests;
