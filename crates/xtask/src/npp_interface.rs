//! `update-npp-interface`: regenerates `crates/npp/src/interface.rs` from the official
//! Notepad++ headers of a given release.
//!
//! Only names and values are taken over, so the GPL licensed Notepad++ headers are not
//! redistributed. Downloads use `curl` (part of Windows 10+, macOS and most Linux systems) and are
//! cached in `target/npp-interface/<tag>/`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::header_parser::{parse_header, struct_fields};

const REPOSITORY_URL: &str = "https://github.com/notepad-plus-plus/notepad-plus-plus";
const RAW_FILES_URL: &str = "https://raw.githubusercontent.com/notepad-plus-plus/notepad-plus-plus";
pub const GENERATED_FILE: &str = "crates/npp/src/interface.rs";
const STRUCT_LINE_PREFIX: &str = "// struct ";

struct InterfaceHeader {
	repository_path: &'static str,
	module_name: &'static str,
	/// Structs mirrored by hand in `npp::ffi`. Snapshotted to detect layout changes.
	mirrored_structs: &'static [&'static str],
}

/// In dependency order: later headers may refer to constants of earlier ones.
const INTERFACE_HEADERS: [InterfaceHeader; 4] = [
	InterfaceHeader {
		repository_path: "scintilla/include/Scintilla.h",
		module_name: "scintilla",
		mirrored_structs: &["Sci_CharacterRangeFull", "Sci_TextRangeFull", "Sci_NotifyHeader"],
	},
	InterfaceHeader {
		repository_path: "PowerEditor/src/menuCmdID.h",
		module_name: "menu_cmd_id",
		mirrored_structs: &[],
	},
	InterfaceHeader {
		repository_path: "PowerEditor/src/MISC/PluginsManager/Notepad_plus_msgs.h",
		module_name: "notepad_plus_msgs",
		mirrored_structs: &["toolbarIconsWithDarkMode"],
	},
	InterfaceHeader {
		repository_path: "PowerEditor/src/MISC/PluginsManager/PluginInterface.h",
		module_name: "plugin_interface",
		mirrored_structs: &["NppData", "ShortcutKey", "FuncItem"],
	},
];

/// Windows SDK macros the headers build on.
const SYSTEM_MACROS: [(&str, i64); 1] = [("WM_USER", 0x0400)];

/// `requested_version`: e.g. `8.9.8`, `v8.9.8` or `latest` (also when `None`).
pub fn update(workspace_root: &Path, requested_version: Option<&str>) -> Result<(), String> {
	let tag = match requested_version {
		None | Some("latest") => resolve_latest_release_tag()?,
		Some(version) => format!("v{}", version.trim_start_matches('v')),
	};
	let version = tag.trim_start_matches('v');
	let cache_directory = workspace_root.join("target").join("npp-interface").join(&tag);
	fs::create_dir_all(&cache_directory).map_err(|error| error.to_string())?;

	let mut known: HashMap<String, i64> =
		SYSTEM_MACROS.iter().map(|&(name, value)| (name.to_owned(), value)).collect();
	let mut modules = String::new();
	let mut struct_lines = Vec::new();
	for header in &INTERFACE_HEADERS {
		let file_name = header.repository_path.rsplit('/').next().unwrap_or(header.repository_path);
		let cached_file = cache_directory.join(file_name);
		if !cached_file.exists() {
			download(&format!("{RAW_FILES_URL}/{tag}/{}", header.repository_path), &cached_file)?;
		}
		let source = fs::read_to_string(&cached_file).map_err(|error| error.to_string())?;
		let parsed = parse_header(&source, &mut known);
		write_module(&mut modules, header, file_name, &parsed);
		for &struct_name in header.mirrored_structs {
			let fields = struct_fields(&source, struct_name)
				.ok_or_else(|| format!("struct {struct_name} not found in {file_name}"))?;
			struct_lines
				.push(format!("{STRUCT_LINE_PREFIX}{struct_name} {{ {}; }}", fields.join("; ")));
		}
	}

	let generated_file = workspace_root.join(GENERATED_FILE);
	let previous_struct_lines: Vec<String> = fs::read_to_string(&generated_file)
		.unwrap_or_default()
		.lines()
		.filter(|line| line.starts_with(STRUCT_LINE_PREFIX))
		.map(str::to_owned)
		.collect();
	fs::write(&generated_file, generated_source(version, &tag, &modules, &struct_lines))
		.map_err(|error| error.to_string())?;
	run_rustfmt(&generated_file)?;

	println!("Updated {GENERATED_FILE} to Notepad++ {version}.");
	if !previous_struct_lines.is_empty() && previous_struct_lines != struct_lines {
		println!(
			"WARNING: struct declarations changed. Review the hand written layouts in crates/npp/src/ffi.rs:"
		);
		for line in struct_lines.iter().filter(|line| !previous_struct_lines.contains(line)) {
			println!("  {line}");
		}
	}
	Ok(())
}

fn write_module(
	output: &mut String,
	header: &InterfaceHeader,
	file_name: &str,
	parsed: &crate::header_parser::ParsedHeader,
) {
	writeln!(output, "\n/// Constants from `{file_name}`.\npub mod {} {{", header.module_name)
		.unwrap();
	for (name, value) in &parsed.constants {
		writeln!(output, "\t{}", constant_item(name, *value)).unwrap();
	}
	for enumeration in &parsed.enums {
		writeln!(
			output,
			"\n\t/// Members of `enum {}`.\n\tpub mod {} {{",
			enumeration.name,
			snake_case(&enumeration.name)
		)
		.unwrap();
		for (name, value) in &enumeration.members {
			writeln!(output, "\t\t{}", constant_item(name, *value)).unwrap();
		}
		output.push_str("\t}\n");
	}
	output.push_str("}\n");
}

fn generated_source(version: &str, tag: &str, modules: &str, struct_lines: &[String]) -> String {
	format!(
		"//! Constants of the Notepad++ {version} plugin interface.\n\
		//!\n\
		//! Generated by `cargo xtask update-npp-interface --version {version}` from the official \
		headers at {REPOSITORY_URL}/tree/{tag}. Do not edit.\n\
		//! Names are kept as in the headers. Non-negative values are `u32`, negative ones `i64`.\n\
		#![allow(non_upper_case_globals)]\n\n\
		pub const NOTEPAD_PLUS_PLUS_VERSION: &str = \"{version}\";\n\
		{modules}\n\
		// Struct declarations of this version. `ffi.rs` mirrors them by hand and must be reviewed\n\
		// whenever one of the following lines changes.\n\
		{}\n",
		struct_lines.join("\n")
	)
}

fn constant_item(name: &str, value: i64) -> String {
	if u32::try_from(value).is_ok() {
		format!("pub const {name}: u32 = {value};")
	} else {
		format!("pub const {name}: i64 = {value};")
	}
}

fn snake_case(name: &str) -> String {
	let mut snake = String::new();
	for (index, character) in name.char_indices() {
		if character.is_ascii_uppercase() && index > 0 {
			snake.push('_');
		}
		snake.push(character.to_ascii_lowercase());
	}
	snake
}

/// GitHub redirects `releases/latest/download/<file>` to the newest release's tag.
fn resolve_latest_release_tag() -> Result<String, String> {
	let headers = run_curl(&["-sSI", &format!("{REPOSITORY_URL}/releases/latest/download/probe")])
		.unwrap_or_default();
	let from_redirect = headers
		.lines()
		.find(|line| line.to_ascii_lowercase().starts_with("location:"))
		.and_then(|line| line.split("/releases/download/").nth(1))
		.and_then(|rest| rest.split('/').next())
		.map(str::to_owned);
	match from_redirect {
		Some(tag) => Ok(tag),
		None => newest_tag_from_git(),
	}
}

fn newest_tag_from_git() -> Result<String, String> {
	let output = Command::new("git")
		.args(["ls-remote", "--tags", "--refs", REPOSITORY_URL])
		.output()
		.map_err(|error| format!("cannot run git to list releases: {error}"))?;
	let listing = String::from_utf8_lossy(&output.stdout);
	listing
		.lines()
		.filter_map(|line| line.rsplit("refs/tags/").next())
		.filter_map(|tag| Some((version_numbers(tag)?, tag.to_owned())))
		.max()
		.map(|(_, tag)| tag)
		.ok_or_else(|| "cannot determine the newest Notepad++ release".to_owned())
}

/// `v8.9.8.1` → `[8, 9, 8, 1]`. `None` for other tags (e.g. release candidates).
fn version_numbers(tag: &str) -> Option<Vec<u64>> {
	tag.strip_prefix('v')?.split('.').map(|part| part.parse().ok()).collect()
}

pub fn download(url: &str, destination: &Path) -> Result<(), String> {
	let destination_text = destination.to_string_lossy();
	run_curl(&["-sSfL", "-o", &destination_text, url])
		.map(drop)
		.map_err(|error| format!("downloading {url} failed: {error}"))
}

fn run_curl(arguments: &[&str]) -> Result<String, String> {
	let output = Command::new("curl")
		.args(arguments)
		.output()
		.map_err(|error| format!("cannot run curl: {error}"))?;
	if output.status.success() {
		Ok(String::from_utf8_lossy(&output.stdout).into_owned())
	} else {
		Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
	}
}

pub fn run_rustfmt(file: &Path) -> Result<(), String> {
	let status = Command::new("rustfmt")
		.args(["--edition", "2024"])
		.arg(file)
		.status()
		.map_err(|error| format!("cannot run rustfmt: {error}"))?;
	status.success().then_some(()).ok_or_else(|| "rustfmt failed".to_owned())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn orders_release_tags_numerically() {
		assert!(version_numbers("v8.9.10") > version_numbers("v8.9.8.1"));
		assert_eq!(version_numbers("v8.9.8.1"), Some(vec![8, 9, 8, 1]));
		assert_eq!(version_numbers("v8.9-RC"), None);
	}

	#[test]
	fn converts_enum_names_to_module_names() {
		assert_eq!(snake_case("LangType"), "lang_type");
		assert_eq!(snake_case("winVer"), "win_ver");
	}
}
