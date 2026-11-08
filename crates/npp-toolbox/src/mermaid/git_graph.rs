//! Git graphs (`gitGraph.langium`, `gitGraphParser.ts`, `gitGraphAst.ts`,
//! `gitGraphRenderer.ts`, `styles.js`).

mod parser;

use std::collections::HashMap;
use std::fmt::Write;

use self::parser::{Commit, CommitType, Database, Direction};
use super::color::is_color;
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::{Font, parse_font_size};
use super::value::Value;
use super::{Context, Diagram, Look, Result};

const SECTION: &str = "gitGraph";
const LAYOUT_OFFSET: f64 = 10.0;
const COMMIT_STEP: f64 = 40.0;
const PX: f64 = 4.0;
const PY: f64 = 2.0;
const THEME_COLOR_LIMIT: usize = 8;
/// `defaultPos`: where vertical graphs start.
const DEFAULT_POS: f64 = 30.0;
const REDUX_BRANCH_LABEL_PADDING_Y: f64 = 12.0;

/// `GitGraphDiagramConfig`
struct Settings {
	title_top_margin: f64,
	diagram_padding: f64,
	show_commit_label: bool,
	show_branches: bool,
	rotate_commit_label: bool,
	parallel_commits: bool,
	use_max_width: bool,
}

impl Settings {
	fn new(context: &Context) -> Self {
		let flag = |key: &str, default: bool| context.config_bool(SECTION, key, default);
		Self {
			title_top_margin: context.config_number(SECTION, "titleTopMargin", 25.0),
			diagram_padding: context.config_number(SECTION, "diagramPadding", 8.0),
			show_commit_label: flag("showCommitLabel", true),
			show_branches: flag("showBranches", true),
			rotate_commit_label: flag("rotateCommitLabel", true),
			parallel_commits: flag("parallelCommits", false),
			use_max_width: flag("useMaxWidth", true),
		}
	}
}

/// The theme groups the renderer and `styles.js` tell apart.
struct ThemeGroups {
	/// Smaller bullets and padded branch labels.
	redux_geometry: bool,
	/// Branch colors that skip the first color.
	color_theme: bool,
	is_dark: bool,
	is_neo: bool,
	/// `genColor` instead of `normalTheme`.
	neo_color_gen: bool,
}

impl ThemeGroups {
	fn new(theme: &str) -> Self {
		let redux_geometry =
			matches!(theme, "redux" | "redux-dark" | "redux-color" | "redux-dark-color");
		let is_neo = matches!(theme, "neo" | "neo-dark");
		Self {
			redux_geometry,
			color_theme: matches!(theme, "redux-color" | "redux-dark-color"),
			is_dark: matches!(theme, "dark" | "redux-dark" | "redux-dark-color" | "neo-dark"),
			is_neo,
			neo_color_gen: redux_geometry || is_neo,
		}
	}
}

/// `calcColorIndex`, `None` where JavaScript computes `NaN`.
fn color_index(raw: usize, limit: Option<usize>, avoid_default_color: bool) -> Option<usize> {
	let limit = limit?;
	if avoid_default_color && raw > 0 {
		return (limit > 1).then(|| (raw - 1) % (limit - 1) + 1);
	}
	(limit > 0).then(|| raw % limit)
}

fn class_number(index: Option<usize>) -> String {
	index.map_or_else(|| "NaN".to_owned(), |index| index.to_string())
}

/// A rule of the style sheet for elements with the class `class`. All rules of `styles.js`
/// have the same specificity, so the last declaration wins.
struct Rule {
	class: String,
	declarations: Vec<(&'static str, String)>,
}

fn is_valid_declaration(property: &str, value: &str) -> bool {
	let value = value.trim();
	let is_length = || {
		let number = value.trim_end_matches("px");
		number.parse::<f64>().is_ok_and(|number| number >= 0.0)
	};
	match property {
		"fill" | "stroke" => value == "none" || value.starts_with("url(") || is_color(value),
		"stroke-width" | "opacity" => is_length(),
		"font-size" => parse_font_size(value, 16.0).is_some(),
		"font-weight" => font_weight(value).is_some(),
		_ => !value.is_empty() && value != "undefined",
	}
}

fn font_weight(value: &str) -> Option<u16> {
	match value.trim() {
		"normal" | "lighter" => Some(400),
		"bold" | "bolder" => Some(700),
		number => number.parse::<u16>().ok().filter(|weight| (1..=1000).contains(weight)),
	}
}

struct StyleSheet {
	rules: Vec<Rule>,
}

impl StyleSheet {
	/// The rules of `styles.js` for the theme, as far as the classes go that the graph uses.
	fn new(context: &Context, groups: &ThemeGroups, needed_colors: usize) -> Self {
		let theme = &context.theme;
		let css = |key: &str| theme.css(key);
		let limit = match theme.get("THEME_COLOR_LIMIT") {
			Some(Value::Number(limit)) if *limit > 0.0 => {
				(limit.ceil() as usize).min(needed_colors)
			}
			_ => 0,
		};
		let mut rules = Vec::new();
		let mut rule = |class: String, declarations: Vec<(&'static str, String)>| {
			rules.push(Rule { class, declarations });
		};
		for class in ["commit-id", "commit-msg", "branch-label"] {
			rule(class.to_owned(), vec![("fill", "lightgrey".to_owned())]);
		}
		let note_weight = css("noteFontWeight");
		let redux_weight = |declarations: &mut Vec<(&'static str, String)>| {
			if groups.redux_geometry {
				declarations.push(("font-weight", note_weight.clone()));
			}
		};
		let named = |key: &str, index: usize| css(&format!("{key}{}", index % THEME_COLOR_LIMIT));
		let node_border = css("nodeBorder");
		let main_bkg = css("mainBkg");
		let stroke_width = css("strokeWidth");
		let uses_gradient = matches!(theme.get("useGradient"), Some(Value::Bool(true)));
		if groups.is_neo {
			for index in 0..limit {
				if index == 0 {
					rule("branch-label0".to_owned(), vec![("fill", node_border.clone())]);
					rule("commit0".to_owned(), vec![("stroke", node_border.clone())]);
					rule(
						"commit-highlight0".to_owned(),
						vec![("stroke", node_border.clone()), ("fill", node_border.clone())],
					);
					rule("arrow0".to_owned(), vec![("stroke", node_border.clone())]);
					rule("commit-bullets".to_owned(), vec![("fill", node_border.clone())]);
					rule("commit-cherry-pick0".to_owned(), vec![("stroke", node_border.clone())]);
					if uses_gradient {
						for label in 0..limit {
							rule(
								format!("label{label}"),
								vec![
									("fill", main_bkg.clone()),
									("stroke", format!("url(#{}-gradient)", context.id)),
									("stroke-width", stroke_width.clone()),
								],
							);
						}
					}
				} else {
					rule(
						format!("branch-label{index}"),
						vec![("fill", named("gitBranchLabel", index))],
					);
					rule(
						format!("commit{index}"),
						vec![("stroke", named("git", index)), ("fill", named("git", index))],
					);
					rule(
						format!("commit-highlight{index}"),
						vec![("stroke", named("gitInv", index)), ("fill", named("gitInv", index))],
					);
					rule(format!("arrow{index}"), vec![("stroke", named("git", index))]);
				}
			}
		} else if groups.neo_color_gen && !groups.color_theme {
			for index in 0..limit {
				let mut branch_label = vec![("fill", node_border.clone())];
				redux_weight(&mut branch_label);
				rule(format!("branch-label{index}"), branch_label);
				rule(format!("commit{index}"), vec![("stroke", node_border.clone())]);
				rule(
					format!("commit-highlight{index}"),
					vec![("stroke", node_border.clone()), ("fill", node_border.clone())],
				);
				let mut label = vec![
					("fill", main_bkg.clone()),
					("stroke", node_border.clone()),
					("stroke-width", stroke_width.clone()),
				];
				redux_weight(&mut label);
				rule(format!("label{index}"), label);
				rule(format!("arrow{index}"), vec![("stroke", node_border.clone())]);
				rule("commit-bullets".to_owned(), vec![("fill", node_border.clone())]);
				rule(format!("commit-cherry-pick{index}"), vec![("stroke", node_border.clone())]);
			}
		} else if groups.neo_color_gen {
			let border_colors: Vec<String> = match theme.get("borderColorArray") {
				Some(Value::Array(colors)) => {
					colors.iter().map(|c| c.to_text().unwrap_or_default()).collect()
				}
				_ => Vec::new(),
			};
			for index in 0..limit {
				let mut branch_label = vec![("fill", node_border.clone())];
				redux_weight(&mut branch_label);
				rule(format!("branch-label{index}"), branch_label);
				if index == 0 {
					rule("commit0".to_owned(), vec![("stroke", node_border.clone())]);
					rule(
						"commit-highlight0".to_owned(),
						vec![("stroke", node_border.clone()), ("fill", main_bkg.clone())],
					);
					let mut label = vec![
						("fill", main_bkg.clone()),
						("stroke", node_border.clone()),
						("stroke-width", stroke_width.clone()),
					];
					redux_weight(&mut label);
					rule("label0".to_owned(), label);
					rule("arrow0".to_owned(), vec![("stroke", node_border.clone())]);
					rule("commit-bullets".to_owned(), vec![("fill", node_border.clone())]);
				} else {
					// `undefined` for an empty array.
					let color = match border_colors.len() {
						0 => String::new(),
						count => border_colors[index % count].clone(),
					};
					rule(
						format!("commit{index}"),
						vec![("stroke", color.clone()), ("fill", color.clone())],
					);
					rule(
						format!("commit-highlight{index}"),
						vec![("stroke", color.clone()), ("fill", color.clone())],
					);
					let fill = if groups.is_dark { main_bkg.clone() } else { color.clone() };
					rule(
						format!("label{index}"),
						vec![
							("fill", fill),
							("stroke", color.clone()),
							("stroke-width", stroke_width.clone()),
						],
					);
					rule(format!("arrow{index}"), vec![("stroke", color)]);
				}
			}
		} else {
			for index in 0..limit {
				rule(
					format!("branch-label{index}"),
					vec![("fill", named("gitBranchLabel", index))],
				);
				rule(
					format!("commit{index}"),
					vec![("stroke", named("git", index)), ("fill", named("git", index))],
				);
				rule(
					format!("commit-highlight{index}"),
					vec![("stroke", named("gitInv", index)), ("fill", named("gitInv", index))],
				);
				rule(format!("label{index}"), vec![("fill", named("git", index))]);
				rule(format!("arrow{index}"), vec![("stroke", named("git", index))]);
			}
		}
		let is_neo_color = groups.neo_color_gen;
		let pick = |neo: String, classic: String| if is_neo_color { neo } else { classic };
		let line_color = theme.text("commitLineColor").unwrap_or_else(|| css("lineColor"));
		rule(
			"branch".to_owned(),
			vec![
				("stroke-width", stroke_width.clone()),
				("stroke", line_color),
				("stroke-dasharray", pick("4 2".to_owned(), "2".to_owned())),
			],
		);
		let mut commit_label = vec![
			("font-size", css("commitLabelFontSize")),
			("fill", pick(node_border.clone(), css("commitLabelColor"))),
		];
		if is_neo_color {
			commit_label.push(("font-weight", note_weight.clone()));
		}
		rule("commit-label".to_owned(), commit_label);
		let mut label_background = vec![
			("font-size", css("commitLabelFontSize")),
			("fill", pick("transparent".to_owned(), css("commitLabelBackground"))),
		];
		if !is_neo_color {
			label_background.push(("opacity", "0.5".to_owned()));
		}
		rule("commit-label-bkg".to_owned(), label_background);
		rule(
			"tag-label".to_owned(),
			vec![("font-size", css("tagLabelFontSize")), ("fill", css("tagLabelColor"))],
		);
		let mut tag_background = vec![
			("fill", pick(main_bkg.clone(), css("tagLabelBackground"))),
			("stroke", pick(node_border.clone(), css("tagLabelBorder"))),
		];
		if is_neo_color {
			tag_background.push(("filter", css("dropShadow")));
		}
		rule("tag-label-bkg".to_owned(), tag_background);
		rule("tag-hole".to_owned(), vec![("fill", css("textColor"))]);
		let symbol = pick(main_bkg.clone(), css("primaryColor"));
		rule("commit-merge".to_owned(), vec![("stroke", symbol.clone()), ("fill", symbol.clone())]);
		rule(
			"commit-reverse".to_owned(),
			vec![
				("stroke", symbol.clone()),
				("fill", symbol.clone()),
				("stroke-width", pick(stroke_width.clone(), "3".to_owned())),
			],
		);
		rule(
			"commit-highlight-inner".to_owned(),
			vec![("stroke", symbol.clone()), ("fill", symbol)],
		);
		let arrow_width = if groups.redux_geometry { stroke_width } else { "8".to_owned() };
		rule(
			"arrow".to_owned(),
			vec![
				("stroke-width", arrow_width),
				("stroke-linecap", "round".to_owned()),
				("fill", "none".to_owned()),
			],
		);
		rule(
			"gitTitleText".to_owned(),
			vec![("font-size", "18px".to_owned()), ("fill", css("textColor"))],
		);
		Self { rules }
	}

	/// The values of the properties the rules set for an element with `classes`.
	fn computed(&self, classes: &str) -> Style {
		let classes: Vec<&str> = classes.split_whitespace().collect();
		let mut style = Style::default();
		for rule in &self.rules {
			if !classes.contains(&rule.class.as_str()) {
				continue;
			}
			for (property, value) in &rule.declarations {
				if !is_valid_declaration(property, value) {
					continue;
				}
				let value = value.trim().to_owned();
				match style.values.iter_mut().find(|(name, _)| name == property) {
					Some(existing) => existing.1 = value,
					None => style.values.push((property, value)),
				}
			}
		}
		style
	}
}

#[derive(Default)]
struct Style {
	values: Vec<(&'static str, String)>,
}

impl Style {
	fn get(&self, property: &str) -> Option<&str> {
		self.values.iter().find(|(name, _)| *name == property).map(|(_, value)| value.as_str())
	}

	/// Writes the paint as presentation attributes; `fill` and `stroke` fall back to the
	/// element's own attributes, which the style sheet overrides.
	fn write(
		&self,
		svg: &mut Svg,
		own_fill: Option<&str>,
		own_stroke: Option<&str>,
		context: &Context,
	) {
		let fill = self.get("fill").or(own_fill);
		let stroke = self.get("stroke").or(own_stroke);
		if let Some(fill) = fill {
			svg.attr("fill", fill);
		}
		if let Some(stroke) = stroke {
			svg.attr("stroke", stroke);
		}
		for property in ["stroke-width", "opacity", "stroke-dasharray", "stroke-linecap"] {
			if let Some(value) = self.get(property) {
				svg.attr(property, value);
			}
		}
		if let Some(filter) = self.get("filter").and_then(|filter| css_filter(filter, context)) {
			svg.attr("filter", &filter);
		}
	}

	fn font(&self, base: &Font) -> Font {
		let mut font = base.clone();
		if let Some(size) = self.get("font-size").and_then(|size| parse_font_size(size, base.size))
		{
			font.size = size;
		}
		if let Some(weight) = self.get("font-weight").and_then(font_weight) {
			font.weight = weight;
		}
		font
	}
}

/// A CSS `filter` for the `filter` attribute: filter functions stay, references to the filters
/// Mermaid never defines for git graphs (`url(#drop-shadow)`) draw nothing in Chromium.
fn css_filter(filter: &str, context: &Context) -> Option<String> {
	let filter = filter.trim().trim_end_matches(';').trim_end();
	if filter.is_empty() || filter == "none" {
		return None;
	}
	if filter.contains("url(") && !filter.contains(&format!("url(#{}-", context.id)) {
		return None;
	}
	Some(filter.to_owned())
}

/// An SVG transform matrix `[a c e; b d f]`.
#[derive(Clone, Copy)]
struct Transform {
	a: f64,
	b: f64,
	c: f64,
	d: f64,
	e: f64,
	f: f64,
}

impl Transform {
	const IDENTITY: Transform = Transform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

	fn then(self, other: Transform) -> Transform {
		// `self` applied after `other`, like `transform="self other"`.
		Transform {
			a: self.a * other.a + self.c * other.b,
			b: self.b * other.a + self.d * other.b,
			c: self.a * other.c + self.c * other.d,
			d: self.b * other.c + self.d * other.d,
			e: self.a * other.e + self.c * other.f + self.e,
			f: self.b * other.e + self.d * other.f + self.f,
		}
	}

	fn translate(x: f64, y: f64) -> Transform {
		Transform { e: x, f: y, ..Self::IDENTITY }
	}

	/// `rotate(angle, x, y)`
	fn rotate(angle: f64, x: f64, y: f64) -> Transform {
		let (sin, cos) = angle.to_radians().sin_cos();
		let rotation = Transform { a: cos, b: sin, c: -sin, d: cos, e: 0.0, f: 0.0 };
		Self::translate(x, y).then(rotation).then(Self::translate(-x, -y))
	}

	fn map(&self, x: f64, y: f64) -> (f64, f64) {
		(self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
	}
}

/// `getBBox` of the root: Blink unites the boxes of all shapes, also empty lines, and maps
/// each through its transforms; rects and circles without area and empty texts are skipped.
#[derive(Default)]
struct Bounds {
	extent: Option<(f64, f64, f64, f64)>,
}

impl Bounds {
	fn include(&mut self, transform: Transform, x: f64, y: f64, width: f64, height: f64) {
		let corners = [(x, y), (x + width, y), (x, y + height), (x + width, y + height)];
		for (x, y) in corners.map(|(x, y)| transform.map(x, y)) {
			if !(x.is_finite() && y.is_finite()) {
				return;
			}
			self.extent = Some(match self.extent {
				Some((left, top, right, bottom)) => {
					(left.min(x), top.min(y), right.max(x), bottom.max(y))
				}
				None => (x, y, x, y),
			});
		}
	}

	fn include_points(&mut self, transform: Transform, points: &[(f64, f64)]) {
		let Some(&(first_x, first_y)) = points.first() else { return };
		let (mut left, mut top, mut right, mut bottom) = (first_x, first_y, first_x, first_y);
		for &(x, y) in points {
			(left, top, right, bottom) = (left.min(x), top.min(y), right.max(x), bottom.max(y));
		}
		self.include(transform, left, top, right - left, bottom - top);
	}

	fn rect(&self) -> (f64, f64, f64, f64) {
		let (left, top, right, bottom) = self.extent.unwrap_or_default();
		(left, top, right - left, bottom - top)
	}
}

#[derive(Clone, Copy, Default)]
struct Position {
	x: f64,
	y: f64,
}

#[derive(Clone, Copy)]
struct CommitPosition {
	x: f64,
	y: f64,
	pos_with_offset: f64,
}

struct Layout<'d> {
	database: &'d Database,
	direction: Direction,
	redux_geometry: bool,
	/// Position and index of each branch.
	branch_positions: HashMap<String, (f64, usize)>,
	commit_positions: HashMap<String, Position>,
	max_pos: f64,
	lanes: Vec<f64>,
}

impl Layout<'_> {
	fn is_vertical(&self) -> bool {
		self.direction != Direction::LeftRight
	}

	fn branch_position(&self, commit: &Commit) -> Result<f64> {
		match self.branch_positions.get(&commit.branch) {
			Some(&(position, _)) => Ok(position),
			None => Err(super::Error::new(format!("Branch not found for commit {}", commit.id))),
		}
	}

	fn branch_index(&self, branch: &str) -> usize {
		self.branch_positions.get(branch).map_or(0, |&(_, index)| index)
	}

	/// `findClosestParent`
	fn closest_parent(&self, parents: &[String]) -> Option<String> {
		let is_bottom_top = self.direction == Direction::BottomTop;
		let mut target = if is_bottom_top { f64::INFINITY } else { 0.0 };
		let mut closest = None;
		for parent in parents {
			let Some(position) = self.commit_positions.get(parent) else { continue };
			let value = if self.is_vertical() { position.y } else { position.x };
			let is_closer = if is_bottom_top { value <= target } else { value >= target };
			if is_closer {
				closest = Some(parent.clone());
				target = value;
			}
		}
		closest
	}

	/// `calculateCommitPosition` of `setParallelBTPos`.
	fn parent_step(&self, commit: &Commit) -> Result<f64> {
		let not_found =
			|| super::Error::new(format!("Closest parent not found for commit {}", commit.id));
		let closest = self.closest_parent(&commit.parents).ok_or_else(not_found)?;
		let position = self.commit_positions.get(&closest).ok_or_else(not_found)?;
		Ok(position.y + COMMIT_STEP)
	}

	fn set_commit_position(&mut self, commit: &Commit, x: f64, y: f64) {
		self.commit_positions.insert(commit.id.clone(), Position { x, y });
	}

	/// `setParallelBTPos`
	fn set_parallel_bottom_top_positions(&mut self, order: &[usize]) -> Result<()> {
		let database = self.database;
		let mut current = DEFAULT_POS;
		let mut max_position = DEFAULT_POS;
		let mut roots = Vec::new();
		for &index in order {
			let commit = &database.commits[index];
			if commit.parents.is_empty() {
				roots.push(index);
			} else {
				current = self.parent_step(commit)?;
				max_position = max_position.max(current);
			}
			let x = self.branch_position(commit)?;
			self.set_commit_position(commit, x, current + LAYOUT_OFFSET);
		}
		current = max_position;
		for index in roots {
			let commit = &database.commits[index];
			let x = self.branch_position(commit)?;
			self.set_commit_position(commit, x, current + DEFAULT_POS);
		}
		for &index in order {
			let commit = &database.commits[index];
			if commit.parents.is_empty() {
				continue;
			}
			// `findClosestParentBT`, which reads every parent's position.
			let mut closest = None;
			let mut max_parent = f64::INFINITY;
			for parent in &commit.parents {
				let Some(position) = self.commit_positions.get(parent) else {
					return Err(super::Error::new(
						"Cannot read properties of undefined (reading 'y')",
					));
				};
				if position.y <= max_parent {
					closest = Some(position.y);
					max_parent = position.y;
				}
			}
			let Some(parent_y) = closest else {
				return Err(super::Error::new("Cannot read properties of undefined (reading 'y')"));
			};
			current = parent_y - COMMIT_STEP;
			if current <= max_position {
				max_position = current;
			}
			let x = self.branch_position(commit)?;
			self.set_commit_position(commit, x, current - LAYOUT_OFFSET);
		}
		Ok(())
	}

	/// `calculatePosition`
	fn calculate_position(&self, commit: &Commit) -> f64 {
		let own = || self.commit_positions.get(&commit.id).copied().unwrap_or_default();
		if commit.parents.is_empty() {
			return match self.direction {
				Direction::TopBottom => DEFAULT_POS,
				Direction::BottomTop => own().y - COMMIT_STEP,
				Direction::LeftRight => 0.0,
			};
		}
		let Some(closest) = self.closest_parent(&commit.parents) else { return 0.0 };
		let parent = self.commit_positions.get(&closest).copied().unwrap_or_default();
		match self.direction {
			Direction::TopBottom => parent.y + COMMIT_STEP,
			Direction::BottomTop => own().y - COMMIT_STEP,
			Direction::LeftRight => parent.x + COMMIT_STEP,
		}
	}

	/// `getCommitPosition`
	fn commit_position(
		&self,
		commit: &Commit,
		pos: f64,
		is_parallel: bool,
	) -> Result<CommitPosition> {
		let is_bottom_top = self.direction == Direction::BottomTop;
		let pos_with_offset = if is_bottom_top && is_parallel { pos } else { pos + LAYOUT_OFFSET };
		let branch = self.branch_position(commit)?;
		let spine_offset =
			if self.redux_geometry { REDUX_BRANCH_LABEL_PADDING_Y / 2.0 + 1.0 } else { -2.0 };
		Ok(match self.is_vertical() {
			true => CommitPosition { x: branch, y: pos_with_offset, pos_with_offset },
			false => {
				CommitPosition { x: pos_with_offset, y: branch + spine_offset, pos_with_offset }
			}
		})
	}

	/// The position pass of `drawCommits`: the commits in drawing order with their positions and
	/// `pos`.
	fn place_commits(&mut self, is_parallel: bool) -> Result<Vec<(usize, CommitPosition, f64)>> {
		let database = self.database;
		let mut pos = if self.is_vertical() { DEFAULT_POS } else { 0.0 };
		let mut order: Vec<usize> = (0..database.commits.len()).collect();
		npp::sort::sort_by_key(&mut order[..], |&index| database.commits[index].seq);
		let is_bottom_top = self.direction == Direction::BottomTop;
		if is_bottom_top {
			if is_parallel {
				self.set_parallel_bottom_top_positions(&order)?;
			}
			order.reverse();
		}
		let mut placed = Vec::with_capacity(order.len());
		for index in order {
			let commit = &database.commits[index];
			if is_parallel {
				pos = self.calculate_position(commit);
			}
			let position = self.commit_position(commit, pos, is_parallel)?;
			placed.push((index, position, pos));
			let stored = match self.is_vertical() {
				true => Position { x: position.x, y: position.pos_with_offset },
				false => Position { x: position.pos_with_offset, y: position.y },
			};
			self.commit_positions.insert(commit.id.clone(), stored);
			pos += if is_bottom_top && is_parallel {
				COMMIT_STEP
			} else {
				COMMIT_STEP + LAYOUT_OFFSET
			};
			if pos > self.max_pos {
				self.max_pos = pos;
			}
		}
		Ok(placed)
	}

	/// `findLane`
	fn find_lane(&mut self, y1: f64, y2: f64) -> f64 {
		let mut y2 = y2;
		for depth in 0.. {
			let candidate = y1 + (y1 - y2).abs() / 2.0;
			if depth > 5 {
				return candidate;
			}
			if self.lanes.iter().all(|lane| (lane - candidate).abs() >= 10.0) {
				self.lanes.push(candidate);
				return candidate;
			}
			y2 -= (y1 - y2).abs() / 5.0;
		}
		unreachable!()
	}
}

/// `drawText`: the rows of a branch label.
fn label_rows(text: &str) -> Vec<String> {
	let mut rows = Vec::new();
	let mut row = String::new();
	let mut rest = text;
	while let Some(character) = rest.chars().next() {
		if rest.starts_with("\\n") {
			rows.push(std::mem::take(&mut row));
			rest = &rest[2..];
			continue;
		}
		if character == '\n' {
			rows.push(std::mem::take(&mut row));
			rest = &rest[1..];
			continue;
		}
		if let Some(length) = line_break_length(rest) {
			rows.push(std::mem::take(&mut row));
			rest = &rest[length..];
			continue;
		}
		row.push(character);
		rest = &rest[character.len_utf8()..];
	}
	rows.push(row);
	rows.into_iter().map(|row| row.trim().to_owned()).collect()
}

/// Length of `<br\s*\/?>`, case-insensitive, at the start of `text`.
fn line_break_length(text: &str) -> Option<usize> {
	let bytes = text.as_bytes();
	if bytes.len() < 4 || !bytes[..3].eq_ignore_ascii_case(b"<br") {
		return None;
	}
	let mut index = 3;
	while index < bytes.len() && (bytes[index] as char).is_ascii_whitespace() {
		index += 1;
	}
	if bytes.get(index) == Some(&b'/') {
		index += 1;
	}
	(bytes.get(index) == Some(&b'>')).then_some(index + 1)
}

/// A text box in the coordinates of its element.
#[derive(Clone, Copy, Default)]
struct TextBox {
	x: f64,
	y: f64,
	width: f64,
	height: f64,
}

fn text_box(
	context: &mut Context,
	content: &str,
	font: &Font,
	x: f64,
	baseline: f64,
) -> Option<TextBox> {
	if content.trim().is_empty() {
		return None;
	}
	let (width, height) = context.bounding_box(content, font);
	let left = context.bounding_box_left(content, font);
	let top = context.bounding_box_top(content, font);
	Some(TextBox { x: x + left, y: baseline + top, width, height })
}

/// The rows of a `drawText` label at their baselines (`dy="1em"` each, which empty rows lose)
/// and the box of the whole text.
fn label_text(
	context: &mut Context,
	rows: &[String],
	font: &Font,
) -> (Vec<(String, f64)>, TextBox) {
	let mut baseline = 0.0;
	let mut placed = Vec::new();
	let mut extent: Option<(f64, f64, f64, f64)> = None;
	for row in rows {
		if row.is_empty() {
			continue;
		}
		baseline += font.size;
		if let Some(row_box) = text_box(context, row, font, 0.0, baseline) {
			let (right, bottom) = (row_box.x + row_box.width, row_box.y + row_box.height);
			extent = Some(match extent {
				Some((l, t, r, b)) => {
					(l.min(row_box.x), t.min(row_box.y), r.max(right), b.max(bottom))
				}
				None => (row_box.x, row_box.y, right, bottom),
			});
		}
		placed.push((row.clone(), baseline));
	}
	let (left, top, right, bottom) = extent.unwrap_or_default();
	(placed, TextBox { x: left, y: top, width: right - left, height: bottom - top })
}

struct Renderer<'c, 'm, 'd> {
	context: &'c mut Context<'m>,
	settings: Settings,
	groups: ThemeGroups,
	styles: StyleSheet,
	layout: Layout<'d>,
	bounds: Bounds,
	font: Font,
}

impl Renderer<'_, '_, '_> {
	fn circle(
		&mut self,
		svg: &mut Svg,
		classes: &str,
		x: f64,
		y: f64,
		radius: f64,
		own_fill: Option<&str>,
	) {
		svg.open("circle").attr("class", classes);
		svg.number_attr("cx", x).number_attr("cy", y).number_attr("r", radius);
		self.styles.computed(classes).write(svg, own_fill, None, self.context);
		svg.close();
		if radius > 0.0 {
			self.bounds.include(
				Transform::IDENTITY,
				x - radius,
				y - radius,
				2.0 * radius,
				2.0 * radius,
			);
		}
	}

	fn bullet(&mut self, svg: &mut Svg, commit: &Commit, position: CommitPosition) {
		let redux = self.groups.redux_geometry;
		let symbol = commit.custom_type.unwrap_or(commit.kind);
		let type_class = match symbol {
			CommitType::Normal => "commit-normal",
			CommitType::Reverse => "commit-reverse",
			CommitType::Highlight => "commit-highlight",
			CommitType::Merge => "commit-merge",
			CommitType::CherryPick => "commit-cherry-pick",
		};
		let branch_index = self.layout.branch_index(&commit.branch);
		let color = class_number(color_index(
			branch_index,
			Some(THEME_COLOR_LIMIT),
			self.groups.color_theme,
		));
		let id = &commit.id;
		let (x, y) = (position.x, position.y);
		let shift = |value: f64| if redux { value } else { 0.0 };
		match symbol {
			CommitType::Highlight => {
				let size = if redux { 14.0 } else { 20.0 };
				let classes = format!("commit {id} commit-highlight{color} {type_class}-outer");
				self.rect(
					svg,
					&classes,
					(x - 10.0 + shift(3.0), y - 10.0 + shift(3.0), size, size),
					Transform::IDENTITY,
					"",
					false,
				);
				let size = if redux { 8.0 } else { 12.0 };
				let classes = format!("commit {id} commit{color} {type_class}-inner");
				self.rect(
					svg,
					&classes,
					(x - 6.0 + shift(2.0), y - 6.0 + shift(2.0), size, size),
					Transform::IDENTITY,
					"",
					false,
				);
			}
			CommitType::CherryPick => {
				let classes = format!("commit {id} {type_class}");
				let contrast = if self.groups.is_dark { "#000000" } else { "#fff" };
				self.circle(svg, &classes, x, y, if redux { 7.0 } else { 10.0 }, None);
				let small = if redux { 2.5 } else { 2.75 };
				self.circle(svg, &classes, x - 3.0, y + 2.0, small, Some(contrast));
				self.circle(svg, &classes, x + 3.0, y + 2.0, small, Some(contrast));
				for start_x in [x + 3.0, x - 3.0] {
					svg.open("line").attr("class", &classes);
					svg.number_attr("x1", start_x).number_attr("y1", y + 1.0);
					svg.number_attr("x2", x).number_attr("y2", y - 5.0);
					self.styles.computed(&classes).write(svg, None, Some(contrast), self.context);
					svg.close();
					self.bounds
						.include_points(Transform::IDENTITY, &[(start_x, y + 1.0), (x, y - 5.0)]);
				}
			}
			_ => {
				let classes = format!("commit {id} commit{color}");
				self.circle(svg, &classes, x, y, if redux { 7.0 } else { 10.0 }, None);
				let classes = format!("commit {type_class} {id} commit{color}");
				if symbol == CommitType::Merge {
					self.circle(svg, &classes, x, y, if redux { 5.0 } else { 6.0 }, None);
				}
				if symbol == CommitType::Reverse {
					let size = if redux { 4.0 } else { 5.0 };
					let (left, right, top, bottom) = (x - size, x + size, y - size, y + size);
					let path = format!(
						"M {},{}L{},{}M{},{}L{},{}",
						number(left),
						number(top),
						number(right),
						number(bottom),
						number(left),
						number(bottom),
						number(right),
						number(top)
					);
					svg.open("path").attr("d", &path).attr("class", &classes);
					self.styles.computed(&classes).write(svg, None, None, self.context);
					svg.close();
					self.bounds
						.include_points(Transform::IDENTITY, &[(left, top), (right, bottom)]);
				}
			}
		}
	}

	/// `drawCommitLabel`
	fn commit_label(&mut self, svg: &mut Svg, commit: &Commit, position: CommitPosition, pos: f64) {
		let shows_label = commit.kind != CommitType::CherryPick
			&& (commit.kind != CommitType::Merge || commit.custom_id)
			&& self.settings.show_commit_label;
		if !shows_label {
			return;
		}
		let font = self.styles.computed("commit-label").font(&self.font);
		let (x, y) = (position.x, position.y);
		let measured = text_box(self.context, &commit.id, &font, 0.0, 0.0);
		let (width, height) = measured.map_or((0.0, 0.0), |b| (b.width, b.height));
		let mut background = (
			position.pos_with_offset - width / 2.0 - PY,
			y + 13.5,
			width + 2.0 * PY,
			height + 2.0 * PY,
		);
		let mut text_at = (position.pos_with_offset - width / 2.0, y + 25.0);
		if self.layout.is_vertical() {
			background.0 = x - (width + 4.0 * PX + 5.0);
			background.1 = y - 12.0;
			text_at = (x - (width + 4.0 * PX), y + height - 12.0);
		}
		let mut wrapper = Transform::IDENTITY;
		let mut element = Transform::IDENTITY;
		let mut wrapper_attribute = String::new();
		let mut element_attribute = String::new();
		if self.settings.rotate_commit_label {
			if self.layout.is_vertical() {
				element = Transform::rotate(-45.0, x, y);
				element_attribute = format!("rotate(-45, {}, {})", number(x), number(y));
			} else {
				let rotate_x = -7.5 - ((width + 10.0) / 25.0) * 9.5;
				let rotate_y = 10.0 + (width / 25.0) * 8.5;
				wrapper =
					Transform::translate(rotate_x, rotate_y).then(Transform::rotate(-45.0, pos, y));
				wrapper_attribute = format!(
					"translate({}, {}) rotate(-45, {}, {})",
					number(rotate_x),
					number(rotate_y),
					number(pos),
					number(y)
				);
			}
		}
		svg.open("g");
		if !wrapper_attribute.is_empty() {
			svg.attr("transform", &wrapper_attribute);
		}
		svg.end();
		let transform = wrapper.then(element);
		let extra = match element_attribute.is_empty() {
			true => String::new(),
			false => format!(" transform=\"{element_attribute}\""),
		};
		self.rect(svg, "commit-label-bkg", background, transform, &extra, false);
		let style = self.styles.computed("commit-label");
		let run = TextRun {
			x: text_at.0,
			y: text_at.1,
			anchor: Anchor::Start,
			baseline: Baseline::Alphabetic,
			font: &font,
			content: &commit.id,
		};
		svg.text(run, |svg| {
			svg.attr("class", "commit-label");
			if let Some(fill) = style.get("fill") {
				svg.attr("fill", fill);
			}
			if !element_attribute.is_empty() {
				svg.attr("transform", &element_attribute);
			}
		});
		if let Some(text) = text_box(self.context, &commit.id, &font, text_at.0, text_at.1) {
			self.bounds.include(transform, text.x, text.y, text.width, text.height);
		}
		svg.close_tag("g");
	}

	/// `drawCommitTags`
	fn commit_tags(&mut self, svg: &mut Svg, commit: &Commit, position: CommitPosition, pos: f64) {
		if commit.tags.is_empty() {
			return;
		}
		let font = self.styles.computed("tag-label").font(&self.font);
		let (x, y) = (position.x, position.y);
		let mut tags = Vec::new();
		let (mut max_width, mut max_height) = (0.0_f64, 0.0_f64);
		for (index, tag) in commit.tags.iter().rev().enumerate() {
			let offset = 20.0 * index as f64;
			let measured = text_box(self.context, tag, &font, 0.0, 0.0);
			let (width, height) = measured.map_or((0.0, 0.0), |b| (b.width, b.height));
			max_width = max_width.max(width);
			max_height = max_height.max(height);
			tags.push((tag, offset, position.pos_with_offset - width / 2.0, y - 16.0 - offset));
		}
		let half = max_height / 2.0;
		let tag_style = self.styles.computed("tag-label");
		for (tag, offset, mut text_x, mut text_y) in tags {
			let line_y = y - 19.2 - offset;
			let mut points = vec![
				(pos - max_width / 2.0 - PX / 2.0, line_y + PY),
				(pos - max_width / 2.0 - PX / 2.0, line_y - PY),
				(position.pos_with_offset - max_width / 2.0 - PX, line_y - half - PY),
				(position.pos_with_offset + max_width / 2.0 + PX, line_y - half - PY),
				(position.pos_with_offset + max_width / 2.0 + PX, line_y + half + PY),
				(position.pos_with_offset - max_width / 2.0 - PX, line_y + half + PY),
			];
			let mut hole = (pos - max_width / 2.0 + PX / 2.0, line_y);
			let mut shape_transform = (Transform::IDENTITY, String::new());
			let mut text_transform = (Transform::IDENTITY, String::new());
			if self.layout.is_vertical() {
				let origin = pos + offset;
				points = vec![
					(x, origin + 2.0),
					(x, origin - 2.0),
					(x + LAYOUT_OFFSET, origin - half - 2.0),
					(x + LAYOUT_OFFSET + max_width + 4.0, origin - half - 2.0),
					(x + LAYOUT_OFFSET + max_width + 4.0, origin + half + 2.0),
					(x + LAYOUT_OFFSET, origin + half + 2.0),
				];
				hole = (x + PX / 2.0, origin);
				let rotation = Transform::rotate(45.0, x, pos);
				let attribute = |shift: f64| {
					format!("translate({shift},{shift}) rotate(45, {},{})", number(x), number(pos))
				};
				shape_transform =
					(Transform::translate(12.0, 12.0).then(rotation), attribute(12.0));
				text_transform = (Transform::translate(14.0, 14.0).then(rotation), attribute(14.0));
				text_x = x + 5.0;
				text_y = origin + 3.0;
			}
			let points_text = points
				.iter()
				.map(|(px, py)| format!("{},{}", number(*px), number(*py)))
				.collect::<Vec<_>>()
				.join(" ");
			svg.open("polygon").attr("class", "tag-label-bkg").attr("points", &points_text);
			if !shape_transform.1.is_empty() {
				svg.attr("transform", &shape_transform.1);
			}
			self.styles.computed("tag-label-bkg").write(svg, None, None, self.context);
			svg.close();
			self.bounds.include_points(shape_transform.0, &points);
			svg.open("circle")
				.number_attr("cy", hole.1)
				.number_attr("cx", hole.0)
				.attr("r", "1.5")
				.attr("class", "tag-hole");
			if !shape_transform.1.is_empty() {
				svg.attr("transform", &shape_transform.1);
			}
			self.styles.computed("tag-hole").write(svg, None, None, self.context);
			svg.close();
			self.bounds.include(shape_transform.0, hole.0 - 1.5, hole.1 - 1.5, 3.0, 3.0);
			let run = TextRun {
				x: text_x,
				y: text_y,
				anchor: Anchor::Start,
				baseline: Baseline::Alphabetic,
				font: &font,
				content: tag,
			};
			svg.text(run, |svg| {
				svg.attr("class", "tag-label");
				if let Some(fill) = tag_style.get("fill") {
					svg.attr("fill", fill);
				}
				if !text_transform.1.is_empty() {
					svg.attr("transform", &text_transform.1);
				}
			});
			if let Some(text) = text_box(self.context, tag, &font, text_x, text_y) {
				self.bounds.include(text_transform.0, text.x, text.y, text.width, text.height);
			}
		}
	}

	/// `drawBranches`
	fn branches(&mut self, svg: &mut Svg, branches: &[String]) {
		let redux = self.groups.redux_geometry;
		let theme_limit = match self.context.theme.get("THEME_COLOR_LIMIT") {
			Some(Value::Number(limit)) if *limit >= 0.0 && limit.fract() == 0.0 => {
				Some(*limit as usize)
			}
			_ => None,
		};
		let limit = if redux { theme_limit } else { Some(THEME_COLOR_LIMIT) };
		let max_pos = self.layout.max_pos;
		let rotation_shift = if self.settings.rotate_commit_label { 30.0 } else { 0.0 };
		let (padding_x, padding_y) =
			if redux { (16.0, REDUX_BRANCH_LABEL_PADDING_Y) } else { (0.0, 0.0) };
		let neo_filter = match (self.context.look, redux) {
			(Look::Neo, true) => Some(format!("url(#{}-drop-shadow)", self.context.id)),
			(Look::Neo, false) => css_filter(&self.context.theme.css("dropShadow"), self.context),
			_ => None,
		};
		svg.open("g").end();
		for (index, name) in branches.iter().enumerate() {
			let color = class_number(color_index(index, limit, self.groups.color_theme));
			let Some(&(pos, _)) = self.layout.branch_positions.get(name) else { continue };
			let spine = match (self.layout.direction, redux) {
				(Direction::LeftRight, true) => pos + REDUX_BRANCH_LABEL_PADDING_Y / 2.0 + 1.0,
				(Direction::LeftRight, false) => pos - 2.0,
				_ => pos,
			};
			let (start, end) = match self.layout.direction {
				Direction::LeftRight => ((0.0, spine), (max_pos, spine)),
				Direction::TopBottom => ((pos, DEFAULT_POS), (pos, max_pos)),
				Direction::BottomTop => ((pos, max_pos), (pos, DEFAULT_POS)),
			};
			let classes = format!("branch branch{color}");
			svg.open("line")
				.number_attr("x1", start.0)
				.number_attr("y1", start.1)
				.number_attr("x2", end.0)
				.number_attr("y2", end.1)
				.attr("class", &classes);
			self.styles.computed(&classes).write(svg, None, None, self.context);
			svg.close();
			self.bounds.include_points(Transform::IDENTITY, &[start, end]);
			self.layout.lanes.push(spine);

			let label_classes = format!("label branch-label{color}");
			let label_style = self.styles.computed(&label_classes);
			let font = label_style.font(&self.font);
			let rows = label_rows(name);
			let (rows, text) = label_text(self.context, &rows, &font);
			let (width, height) = (text.width, text.height);
			let mut background = (
				-width - 4.0 - rotation_shift,
				-height / 2.0 + 10.0,
				width + 18.0 + padding_x,
				height + 4.0 + padding_y,
			);
			let mut label =
				(-width - 14.0 - rotation_shift + padding_x / 2.0, spine - height / 2.0 - 2.0);
			let mut background_shift = None;
			match self.layout.direction {
				Direction::TopBottom => {
					(background.0, background.1) = (pos - width / 2.0 - 10.0, 0.0);
					label = (pos - width / 2.0 - 5.0, 0.0);
					if redux {
						background_shift = Some((-padding_x / 2.0 - 3.0, -padding_y - 10.0));
						label.1 = -padding_y * 2.0 + 7.0;
					}
				}
				Direction::BottomTop => {
					(background.0, background.1) = (pos - width / 2.0 - 10.0, max_pos);
					label = (pos - width / 2.0 - 5.0, max_pos);
					if redux {
						background_shift = Some((-padding_x / 2.0 - 3.0, padding_y + 10.0));
						label.1 = max_pos + padding_y * 2.0 + 4.0;
					}
				}
				Direction::LeftRight => {
					background_shift = Some((-19.0, spine - 12.0 - padding_y / 2.0));
				}
			}
			let corner = if redux { 0.0 } else { 4.0 };
			let background_classes = format!("branchLabelBkg label{color}");
			let mut extra = String::new();
			let _ = write!(extra, " rx=\"{}\" ry=\"{}\"", number(corner), number(corner));
			let transform = match background_shift {
				Some((shift_x, shift_y)) => {
					let _ = write!(
						extra,
						" transform=\"translate({}, {})\"",
						number(shift_x),
						number(shift_y)
					);
					Transform::translate(shift_x, shift_y)
				}
				None => Transform::IDENTITY,
			};
			if let Some(filter) = &neo_filter {
				let _ = write!(extra, " filter=\"{filter}\"");
			}
			self.rect(
				svg,
				&background_classes,
				background,
				transform,
				&extra,
				neo_filter.is_some(),
			);

			let label_transform = Transform::translate(label.0, label.1);
			svg.open("g").attr("class", "branchLabel").end();
			svg.open("g")
				.attr("class", &label_classes)
				.attr("transform", &format!("translate({}, {})", number(label.0), number(label.1)));
			if let Some(fill) = label_style.get("fill") {
				svg.attr("fill", fill);
			}
			svg.end();
			for (row, baseline) in &rows {
				let run = TextRun {
					x: 0.0,
					y: *baseline,
					anchor: Anchor::Start,
					baseline: Baseline::Alphabetic,
					font: &font,
					content: row,
				};
				svg.text(run, |_| {});
			}
			if text.width > 0.0 || text.height > 0.0 {
				self.bounds.include(label_transform, text.x, text.y, text.width, text.height);
			}
			svg.close_tag("g").close_tag("g");
		}
		svg.close_tag("g");
	}

	/// A rect; with `has_own_filter`, its inline `filter` style overrides the style sheet's.
	fn rect(
		&mut self,
		svg: &mut Svg,
		classes: &str,
		rect: (f64, f64, f64, f64),
		transform: Transform,
		extra: &str,
		has_own_filter: bool,
	) {
		let (x, y, width, height) = rect;
		svg.open("rect").attr("class", classes);
		svg.number_attr("x", x)
			.number_attr("y", y)
			.number_attr("width", width)
			.number_attr("height", height);
		svg.out.push_str(extra);
		let mut style = self.styles.computed(classes);
		if has_own_filter {
			style.values.retain(|(property, _)| *property != "filter");
		}
		style.write(svg, None, None, self.context);
		svg.close();
		if width > 0.0 && height > 0.0 {
			self.bounds.include(transform, x, y, width, height);
		}
	}

	/// `drawArrow`
	fn arrow(&mut self, svg: &mut Svg, from: &Commit, to: &Commit) -> Result<()> {
		let positions = &self.layout.commit_positions;
		let (Some(&p1), Some(&p2)) = (positions.get(&from.id), positions.get(&to.id)) else {
			return Err(super::Error::new(format!(
				"Commit positions not found for commits {} and {}",
				from.id, to.id
			)));
		};
		let direction = self.layout.direction;
		let is_vertical = self.layout.is_vertical();
		// `shouldRerouteArrow`
		let to_is_furthest = if is_vertical { p1.x < p2.x } else { p1.y < p2.y };
		let curved_branch = if to_is_furthest { &to.branch } else { &from.branch };
		let needs_rerouting = self.layout.database.commits.iter().any(|commit| {
			commit.seq > from.seq && commit.seq < to.seq && commit.branch == *curved_branch
		});
		let is_merge_from_other = to.kind == CommitType::Merge
			&& to.parents.first().is_none_or(|first| *first != from.id);
		let mut color = self.layout.branch_index(&to.branch);
		if is_merge_from_other {
			color = self.layout.branch_index(&from.branch);
		}
		let n = number;
		let mut line = None;
		if needs_rerouting {
			let arc = "A 10 10, 0, 0, 0,";
			let arc2 = "A 10 10, 0, 0, 1,";
			let (radius, offset) = (10.0, 10.0);
			let line_y = if p1.y < p2.y {
				self.layout.find_lane(p1.y, p2.y)
			} else {
				self.layout.find_lane(p2.y, p1.y)
			};
			let line_x = if p1.x < p2.x {
				self.layout.find_lane(p1.x, p2.x)
			} else {
				self.layout.find_lane(p2.x, p1.x)
			};
			line = Some(match direction {
				Direction::TopBottom if p1.x < p2.x => format!(
					"M {} {} L {} {} {arc2} {} {} L {} {} {arc} {} {} L {} {}",
					n(p1.x),
					n(p1.y),
					n(line_x - radius),
					n(p1.y),
					n(line_x),
					n(p1.y + offset),
					n(line_x),
					n(p2.y - radius),
					n(line_x + offset),
					n(p2.y),
					n(p2.x),
					n(p2.y)
				),
				Direction::TopBottom => {
					color = self.layout.branch_index(&from.branch);
					format!(
						"M {} {} L {} {} {arc} {} {} L {} {} {arc2} {} {} L {} {}",
						n(p1.x),
						n(p1.y),
						n(line_x + radius),
						n(p1.y),
						n(line_x),
						n(p1.y + offset),
						n(line_x),
						n(p2.y - radius),
						n(line_x - offset),
						n(p2.y),
						n(p2.x),
						n(p2.y)
					)
				}
				Direction::BottomTop if p1.x < p2.x => format!(
					"M {} {} L {} {} {arc} {} {} L {} {} {arc2} {} {} L {} {}",
					n(p1.x),
					n(p1.y),
					n(line_x - radius),
					n(p1.y),
					n(line_x),
					n(p1.y - offset),
					n(line_x),
					n(p2.y + radius),
					n(line_x + offset),
					n(p2.y),
					n(p2.x),
					n(p2.y)
				),
				Direction::BottomTop => {
					color = self.layout.branch_index(&from.branch);
					format!(
						"M {} {} L {} {} {arc2} {} {} L {} {} {arc} {} {} L {} {}",
						n(p1.x),
						n(p1.y),
						n(line_x + radius),
						n(p1.y),
						n(line_x),
						n(p1.y - offset),
						n(line_x),
						n(p2.y + radius),
						n(line_x - offset),
						n(p2.y),
						n(p2.x),
						n(p2.y)
					)
				}
				Direction::LeftRight if p1.y < p2.y => format!(
					"M {} {} L {} {} {arc} {} {} L {} {} {arc2} {} {} L {} {}",
					n(p1.x),
					n(p1.y),
					n(p1.x),
					n(line_y - radius),
					n(p1.x + offset),
					n(line_y),
					n(p2.x - radius),
					n(line_y),
					n(p2.x),
					n(line_y + offset),
					n(p2.x),
					n(p2.y)
				),
				Direction::LeftRight => {
					color = self.layout.branch_index(&from.branch);
					format!(
						"M {} {} L {} {} {arc2} {} {} L {} {} {arc} {} {} L {} {}",
						n(p1.x),
						n(p1.y),
						n(p1.x),
						n(line_y + radius),
						n(p1.x + offset),
						n(line_y),
						n(p2.x - radius),
						n(line_y),
						n(p2.x),
						n(line_y - offset),
						n(p2.x),
						n(p2.y)
					)
				}
			});
		} else {
			let arc = "A 20 20, 0, 0, 0,";
			let arc2 = "A 20 20, 0, 0, 1,";
			let (radius, offset) = (20.0, 20.0);
			let straight = format!("M {} {} L {} {}", n(p1.x), n(p1.y), n(p2.x), n(p2.y));
			match direction {
				Direction::TopBottom => {
					if p1.x < p2.x {
						line = Some(match is_merge_from_other {
							true => format!(
								"M {} {} L {} {} {arc} {} {} L {} {}",
								n(p1.x),
								n(p1.y),
								n(p1.x),
								n(p2.y - radius),
								n(p1.x + offset),
								n(p2.y),
								n(p2.x),
								n(p2.y)
							),
							false => format!(
								"M {} {} L {} {} {arc2} {} {} L {} {}",
								n(p1.x),
								n(p1.y),
								n(p2.x - radius),
								n(p1.y),
								n(p2.x),
								n(p1.y + offset),
								n(p2.x),
								n(p2.y)
							),
						});
					}
					if p1.x > p2.x {
						line = Some(match is_merge_from_other {
							true => format!(
								"M {} {} L {} {} {arc2} {} {} L {} {}",
								n(p1.x),
								n(p1.y),
								n(p1.x),
								n(p2.y - radius),
								n(p1.x - offset),
								n(p2.y),
								n(p2.x),
								n(p2.y)
							),
							false => format!(
								"M {} {} L {} {} {arc} {} {} L {} {}",
								n(p1.x),
								n(p1.y),
								n(p2.x + radius),
								n(p1.y),
								n(p2.x),
								n(p1.y + offset),
								n(p2.x),
								n(p2.y)
							),
						});
					}
					if p1.x == p2.x {
						line = Some(straight);
					}
				}
				Direction::BottomTop => {
					if p1.x < p2.x {
						line = Some(match is_merge_from_other {
							true => format!(
								"M {} {} L {} {} {arc2} {} {} L {} {}",
								n(p1.x),
								n(p1.y),
								n(p1.x),
								n(p2.y + radius),
								n(p1.x + offset),
								n(p2.y),
								n(p2.x),
								n(p2.y)
							),
							false => format!(
								"M {} {} L {} {} {arc} {} {} L {} {}",
								n(p1.x),
								n(p1.y),
								n(p2.x - radius),
								n(p1.y),
								n(p2.x),
								n(p1.y - offset),
								n(p2.x),
								n(p2.y)
							),
						});
					}
					if p1.x > p2.x {
						line = Some(match is_merge_from_other {
							true => format!(
								"M {} {} L {} {} {arc} {} {} L {} {}",
								n(p1.x),
								n(p1.y),
								n(p1.x),
								n(p2.y + radius),
								n(p1.x - offset),
								n(p2.y),
								n(p2.x),
								n(p2.y)
							),
							false => format!(
								"M {} {} L {} {} {arc2} {} {} L {} {}",
								n(p1.x),
								n(p1.y),
								n(p2.x + radius),
								n(p1.y),
								n(p2.x),
								n(p1.y - offset),
								n(p2.x),
								n(p2.y)
							),
						});
					}
					if p1.x == p2.x {
						line = Some(straight);
					}
				}
				Direction::LeftRight => {
					if p1.y < p2.y {
						line = Some(match is_merge_from_other {
							true => format!(
								"M {} {} L {} {} {arc2} {} {} L {} {}",
								n(p1.x),
								n(p1.y),
								n(p2.x - radius),
								n(p1.y),
								n(p2.x),
								n(p1.y + offset),
								n(p2.x),
								n(p2.y)
							),
							false => format!(
								"M {} {} L {} {} {arc} {} {} L {} {}",
								n(p1.x),
								n(p1.y),
								n(p1.x),
								n(p2.y - radius),
								n(p1.x + offset),
								n(p2.y),
								n(p2.x),
								n(p2.y)
							),
						});
					}
					if p1.y > p2.y {
						line = Some(match is_merge_from_other {
							true => format!(
								"M {} {} L {} {} {arc} {} {} L {} {}",
								n(p1.x),
								n(p1.y),
								n(p2.x - radius),
								n(p1.y),
								n(p2.x),
								n(p1.y - offset),
								n(p2.x),
								n(p2.y)
							),
							false => format!(
								"M {} {} L {} {} {arc2} {} {} L {} {}",
								n(p1.x),
								n(p1.y),
								n(p1.x),
								n(p2.y + radius),
								n(p1.x + offset),
								n(p2.y),
								n(p2.x),
								n(p2.y)
							),
						});
					}
					if p1.y == p2.y {
						line = Some(straight);
					}
				}
			}
		}
		let Some(line) = line else {
			return Err(super::Error::new("Line definition not found"));
		};
		let classes = format!(
			"arrow arrow{}",
			class_number(color_index(color, Some(THEME_COLOR_LIMIT), self.groups.color_theme))
		);
		svg.open("path").attr("d", &line).attr("class", &classes);
		self.styles.computed(&classes).write(svg, None, None, self.context);
		svg.close();
		self.bounds.include_points(Transform::IDENTITY, &path_points(&line));
		Ok(())
	}
}

/// The end points of the segments of an arrow path. Its arcs are quarter circles between
/// their end points, which therefore bound them.
fn path_points(path: &str) -> Vec<(f64, f64)> {
	let mut numbers = Vec::new();
	let mut points = Vec::new();
	for part in path.split(['M', 'L']) {
		let part = part.trim();
		if part.is_empty() {
			continue;
		}
		numbers.clear();
		let segment = match part.find('A') {
			Some(arc) => {
				// `x y A rx ry, rotation, large, sweep, x y`: the start and the arc's end.
				let (line, arc) = part.split_at(arc);
				numbers
					.extend(line.split([' ', ',']).filter_map(|value| value.parse::<f64>().ok()));
				let arc_values: Vec<f64> = arc[1..]
					.split([' ', ','])
					.filter_map(|value| value.parse::<f64>().ok())
					.collect();
				numbers.extend(arc_values.iter().skip(5).copied());
				&numbers
			}
			None => {
				numbers
					.extend(part.split([' ', ',']).filter_map(|value| value.parse::<f64>().ok()));
				&numbers
			}
		};
		points.extend(segment.as_chunks::<2>().0.iter().map(|&[x, y]| (x, y)));
	}
	points
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let main_branch =
		context.config_text(SECTION, "mainBranchName").unwrap_or_else(|| "main".to_owned());
	let main_order = context.config_number(SECTION, "mainBranchOrder", 0.0);
	let database = parser::parse(code, context, &main_branch, main_order)?;
	let settings = Settings::new(context);
	let groups = ThemeGroups::new(&context.theme_name);
	let branches = database.sorted_branches();
	let font = Font::new(context.font_family(), context.font_size());
	let styles = StyleSheet::new(context, &groups, branches.len().max(THEME_COLOR_LIMIT));

	// The branch positions, measured with the plain font of the root.
	let mut layout = Layout {
		database: &database,
		direction: database.direction,
		redux_geometry: groups.redux_geometry,
		branch_positions: HashMap::new(),
		commit_positions: HashMap::new(),
		max_pos: 0.0,
		lanes: Vec::new(),
	};
	let mut pos = 0.0;
	for (index, name) in branches.iter().enumerate() {
		let (_, text) = label_text(context, &label_rows(name), &font);
		layout.branch_positions.insert(name.clone(), (pos, index));
		pos += 50.0
			+ if settings.rotate_commit_label { 40.0 } else { 0.0 }
			+ if layout.is_vertical() { text.width / 2.0 } else { 0.0 };
	}
	layout.place_commits(settings.parallel_commits)?;

	let mut renderer =
		Renderer { context, settings, groups, styles, layout, bounds: Bounds::default(), font };
	let mut body = Svg::new();
	write_defs(&mut body, renderer.context, &renderer.groups);
	if renderer.settings.show_branches {
		renderer.branches(&mut body, &branches);
	}
	body.open("g").attr("class", "commit-arrows").end();
	for commit in &database.commits {
		for parent in &commit.parents {
			if let Some(parent) = database.commit_by_id(parent) {
				renderer.arrow(&mut body, parent, commit)?;
			}
		}
	}
	body.close_tag("g");

	let placed = renderer.layout.place_commits(renderer.settings.parallel_commits)?;
	body.open("g").attr("class", "commit-bullets");
	let bullets_style = renderer.styles.computed("commit-bullets");
	if let Some(fill) = bullets_style.get("fill") {
		body.attr("fill", fill);
	}
	body.end();
	for &(index, position, _) in &placed {
		renderer.bullet(&mut body, &database.commits[index], position);
	}
	body.close_tag("g");
	body.open("g").attr("class", "commit-labels").end();
	for &(index, position, pos) in &placed {
		let commit = &database.commits[index];
		renderer.commit_label(&mut body, commit, position, pos);
		renderer.commit_tags(&mut body, commit, position, pos);
	}
	body.close_tag("g");

	if let Some(title) = database.common.title.as_deref().filter(|title| !title.is_empty()) {
		let (x, _, width, _) = renderer.bounds.rect();
		let style = renderer.styles.computed("gitTitleText");
		let font = style.font(&renderer.font);
		let (x, y) = (x + width / 2.0, -renderer.settings.title_top_margin);
		let run = TextRun {
			x,
			y,
			anchor: Anchor::Middle,
			baseline: Baseline::Alphabetic,
			font: &font,
			content: title,
		};
		body.text(run, |svg| {
			svg.attr("class", "gitTitleText");
			if let Some(fill) = style.get("fill") {
				svg.attr("fill", fill);
			}
		});
		// Blink's layout units: 1/64 pixel, rounded up.
		let advance = (renderer.context.measure(title, &font).0 * 64.0 - 0.001).ceil() / 64.0;
		if let Some(text) = text_box(renderer.context, title, &font, x - advance / 2.0, y) {
			renderer.bounds.include(Transform::IDENTITY, text.x, text.y, text.width, text.height);
		}
	}

	let (x, y, width, height) = renderer.bounds.rect();
	let padding = renderer.settings.diagram_padding;
	let frame = Frame {
		x: x - padding,
		y: y - padding,
		width: width + 2.0 * padding,
		height: height + 2.0 * padding,
		use_max_width: renderer.settings.use_max_width,
	};
	Ok(document(renderer.context, "gitGraph", &frame, &database.common.accessibility, body))
}

/// The gradient of the neo look and the drop shadow of the redux themes in the neo look, with
/// `feDropShadow` written as the primitives it is defined as, for EdgeHTML.
fn write_defs(svg: &mut Svg, context: &Context, groups: &ThemeGroups) {
	let theme = &context.theme;
	if matches!(theme.get("useGradient"), Some(Value::Bool(true))) {
		svg.open("defs").end();
		svg.open("linearGradient")
			.attr("id", &format!("{}-gradient", context.id))
			.attr("gradientUnits", "objectBoundingBox")
			.attr("x1", "0%")
			.attr("y1", "0%")
			.attr("x2", "100%")
			.attr("y2", "0%")
			.end();
		for (offset, key) in [("0%", "gradientStart"), ("100%", "gradientStop")] {
			svg.open("stop")
				.attr("offset", offset)
				.attr_if("stop-color", &theme.css(key))
				.attr("stop-opacity", "1")
				.close();
		}
		svg.close_tag("linearGradient").close_tag("defs");
	}
	if context.look == Look::Neo && groups.redux_geometry {
		svg.open("defs").end();
		svg.open("filter")
			.attr("id", &format!("{}-drop-shadow", context.id))
			.attr("height", "130%")
			.attr("width", "130%")
			.end();
		svg.open("feOffset")
			.attr("in", "SourceAlpha")
			.attr("dx", "4")
			.attr("dy", "4")
			.attr("result", "offset")
			.close();
		svg.open("feFlood")
			.attr("flood-opacity", "0.06")
			.attr_if("flood-color", &theme.css("filterColor"))
			.close();
		svg.open("feComposite").attr("in2", "offset").attr("operator", "in").close();
		svg.open("feMerge").end();
		svg.open("feMergeNode").close();
		svg.open("feMergeNode").attr("in", "SourceGraphic").close();
		svg.close_tag("feMerge").close_tag("filter").close_tag("defs");
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn splits_label_rows_like_draw_text() {
		assert_eq!(label_rows("a\\nb<BR />c\nd <br>"), ["a", "b", "c", "d", ""]);
		assert_eq!(label_rows("x<br"), ["x<br"]);
	}

	#[test]
	fn finds_path_points() {
		assert_eq!(
			path_points("M 60 -2 L 60 68 A 20 20, 0, 0, 0, 80 88 L 110 88"),
			[(60.0, -2.0), (60.0, 68.0), (80.0, 88.0), (110.0, 88.0)]
		);
	}

	#[test]
	fn computes_color_indexes() {
		assert_eq!(color_index(9, Some(8), false), Some(1));
		assert_eq!(color_index(8, Some(8), true), Some(1));
		assert_eq!(color_index(0, Some(8), true), Some(0));
		assert_eq!(color_index(3, None, false), None);
		assert_eq!(color_index(3, Some(1), true), None);
	}
}
