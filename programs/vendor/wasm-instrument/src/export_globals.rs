use alloc::{format, vec::Vec};
use parity_wasm::elements;

/// Export all declared mutable globals as `prefix_index`.
///
/// This will export all internal mutable globals under the name of
/// concat(`prefix`, `"_"`, `i`) where i is the index inside the range of
/// [0..total number of internal mutable globals].
pub fn export_mutable_globals(module: &mut elements::Module, prefix: &str) {
	let exports = global_section(module)
		.map(|section| {
			section
				.entries()
				.iter()
				.enumerate()
				.filter_map(
					|(index, global)| {
						if global.global_type().is_mutable() {
							Some(index)
						} else {
							None
						}
					},
				)
				.collect::<Vec<_>>()
		})
		.unwrap_or_default();

	if module.export_section().is_none() {
		module
			.sections_mut()
			.push(elements::Section::Export(elements::ExportSection::default()));
	}

	for (symbol_index, export) in exports.into_iter().enumerate() {
		let new_entry = elements::ExportEntry::new(
			format!("{}_{}", prefix, symbol_index),
			elements::Internal::Global(
				(module.import_count(elements::ImportCountType::Global) + export) as _,
			),
		);
		export_section(module)
			.expect("added above if does not exists")
			.entries_mut()
			.push(new_entry);
	}
}

fn export_section(module: &mut elements::Module) -> Option<&mut elements::ExportSection> {
	for section in module.sections_mut() {
		if let elements::Section::Export(sect) = section {
			return Some(sect)
		}
	}
	None
}

fn global_section(module: &mut elements::Module) -> Option<&mut elements::GlobalSection> {
	for section in module.sections_mut() {
		if let elements::Section::Global(sect) = section {
			return Some(sect)
		}
	}
	None
}

#[cfg(test)]
mod tests {

	use super::export_mutable_globals;
	use parity_wasm::elements;

	fn parse_wat(source: &str) -> elements::Module {
		let module_bytes = wat::parse_str(source).unwrap();
		wasmparser::validate(&module_bytes).unwrap();
		elements::deserialize_buffer(module_bytes.as_ref()).expect("failed to parse module")
	}

	macro_rules! test_export_global {
		(name = $name:ident; input = $input:expr; expected = $expected:expr) => {
			#[test]
			fn $name() {
				let mut input_module = parse_wat($input);
				let expected_module = parse_wat($expected);

				export_mutable_globals(&mut input_module, "exported_internal_global");

				let actual_bytes = elements::serialize(input_module)
					.expect("injected module must have a function body");

				let expected_bytes = elements::serialize(expected_module)
					.expect("injected module must have a function body");

				let actual_wat = wasmprinter::print_bytes(actual_bytes).unwrap();
				let expected_wat = wasmprinter::print_bytes(expected_bytes).unwrap();

				if actual_wat != expected_wat {
					for diff in diff::lines(&expected_wat, &actual_wat) {
						match diff {
							diff::Result::Left(l) => println!("-{}", l),
							diff::Result::Both(l, _) => println!(" {}", l),
							diff::Result::Right(r) => println!("+{}", r),
						}
					}
					panic!()
				}
			}
		};
	}

	test_export_global! {
		name = simple;
		input = r#"
		(module
			(global (;0;) (mut i32) (i32.const 1))
			(global (;1;) (mut i32) (i32.const 0)))
		"#;
		expected = r#"
		(module
			(global (;0;) (mut i32) (i32.const 1))
			(global (;1;) (mut i32) (i32.const 0))
			(export "exported_internal_global_0" (global 0))
			(export "exported_internal_global_1" (global 1)))
		"#
	}

	test_export_global! {
		name = with_import;
		input = r#"
		(module
			(import "env" "global" (global $global i64))
			(global (;0;) (mut i32) (i32.const 1))
			(global (;1;) (mut i32) (i32.const 0)))
		"#;
		expected = r#"
		(module
			(import "env" "global" (global $global i64))
			(global (;0;) (mut i32) (i32.const 1))
			(global (;1;) (mut i32) (i32.const 0))
			(export "exported_internal_global_0" (global 1))
			(export "exported_internal_global_1" (global 2)))
		"#
	}

	test_export_global! {
		name = with_import_and_some_are_immutable;
		input = r#"
		(module
			(import "env" "global" (global $global i64))
			(global (;0;) i32 (i32.const 1))
			(global (;1;) (mut i32) (i32.const 0)))
		"#;
		expected = r#"
		(module
			(import "env" "global" (global $global i64))
			(global (;0;) i32 (i32.const 1))
			(global (;1;) (mut i32) (i32.const 0))
			(export "exported_internal_global_0" (global 2)))
		"#
	}
}
