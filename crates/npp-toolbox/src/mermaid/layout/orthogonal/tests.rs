use super::*;

fn node(x: f64, y: f64, width: f64, height: f64, rank: i64) -> Node {
	Node { bounds: Rect { x, y, width, height }, rank }
}

fn edge(source: usize, target: usize, waypoints: &[(f64, f64)]) -> Edge {
	Edge {
		source,
		target,
		waypoints: waypoints.iter().map(|&(x, y)| Point { x, y }).collect(),
		..Edge::default()
	}
}

fn points(route: &Route) -> Vec<(f64, f64)> {
	route.points.iter().map(|point| (point.x, point.y)).collect()
}

#[test]
fn routes_a_chain_straight() {
	let mut layout = Layout {
		nodes: vec![node(100.0, 20.0, 100.0, 40.0, 0), node(100.0, 100.0, 100.0, 40.0, 2)],
		edges: vec![edge(0, 1, &[(100.0, 60.0)])],
		layer_spacing: 40.0,
		..Layout::default()
	};
	let routes = route(&mut layout);
	assert_eq!(points(&routes[0]), [(100.0, 40.0), (100.0, 80.0)]);
}

#[test]
fn spreads_ports_and_turns_in_a_track() {
	let mut layout = Layout {
		nodes: vec![
			node(200.0, 20.0, 152.0, 40.0, 0),
			node(40.0, 100.0, 60.0, 40.0, 2),
			node(200.0, 100.0, 60.0, 40.0, 2),
			node(360.0, 100.0, 60.0, 40.0, 2),
		],
		edges: vec![
			edge(0, 1, &[(40.0, 60.0)]),
			edge(0, 2, &[(200.0, 60.0)]),
			edge(0, 3, &[(360.0, 60.0)]),
		],
		layer_spacing: 40.0,
		..Layout::default()
	};
	let routes = route(&mut layout);
	assert_eq!(points(&routes[0]), [(167.0, 40.0), (167.0, 60.0), (40.0, 60.0), (40.0, 80.0)]);
	assert_eq!(points(&routes[1]), [(200.0, 40.0), (200.0, 80.0)]);
	assert_eq!(points(&routes[2]), [(233.0, 40.0), (233.0, 60.0), (360.0, 60.0), (360.0, 80.0)]);
}

#[test]
fn grows_gaps_for_overlapping_tracks() {
	// A and B above, C and D below, edges A-D and B-C cross.
	let mut layout = Layout {
		nodes: vec![
			node(50.0, 20.0, 60.0, 40.0, 0),
			node(250.0, 20.0, 60.0, 40.0, 0),
			node(50.0, 100.0, 60.0, 40.0, 2),
			node(250.0, 100.0, 60.0, 40.0, 2),
		],
		edges: vec![edge(0, 3, &[(250.0, 60.0)]), edge(1, 2, &[(50.0, 60.0)])],
		layer_spacing: 40.0,
		..Layout::default()
	};
	let routes = route(&mut layout);
	// Two tracks need 60 pixels: the lower nodes move down by 20.
	assert_eq!(layout.nodes[2].bounds.y, 120.0);
	assert_eq!(layout.nodes[3].bounds.y, 120.0);
	let tracks: Vec<f64> = routes.iter().map(|route| route.points[1].y).collect();
	let mut sorted = tracks.clone();
	sorted.sort_by(f64::total_cmp);
	assert_eq!(sorted, [60.0, 80.0]);
	for route in &routes {
		assert_eq!(route.points.first().map(|point| point.y), Some(40.0));
		assert_eq!(route.points.last().map(|point| point.y), Some(100.0));
	}
}

#[test]
fn maps_left_to_right_layouts() {
	let mut layout = Layout {
		direction: RankDir::LeftRight,
		nodes: vec![node(20.0, 50.0, 40.0, 30.0, 0), node(100.0, 50.0, 40.0, 30.0, 2)],
		edges: vec![edge(0, 1, &[(60.0, 50.0)])],
		layer_spacing: 40.0,
	};
	let routes = route(&mut layout);
	assert_eq!(points(&routes[0]), [(40.0, 50.0), (80.0, 50.0)]);

	let mut layout = Layout { direction: RankDir::BottomTop, ..layout };
	layout.nodes = vec![node(50.0, 100.0, 40.0, 30.0, 0), node(50.0, 20.0, 40.0, 30.0, 2)];
	layout.edges = vec![edge(0, 1, &[(50.0, 60.0)])];
	let routes = route(&mut layout);
	assert_eq!(points(&routes[0]), [(50.0, 85.0), (50.0, 45.0)]);
}

#[test]
fn draws_self_loops_on_the_left() {
	let mut layout = Layout {
		nodes: vec![node(100.0, 20.0, 60.0, 40.0, 0)],
		edges: vec![edge(0, 0, &[]), edge(0, 0, &[])],
		layer_spacing: 40.0,
		..Layout::default()
	};
	let routes = route(&mut layout);
	assert_eq!(points(&routes[0]), [(70.0, 16.5), (50.0, 16.5), (50.0, 23.5), (70.0, 23.5)]);
	assert_eq!(routes[1].points[1].x, 30.0);
}

#[test]
fn survives_inconsistent_input() {
	let mut layout = Layout {
		nodes: vec![node(f64::NAN, 0.0, -5.0, f64::INFINITY, 0), node(0.0, 0.0, 0.0, 0.0, 0)],
		edges: vec![edge(0, 1, &[(1.0, 1.0)]), edge(0, 7, &[]), edge(5, 5, &[])],
		layer_spacing: 40.0,
		..Layout::default()
	};
	let routes = route(&mut layout);
	assert_eq!(routes.len(), 3);
}
