use std::path::{Path, PathBuf};

const STYLE_SHEET: &str = "assets/github-markdown-light.css";
const RESOURCE_SCRIPT: &str = "NppToolBox.rc";

fn main() {
	let output_directory = PathBuf::from(std::env::var_os("OUT_DIR").expect("set by Cargo"));
	if let Err(error) = npp_build::generate_toolbar_icons(
		Path::new("icons"),
		&output_directory.join("toolbar_icons.rs"),
		&npp_build::icon_style::DEFAULT,
	) {
		// Printed as is, so every style violation ends up on its own line.
		eprintln!("{error}");
		std::process::exit(1);
	}

	println!("cargo:rerun-if-changed={STYLE_SHEET}");
	let style_sheet = std::fs::read_to_string(STYLE_SHEET).expect("readable style sheet");
	std::fs::write(
		output_directory.join("github-markdown-light.min.css"),
		npp_build::minify_css(&style_sheet),
	)
	.expect("writable OUT_DIR");

	if let Err(error) =
		npp_build::link_resource_script(Path::new(RESOURCE_SCRIPT), &output_directory)
	{
		eprintln!("{error}");
		std::process::exit(1);
	}
}
