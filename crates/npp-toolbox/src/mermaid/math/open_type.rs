//! The OpenType `MATH` table: constants, italic corrections, size variants and assemblies of
//! glyphs. Offsets come from the font file, so every read is checked.

/// `MathConstants` in the order of the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Constant {
	ScriptPercentScaleDown = 0,
	ScriptScriptPercentScaleDown = 1,
	/// Blink's large operators are as tall as this (Cambria Math has 3000 here and 2500 in the next
	/// field, `displayOperatorMinHeight`; probed in Edge from 8 to 56 pixels).
	DelimitedSubFormulaMinHeight = 2,
	AxisHeight = 5,
	AccentBaseHeight = 6,
	SubscriptShiftDown = 8,
	SubscriptTopMax = 9,
	SubscriptBaselineDropMin = 10,
	SuperscriptShiftUp = 11,
	SuperscriptShiftUpCramped = 12,
	SuperscriptBottomMin = 13,
	SuperscriptBaselineDropMax = 14,
	SubSuperscriptGapMin = 15,
	SuperscriptBottomMaxWithSubscript = 16,
	SpaceAfterScript = 17,
	UpperLimitGapMin = 18,
	UpperLimitBaselineRiseMin = 19,
	LowerLimitGapMin = 20,
	LowerLimitBaselineDropMin = 21,
	StackTopShiftUp = 22,
	StackTopDisplayStyleShiftUp = 23,
	StackBottomShiftDown = 24,
	StackBottomDisplayStyleShiftDown = 25,
	StackGapMin = 26,
	StackDisplayStyleGapMin = 27,
	StretchStackTopShiftUp = 28,
	StretchStackBottomShiftDown = 29,
	StretchStackGapAboveMin = 30,
	StretchStackGapBelowMin = 31,
	FractionNumeratorShiftUp = 32,
	FractionNumeratorDisplayStyleShiftUp = 33,
	FractionDenominatorShiftDown = 34,
	FractionDenominatorDisplayStyleShiftDown = 35,
	FractionNumeratorGapMin = 36,
	FractionNumDisplayStyleGapMin = 37,
	FractionRuleThickness = 38,
	FractionDenominatorGapMin = 39,
	FractionDenomDisplayStyleGapMin = 40,
	OverbarVerticalGap = 43,
	OverbarExtraAscender = 45,
	UnderbarVerticalGap = 46,
	UnderbarExtraDescender = 48,
	RadicalVerticalGap = 49,
	RadicalDisplayStyleVerticalGap = 50,
	RadicalRuleThickness = 51,
	RadicalExtraAscender = 52,
	RadicalKernBeforeDegree = 53,
	RadicalKernAfterDegree = 54,
	RadicalDegreeBottomRaisePercent = 55,
}

/// HarfBuzz returns at most this many records; fonts have fewer.
const MAX_RECORDS: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphPart {
	pub glyph: u16,
	pub start_connector: u16,
	pub end_connector: u16,
	pub full_advance: u16,
	pub is_extender: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
	Vertical,
	Horizontal,
}

/// A `MATH` table; all values are in design units.
#[derive(Debug)]
pub struct MathTable {
	data: Vec<u8>,
}

impl MathTable {
	pub fn new(data: Vec<u8>) -> Self {
		Self { data }
	}

	fn u16_at(&self, offset: usize) -> Option<u16> {
		let bytes = self.data.get(offset..offset.checked_add(2)?)?;
		Some(u16::from_be_bytes([bytes[0], bytes[1]]))
	}

	fn i16_at(&self, offset: usize) -> Option<i16> {
		self.u16_at(offset).map(|value| value as i16)
	}

	/// The table at the 16 bit offset stored at `offset` relative to `base`.
	fn subtable(&self, base: usize, offset: usize) -> Option<usize> {
		match self.u16_at(base.checked_add(offset)?)? {
			0 => None,
			value => base.checked_add(usize::from(value)),
		}
	}

	pub fn constant(&self, constant: Constant) -> Option<f64> {
		let table = self.subtable(0, 4)?;
		let index = constant as usize;
		// Two percentages and two heights are plain values, the others `MathValueRecord`s.
		let offset = match index {
			0..4 => index * 2,
			_ => 8 + (index - 4) * 4,
		};
		self.i16_at(table + offset).map(f64::from)
	}

	/// The index of `glyph` in the coverage table at `coverage`.
	fn coverage_index(&self, coverage: usize, glyph: u16) -> Option<usize> {
		let count = usize::from(self.u16_at(coverage + 2)?);
		match self.u16_at(coverage)? {
			1 => (0..count).find(|&index| self.u16_at(coverage + 4 + index * 2) == Some(glyph)),
			2 => (0..count).find_map(|index| {
				let record = coverage + 4 + index * 6;
				let start = self.u16_at(record)?;
				let end = self.u16_at(record + 2)?;
				let first_index = self.u16_at(record + 4)?;
				(start <= glyph && glyph <= end)
					.then(|| usize::from(first_index) + usize::from(glyph - start))
			}),
			_ => None,
		}
	}

	pub fn italic_correction(&self, glyph: u16) -> Option<f64> {
		let glyph_info = self.subtable(0, 6)?;
		let corrections = self.subtable(glyph_info, 0)?;
		let coverage = self.subtable(corrections, 0)?;
		let index = self.coverage_index(coverage, glyph)?;
		if index >= usize::from(self.u16_at(corrections + 2)?) {
			return None;
		}
		self.i16_at(corrections + 4 + index * 4).map(f64::from)
	}

	/// `MathVariants.minConnectorOverlap`.
	pub fn min_connector_overlap(&self) -> f64 {
		self.subtable(0, 8).and_then(|variants| self.u16_at(variants)).map_or(0.0, f64::from)
	}

	/// The `MathGlyphConstruction` of `glyph` along `axis`.
	fn construction(&self, glyph: u16, axis: Axis) -> Option<usize> {
		let variants = self.subtable(0, 8)?;
		let vertical_count = usize::from(self.u16_at(variants + 6)?);
		let horizontal_count = usize::from(self.u16_at(variants + 8)?);
		let (coverage_offset, first, count) = match axis {
			Axis::Vertical => (2, 0, vertical_count),
			Axis::Horizontal => (4, vertical_count, horizontal_count),
		};
		let coverage = self.subtable(variants, coverage_offset)?;
		let index = self.coverage_index(coverage, glyph)?;
		if index >= count {
			return None;
		}
		self.subtable(variants, 10 + (first + index) * 2)
	}

	/// The size variants of `glyph` along `axis`, smallest first, without `glyph` itself.
	pub fn variants(&self, glyph: u16, axis: Axis) -> Vec<u16> {
		let Some(construction) = self.construction(glyph, axis) else { return Vec::new() };
		let count = self.u16_at(construction + 2).map_or(0, usize::from).min(MAX_RECORDS);
		(0..count).filter_map(|index| self.u16_at(construction + 4 + index * 4)).collect()
	}

	/// The parts of the assembly of `glyph` along `axis`, bottom or left first, and its italic
	/// correction.
	pub fn assembly(&self, glyph: u16, axis: Axis) -> Option<(Vec<GlyphPart>, f64)> {
		let construction = self.construction(glyph, axis)?;
		let assembly = self.subtable(construction, 0)?;
		let italic_correction = f64::from(self.i16_at(assembly)?);
		let count = usize::from(self.u16_at(assembly + 4)?).min(MAX_RECORDS);
		let mut parts = Vec::with_capacity(count);
		for index in 0..count {
			let record = assembly + 6 + index * 10;
			parts.push(GlyphPart {
				glyph: self.u16_at(record)?,
				start_connector: self.u16_at(record + 2)?,
				end_connector: self.u16_at(record + 4)?,
				full_advance: self.u16_at(record + 6)?,
				is_extender: self.u16_at(record + 8)? & 1 != 0,
			});
		}
		Some((parts, italic_correction))
	}
}

#[cfg(test)]
pub mod testing {
	/// A small `MATH` table: axis height 250, script scale 70 %, glyph 5 with an italic correction
	/// of 30, vertical variants 6 and 7 and an assembly of the bottom 8, extender 9 and top 10.
	pub fn table_bytes() -> Vec<u8> {
		fn words(values: &[u16]) -> Vec<u8> {
			values.iter().flat_map(|value| value.to_be_bytes()).collect()
		}
		let mut constants = vec![0u8; 214];
		constants[0..2].copy_from_slice(&70u16.to_be_bytes());
		constants[2..4].copy_from_slice(&50u16.to_be_bytes());
		// Index 5, the second `MathValueRecord`.
		constants[12..14].copy_from_slice(&250u16.to_be_bytes());
		// `MathGlyphInfo` -> corrections at 8 -> coverage at 8 of those.
		let glyph_info = words(&[8, 0, 0, 0, 8, 1, 30, 0, 1, 1, 5]);
		// `MathVariants` with the coverage format 2 at 12 and the construction at 18.
		let mut variants = words(&[50, 12, 0, 1, 0, 18, 2, 1, 5, 5, 0]);
		variants.truncate(12 + 10);
		variants[10..12].copy_from_slice(&22u16.to_be_bytes());
		// The construction: its assembly at 10, and two variants.
		variants.extend(words(&[10, 2, 6, 100, 7, 200]));
		variants.truncate(22 + 10);
		variants.extend(words(&[40, 0, 3]));
		variants.extend(words(&[8, 0, 60, 300, 0, 9, 60, 60, 200, 1, 10, 60, 0, 300, 0]));
		let constants_offset = 10u16;
		let glyph_info_offset = constants_offset + constants.len() as u16;
		let variants_offset = glyph_info_offset + glyph_info.len() as u16;
		let mut table = words(&[1, 0, constants_offset, glyph_info_offset, variants_offset]);
		table.extend(constants);
		table.extend(glyph_info);
		table.extend(variants);
		table
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn reads_a_math_table() {
		let table = MathTable::new(testing::table_bytes());
		assert_eq!(table.constant(Constant::ScriptPercentScaleDown), Some(70.0));
		assert_eq!(table.constant(Constant::ScriptScriptPercentScaleDown), Some(50.0));
		assert_eq!(table.constant(Constant::AxisHeight), Some(250.0));
		assert_eq!(table.italic_correction(5), Some(30.0));
		assert_eq!(table.italic_correction(6), None);
		assert_eq!(table.min_connector_overlap(), 50.0);
		assert_eq!(table.variants(5, Axis::Vertical), [6, 7]);
		assert_eq!(table.variants(5, Axis::Horizontal), Vec::<u16>::new());
		assert_eq!(table.variants(6, Axis::Vertical), Vec::<u16>::new());
		let (parts, italic_correction) = table.assembly(5, Axis::Vertical).unwrap();
		assert_eq!(italic_correction, 40.0);
		assert_eq!(parts.len(), 3);
		assert!(parts[1].is_extender && !parts[0].is_extender);
		assert_eq!((parts[2].glyph, parts[2].full_advance), (10, 300));
	}

	#[test]
	fn survives_broken_tables() {
		let bytes = testing::table_bytes();
		for length in 0..bytes.len() {
			let table = MathTable::new(bytes[..length].to_vec());
			let _ = table.constant(Constant::RadicalDegreeBottomRaisePercent);
			let _ = table.italic_correction(5);
			let _ = table.variants(5, Axis::Vertical);
			let _ = table.assembly(5, Axis::Vertical);
			let _ = table.min_connector_overlap();
		}
		let table = MathTable::new(vec![0xff; 64]);
		assert_eq!(table.variants(5, Axis::Vertical), Vec::<u16>::new());
		assert!(table.assembly(5, Axis::Horizontal).is_none());
	}
}
