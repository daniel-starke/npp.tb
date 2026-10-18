//! Release build of the plugin DLL with MinGW-w64 linker detection.
//!
//! `.cargo/config.toml` links with `<prefix>-gcc`. When that is missing, a plain multilib `gcc`
//! that targets MinGW-w64 with `-m32`/`-m64` (e.g. a 64-bit MinGW-w64 GCC on Windows) is used
//! instead.

use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Stdio};

struct Architecture {
	name: &'static str,
	target: &'static str,
	tool_prefix: &'static str,
	gcc_flag: &'static str,
	/// Predefined by GCC when it generates code for this architecture.
	architecture_macro: &'static str,
}

const ARCHITECTURES: [Architecture; 2] = [
	Architecture {
		name: "x64",
		target: "x86_64-pc-windows-gnu",
		tool_prefix: "x86_64-w64-mingw32",
		gcc_flag: "-m64",
		architecture_macro: "__x86_64__",
	},
	Architecture {
		name: "x86",
		target: "i686-pc-windows-gnu",
		tool_prefix: "i686-w64-mingw32",
		gcc_flag: "-m32",
		architecture_macro: "__i386__",
	},
];

const FALLBACK_GCC: &str = "gcc";

/// Builds `NppToolBox.dll` for `architecture_name` (`x64` or `x86`) in release mode. Further
/// `cargo_arguments` are passed to `cargo build`.
pub fn build(
	workspace_root: &Path,
	architecture_name: Option<&str>,
	cargo_arguments: &[String],
) -> Result<(), String> {
	let architecture = ARCHITECTURES
		.iter()
		.find(|architecture| Some(architecture.name) == architecture_name)
		.ok_or_else(|| "build needs the architecture x64 or x86".to_owned())?;
	let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
	let mut command = Command::new(cargo);
	command
		.current_dir(workspace_root)
		.args(["build", "--release", "--package", "npp-toolbox", "--target"])
		.arg(architecture.target)
		.args(cargo_arguments);

	let linker_variable =
		format!("CARGO_TARGET_{}_LINKER", architecture.target.to_uppercase().replace('-', "_"));
	let prefixed_gcc = format!("{}-gcc", architecture.tool_prefix);
	if std::env::var_os(&linker_variable).is_none() && !runs(&prefixed_gcc) {
		let link_arguments = multilib_gcc_link_arguments(architecture).ok_or_else(|| {
			format!(
				"neither {prefixed_gcc} nor a MinGW-w64 {FALLBACK_GCC} supporting {} was found",
				architecture.gcc_flag
			)
		})?;
		eprintln!(
			"{prefixed_gcc} not found, linking with {FALLBACK_GCC} {}",
			architecture.gcc_flag
		);
		// Cargo appends environment flags to the `rustflags` of `.cargo/config.toml`.
		let flags_variable = format!(
			"CARGO_TARGET_{}_RUSTFLAGS",
			architecture.target.to_uppercase().replace('-', "_")
		);
		let mut flags = std::env::var(&flags_variable).unwrap_or_default();
		for link_argument in link_arguments {
			flags.push_str(" -C link-arg=");
			flags.push_str(&link_argument);
		}
		command.env(linker_variable, FALLBACK_GCC).env(flags_variable, flags.trim_start());
	}

	let status = command.status().map_err(|error| format!("cannot run cargo: {error}"))?;
	if status.success() { Ok(()) } else { Err(format!("cargo build failed ({status})")) }
}

fn runs(program: &str) -> bool {
	Command::new(program)
		.arg("-dumpversion")
		.stdout(Stdio::null())
		.stderr(Stdio::null())
		.status()
		.is_ok_and(|status| status.success())
}

/// Link arguments that make the plain `gcc` link for `architecture`, or `None` if it cannot.
fn multilib_gcc_link_arguments(architecture: &Architecture) -> Option<Vec<String>> {
	let output = Command::new(FALLBACK_GCC)
		.args([architecture.gcc_flag, "-dM", "-E", "-x", "c", "-"])
		.stdin(Stdio::null())
		.stderr(Stdio::null())
		.output()
		.ok()
		.filter(|output| output.status.success())?;
	let definitions = String::from_utf8_lossy(&output.stdout);
	let macros = predefined_macros(&definitions);
	if !macros.contains("__MINGW32__") || !macros.contains(architecture.architecture_macro) {
		return None;
	}
	let mut link_arguments = vec![architecture.gcc_flag.to_owned()];
	if macros.contains("__USING_SJLJ_EXCEPTIONS__") {
		link_arguments.extend(sjlj_link_arguments(user_label_prefix(&definitions)));
	}
	Some(link_arguments)
}

/// A 64-bit MinGW-w64 GCC built with SEH exceptions has an SJLJ `libgcc` for `-m32`, while Rust
/// expects the DWARF unwinder. With `panic = "abort"` nothing unwinds, so the `_Unwind_Resume`
/// of the precompiled standard library only needs to resolve. The SJLJ unwinder of `libgcc` uses
/// `winpthreads`, which Rust links before `libgcc`, so it is linked again (statically) after it.
fn sjlj_link_arguments(user_label_prefix: &str) -> [String; 4] {
	[
		format!("-Wl,--defsym,{user_label_prefix}_Unwind_Resume={user_label_prefix}abort"),
		"-l:libpthread.a".to_owned(),
		"-lmsvcrt".to_owned(),
		"-lkernel32".to_owned(),
	]
}

fn predefined_macros(definitions: &str) -> HashSet<&str> {
	definitions
		.lines()
		.filter_map(|line| line.strip_prefix("#define ")?.split_whitespace().next())
		.collect()
}

/// Prefix of C symbols in object files (`_` on x86, empty on x64).
fn user_label_prefix(definitions: &str) -> &str {
	definitions
		.lines()
		.find_map(|line| line.strip_prefix("#define __USER_LABEL_PREFIX__"))
		.map_or("", str::trim)
}

#[cfg(test)]
mod tests {
	use super::*;

	const MULTILIB_X86_MACROS: &str = "\
#define __MINGW32__ 1
#define __USING_SJLJ_EXCEPTIONS__ 1
#define __USER_LABEL_PREFIX__ _
#define __i386__ 1
";

	#[test]
	fn reads_macro_names() {
		let macros = predefined_macros(MULTILIB_X86_MACROS);
		assert!(macros.contains("__MINGW32__"));
		assert!(macros.contains("__USING_SJLJ_EXCEPTIONS__"));
		assert!(!macros.contains("__x86_64__"));
	}

	#[test]
	fn reads_user_label_prefix() {
		assert_eq!(user_label_prefix(MULTILIB_X86_MACROS), "_");
		assert_eq!(user_label_prefix("#define __USER_LABEL_PREFIX__ \n"), "");
		assert_eq!(user_label_prefix("#define __MINGW32__ 1\n"), "");
	}

	#[test]
	fn resolves_unwind_resume_to_abort() {
		assert_eq!(sjlj_link_arguments("_")[0], "-Wl,--defsym,__Unwind_Resume=_abort");
	}
}
