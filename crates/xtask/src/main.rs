//! Development tasks, run with `cargo xtask <task>`.

mod deploy;
mod emoji_table;
mod fontawesome;
mod header_parser;
mod icon_preview;
mod json;
mod mermaid_themes;
mod npp_interface;
mod plugin_build;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
Usage: cargo xtask <task>

Tasks:
  build <x64|x86> [<cargo build arguments>]
      Builds NppToolBox.dll in release mode. Links with <arch>-w64-mingw32-gcc, or with a
      MinGW-w64 gcc supporting -m64/-m32 if the former is missing.
  deploy <x64|x86> [<cargo build arguments>]
      Builds like build, checks that NppToolBox.dll imports only Windows system DLLs and copies
      it with LICENSE, README.md and THIRD-PARTY-NOTICES.md to
      target/<target>/package/NppToolBox.
  update-npp-interface [--version <version>]
      Regenerates crates/npp/src/interface.rs from the official Notepad++ headers and the
      language names of ScintillaEditView.cpp.
      <version> is e.g. 8.9.8. The default is the newest release (\"latest\").
  update-emoji [--revision <revision>]
      Regenerates crates/npp-toolbox/src/emoji_table.rs from GitHub's gemoji database.
      <revision> is a gemoji branch, tag or commit. The default is master.
  update-mermaid-themes [--version <version>]
      Regenerates crates/npp-toolbox/src/mermaid/theme_table.rs, Mermaid's themes compiled to
      bytecode. <version> is a Mermaid release, e.g. 12.2.0 (the default).
  update-fontawesome [--version <version>]
      Regenerates crates/npp-toolbox/src/mermaid/icon_table.rs, the Font Awesome Free icons in a
      compact byte code. <version> is a Font Awesome 6 release, e.g. 6.7.2 (the default).
  icon-preview [--icons <directory>] [--output <png>]
      Renders the toolbar icons in the common icon style for a design review and reports style
      violations. Defaults: crates/npp-toolbox/icons, target/icon-preview.png";

fn main() -> ExitCode {
	let arguments: Vec<String> = std::env::args().skip(1).collect();
	let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
	let workspace_root = workspace_root.canonicalize().unwrap_or(workspace_root);
	let result = match arguments.first().map(String::as_str) {
		Some("build") => plugin_build::build(
			&workspace_root,
			arguments.get(1).map(String::as_str),
			arguments.get(2..).unwrap_or_default(),
		)
		.map(drop),
		Some("deploy") => deploy::deploy(
			&workspace_root,
			arguments.get(1).map(String::as_str),
			arguments.get(2..).unwrap_or_default(),
		),
		Some("update-npp-interface") => option_value(&arguments, "--version")
			.and_then(|version| npp_interface::update(&workspace_root, version)),
		Some("update-emoji") => option_value(&arguments, "--revision")
			.and_then(|revision| emoji_table::update(&workspace_root, revision)),
		Some("update-mermaid-themes") => option_value(&arguments, "--version")
			.and_then(|version| mermaid_themes::update(&workspace_root, version)),
		Some("update-fontawesome") => option_value(&arguments, "--version")
			.and_then(|version| fontawesome::update(&workspace_root, version)),
		Some("icon-preview") => option_value(&arguments, "--icons").and_then(|icons| {
			let output = option_value(&arguments, "--output")?;
			icon_preview::write_preview(
				&icons
					.map_or_else(|| workspace_root.join("crates/npp-toolbox/icons"), PathBuf::from),
				&output
					.map_or_else(|| workspace_root.join("target/icon-preview.png"), PathBuf::from),
				&npp_build::icon_style::DEFAULT,
			)
		}),
		_ => Err(USAGE.to_owned()),
	};
	match result {
		Ok(()) => ExitCode::SUCCESS,
		Err(message) => {
			eprintln!("{message}");
			ExitCode::FAILURE
		}
	}
}

/// Value following `name`, if present. A missing value is an error.
fn option_value<'a>(arguments: &'a [String], name: &str) -> Result<Option<&'a str>, String> {
	match arguments.iter().position(|argument| argument == name) {
		None => Ok(None),
		Some(index) => arguments
			.get(index + 1)
			.map(|value| Some(value.as_str()))
			.ok_or_else(|| format!("{name} needs a value\n\n{USAGE}")),
	}
}
