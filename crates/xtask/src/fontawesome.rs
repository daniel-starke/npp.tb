//! `update-fontawesome`: regenerates `crates/npp-toolbox/src/mermaid/icon_table.rs`, the Font
//! Awesome Free icons in a compact bit code, from the metadata of the Font Awesome repository.
//!
//! The downloads are cached in `target/fontawesome/<version>/`.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use crate::json::{JsonValue, parse_json};
use crate::npp_interface::{download, run_rustfmt};

const RAW_FILES_URL: &str = "https://raw.githubusercontent.com/FortAwesome/Font-Awesome";
const ICONS_PATH: &str = "metadata/icon-families.json";
const SHIMS_PATH: &str = "css/v4-shims.css";
const DEFAULT_VERSION: &str = "6.7.2";
const GENERATED_FILE: &str = "crates/npp-toolbox/src/mermaid/icon_table.rs";
/// The height of all icons, which the plugin relies on.
const ICON_HEIGHT: f64 = 512.0;
/// The styles of the free classic family, in the order of the plugin's `Style`.
const STYLES: [&str; 3] = ["solid", "regular", "brands"];
/// The path commands in the order of their code.
const COMMANDS: &[u8] = b"MmLlHhVvCcSsQqTtAaZz";
/// The code of a reference to a shared sub-path, after the commands.
const SHARED_CODE: u64 = 20;
/// The code that ends an icon or a shared sub-path.
const END_CODE: u64 = 21;
/// The bits of a command code.
const CODE_BITS: u32 = 5;
/// The bits of the width of an icon.
const WIDTH_BITS: u32 = 10;
/// The bits of the decimals of a command and of the bit count of its numbers.
const SCALE_BITS: u32 = 2;
const NUMBER_WIDTH_BITS: u32 = 5;
/// The most decimals of a number; others are rounded, which is at most 1/2000 of a unit of the
/// 512 high view box.
const MAX_DECIMALS: u32 = 3;

/// `version`: a Font Awesome release of major version 6. 6.7.2 when `None`.
pub fn update(workspace_root: &Path, version: Option<&str>) -> Result<(), String> {
	let version = version.unwrap_or(DEFAULT_VERSION);
	let cache_directory = workspace_root.join("target/fontawesome").join(version);
	fs::create_dir_all(&cache_directory)
		.map_err(|error| format!("cannot create {}: {error}", cache_directory.display()))?;
	let icons_text = cached_download(&cache_directory, version, ICONS_PATH)?;
	let shims_text = cached_download(&cache_directory, version, SHIMS_PATH)?;

	let icons = icons_of(&parse_json(&icons_text)?)?;
	let shims = parse_shims(&shims_text)?;
	let table = Table::new(icons, &shims)?;
	let generated_file = workspace_root.join(GENERATED_FILE);
	fs::write(&generated_file, table.generate(version))
		.map_err(|error| format!("cannot write {}: {error}", generated_file.display()))?;
	run_rustfmt(&generated_file)?;
	println!(
		"{GENERATED_FILE}: {} icons, {} names, {} shims, {} bytes of paths from Font Awesome Free {version}",
		table.path_starts.len(),
		table.names_of_styles.iter().map(Vec::len).sum::<usize>(),
		table.shims.len(),
		table.paths.len(),
	);
	println!("{} sub-paths are shared", table.shared_starts.len());
	Ok(())
}

fn cached_download(directory: &Path, version: &str, path: &str) -> Result<String, String> {
	let file_name = path.rsplit('/').next().unwrap_or(path);
	let file = directory.join(file_name);
	if !file.exists() {
		download(&format!("{RAW_FILES_URL}/{version}/{path}"), &file)?;
	}
	fs::read_to_string(&file).map_err(|error| format!("cannot read {}: {error}", file.display()))
}

#[derive(Debug, PartialEq)]
struct Icon {
	name: String,
	/// An index into [`STYLES`].
	style: usize,
	aliases: Vec<String>,
	/// The code point of the icon font.
	unicode: String,
	width: u32,
	path: String,
}

/// The free icons of the classic family.
fn icons_of(families: &JsonValue) -> Result<Vec<Icon>, String> {
	let JsonValue::Object(entries) = families else {
		return Err(format!("{ICONS_PATH}: expected an object of icons"));
	};
	let mut icons = Vec::new();
	for (name, entry) in entries {
		let Some(JsonValue::Array(free)) =
			entry.field("familyStylesByLicense").and_then(|licenses| licenses.field("free"))
		else {
			continue;
		};
		for family_style in free {
			let text_of = |field: &str| match family_style.field(field) {
				Some(JsonValue::String(text)) => Ok(text.as_str()),
				_ => Err(format!("{ICONS_PATH}: {name}: free style without {field:?}")),
			};
			let (family, style_name) = (text_of("family")?, text_of("style")?);
			if family != "classic" {
				return Err(format!("{ICONS_PATH}: {name}: unexpected family {family:?}"));
			}
			let Some(style) = STYLES.iter().position(|known| *known == style_name) else {
				return Err(format!("{ICONS_PATH}: {name}: unexpected style {style_name:?}"));
			};
			icons.push(icon_of(name, entry, style)?);
		}
	}
	Ok(icons)
}

fn icon_of(name: &str, entry: &JsonValue, style: usize) -> Result<Icon, String> {
	let error = |message: &str| format!("{ICONS_PATH}: {name}: {message}");
	let svg = entry
		.field("svgs")
		.and_then(|svgs| svgs.field("classic"))
		.and_then(|classic| classic.field(STYLES[style]))
		.ok_or_else(|| error("no SVG of its style"))?;
	let (Some(JsonValue::Number(width)), Some(JsonValue::Number(height))) =
		(svg.field("width"), svg.field("height"))
	else {
		return Err(error("SVG without a size"));
	};
	if *height != ICON_HEIGHT || width.fract() != 0.0 || *width <= 0.0 {
		return Err(error(&format!("unexpected SVG size {width} x {height}")));
	}
	let Some(JsonValue::String(path)) = svg.field("path") else {
		return Err(error("SVG without a single path"));
	};
	let mut aliases = Vec::new();
	if let Some(JsonValue::Array(names)) =
		entry.field("aliases").and_then(|aliases| aliases.field("names"))
	{
		for alias in names {
			let JsonValue::String(alias) = alias else {
				return Err(error("alias is not a string"));
			};
			aliases.push(alias.clone());
		}
	}
	for candidate in std::iter::once(&name.to_owned()).chain(&aliases) {
		let is_name_byte =
			|byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-';
		if !candidate.bytes().all(is_name_byte) {
			return Err(error(&format!("unexpected icon name {candidate:?}")));
		}
	}
	let Some(JsonValue::String(unicode)) = entry.field("unicode") else {
		return Err(error("no code point"));
	};
	Ok(Icon {
		name: name.to_owned(),
		style,
		aliases,
		unicode: unicode.clone(),
		width: *width as u32,
		path: path.clone(),
	})
}

/// A name of Font Awesome 4: the glyph that `v4-shims.css` gives the classes `fa fa-<name>`.
#[derive(Debug, PartialEq)]
struct Shim {
	/// An index into [`STYLES`].
	style: usize,
	/// The code point, if the name is not the one of the icon.
	unicode: Option<String>,
}

/// `css/v4-shims.css`: the rules for `.fa.fa-<name>` set the font (the brands font, or a weight of
/// 400 for the regular style, else the solid style of `.fa`) and the code point. Fails on any
/// other rule, so that a change of the format does not go unnoticed.
fn parse_shims(text: &str) -> Result<BTreeMap<String, Shim>, String> {
	#[derive(Default)]
	struct Declarations {
		is_brands: bool,
		is_regular: bool,
		unicode: Option<String>,
	}
	let mut declarations: BTreeMap<String, Declarations> = BTreeMap::new();
	let mut rest = text;
	while let Some(start) = rest.find("/*") {
		let (before, comment) = rest.split_at(start);
		if !before.trim().is_empty() {
			break;
		}
		let end = comment.find("*/").ok_or_else(|| format!("{SHIMS_PATH}: open comment"))?;
		rest = &comment[end + 2..];
	}
	while let Some((selectors, after)) = rest.split_once('{') {
		let error = |what: &str, text: &str| format!("{SHIMS_PATH}: unexpected {what} {text:?}");
		let (body, after) =
			after.split_once('}').ok_or_else(|| error("end of the rule", selectors))?;
		rest = after;
		for selector in selectors.split(',') {
			let name = selector
				.trim()
				.strip_prefix(".fa.fa-")
				.ok_or_else(|| error("selector", selector))?;
			let entry = declarations.entry(name.to_owned()).or_default();
			for declaration in body.split(';').filter(|text| !text.trim().is_empty()) {
				let (key, value) =
					declaration.split_once(':').ok_or_else(|| error("declaration", declaration))?;
				match (key.trim(), value.trim()) {
					("font-family", "'Font Awesome 6 Brands'") => entry.is_brands = true,
					("font-family", "'Font Awesome 6 Free'") => {}
					("font-weight", "400") => entry.is_regular = true,
					("--fa", code) => {
						let code = code.strip_prefix("\"\\").and_then(|c| c.strip_suffix('"'));
						let code = code.ok_or_else(|| error("declaration", declaration))?;
						entry.unicode = Some(code.to_ascii_lowercase());
					}
					_ => return Err(error("declaration", declaration)),
				}
			}
		}
	}
	if !rest.trim().is_empty() {
		return Err(format!("{SHIMS_PATH}: unexpected text at the end: {rest:?}"));
	}
	Ok(declarations
		.into_iter()
		.map(|(name, entry)| {
			let style = match (entry.is_brands, entry.is_regular) {
				(true, _) => 2,
				(false, true) => 1,
				(false, false) => 0,
			};
			(name, Shim { style, unicode: entry.unicode })
		})
		.collect())
}

/// The tables as the plugin reads them.
#[derive(Debug)]
struct Table {
	/// The names, each ended by a line end.
	names: String,
	/// `(start of the name, icon)` per style, sorted by name.
	names_of_styles: [Vec<(u16, u16)>; 3],
	/// `(start of the name, icon)`, sorted by name.
	shims: Vec<(u16, u16)>,
	/// The bit position of each icon in `paths`.
	path_starts: Vec<u32>,
	/// The bit position of each shared sub-path in `paths`.
	shared_starts: Vec<u32>,
	/// The bits of the index of a shared sub-path.
	shared_index_bits: u32,
	paths: Vec<u8>,
}

impl Table {
	fn new(icons: Vec<Icon>, shims: &BTreeMap<String, Shim>) -> Result<Self, String> {
		let mut sub_paths_of_icons = Vec::new();
		let mut icons_of_styles: [BTreeMap<String, u16>; 3] = Default::default();
		let mut icons_of_code_points: BTreeMap<(usize, &str), u16> = BTreeMap::new();
		for (index, icon) in icons.iter().enumerate() {
			let index = u16::try_from(index).map_err(|_| "too many icons".to_owned())?;
			if icon.width >= 1 << WIDTH_BITS {
				return Err(format!("{ICONS_PATH}: {}: too wide", icon.name));
			}
			let commands = parse_path(&icon.path)
				.map_err(|error| format!("{ICONS_PATH}: {}: {error}", icon.name))?;
			sub_paths_of_icons.push(sub_paths(commands));
			icons_of_code_points.insert((icon.style, &icon.unicode), index);
			for name in std::iter::once(&icon.name).chain(&icon.aliases) {
				icons_of_styles[icon.style].insert(name.clone(), index);
			}
		}
		let paths = PathTable::new(&icons, &sub_paths_of_icons)?;

		let mut shim_icons = BTreeMap::new();
		for (name, shim) in shims {
			let icon = match &shim.unicode {
				Some(code) => icons_of_code_points.get(&(shim.style, code.as_str())),
				None => icons_of_styles[shim.style].get(name),
			};
			let Some(&icon) = icon else {
				return Err(format!(
					"{SHIMS_PATH}: {name}: no icon {:?} in style {}",
					shim.unicode, STYLES[shim.style]
				));
			};
			shim_icons.insert(name.clone(), icon);
		}

		// Styles share the text of the names they have in common.
		let mut names = String::new();
		let mut starts: BTreeMap<&str, u16> = BTreeMap::new();
		let all_names = icons_of_styles.iter().flat_map(BTreeMap::keys).chain(shim_icons.keys());
		for name in all_names {
			if !starts.contains_key(name.as_str()) {
				let start = u16::try_from(names.len()).map_err(|_| "too many names".to_owned())?;
				starts.insert(name, start);
				names.push_str(name);
				names.push('\n');
			}
		}
		let entries = |icons: &BTreeMap<String, u16>| -> Vec<(u16, u16)> {
			icons.iter().map(|(name, &icon)| (starts[name.as_str()], icon)).collect()
		};
		let names_of_styles = [
			entries(&icons_of_styles[0]),
			entries(&icons_of_styles[1]),
			entries(&icons_of_styles[2]),
		];
		let shims = entries(&shim_icons);
		Ok(Self {
			names,
			names_of_styles,
			shims,
			path_starts: paths.path_starts,
			shared_starts: paths.shared_starts,
			shared_index_bits: paths.shared_index_bits,
			paths: paths.bits.bytes,
		})
	}

	fn generate(&self, version: &str) -> String {
		let mut source = format!(
			"//! The icons of Font Awesome Free {version}, generated by `cargo xtask update-fontawesome`.\n\
			 //! Do not edit by hand.\n\
			 //!\n\
			 //! Font Awesome Free: <https://fontawesome.com/license/free>, icons CC BY 4.0 License,\n\
			 //! Copyright 2024 Fonticons, Inc.\n\n\
			 /// The names, each ended by a line end.\n\
			 pub static NAMES: &str = \"\\\n"
		);
		push_text_lines(&mut source, self.names.as_bytes());
		source.push_str("\";\n\n");
		for (style, entries) in STYLES.iter().zip(&self.names_of_styles) {
			let _ = writeln!(
				source,
				"/// `(start of the name, icon)` of the {style} style, sorted by name."
			);
			push_entries(&mut source, &style.to_uppercase(), entries);
		}
		source.push_str(
			"/// `(start of the name, icon)` of the names of version 4, sorted by name.\n",
		);
		push_entries(&mut source, "SHIMS", &self.shims);
		let _ = writeln!(
			source,
			"/// The bit position of each icon in [`PATHS`].\n\
			 pub static PATH_STARTS: [u32; {}] = {:?};\n\n\
			 /// The bit position of each shared sub-path in [`PATHS`].\n\
			 pub static SHARED_STARTS: [u32; {}] = {:?};\n\n\
			 /// The bits of the index of a shared sub-path.\n\
			 pub const SHARED_INDEX_BITS: u32 = {};\n",
			self.path_starts.len(),
			self.path_starts,
			self.shared_starts.len(),
			self.shared_starts,
			self.shared_index_bits,
		);
		source.push_str(
			"/// The icons and the shared sub-paths as bits, each value with its lowest bit first. An icon\n\
			 /// is its width (10 bits) and its commands, a shared sub-path only commands. A command is\n\
			 /// a code of 5 bits: the index in `MmLlHhVvCcSsQqTtAaZz`, 20 for a shared sub-path (followed\n\
			 /// by its index) or 21 for the end. An arc is followed by its large arc and sweep flags. A\n\
			 /// command with numbers then has the count of their decimals (2 bits), the bit count of\n\
			 /// each number (5 bits) and the numbers, as zigzag coded integers of these decimals.\n\
			 pub static PATHS: &[u8] = b\"\\\n",
		);
		push_text_lines(&mut source, &self.paths);
		source.push_str("\";\n");
		source
	}
}

fn push_entries(source: &mut String, name: &str, entries: &[(u16, u16)]) {
	let _ = writeln!(source, "pub static {name}: [(u16, u16); {}] = {entries:?};\n", entries.len());
}

/// `bytes` as the inside of a Rust string literal in lines ended by a continuation, which skips
/// the whitespace at the start of the next line, so that spaces are always escaped.
fn push_text_lines(source: &mut String, bytes: &[u8]) {
	const LINE_LENGTH: usize = 96;
	let mut length = 0;
	for &byte in bytes {
		let start = source.len();
		match byte {
			b'"' => source.push_str("\\\""),
			b'\\' => source.push_str("\\\\"),
			b'\n' => source.push_str("\\n"),
			b'!'..=b'~' => source.push(byte as char),
			_ => {
				let _ = write!(source, "\\x{byte:02x}");
			}
		}
		length += source.len() - start;
		if length >= LINE_LENGTH {
			source.push_str("\\\n");
			length = 0;
		}
	}
}

/// A path command as the table stores it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
struct PathCommand {
	/// The index in [`COMMANDS`].
	code: u8,
	is_large_arc: bool,
	is_sweep: bool,
	/// The decimals of all numbers.
	scale: u32,
	/// The numbers as integers of `scale` decimals, without the flags of an arc.
	numbers: Vec<i64>,
}

impl PathCommand {
	/// Numbers with up to [`MAX_DECIMALS`] decimals, which take the most decimals among them.
	fn new(
		code: u8,
		is_large_arc: bool,
		is_sweep: bool,
		values: impl Iterator<Item = f64>,
	) -> Self {
		let digits: Vec<(i64, u32)> = values.map(digits_of).collect();
		let scale = digits.iter().map(|&(_, decimals)| decimals).max().unwrap_or(0);
		let numbers =
			digits.iter().map(|&(digits, decimals)| digits * 10i64.pow(scale - decimals)).collect();
		let is_arc = COMMANDS[usize::from(code)].eq_ignore_ascii_case(&b'a');
		Self {
			code,
			is_large_arc: is_arc && is_large_arc,
			is_sweep: is_arc && is_sweep,
			scale,
			numbers,
		}
	}

	fn is_arc(&self) -> bool {
		COMMANDS[usize::from(self.code)].eq_ignore_ascii_case(&b'a')
	}

	fn is_move(&self) -> bool {
		COMMANDS[usize::from(self.code)].eq_ignore_ascii_case(&b'm')
	}

	/// The bits of each number: enough for the largest one.
	fn number_width(&self) -> u32 {
		let largest = self.numbers.iter().map(|&number| zigzag(number)).max().unwrap_or(0);
		u64::BITS - largest.leading_zeros()
	}

	fn bit_count(&self) -> usize {
		let arc_bits = if self.is_arc() { 2 } else { 0 };
		let number_bits = match self.numbers.is_empty() {
			true => 0,
			false => {
				SCALE_BITS + NUMBER_WIDTH_BITS + self.number_width() * self.numbers.len() as u32
			}
		};
		(CODE_BITS + arc_bits + number_bits) as usize
	}

	fn write(&self, bits: &mut BitWriter) {
		bits.push(u64::from(self.code), CODE_BITS);
		if self.is_arc() {
			bits.push(u64::from(self.is_large_arc), 1);
			bits.push(u64::from(self.is_sweep), 1);
		}
		if !self.numbers.is_empty() {
			let width = self.number_width();
			bits.push(u64::from(self.scale), SCALE_BITS);
			bits.push(u64::from(width), NUMBER_WIDTH_BITS);
			for &number in &self.numbers {
				bits.push(zigzag(number), width);
			}
		}
	}
}

fn zigzag(value: i64) -> u64 {
	((value << 1) ^ (value >> 63)) as u64
}

/// The digits of `value` rounded to [`MAX_DECIMALS`] decimals, and the fewest decimals they need.
fn digits_of(value: f64) -> (i64, u32) {
	let mut digits = (value * 10f64.powi(MAX_DECIMALS as i32)).round() as i64;
	let mut decimals = MAX_DECIMALS;
	while decimals > 0 && digits % 10 == 0 {
		digits /= 10;
		decimals -= 1;
	}
	(digits, decimals)
}

/// The commands from each move to the next.
fn sub_paths(commands: Vec<PathCommand>) -> Vec<Vec<PathCommand>> {
	let mut sub_paths: Vec<Vec<PathCommand>> = Vec::new();
	for command in commands {
		match sub_paths.last_mut() {
			Some(sub_path) if !command.is_move() => sub_path.push(command),
			_ => sub_paths.push(vec![command]),
		}
	}
	sub_paths
}

fn bit_count(sub_path: &[PathCommand]) -> usize {
	sub_path.iter().map(PathCommand::bit_count).sum()
}

/// Bits, each value with its lowest bit first.
#[derive(Debug, Default)]
struct BitWriter {
	bytes: Vec<u8>,
	length: usize,
}

impl BitWriter {
	fn push(&mut self, value: u64, count: u32) {
		for bit in 0..count {
			if self.length.is_multiple_of(8) {
				self.bytes.push(0);
			}
			if value >> bit & 1 != 0
				&& let Some(last) = self.bytes.last_mut()
			{
				*last |= 1 << (self.length % 8);
			}
			self.length += 1;
		}
	}

	fn position(&self) -> Result<u32, String> {
		u32::try_from(self.length).map_err(|_| "too many paths".to_owned())
	}
}

/// The paths of all icons. A sub-path that occurs often enough is stored once after the icons,
/// and the icons refer to it.
struct PathTable {
	path_starts: Vec<u32>,
	shared_starts: Vec<u32>,
	shared_index_bits: u32,
	bits: BitWriter,
}

impl PathTable {
	fn new(icons: &[Icon], sub_paths_of_icons: &[Vec<Vec<PathCommand>>]) -> Result<Self, String> {
		// The repeated sub-paths in the order of their first use, and how often they occur.
		let mut counts: HashMap<&[PathCommand], usize> = HashMap::new();
		let mut repeated = Vec::new();
		for sub_path in sub_paths_of_icons.iter().flatten() {
			let count = counts.entry(sub_path).or_default();
			*count += 1;
			if *count == 2 {
				repeated.push(sub_path.as_slice());
			}
		}
		let index_bits = |count: usize| usize::BITS - count.saturating_sub(1).leading_zeros();
		// Each use costs a reference; the stored sub-path adds an end code and its start.
		let saves = |sub_path: &[PathCommand], index_bits: u32| {
			let count = counts[sub_path];
			let reference = (CODE_BITS + index_bits) as usize;
			(count - 1) * bit_count(sub_path) > count * reference + CODE_BITS as usize + 32
		};
		let candidates: Vec<&[PathCommand]> = repeated
			.iter()
			.copied()
			.filter(|sub_path| saves(sub_path, index_bits(repeated.len())))
			.collect();
		let shared_index_bits = index_bits(candidates.len());
		let shared: Vec<&[PathCommand]> =
			candidates.into_iter().filter(|sub_path| saves(sub_path, shared_index_bits)).collect();
		let shared_index: HashMap<&[PathCommand], u64> =
			shared.iter().zip(0..).map(|(&sub_path, index)| (sub_path, index)).collect();

		let mut bits = BitWriter::default();
		let mut path_starts = Vec::new();
		for (icon, sub_paths) in icons.iter().zip(sub_paths_of_icons) {
			path_starts.push(bits.position()?);
			bits.push(u64::from(icon.width), WIDTH_BITS);
			for sub_path in sub_paths {
				match shared_index.get(sub_path.as_slice()) {
					Some(&index) => {
						bits.push(SHARED_CODE, CODE_BITS);
						bits.push(index, shared_index_bits);
					}
					None => sub_path.iter().for_each(|command| command.write(&mut bits)),
				}
			}
			bits.push(END_CODE, CODE_BITS);
		}
		let mut shared_starts = Vec::new();
		for sub_path in shared {
			shared_starts.push(bits.position()?);
			sub_path.iter().for_each(|command| command.write(&mut bits));
			bits.push(END_CODE, CODE_BITS);
		}
		// The reader loads 8 bytes at once.
		bits.bytes.extend_from_slice(&[0; 7]);
		Ok(Self { path_starts, shared_starts, shared_index_bits, bits })
	}
}

/// The number of arguments of a path command.
fn argument_count(command: u8) -> usize {
	match command.to_ascii_uppercase() {
		b'H' | b'V' => 1,
		b'M' | b'L' | b'T' => 2,
		b'S' | b'Q' => 4,
		b'C' => 6,
		b'A' => 7,
		_ => 0,
	}
}

/// Reads the path data `path` as commands, with every command written out.
fn parse_path(path: &str) -> Result<Vec<PathCommand>, String> {
	let mut reader = PathReader { bytes: path.as_bytes(), position: 0 };
	let mut command = None;
	let mut commands = Vec::new();
	while reader.skip_separators() {
		let next = reader.bytes[reader.position];
		if next.is_ascii_alphabetic() {
			reader.position += 1;
			command = Some(next);
			if next.eq_ignore_ascii_case(&b'z') {
				let code = command_code(next)?;
				commands.push(PathCommand { code, ..PathCommand::default() });
				continue;
			}
		}
		let Some(current) = command.filter(|command| !command.eq_ignore_ascii_case(&b'z')) else {
			return Err(format!("number without a command at byte {}", reader.position));
		};
		let is_arc = current.eq_ignore_ascii_case(&b'a');
		let code = command_code(current)?;
		let mut numbers = [0.0; 7];
		let count = argument_count(current);
		for (index, number) in numbers.iter_mut().enumerate().take(count) {
			*number = match is_arc && (index == 3 || index == 4) {
				true => reader.flag()?,
				false => reader.number()?,
			};
		}
		let values = numbers
			.iter()
			.enumerate()
			.take(count)
			.filter(|&(index, _)| !(is_arc && (index == 3 || index == 4)))
			.map(|(_, &number)| number);
		let parsed = PathCommand::new(code, numbers[3] != 0.0, numbers[4] != 0.0, values);
		if parsed.number_width() >= 1 << NUMBER_WIDTH_BITS {
			return Err(format!("number too large before byte {}", reader.position));
		}
		commands.push(parsed);
		// Further coordinates after a moveto are implicit linetos.
		match current {
			b'M' => command = Some(b'L'),
			b'm' => command = Some(b'l'),
			_ => {}
		}
	}
	Ok(commands)
}

fn command_code(command: u8) -> Result<u8, String> {
	COMMANDS
		.iter()
		.position(|&known| known == command)
		.map(|index| index as u8)
		.ok_or_else(|| format!("unexpected path command {:?}", command as char))
}

struct PathReader<'a> {
	bytes: &'a [u8],
	position: usize,
}

impl PathReader<'_> {
	/// Whether anything follows the separators.
	fn skip_separators(&mut self) -> bool {
		while self.bytes.get(self.position).is_some_and(|byte| b" ,\t\r\n".contains(byte)) {
			self.position += 1;
		}
		self.position < self.bytes.len()
	}

	fn flag(&mut self) -> Result<f64, String> {
		self.skip_separators();
		let flag = self.bytes.get(self.position).copied();
		self.position += 1;
		match flag {
			Some(b'0') => Ok(0.0),
			Some(b'1') => Ok(1.0),
			_ => Err(format!("expected an arc flag at byte {}", self.position - 1)),
		}
	}

	fn digits(&mut self) {
		while self.bytes.get(self.position).is_some_and(u8::is_ascii_digit) {
			self.position += 1;
		}
	}

	fn sign(&mut self) {
		if self.bytes.get(self.position).is_some_and(|byte| b"+-".contains(byte)) {
			self.position += 1;
		}
	}

	fn number(&mut self) -> Result<f64, String> {
		self.skip_separators();
		let start = self.position;
		self.sign();
		self.digits();
		if self.bytes.get(self.position) == Some(&b'.') {
			self.position += 1;
			self.digits();
		}
		if self.bytes.get(self.position).is_some_and(|byte| b"eE".contains(byte)) {
			self.position += 1;
			self.sign();
			self.digits();
		}
		std::str::from_utf8(&self.bytes[start..self.position])
			.ok()
			.and_then(|text| text.parse().ok())
			.ok_or_else(|| format!("expected a number at byte {start}"))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn parsed(path: &str) -> Vec<PathCommand> {
		parse_path(path).expect("valid path")
	}

	fn command(code: u8, scale: u32, numbers: &[i64]) -> PathCommand {
		PathCommand { code, scale, numbers: numbers.to_vec(), ..PathCommand::default() }
	}

	/// `count` bits of `bytes` from the bit `position`, lowest first.
	fn bits_at(bytes: &[u8], position: usize, count: usize) -> u64 {
		(0..count).fold(0, |value, bit| {
			let index = position + bit;
			value | u64::from(bytes[index / 8] >> (index % 8) & 1) << bit
		})
	}

	#[test]
	fn parses_commands_with_one_scale_of_numbers() {
		// The numbers of a command share the most decimals among them.
		assert_eq!(parsed("M1-1.5"), [command(0, 1, &[10, -15])]);
		assert_eq!(parsed("M1 0z"), [command(0, 0, &[1, 0]), command(19, 0, &[])]);
		assert_eq!(parsed("h.125"), [command(5, 3, &[125])]);
		assert_eq!(parsed("h.12504"), parsed("h.125"));
		assert_eq!(parsed("h-200"), [command(5, 0, &[-200])]);
	}

	#[test]
	fn splits_implicit_commands_and_keeps_arc_flags() {
		assert_eq!(parsed("M1 1 2 2"), parsed("M1 1L2 2"));
		assert_eq!(parsed("m1 1 2 2"), parsed("m1 1l2 2"));
		assert_eq!(parsed("c1 1 1 1 1 1 2 2 2 2 2 2"), parsed("c1 1 1 1 1 1c2 2 2 2 2 2"));
		assert_eq!(parsed("a1 1 0 011 1"), parsed("a1,1,0,0,1,1,1"));
		let arc = &parsed("a1 1 0 011 1")[0];
		assert_eq!(
			(arc.code, arc.is_large_arc, arc.is_sweep, arc.numbers.len()),
			(17, false, true, 5)
		);
		let arc = &parsed("a1 1 0 1 0 1 1")[0];
		assert_eq!((arc.is_large_arc, arc.is_sweep), (true, false));
		assert_eq!(parsed("l.5.5"), parsed("l.5 .5"));
	}

	#[test]
	fn rejects_broken_paths() {
		assert!(parse_path("1 2").is_err());
		assert!(parse_path("M1").is_err());
		assert!(parse_path("X1 2").is_err());
		assert!(parse_path("a1 1 0 2 1 1 1").is_err());
		// More than 31 bits per number.
		assert!(parse_path("h2000000.001").is_err());
		assert!(parse_path("h1000000.001").is_ok());
	}

	#[test]
	fn writes_bits_lowest_first() {
		let mut bits = BitWriter::default();
		bits.push(0b101, 3);
		bits.push(1, 6);
		assert_eq!((bits.bytes.as_slice(), bits.length), (&[0b1101, 0][..], 9));
		let mut bits = BitWriter::default();
		command(10, 1, &[-15, 2]).write(&mut bits);
		// `C`, one decimal, 5 bits (zigzag 29 and 4), then the numbers.
		let expected = [(10, 5), (1, 2), (5, 5), (29, 5), (4, 5)];
		let mut position = 0;
		for (value, count) in expected {
			assert_eq!(bits_at(&bits.bytes, position, count), value);
			position += count;
		}
		assert_eq!(bits.length, position);
		assert_eq!(command(10, 1, &[-15, 2]).bit_count(), position);
	}

	#[test]
	fn shares_repeated_sub_paths() {
		let long = "M10 10h100v100h-100z";
		let icon = |path: String| Icon {
			name: "a".to_owned(),
			style: 0,
			aliases: Vec::new(),
			unicode: "f000".to_owned(),
			width: 512,
			path,
		};
		let icons =
			[icon(format!("{long}M1 1h1z")), icon(long.to_owned()), icon(format!("M2 2h2z{long}"))];
		let sub_paths_of_icons: Vec<_> =
			icons.iter().map(|icon| sub_paths(parsed(&icon.path))).collect();
		let table = PathTable::new(&icons, &sub_paths_of_icons).expect("valid paths");
		assert_eq!(table.shared_starts.len(), 1);
		assert_eq!(table.shared_index_bits, 0);
		// The second icon: its width, the reference without index bits, the end.
		let start = table.path_starts[1] as usize;
		let bytes = &table.bits.bytes;
		assert_eq!(bits_at(bytes, start, 10), 512);
		assert_eq!(bits_at(bytes, start + 10, 5), SHARED_CODE);
		assert_eq!(bits_at(bytes, start + 15, 5), END_CODE);
		assert_eq!(table.path_starts[2] as usize, start + 20);
		// The shared sub-path comes after the icons: its commands and the end.
		let shared = table.shared_starts[0] as usize;
		assert_eq!(bits_at(bytes, shared, 5), 0);
		let length = bit_count(&sub_paths_of_icons[1][0]);
		assert_eq!(bits_at(bytes, shared + length, 5), END_CODE);
		// Sub-paths that occur once, or are too short to pay for a reference, stay inline.
		let icons = [icon("M1 1z".to_owned()), icon("M1 1z".to_owned()), icon(long.to_owned())];
		let sub_paths_of_icons: Vec<_> =
			icons.iter().map(|icon| sub_paths(parsed(&icon.path))).collect();
		assert!(
			PathTable::new(&icons, &sub_paths_of_icons).expect("valid").shared_starts.is_empty()
		);
	}

	#[test]
	fn parses_shims() {
		let shims = parse_shims(concat!(
			"/*!\n * License\n */\n.fa.fa-glass {\n  --fa: \"\\f000\"; }\n\n",
			".fa.fa-clock-o {\n  font-family: 'Font Awesome 6 Free';\n  font-weight: 400; }\n\n",
			".fa.fa-clock-o {\n  --fa: \"\\F017\"; }\n\n",
			".fa.fa-twitter, .fa.fa-x {\n  font-family: 'Font Awesome 6 Brands';\n  font-weight: 400; }\n",
		))
		.expect("valid shims");
		assert_eq!(shims["glass"], Shim { style: 0, unicode: Some("f000".to_owned()) });
		assert_eq!(shims["clock-o"], Shim { style: 1, unicode: Some("f017".to_owned()) });
		assert_eq!(shims["twitter"], Shim { style: 2, unicode: None });
		assert_eq!(shims["x"], Shim { style: 2, unicode: None });
		assert!(parse_shims(".fa { --fa: \"\\f000\"; }").is_err());
		assert!(parse_shims(".fa.fa-a { color: red; }").is_err());
		assert!(parse_shims(".fa.fa-a { --fa: f000; }").is_err());
		assert!(parse_shims(".fa.fa-a { font-family: 'Other'; }").is_err());
		assert!(parse_shims(".fa.fa-a { --fa: \"\\f000\";").is_err());
		assert!(parse_shims("/* open").is_err());
		assert!(parse_shims(".fa.fa-a { } trailing").is_err());
	}

	fn test_icons() -> Vec<Icon> {
		let families = parse_json(
			r#"{"car": {"aliases": {"names": ["automobile"]}, "unicode": "f1b9",
				"familyStylesByLicense": {"free": [{"family": "classic", "style": "solid"}]},
				"svgs": {"classic": {"solid": {"width": 512, "height": 512, "path": "M1 1z"}}}},
			"clock": {"unicode": "f017", "familyStylesByLicense": {"free": [
				{"family": "classic", "style": "solid"}, {"family": "classic", "style": "regular"}]},
				"svgs": {"classic": {
					"solid": {"width": 448, "height": 512, "path": "M2 2z"},
					"regular": {"width": 448, "height": 512, "path": "M3 3z"}}}},
			"pro": {"familyStylesByLicense": {"free": []}}}"#,
		)
		.expect("valid JSON");
		icons_of(&families).expect("valid icons")
	}

	#[test]
	fn collects_free_icons_per_style() {
		let icons = test_icons();
		assert_eq!(icons.len(), 3);
		assert_eq!(icons[0].aliases, ["automobile"]);
		assert_eq!((icons[2].name.as_str(), icons[2].style, icons[2].width), ("clock", 1, 448));
	}

	#[test]
	fn builds_sorted_tables_with_shared_names() {
		let shims = parse_shims(concat!(
			".fa.fa-clock-o { font-weight: 400; }\n.fa.fa-clock-o { --fa: \"\\f017\"; }\n",
			".fa.fa-automobile { --fa: \"\\f1b9\"; }\n",
		))
		.expect("valid shims");
		let table = Table::new(test_icons(), &shims).expect("valid table");
		let name_at = |start: u16| {
			table.names[usize::from(start)..].split('\n').next().expect("a name").to_owned()
		};
		let names: Vec<(String, u16)> =
			table.names_of_styles[0].iter().map(|&(start, icon)| (name_at(start), icon)).collect();
		assert_eq!(
			names,
			[("automobile".to_owned(), 0), ("car".to_owned(), 0), ("clock".to_owned(), 1)]
		);
		assert_eq!(name_at(table.names_of_styles[1][0].0), "clock");
		assert_eq!(table.names_of_styles[1][0], (table.names_of_styles[0][2].0, 2));
		let shims: Vec<(String, u16)> =
			table.shims.iter().map(|&(start, icon)| (name_at(start), icon)).collect();
		assert_eq!(shims, [("automobile".to_owned(), 0), ("clock-o".to_owned(), 2)]);
		assert_eq!(table.path_starts.len(), 3);
		// The width comes first.
		assert_eq!(bits_at(&table.paths, table.path_starts[1] as usize, 10), 448);
		assert!(table.generate("6.7.2").contains("PATH_STARTS: [u32; 3]"));
	}

	#[test]
	fn rejects_shims_of_missing_icons() {
		let shims = parse_shims(".fa.fa-a { --fa: \"\\f000\"; }").expect("valid shims");
		assert!(Table::new(test_icons(), &shims).is_err());
		let shims = parse_shims(".fa.fa-car { font-weight: 400; }").expect("valid shims");
		assert!(Table::new(test_icons(), &shims).is_err());
	}

	#[test]
	fn writes_string_literal_lines() {
		let mut source = String::new();
		push_text_lines(&mut source, b"a \"\\\n\xff");
		assert_eq!(source, "a\\x20\\\"\\\\\\n\\xff");
	}
}
