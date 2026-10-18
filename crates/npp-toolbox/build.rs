use std::path::{Path, PathBuf};

const STYLE_SHEETS: [&str; 2] = ["github-markdown-light", "github-markdown-dark"];
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

	for name in STYLE_SHEETS {
		let path = format!("assets/{name}.css");
		println!("cargo:rerun-if-changed={path}");
		let style_sheet = std::fs::read_to_string(&path).expect("readable style sheet");
		std::fs::write(
			output_directory.join(format!("{name}.min.css")),
			npp_build::minify_css(&style_sheet),
		)
		.expect("writable OUT_DIR");
	}

	if let Err(error) =
		npp_build::link_resource_script(Path::new(RESOURCE_SCRIPT), &output_directory)
	{
		eprintln!("{error}");
		std::process::exit(1);
	}
}
