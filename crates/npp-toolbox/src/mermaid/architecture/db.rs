//! The architecture database (`architectureDb.ts`, `architectureTypes.ts`) and the constraints
//! the renderer derives from it (`getAlignments`, `getRelativeConstraints` of
//! `architectureRenderer.ts`).
//!
//! JavaScript keeps ids and positions in `Map`s, iterated in insertion order; positions are
//! overwritten in place when the BFS reaches a node twice.

use std::collections::{HashMap, HashSet, VecDeque};

use super::super::color::js_number;
use super::super::{Context, Result};
use super::parser::{Architecture, Direction, Edge, Group, Junction, Service};

/// A service or junction.
#[derive(Clone, Debug)]
pub struct Node {
	pub id: String,
	pub is_junction: bool,
	pub icon: Option<String>,
	pub icon_text: Option<String>,
	pub title: Option<String>,
	pub parent: Option<String>,
	/// The edges at the node, in order; a loop is listed twice.
	pub edges: Vec<usize>,
}

pub struct Db {
	pub groups: Vec<Group>,
	/// Services, then junctions.
	pub nodes: Vec<Node>,
	pub edges: Vec<Edge>,
	pub hints: Vec<(bool, Vec<String>)>,
	node_index: HashMap<String, usize>,
	group_index: HashMap<String, usize>,
}

/// A relative placement in ids: `second` right of (or below) `first` by `gap`.
#[derive(Clone, Debug, PartialEq)]
pub struct Relative {
	pub first: String,
	pub second: String,
	pub is_horizontal: bool,
	pub gap: f64,
}

/// The constraints of the fcose layout in ids.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Constraints {
	pub horizontal: Vec<Vec<String>>,
	pub vertical: Vec<Vec<String>>,
	pub relative: Vec<Relative>,
}

/// `ArchitectureSpatialMap`: grid positions by id, `+y` up.
type SpatialMap = Vec<(String, (i64, i64))>;

/// Grid positions by node index.
type NodePositions = Vec<(usize, (i64, i64))>;

/// The ids of a row or column by group.
type GroupedIds = Vec<(String, Vec<String>)>;

/// The order of the BFS over relative positions: `Object.entries` of `{L, R, T, B}`.
const BFS_DIRECTIONS: [(Direction, (i64, i64)); 4] = [
	(Direction::Left, (-1, 0)),
	(Direction::Right, (1, 0)),
	(Direction::Top, (0, 1)),
	(Direction::Bottom, (0, -1)),
];

/// The number of nodes up to which Mermaid's spatial map BFS, which queues nodes again, is
/// followed exactly.
const MAX_QUEUED_NODES: usize = 100_000;

/// The number of relative constraints up to which Mermaid's BFS, which repeats them, is
/// followed exactly.
const MAX_RELATIVE_CONSTRAINTS: usize = 5_000;

impl Db {
	/// `populateDb`: the groups, services, junctions, edges and alignments in this order, each
	/// checked like `ArchitectureDB`.
	pub fn new(architecture: Architecture, context: &Context) -> Result<Db> {
		let mut db = Db {
			groups: Vec::new(),
			nodes: Vec::new(),
			edges: Vec::new(),
			hints: Vec::new(),
			node_index: HashMap::new(),
			group_index: HashMap::new(),
		};
		for group in architecture.groups {
			db.check_new(&group.id, group.parent.as_deref(), "group", group.line, context)?;
			db.group_index.insert(group.id.clone(), db.groups.len());
			db.groups.push(group);
		}
		for Service { id, icon, icon_text, title, parent, line } in architecture.services {
			db.check_new(&id, parent.as_deref(), "service", line, context)?;
			db.add_node(Node {
				id,
				is_junction: false,
				icon,
				icon_text,
				title,
				parent,
				edges: Vec::new(),
			});
		}
		for Junction { id, parent, line } in architecture.junctions {
			db.check_new(&id, parent.as_deref(), "junction", line, context)?;
			db.add_node(Node {
				id,
				is_junction: true,
				icon: None,
				icon_text: None,
				title: None,
				parent,
				edges: Vec::new(),
			});
		}
		for edge in architecture.edges {
			db.add_edge(edge, context)?;
		}
		for alignment in architecture.alignments {
			let direction = if alignment.is_row { "row" } else { "column" };
			let mut seen = HashSet::new();
			for id in &alignment.members {
				if !db.node_index.contains_key(id) {
					return Err(context.error_at(
						alignment.line,
						format!(
							"align {direction} references [{id}], which is not a service or junction"
						),
					));
				}
				if !seen.insert(id.as_str()) {
					return Err(context.error_at(
						alignment.line,
						format!("align {direction} lists [{id}] more than once"),
					));
				}
			}
			db.hints.push((alignment.is_row, alignment.members));
		}
		Ok(db)
	}

	fn registered(&self, id: &str) -> Option<&'static str> {
		if self.node_index.contains_key(id) {
			Some("node")
		} else if self.group_index.contains_key(id) {
			Some("group")
		} else {
			None
		}
	}

	/// The checks of `addService`, `addJunction` and `addGroup`.
	fn check_new(
		&self,
		id: &str,
		parent: Option<&str>,
		kind: &str,
		line: usize,
		context: &Context,
	) -> Result<()> {
		if let Some(registered) = self.registered(id) {
			return Err(context.error_at(
				line,
				format!("The {kind} id [{id}] is already in use by another {registered}"),
			));
		}
		if let Some(parent) = parent {
			let message = match self.registered(parent) {
				_ if id == parent => format!("The {kind} [{id}] cannot be placed within itself"),
				None => format!(
					"The {kind} [{id}]'s parent does not exist. Please make sure the parent is created before this {kind}"
				),
				Some("node") => format!("The {kind} [{id}]'s parent is not a group"),
				Some(_) => return Ok(()),
			};
			return Err(context.error_at(line, message));
		}
		Ok(())
	}

	fn add_node(&mut self, node: Node) {
		self.node_index.insert(node.id.clone(), self.nodes.len());
		self.nodes.push(node);
	}

	/// `addEdge`.
	fn add_edge(&mut self, edge: Edge, context: &Context) -> Result<()> {
		let line = edge.line;
		for (id, side) in [(&edge.lhs_id, "left"), (&edge.rhs_id, "right")] {
			if self.registered(id).is_none() {
				return Err(context.error_at(
					line,
					format!(
						"The {side}-hand id [{id}] does not yet exist. Please create the service/group before declaring an edge to it."
					),
				));
			}
		}
		// Mermaid reads the group of each end from its node, which a group is not.
		let (Some(&lhs), Some(&rhs)) =
			(self.node_index.get(&edge.lhs_id), self.node_index.get(&edge.rhs_id))
		else {
			return Err(
				context.error_at(line, "Cannot read properties of undefined (reading 'in')")
			);
		};
		let (lhs_group, rhs_group) = (&self.nodes[lhs].parent, &self.nodes[rhs].parent);
		let same_group = lhs_group.is_some() && lhs_group == rhs_group;
		for (modified, id, side) in
			[(edge.lhs_group, &edge.lhs_id, "left"), (edge.rhs_group, &edge.rhs_id, "right")]
		{
			if modified && same_group {
				return Err(context.error_at(
					line,
					format!(
						"The {side}-hand id [{id}] is modified to traverse the group boundary, but the edge does not pass through two groups."
					),
				));
			}
		}
		let index = self.edges.len();
		self.edges.push(edge);
		self.nodes[lhs].edges.push(index);
		self.nodes[rhs].edges.push(index);
		Ok(())
	}

	pub fn node(&self, id: &str) -> Option<&Node> {
		self.node_index.get(id).map(|&index| &self.nodes[index])
	}

	pub fn node_position(&self, id: &str) -> Option<usize> {
		self.node_index.get(id).copied()
	}

	pub fn group_position(&self, id: &str) -> Option<usize> {
		self.group_index.get(id).copied()
	}

	/// `getDataStructures`: the spatial maps of the connected parts and the alignments of
	/// groups that edges connect.
	fn data_structures(&self) -> (Vec<SpatialMap>, HashMap<(String, String), bool>) {
		let mut group_alignments: HashMap<(String, String), bool> = HashMap::new();
		// Per node: direction pair -> other end, in insertion order.
		let mut adjacency: Vec<Vec<((Direction, Direction), usize)>> = Vec::new();
		for (index, node) in self.nodes.iter().enumerate() {
			let mut directions: Vec<((Direction, Direction), usize)> = Vec::new();
			for &edge in &node.edges {
				let edge = &self.edges[edge];
				let lhs_group = self.node(&edge.lhs_id).and_then(|node| node.parent.as_ref());
				let rhs_group = self.node(&edge.rhs_id).and_then(|node| node.parent.as_ref());
				if let (Some(lhs_group), Some(rhs_group)) = (lhs_group, rhs_group)
					&& lhs_group != rhs_group
					&& edge.lhs_dir.is_x() == edge.rhs_dir.is_x()
				{
					// `true` for horizontal.
					group_alignments.insert(group_key(lhs_group, rhs_group), edge.lhs_dir.is_x());
				}
				let (pair, other) = match edge.lhs_id == node.id {
					true => ((edge.lhs_dir, edge.rhs_dir), &edge.rhs_id),
					false => ((edge.rhs_dir, edge.lhs_dir), &edge.lhs_id),
				};
				if pair.0 == pair.1 {
					continue;
				}
				let Some(other) = self.node_position(other) else { continue };
				match directions.iter_mut().find(|(existing, _)| *existing == pair) {
					Some(entry) => entry.1 = other,
					None => directions.push((pair, other)),
				}
			}
			debug_assert_eq!(adjacency.len(), index);
			adjacency.push(directions);
		}
		let spatial_maps = spatial_maps(&adjacency, true)
			.unwrap_or_else(|| spatial_maps(&adjacency, false).unwrap_or_default());
		let spatial_maps = spatial_maps
			.into_iter()
			.map(|positions| {
				positions
					.into_iter()
					.map(|(node, position)| (self.nodes[node].id.clone(), position))
					.collect()
			})
			.collect();
		(spatial_maps, group_alignments)
	}

	/// The alignment and relative placement constraints of fcose.
	pub fn constraints(&self, icon_size: f64, multiplier: f64) -> Constraints {
		let (spatial_maps, group_alignments) = self.data_structures();
		let mut constraints = self.alignments(&spatial_maps, &group_alignments);
		constraints.relative = self.relative_constraints(&spatial_maps, multiplier * icon_size);
		constraints
	}

	/// `getAlignments`.
	fn alignments(
		&self,
		spatial_maps: &[SpatialMap],
		group_alignments: &HashMap<(String, String), bool>,
	) -> Constraints {
		let (mut horizontal, mut vertical) = (Vec::new(), Vec::new());
		for spatial_map in spatial_maps {
			// Coordinate -> group -> ids, in insertion order.
			type Axis = Vec<(i64, Vec<(String, Vec<String>)>)>;
			let mut rows: Axis = Vec::new();
			let mut columns: Axis = Vec::new();
			for (id, (x, y)) in spatial_map {
				let group = self
					.node(id)
					.and_then(|node| node.parent.clone())
					.unwrap_or_else(|| "default".to_owned());
				for (axis, coordinate) in [(&mut rows, *y), (&mut columns, *x)] {
					let index = match axis.iter().position(|(c, _)| *c == coordinate) {
						Some(index) => index,
						None => {
							axis.push((coordinate, Vec::new()));
							axis.len() - 1
						}
					};
					let groups = &mut axis[index].1;
					match groups.iter_mut().find(|(g, _)| *g == group) {
						Some((_, ids)) => ids.push(id.clone()),
						None => groups.push((group.clone(), vec![id.clone()])),
					}
				}
			}
			for (axis, is_horizontal, target) in
				[(&rows, true, &mut horizontal), (&columns, false, &mut vertical)]
			{
				let flattened = flatten_alignments(axis, is_horizontal, group_alignments);
				target.extend(flattened.into_iter().filter(|ids| ids.len() > 1));
			}
		}
		let declared: HashSet<&String> = self.hints.iter().flat_map(|(_, ids)| ids).collect();
		let keep = |lists: Vec<Vec<String>>| -> Vec<Vec<String>> {
			lists.into_iter().filter(|ids| !ids.iter().any(|id| declared.contains(id))).collect()
		};
		let mut constraints = Constraints {
			horizontal: keep(horizontal),
			vertical: keep(vertical),
			relative: Vec::new(),
		};
		for (is_row, members) in &self.hints {
			match is_row {
				true => constraints.horizontal.push(members.clone()),
				false => constraints.vertical.push(members.clone()),
			}
		}
		constraints
	}

	/// `getRelativeConstraints`.
	fn relative_constraints(&self, spatial_maps: &[SpatialMap], gap: f64) -> Vec<Relative> {
		let mut constraints = Vec::new();
		let mut declared: HashSet<(&str, &str)> = HashSet::new();
		for (is_row, members) in &self.hints {
			for pair in members.windows(2) {
				let (a, b) = (&pair[0], &pair[1]);
				declared.insert((a, b));
				declared.insert((b, a));
				constraints.push(Relative {
					first: a.clone(),
					second: b.clone(),
					is_horizontal: *is_row,
					gap,
				});
			}
		}
		let mut spatial = Vec::new();
		let complete = spatial_maps
			.iter()
			.all(|map| neighbor_constraints(map, &declared, gap, true, &mut spatial));
		if !complete {
			spatial.clear();
			for map in spatial_maps {
				neighbor_constraints(map, &declared, gap, false, &mut spatial);
			}
		}
		constraints.extend(spatial);
		constraints
	}
}

/// The BFS of `getRelativeConstraints` over the positions of a spatial map: a constraint
/// between every position and its unvisited neighbors. Mermaid marks positions as visited when
/// it takes them from the queue, so it queues them again from every neighbor before and
/// repeats their constraints, which grows with the number of paths through a grid. Returns
/// false when `revisits` makes more than [`MAX_RELATIVE_CONSTRAINTS`]; without `revisits`
/// every position is queued once.
fn neighbor_constraints(
	spatial_map: &SpatialMap,
	declared: &HashSet<(&str, &str)>,
	gap: f64,
	revisits: bool,
	constraints: &mut Vec<Relative>,
) -> bool {
	// The last id at a position wins.
	let mut inverse: HashMap<(i64, i64), &str> = HashMap::new();
	for (id, position) in spatial_map {
		inverse.insert(*position, id);
	}
	let mut queue = VecDeque::from([(0, 0)]);
	let mut visited: HashSet<(i64, i64)> = HashSet::from([(0, 0)]);
	let mut dequeued: HashSet<(i64, i64)> = HashSet::new();
	while let Some(current) = queue.pop_front() {
		if constraints.len() > MAX_RELATIVE_CONSTRAINTS {
			return false;
		}
		dequeued.insert(current);
		let Some(&current_id) = inverse.get(&current) else { continue };
		for (direction, (dx, dy)) in BFS_DIRECTIONS {
			let position = (current.0 + dx, current.1 + dy);
			let Some(&new_id) = inverse.get(&position) else { continue };
			let seen = if revisits { &dequeued } else { &visited };
			if seen.contains(&position) {
				continue;
			}
			visited.insert(position);
			queue.push_back(position);
			if declared.contains(&(current_id, new_id)) {
				continue;
			}
			// `{[direction]: newId, [opposite]: currId}`.
			let (first, second) = match direction {
				Direction::Left | Direction::Top => (new_id, current_id),
				Direction::Right | Direction::Bottom => (current_id, new_id),
			};
			constraints.push(Relative {
				first: first.to_owned(),
				second: second.to_owned(),
				is_horizontal: direction.is_x(),
				gap,
			});
		}
	}
	true
}

/// The BFS of `getDataStructures`: positions of the nodes of each connected part. Mermaid marks
/// nodes as visited when it takes them from the queue, so it queues them again from every
/// neighbor before, which grows with the number of paths through a grid; `None` when
/// `revisits` queues more than [`MAX_QUEUED_NODES`]. Without `revisits` every node is queued
/// once.
fn spatial_maps(
	adjacency: &[Vec<((Direction, Direction), usize)>],
	revisits: bool,
) -> Option<Vec<NodePositions>> {
	let mut visited = vec![false; adjacency.len()];
	let mut queued = vec![false; adjacency.len()];
	let mut total = 0;
	let mut maps = Vec::new();
	while let Some(start) = visited.iter().position(|&visited| !visited) {
		let mut positions: Vec<(usize, (i64, i64))> = vec![(start, (0, 0))];
		let mut position_of: HashMap<usize, usize> = HashMap::from([(start, 0)]);
		let mut queue = VecDeque::from([start]);
		queued[start] = true;
		while let Some(id) = queue.pop_front() {
			visited[id] = true;
			let (x, y) = positions[position_of[&id]].1;
			for &(pair, other) in &adjacency[id] {
				if visited[other] || (!revisits && queued[other]) {
					continue;
				}
				let shifted = shift_position((x, y), pair);
				match position_of.get(&other) {
					Some(&slot) => positions[slot].1 = shifted,
					None => {
						position_of.insert(other, positions.len());
						positions.push((other, shifted));
					}
				}
				queued[other] = true;
				queue.push_back(other);
				total += 1;
				if total > MAX_QUEUED_NODES {
					return None;
				}
			}
		}
		maps.push(positions);
	}
	Some(maps)
}

/// `architectureGroupAlignmentKey`: the two groups in order.
fn group_key(a: &str, b: &str) -> (String, String) {
	let utf16 = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
	match utf16(a) <= utf16(b) {
		true => (a.to_owned(), b.to_owned()),
		false => (b.to_owned(), a.to_owned()),
	}
}

/// `shiftPositionByArchitectureDirectionPair`.
fn shift_position((x, y): (i64, i64), (lhs, rhs): (Direction, Direction)) -> (i64, i64) {
	let horizontal = |direction: Direction| if direction == Direction::Left { -1 } else { 1 };
	let vertical = |direction: Direction| if direction == Direction::Top { 1 } else { -1 };
	match (lhs.is_x(), rhs.is_x()) {
		(true, false) => (x + horizontal(lhs), y + vertical(rhs)),
		(true, true) => (x + horizontal(lhs), y),
		(false, true) => (x - horizontal(rhs), y + vertical(lhs)),
		(false, false) => (x, y + vertical(lhs)),
	}
}

/// `flattenAlignments`: the groups of one row or column share an alignment list when edges
/// align them that way or one of them is no group; otherwise each pair gets lists of its own.
fn flatten_alignments(
	axis: &[(i64, GroupedIds)],
	is_horizontal: bool,
	group_alignments: &HashMap<(String, String), bool>,
) -> Vec<Vec<String>> {
	let mut flattened: Vec<(String, Vec<String>)> = Vec::new();
	let mut set =
		|key: String, ids: Vec<String>| match flattened.iter_mut().find(|(k, _)| *k == key) {
			Some(entry) => entry.1 = ids,
			None => flattened.push((key, ids)),
		};
	let mut current: HashMap<String, Vec<String>> = HashMap::new();
	for (coordinate, groups) in axis {
		let key = js_number(*coordinate as f64);
		if groups.len() == 1 {
			current.insert(key.clone(), groups[0].1.clone());
			set(key, groups[0].1.clone());
			continue;
		}
		let mut count = 0;
		for i in 0..groups.len() - 1 {
			for j in i + 1..groups.len() {
				let (a_group, a_ids) = &groups[i];
				let (b_group, b_ids) = &groups[j];
				let alignment = group_alignments.get(&group_key(a_group, b_group));
				if alignment == Some(&is_horizontal) || a_group == "default" || b_group == "default"
				{
					let mut ids = current.get(&key).cloned().unwrap_or_default();
					ids.extend(a_ids.iter().cloned());
					ids.extend(b_ids.iter().cloned());
					current.insert(key.clone(), ids.clone());
					set(key.clone(), ids);
				} else {
					for ids in [a_ids, b_ids] {
						let pair_key = format!("{key}-{count}");
						count += 1;
						current.insert(pair_key.clone(), ids.clone());
						set(pair_key, ids.clone());
					}
				}
			}
		}
	}
	flattened.into_iter().map(|(_, ids)| ids).collect()
}

#[cfg(test)]
mod tests {
	use super::super::super::text::testing::FixedMeasure;
	use super::super::parser::parse;
	use super::*;

	fn db(code: &str) -> Result<Db> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		Db::new(parse(code, &context)?, &context)
	}

	fn message(code: &str) -> String {
		db(code).err().map(|error| error.message).unwrap_or_default()
	}

	#[test]
	fn reports_mermaids_errors() {
		assert_eq!(
			message("architecture-beta\n  service a(server)\n  junction a\n"),
			"The junction id [a] is already in use by another node"
		);
		assert_eq!(
			message("architecture-beta\n  service a(server) in g\n"),
			"The service [a]'s parent does not exist. Please make sure the parent is created before this service"
		);
		assert_eq!(
			message("architecture-beta\n  group g\n  service a(server)\n  a:R -- L:g\n"),
			"Cannot read properties of undefined (reading 'in')"
		);
		assert_eq!(
			message(
				"architecture-beta\n  group g\n  service a in g\n  service b in g\n  a{group}:R -- L:b\n"
			),
			"The left-hand id [a] is modified to traverse the group boundary, but the edge does not pass through two groups."
		);
		assert_eq!(
			message("architecture-beta\n  service a\n  service b\n  align row a b a\n"),
			"align row lists [a] more than once"
		);
	}

	/// `a` keeps one neighbor per direction pair, the last: `d` replaces `b`, which starts a
	/// spatial map of its own.
	#[test]
	fn derives_constraints_from_the_spatial_map() {
		let db = db("architecture-beta\n  service a\n  service b\n  service c\n  service d\n  a:R -- L:b\n  b:B -- T:c\n  d:L -- R:a\n")
			.unwrap();
		let constraints = db.constraints(80.0, 1.5);
		let ids =
			|lists: &[Vec<String>]| lists.iter().map(|list| list.join(" ")).collect::<Vec<_>>();
		assert_eq!(ids(&constraints.horizontal), ["a d"]);
		assert_eq!(ids(&constraints.vertical), ["b c"]);
		let relative: Vec<(String, String, bool)> = constraints
			.relative
			.iter()
			.map(|r| (r.first.clone(), r.second.clone(), r.is_horizontal))
			.collect();
		assert_eq!(
			relative,
			[("a".to_owned(), "d".to_owned(), true), ("b".to_owned(), "c".to_owned(), false)]
		);
	}
}
