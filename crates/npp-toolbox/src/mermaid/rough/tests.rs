//! Compares the generators with roughjs. Each line of `fixtures.jsonl` is a call on random input
//! with a summary of the operations roughjs returned (the kinds, the first numbers and two sums).

use super::super::value::{Value, parse_json};
use super::*;

fn numbers(value: Option<&Value>) -> Vec<f64> {
	match value {
		Some(Value::Array(items)) => items.iter().filter_map(Value::as_number).collect(),
		_ => Vec::new(),
	}
}

fn options_of(value: &Value) -> Options {
	let number = |key: &str| value.get(key).and_then(Value::as_number);
	let mut options = Options {
		seed: number("seed").unwrap_or(0.0) as i32,
		roughness: number("roughness").unwrap_or(1.0),
		stroke_width: number("strokeWidth").unwrap_or(1.0).to_string(),
		fill: value.get("fill").and_then(Value::as_str).map(str::to_owned),
		..Options::default()
	};
	if let Some(stroke) = value.get("stroke").and_then(Value::as_str) {
		options.stroke = stroke.to_owned();
	}
	options.fill_style = match value.get("fillStyle").and_then(Value::as_str) {
		Some("solid") => FillStyle::Solid,
		Some("cross-hatch") => FillStyle::CrossHatch,
		_ => FillStyle::Hachure,
	};
	options.fill_weight = number("fillWeight").unwrap_or(-1.0);
	options.hachure_gap = number("hachureGap").unwrap_or(-1.0);
	options.hachure_angle = number("hachureAngle").unwrap_or(-41.0);
	options
}

fn sets_of(call: &str, args: &[Value], options: &Options) -> Vec<OpSet> {
	let n: Vec<f64> = args.iter().filter_map(Value::as_number).collect();
	match call {
		"line" => line_sets([n[0], n[1]], [n[2], n[3]], options),
		"rectangle" => rectangle_sets(n[0], n[1], n[2], n[3], options),
		"circle" => ellipse_sets(n[0], n[1], n[2], n[2], options),
		"ellipse" => ellipse_sets(n[0], n[1], n[2], n[3], options),
		"polygon" => {
			let Value::Array(points) = &args[0] else {
				panic!("polygon without points");
			};
			let points = points
				.iter()
				.map(|point| {
					let point = numbers(Some(point));
					[point[0], point[1]]
				})
				.collect();
			polygon_sets(points, options)
		}
		"path" => path_sets(args[0].as_str().unwrap_or(""), options),
		_ => panic!("unknown call {call}"),
	}
}

fn compare_line(line: &str) -> Result<(), String> {
	let fixture = parse_json(line).expect("JSON");
	let call = fixture.get("call").and_then(Value::as_str).expect("call");
	let Some(Value::Array(args)) = fixture.get("args") else {
		panic!("args");
	};
	let options = options_of(fixture.get("o").expect("options"));
	let sets = sets_of(call, args, &options);
	let Some(Value::Array(expected_sets)) = fixture.get("sets") else {
		panic!("sets");
	};
	if sets.len() != expected_sets.len() {
		return Err(format!("sets {line}"));
	}
	for (set, expected) in sets.iter().zip(expected_sets) {
		let kind = match set.kind {
			PathKind::Outline => "path",
			PathKind::Fill => "fillPath",
			PathKind::FillSketch => "fillSketch",
		};
		if Some(kind) != expected.get("t").and_then(Value::as_str) {
			return Err(format!("kind {line}"));
		}
		let mut kinds = String::new();
		let mut all = Vec::new();
		for op in &set.ops {
			match op {
				Op::Move(point) => {
					kinds.push('m');
					all.extend(point);
				}
				Op::Line(point) => {
					kinds.push('l');
					all.extend(point);
				}
				Op::Curve(data) => {
					kinds.push('b');
					all.extend(data);
				}
			}
		}
		if Some(kinds.as_str()) != expected.get("kinds").and_then(Value::as_str) {
			return Err(format!("ops {line}"));
		}
		for (own, theirs) in all.iter().zip(numbers(expected.get("first"))) {
			if (own - theirs).abs() >= 1e-7 {
				return Err(format!("first {own} {theirs} {line}"));
			}
		}
		let sum: f64 = all.iter().sum();
		let abs: f64 = all.iter().map(|number| number.abs()).sum();
		let tolerance = 1e-9 * (all.len() as f64 + 1.0) * 1000.0;
		let expected_sum = expected.get("sum").and_then(Value::as_number).expect("sum");
		let expected_abs = expected.get("abs").and_then(Value::as_number).expect("abs");
		if (sum - expected_sum).abs() >= tolerance || (abs - expected_abs).abs() >= tolerance {
			return Err(format!("sum {} {line}", sum - expected_sum));
		}
	}
	Ok(())
}

#[test]
fn matches_roughjs() {
	for line in include_str!("fixtures.jsonl").lines() {
		assert_eq!(compare_line(line), Ok(()));
	}
}

/// `ROUGH_FIXTURES=<file> cargo test -p npp-toolbox --lib rough_fixtures -- --ignored`.
#[test]
#[ignore]
fn rough_fixtures() {
	let path = std::env::var("ROUGH_FIXTURES").expect("ROUGH_FIXTURES");
	let text = std::fs::read_to_string(path).expect("fixtures");
	let mut failures = Vec::new();
	let mut count = 0;
	for line in text.lines() {
		count += 1;
		if let Err(message) = compare_line(line) {
			failures.push(message);
		}
	}
	failures.sort_by_key(String::len);
	for failure in failures.iter().take(6) {
		println!("{failure}");
	}
	println!("{} of {count} fixtures differ", failures.len());
	// V8's and the C runtime's `sin` differ in the last bit, which moves a hachure end by a pixel
	// where a rotated coordinate lands on a `Math.round` tie. Arcs that are rotated, which Mermaid
	// does not write, amplify the same bit, so the fixtures for this count have no rotation.
	assert!(failures.len() * 1000 <= count);
}

#[test]
fn generates_the_numbers_of_roughjs() {
	let mut random = Random::new(7);
	let values: Vec<f64> = (0..4).map(|_| random.next()).collect();
	assert_eq!(
		values,
		[0.00015734555199742317, 0.5952271404676139, 0.2092975121922791, 0.00021103350445628166]
	);
	let mut random = Random::new(2147483647);
	assert_eq!(random.next(), 0.9999775220640004);
	assert_eq!(random.next(), 0.9149675513617694);
	let mut random = Random::new(-5);
	assert_eq!(random.next(), 0.9998876103200018);
	assert_eq!(random.next(), 0.5748377568088472);
}

#[test]
fn writes_paths_with_three_decimals() {
	let options = Options { roughness: 0.3, seed: 7, ..Options::default() };
	let sketch = line([0.0, 0.0], [100.0, 50.0], &options);
	assert_eq!(sketch.paths.len(), 1);
	assert!(
		sketch.paths[0].d.starts_with("M-0.6 -0.376C20.56 10.219 40.059 20.144 99.533 49.624M")
	);
	let mut text = String::new();
	for value in [-0.0004, 0.0004, 1.5, -2.05, 1000.001, 12.0, 0.1234, 1e16] {
		push_number(&mut text, value);
		text.push(' ');
		assert_eq!(
			text.trim_end().rsplit(' ').next(),
			Some(super::super::svg::number(value).as_str())
		);
	}
}

#[test]
fn takes_any_size_and_any_path() {
	let filled = Options { fill: Some("#eee".to_owned()), ..Options::default() };
	for value in [0.0, -1.0, 1e-300, 1e9, 1e12, 1e300, f64::MAX, f64::NAN, f64::INFINITY] {
		let _ = rectangle(value, value, value, value, &filled);
		let _ = rectangle(0.0, 0.0, value, 20.0, &filled);
		let _ = ellipse(value, 0.0, value, value, &filled);
		let _ = circle(0.0, 0.0, value, &filled);
		let _ = line([value, 0.0], [0.0, value], &filled);
		let _ = polygon(vec![[0.0, 0.0], [value, 1.0], [1.0, value]], &filled);
		let solid = Options { fill_style: FillStyle::Solid, ..filled.clone() };
		let _ = polygon(vec![[0.0, 0.0], [value, 1.0], [1.0, value]], &solid);
	}
	for d in [
		"",
		" ",
		"Z",
		"z 1",
		"M",
		"M1",
		"M1 2 3",
		"L5 5",
		"x",
		"M0 0 A0 0 0 0 0 5 5",
		"M0 0 A5 5 0 1 1 0 0",
		"M0 0 A1e300 1e300 0 1 1 1e300 1e300",
		"M1e300 1e300 C-1e300 1e300 1e300 -1e300 0 0",
		"M1e999 0 L0 1e999 Z",
		"M0 0 C1e9 1e9 -1e9 1e9 1e9 0 Z",
		"M-",
		"M- 1 - 2L.5.5",
		"M1e 2",
		"é",
	] {
		for fill_style in [FillStyle::Hachure, FillStyle::Solid, FillStyle::CrossHatch] {
			let _ = path(d, &Options { fill_style, ..filled.clone() });
		}
	}
	let long = format!("M0 0{}", " L5 5 L9 0".repeat(5000));
	assert!(!path(&long, &filled).paths.is_empty());
}

#[test]
fn reads_bounds_of_curves() {
	let bounds = ops_bounds(&[Op::Move([0.0, 0.0]), Op::Curve([0.0, 40.0, 10.0, 40.0, 10.0, 0.0])]);
	assert_eq!(bounds, Some(Bounds { x: 0.0, y: 0.0, width: 10.0, height: 30.0 }));
	assert_eq!(ops_bounds(&[]), None);
}
