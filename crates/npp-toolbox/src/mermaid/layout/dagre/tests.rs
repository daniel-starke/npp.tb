use super::super::super::value::{Value, parse_json};
use super::*;

#[derive(Debug)]
struct TestNode {
	width: f64,
	height: f64,
	x: f64,
	y: f64,
}

impl LayoutNode for TestNode {
	fn size(&self) -> (f64, f64) {
		(self.width, self.height)
	}

	fn set_position(&mut self, x: f64, y: f64) {
		self.x = x;
		self.y = y;
	}

	fn set_size(&mut self, width: f64, height: f64) {
		self.width = width;
		self.height = height;
	}
}

struct TestEdge {
	attributes: EdgeAttributes,
	points: Vec<Point>,
	label: Option<Point>,
}

impl LayoutEdge for TestEdge {
	fn attributes(&self) -> EdgeAttributes {
		self.attributes
	}

	fn set_layout(&mut self, points: Vec<Point>, label: Option<Point>) {
		self.points = points;
		self.label = label;
	}
}

fn number(value: &Value, key: &str) -> f64 {
	value.get(key).and_then(Value::as_number).unwrap_or(f64::NAN)
}

fn close(actual: f64, expected: f64) -> bool {
	(actual.is_nan() && expected.is_nan()) || (actual - expected).abs() < 1e-6
}

/// Lays out the input of a fixture line and compares with dagre-d3-es's result. Returns a
/// description of the first difference.
fn check_fixture(line: &str) -> Option<String> {
	let fixture = parse_json(line)?;
	let input = fixture.get("input")?;
	let expected = fixture.get("result")?;
	if expected.get("error").is_some() {
		return None;
	}
	let mut keys = Keys::default();
	let mut graph: Graph<TestNode, TestEdge> = Graph::new(true, true);
	let settings = input.get("graph")?;
	graph.label.rankdir = RankDir::parse(settings.get("rankdir")?.as_str()?);
	graph.label.nodesep = number(settings, "nodesep");
	graph.label.ranksep = number(settings, "ranksep");
	graph.label.edgesep = number(settings, "edgesep");
	graph.label.marginx = number(settings, "marginx");
	graph.label.marginy = number(settings, "marginy");
	let Value::Array(nodes) = input.get("nodes")? else { return None };
	for node in nodes {
		let v = keys.intern(node.get("id")?.as_str()?);
		let size = |key| node.get(key).and_then(Value::as_number).unwrap_or(0.0);
		graph
			.set_node(v, TestNode { width: size("width"), height: size("height"), x: 0.0, y: 0.0 });
	}
	for node in nodes {
		if let Some(parent) = node.get("parent").and_then(Value::as_str) {
			let v = keys.intern(node.get("id")?.as_str()?);
			let parent = keys.intern(parent);
			graph.set_parent(v, Some(parent));
		}
	}
	let Value::Array(edges) = input.get("edges")? else { return None };
	for edge in edges {
		let v = keys.intern(edge.get("v")?.as_str()?);
		let w = keys.intern(edge.get("w")?.as_str()?);
		let name = keys.intern(edge.get("name")?.as_str()?);
		let attributes = EdgeAttributes {
			minlen: number(edge, "minlen"),
			weight: number(edge, "weight"),
			width: number(edge, "width"),
			height: number(edge, "height"),
			labeloffset: number(edge, "labeloffset"),
			labelpos: LabelPos::parse(edge.get("labelpos")?.as_str()?),
		};
		graph.set_edge(v, w, Some(name), TestEdge { attributes, points: Vec::new(), label: None });
	}
	keys.set_unique_counter(number(&fixture, "counter") as u64);
	layout(&mut graph, &mut keys);

	if !close(graph.label.width, number(expected, "width"))
		|| !close(graph.label.height, number(expected, "height"))
	{
		return Some(format!(
			"size {}x{} instead of {}x{}",
			graph.label.width,
			graph.label.height,
			number(expected, "width"),
			number(expected, "height")
		));
	}
	let expected_nodes = expected.get("nodes")?;
	for v in graph.nodes() {
		let node = graph.node(v)?;
		let expected = expected_nodes.get(keys.name(v))?;
		for (actual, key) in
			[(node.x, "x"), (node.y, "y"), (node.width, "width"), (node.height, "height")]
		{
			if !close(actual, number(expected, key)) {
				return Some(format!(
					"node {} {key} {actual} instead of {}",
					keys.name(v),
					number(expected, key)
				));
			}
		}
	}
	let Value::Array(expected_edges) = expected.get("edges")? else { return None };
	for expected in expected_edges {
		let edge = Edge {
			v: keys.get(expected.get("v")?.as_str()?)?,
			w: keys.get(expected.get("w")?.as_str()?)?,
			name: keys.get(expected.get("name")?.as_str()?),
		};
		let actual = graph.edge(edge)?;
		let Value::Array(points) = expected.get("points")? else { return None };
		let matches = actual.points.len() == points.len()
			&& actual.points.iter().zip(points).all(|(point, expected)| {
				close(point.x, number(expected, "x")) && close(point.y, number(expected, "y"))
			});
		let label_matches = match (actual.label, expected.get("x")) {
			(Some(label), Some(_)) => {
				close(label.x, number(expected, "x")) && close(label.y, number(expected, "y"))
			}
			(None, None) => true,
			_ => false,
		};
		if !matches || !label_matches {
			return Some(format!(
				"edge {}->{} points {:?} label {:?}, expected {:?}",
				keys.name(edge.v),
				keys.name(edge.w),
				actual.points,
				actual.label,
				expected
			));
		}
	}
	None
}

#[test]
fn lays_out_like_dagre() {
	for (index, line) in include_str!("fixtures.jsonl").lines().enumerate() {
		if let Some(difference) = check_fixture(line) {
			panic!("fixture {index}: {difference}");
		}
	}
}

/// `DAGRE_FIXTURES=<file> cargo test -p npp-toolbox dagre_fixtures -- --ignored` compares with
/// the output of `dagre-fixtures.mjs` (see `fixtures.jsonl`).
#[test]
#[ignore]
fn dagre_fixtures() {
	let path = std::env::var("DAGRE_FIXTURES").expect("DAGRE_FIXTURES");
	let text = std::fs::read_to_string(path).expect("fixture file");
	let mut failures = 0;
	for (index, line) in text.lines().enumerate() {
		if let Some(difference) = check_fixture(line) {
			failures += 1;
			if failures <= 10 {
				eprintln!("fixture {index}: {difference}");
			}
		}
	}
	assert_eq!(failures, 0, "{failures} of {} fixtures differ", text.lines().count());
}
