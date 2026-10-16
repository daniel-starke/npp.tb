//! Compiles a Windows resource script (`.rc`) with MinGW-w64 `windres` and links it into the DLL.

use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

/// Name of the header generated next to the copied script. It defines `VERSION_MAJOR`,
/// `VERSION_MINOR`, `VERSION_PATCH` and `VERSION_STRING` from the package version.
pub const VERSION_HEADER: &str = "version.h";

/// Compiles `script` and links the resulting object into the `cdylib`. Does nothing for
/// non-Windows targets, so that the unit tests build on any host. Emits the
/// `cargo:rerun-if-changed` lines.
///
/// `windres` is taken from the `WINDRES` environment variable, otherwise
/// `<arch>-w64-mingw32-windres` and finally plain `windres` (MSYS2) are tried.
pub fn link_resource_script(script: &Path, output_directory: &Path) -> io::Result<()> {
	println!("cargo:rerun-if-changed={}", script.display());
	println!("cargo:rerun-if-env-changed=WINDRES");
	if env("CARGO_CFG_TARGET_OS")? != "windows" {
		return Ok(());
	}
	let (tool_prefix, object_format) = match env("CARGO_CFG_TARGET_ARCH")?.as_str() {
		"x86_64" => ("x86_64-w64-mingw32", "pe-x86-64"),
		"x86" => ("i686-w64-mingw32", "pe-i386"),
		architecture => {
			return Err(io::Error::other(format!("unsupported architecture {architecture}")));
		}
	};

	// `windres` hands its arguments to the preprocessor through a shell command line, which
	// breaks on paths with spaces. Working in the output directory with plain file names avoids
	// that, and the script finds the generated header next to itself.
	let script_name = script
		.file_name()
		.ok_or_else(|| io::Error::other(format!("{} is no file", script.display())))?;
	fs::copy(script, output_directory.join(script_name))?;
	fs::write(output_directory.join(VERSION_HEADER), version_header()?)?;
	let object_name = Path::new(script_name).with_extension("res.o");

	let candidates = match std::env::var("WINDRES") {
		Ok(windres) => vec![windres],
		Err(_) => vec![format!("{tool_prefix}-windres"), "windres".to_owned()],
	};
	let mut last_error = None;
	for windres in &candidates {
		let status = Command::new(windres)
			.current_dir(output_directory)
			.arg(format!("--target={object_format}"))
			.args(["--output-format=coff", "--input"])
			.arg(script_name)
			.arg("--output")
			.arg(&object_name)
			.status();
		match status {
			Ok(status) if status.success() => {
				// Objects given on the linker command line are linked completely, so the
				// `.rsrc` section survives `--gc-sections`.
				println!(
					"cargo:rustc-link-arg-cdylib={}",
					output_directory.join(&object_name).display()
				);
				return Ok(());
			}
			Ok(status) => {
				return Err(io::Error::other(format!(
					"{windres} failed for {} ({status})",
					script.display()
				)));
			}
			Err(error) => last_error = Some(format!("{windres}: {error}")),
		}
	}
	Err(io::Error::other(format!(
		"cannot run windres from MinGW-w64 binutils ({})",
		last_error.unwrap_or_default()
	)))
}

fn version_header() -> io::Result<String> {
	Ok(format!(
		"#define VERSION_MAJOR {}\n#define VERSION_MINOR {}\n#define VERSION_PATCH {}\n#define VERSION_STRING \"{}\"\n",
		env("CARGO_PKG_VERSION_MAJOR")?,
		env("CARGO_PKG_VERSION_MINOR")?,
		env("CARGO_PKG_VERSION_PATCH")?,
		env("CARGO_PKG_VERSION")?,
	))
}

fn env(name: &str) -> io::Result<String> {
	std::env::var(name).map_err(|_| io::Error::other(format!("{name} is not set by Cargo")))
}
