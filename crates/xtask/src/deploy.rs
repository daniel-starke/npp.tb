//! Release package of the plugin: build, check the imported DLLs and copy the files that are
//! shipped into `target/<target>/package/NppToolBox`.

use std::path::Path;

use crate::plugin_build;

/// Files of the workspace root shipped next to the DLL.
const PACKAGED_DOCUMENTS: [&str; 3] = ["LICENSE", "README.md", "THIRD-PARTY-NOTICES.md"];

/// The folder name must match the DLL name for Notepad++ to load the plugin.
const PACKAGE_FOLDER: &str = "NppToolBox";

/// DLLs that every supported Windows version provides. `api-ms-win-*` API sets are accepted too.
const SYSTEM_DLLS: [&str; 12] = [
	"kernel32.dll",
	"user32.dll",
	"gdi32.dll",
	"ntdll.dll",
	"msvcrt.dll",
	"advapi32.dll",
	"ole32.dll",
	"oleaut32.dll",
	"shell32.dll",
	"dwmapi.dll",
	"dwrite.dll",
	"bcryptprimitives.dll",
];

/// Builds `NppToolBox.dll` for `architecture_name`, checks its imports and writes the package.
pub fn deploy(
	workspace_root: &Path,
	architecture_name: Option<&str>,
	cargo_arguments: &[String],
) -> Result<(), String> {
	let release_directory =
		plugin_build::build(workspace_root, architecture_name, cargo_arguments)?;
	let dll_path = release_directory.join(plugin_build::DLL_NAME);
	let dll = std::fs::read(&dll_path)
		.map_err(|error| format!("cannot read {}: {error}", dll_path.display()))?;
	let imports = imported_dlls(&dll)
		.ok_or_else(|| format!("{} is not a valid PE file", dll_path.display()))?;
	let unexpected: Vec<&str> =
		imports.iter().map(String::as_str).filter(|name| !is_system_dll(name)).collect();
	if !unexpected.is_empty() {
		return Err(format!(
			"{} imports non-system DLLs: {}",
			plugin_build::DLL_NAME,
			unexpected.join(", ")
		));
	}

	let package_directory = release_directory.with_file_name("package").join(PACKAGE_FOLDER);
	if package_directory.exists() {
		std::fs::remove_dir_all(&package_directory)
			.map_err(|error| format!("cannot remove {}: {error}", package_directory.display()))?;
	}
	std::fs::create_dir_all(&package_directory)
		.map_err(|error| format!("cannot create {}: {error}", package_directory.display()))?;
	let sources = std::iter::once((dll_path, plugin_build::DLL_NAME)).chain(
		PACKAGED_DOCUMENTS.iter().map(|&document| (workspace_root.join(document), document)),
	);
	for (source, name) in sources {
		std::fs::copy(&source, package_directory.join(name))
			.map_err(|error| format!("cannot copy {}: {error}", source.display()))?;
	}
	eprintln!("Imports: {}", imports.join(", "));
	let shown_directory =
		package_directory.strip_prefix(workspace_root).unwrap_or(&package_directory);
	eprintln!("Package: {}", shown_directory.display());
	Ok(())
}

fn is_system_dll(name: &str) -> bool {
	SYSTEM_DLLS.contains(&name) || (name.starts_with("api-ms-win-") && name.ends_with(".dll"))
}

/// Lower case names of the DLLs in the import and delay import tables of a PE image, sorted and
/// without duplicates, or `None` if `image` is malformed.
fn imported_dlls(image: &[u8]) -> Option<Vec<String>> {
	const IMPORT_DIRECTORY: usize = 1;
	const DELAY_IMPORT_DIRECTORY: usize = 13;
	const IMPORT_DESCRIPTOR_SIZE: usize = 20;
	const IMPORT_NAME_OFFSET: usize = 12;
	const DELAY_IMPORT_DESCRIPTOR_SIZE: usize = 32;
	const DELAY_IMPORT_NAME_OFFSET: usize = 4;

	let pe = Pe::parse(image)?;
	let mut names = Vec::new();
	for (directory, descriptor_size, name_offset) in [
		(IMPORT_DIRECTORY, IMPORT_DESCRIPTOR_SIZE, IMPORT_NAME_OFFSET),
		(DELAY_IMPORT_DIRECTORY, DELAY_IMPORT_DESCRIPTOR_SIZE, DELAY_IMPORT_NAME_OFFSET),
	] {
		let Some(table_rva) = pe.directory_rva(directory) else {
			continue;
		};
		let mut table = pe.offset_of_rva(table_rva)?;
		loop {
			let descriptor = image.get(table..table + descriptor_size)?;
			if descriptor.iter().all(|&byte| byte == 0) {
				break;
			}
			let name = pe.offset_of_rva(read_u32(descriptor, name_offset)?)?;
			let name = image.get(name..)?;
			let name = &name[..name.iter().position(|&byte| byte == 0)?];
			names.push(String::from_utf8_lossy(name).to_ascii_lowercase());
			table += descriptor_size;
		}
	}
	names.sort();
	names.dedup();
	Some(names)
}

struct Pe<'a> {
	image: &'a [u8],
	data_directories: &'a [u8],
	sections: &'a [u8],
}

impl<'a> Pe<'a> {
	fn parse(image: &'a [u8]) -> Option<Self> {
		const PE_HEADER_POINTER: usize = 0x3c;
		const COFF_HEADER_SIZE: usize = 20;
		const PE32_MAGIC: u16 = 0x10b;
		const PE32_PLUS_MAGIC: u16 = 0x20b;

		if image.get(..2)? != b"MZ" {
			return None;
		}
		let signature = read_u32(image, PE_HEADER_POINTER)? as usize;
		if image.get(signature..signature + 4)? != b"PE\0\0" {
			return None;
		}
		let coff_header = signature + 4;
		let section_count = read_u16(image, coff_header + 2)? as usize;
		let optional_header_size = read_u16(image, coff_header + 16)? as usize;
		let optional_header = coff_header + COFF_HEADER_SIZE;
		let data_directories_offset = match read_u16(image, optional_header)? {
			PE32_MAGIC => 96,
			PE32_PLUS_MAGIC => 112,
			_ => return None,
		};
		let data_directories = image.get(
			optional_header + data_directories_offset..optional_header + optional_header_size,
		)?;
		let section_table = optional_header + optional_header_size;
		let sections =
			image.get(section_table..section_table + section_count * SECTION_HEADER_SIZE)?;
		Some(Self { image, data_directories, sections })
	}

	/// RVA of the data directory `index`, if the image has it.
	fn directory_rva(&self, index: usize) -> Option<u32> {
		let rva = read_u32(self.data_directories, index * 8)?;
		let size = read_u32(self.data_directories, index * 8 + 4)?;
		(rva != 0 && size != 0).then_some(rva)
	}

	/// File offset of the relative virtual address `rva`.
	fn offset_of_rva(&self, rva: u32) -> Option<usize> {
		self.sections.as_chunks::<SECTION_HEADER_SIZE>().0.iter().find_map(|section| {
			let virtual_address = read_u32(section, 12)?;
			let raw_size = read_u32(section, 16)?;
			let raw_offset = read_u32(section, 20)?;
			let offset = rva.checked_sub(virtual_address).filter(|&offset| offset < raw_size)?;
			let offset = raw_offset.checked_add(offset)? as usize;
			(offset < self.image.len()).then_some(offset)
		})
	}
}

const SECTION_HEADER_SIZE: usize = 40;

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
	Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
	Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A PE32+ image with one section at RVA 0x1000 (file offset 0x200) that holds an import
	/// table of `imports` and a delay import table of `delay_imports`.
	fn image(imports: &[&str], delay_imports: &[&str]) -> Vec<u8> {
		const SECTION_RVA: u32 = 0x1000;
		const SECTION_OFFSET: usize = 0x200;
		let mut section = vec![0; (imports.len() + 1) * 20 + (delay_imports.len() + 1) * 32];
		let delay_table = (imports.len() + 1) * 20;
		let add_name = |section: &mut Vec<u8>, descriptor: usize, name: &str| {
			let rva = SECTION_RVA + section.len() as u32;
			section[descriptor..descriptor + 4].copy_from_slice(&rva.to_le_bytes());
			section.extend_from_slice(name.as_bytes());
			section.push(0);
		};
		for (index, name) in imports.iter().enumerate() {
			add_name(&mut section, index * 20 + 12, name);
		}
		for (index, name) in delay_imports.iter().enumerate() {
			section[delay_table + index * 32] = 1;
			add_name(&mut section, delay_table + index * 32 + 4, name);
		}

		let mut image = vec![0; SECTION_OFFSET];
		image[..2].copy_from_slice(b"MZ");
		image[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
		image[0x80..0x84].copy_from_slice(b"PE\0\0");
		let coff = 0x84;
		image[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
		let optional_header_size: u16 = 112 + 16 * 8;
		image[coff + 16..coff + 18].copy_from_slice(&optional_header_size.to_le_bytes());
		let optional = coff + 20;
		image[optional..optional + 2].copy_from_slice(&0x20bu16.to_le_bytes());
		let directories = optional + 112;
		let mut set_directory = |index: usize, rva: u32, size: usize| {
			let entry = directories + index * 8;
			image[entry..entry + 4].copy_from_slice(&rva.to_le_bytes());
			image[entry + 4..entry + 8].copy_from_slice(&(size as u32).to_le_bytes());
		};
		set_directory(1, SECTION_RVA, delay_table);
		if !delay_imports.is_empty() {
			set_directory(13, SECTION_RVA + delay_table as u32, delay_imports.len() * 32 + 32);
		}
		let section_header = optional + optional_header_size as usize;
		let size = (section.len() as u32).to_le_bytes();
		image[section_header + 8..section_header + 12].copy_from_slice(&size);
		image[section_header + 12..section_header + 16].copy_from_slice(&SECTION_RVA.to_le_bytes());
		image[section_header + 16..section_header + 20].copy_from_slice(&size);
		image[section_header + 20..section_header + 24]
			.copy_from_slice(&(SECTION_OFFSET as u32).to_le_bytes());
		image.extend_from_slice(&section);
		image
	}

	#[test]
	fn reads_imported_dlls() {
		let image = image(&["KERNEL32.dll", "user32.dll", "kernel32.dll"], &["DWrite.dll"]);
		assert_eq!(imported_dlls(&image).unwrap(), ["dwrite.dll", "kernel32.dll", "user32.dll"]);
	}

	#[test]
	fn rejects_malformed_images() {
		let image = image(&["kernel32.dll"], &[]);
		assert_eq!(imported_dlls(&image[..0x100]), None);
		assert_eq!(imported_dlls(b"MZ"), None);
		assert_eq!(imported_dlls(&[]), None);
	}

	#[test]
	fn accepts_only_system_dlls() {
		assert!(is_system_dll("kernel32.dll"));
		assert!(is_system_dll("api-ms-win-core-synch-l1-2-0.dll"));
		assert!(!is_system_dll("libgcc_s_seh-1.dll"));
		assert!(!is_system_dll("libwinpthread-1.dll"));
		assert!(!is_system_dll("webview2loader.dll"));
	}
}
