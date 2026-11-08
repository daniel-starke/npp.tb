//! External lexers: Lexilla's `ILexer5` implemented in Rust, the `IDocument` Scintilla hands to
//! lexers, and Lexilla lexers created by Notepad++ (`NPPM_CREATELEXER`) run on a document whose
//! styles are mapped.
//!
//! The interfaces are C++ classes with only pure virtual methods (`ILexer.h` of Scintilla), so an
//! object is a pointer to its vtable, followed by the implementation's data. The methods use
//! `SCI_METHOD`, which is `__stdcall` on 32-bit Windows (`extern "system"`), with `this` as the
//! first argument.

use core::ffi::{CStr, c_char, c_int, c_void};
use core::ptr::NonNull;

/// `Sci_Position`.
pub type Position = isize;

/// `lvRelease5`: the lexer implements `ILexer5`.
const LEXER_VERSION_5: c_int = 3;
/// `SCLEX_AUTOMATIC`: the identifier of lexers without a Lexilla `SCLEX_*` value.
const AUTOMATIC_LEXER_IDENTIFIER: c_int = 1000;
/// `SC_LINE_END_TYPE_DEFAULT`.
const DEFAULT_LINE_END_TYPES: c_int = 0;

/// `Scintilla::IDocument`, in declaration order of `ILexer.h`.
#[repr(C)]
pub struct DocumentVtable {
	pub version: unsafe extern "system" fn(*const DocumentInterface) -> c_int,
	pub set_error_status: unsafe extern "system" fn(*mut DocumentInterface, c_int),
	pub length: unsafe extern "system" fn(*const DocumentInterface) -> Position,
	pub get_char_range:
		unsafe extern "system" fn(*const DocumentInterface, *mut c_char, Position, Position),
	pub style_at: unsafe extern "system" fn(*const DocumentInterface, Position) -> c_char,
	pub line_from_position:
		unsafe extern "system" fn(*const DocumentInterface, Position) -> Position,
	pub line_start: unsafe extern "system" fn(*const DocumentInterface, Position) -> Position,
	pub get_level: unsafe extern "system" fn(*const DocumentInterface, Position) -> c_int,
	pub set_level: unsafe extern "system" fn(*mut DocumentInterface, Position, c_int) -> c_int,
	pub get_line_state: unsafe extern "system" fn(*const DocumentInterface, Position) -> c_int,
	pub set_line_state: unsafe extern "system" fn(*mut DocumentInterface, Position, c_int) -> c_int,
	pub start_styling: unsafe extern "system" fn(*mut DocumentInterface, Position),
	pub set_style_for: unsafe extern "system" fn(*mut DocumentInterface, Position, c_char) -> bool,
	pub set_styles:
		unsafe extern "system" fn(*mut DocumentInterface, Position, *const c_char) -> bool,
	pub decoration_set_current_indicator: unsafe extern "system" fn(*mut DocumentInterface, c_int),
	pub decoration_fill_range:
		unsafe extern "system" fn(*mut DocumentInterface, Position, c_int, Position),
	pub change_lexer_state: unsafe extern "system" fn(*mut DocumentInterface, Position, Position),
	pub code_page: unsafe extern "system" fn(*const DocumentInterface) -> c_int,
	pub is_dbcs_lead_byte: unsafe extern "system" fn(*const DocumentInterface, c_char) -> bool,
	pub buffer_pointer: unsafe extern "system" fn(*mut DocumentInterface) -> *const c_char,
	pub get_line_indentation: unsafe extern "system" fn(*mut DocumentInterface, Position) -> c_int,
	pub line_end: unsafe extern "system" fn(*const DocumentInterface, Position) -> Position,
	pub get_relative_position:
		unsafe extern "system" fn(*const DocumentInterface, Position, Position) -> Position,
	pub get_character_and_width:
		unsafe extern "system" fn(*const DocumentInterface, Position, *mut Position) -> c_int,
}

/// A `Scintilla::IDocument` object: its vtable pointer.
#[repr(C)]
pub struct DocumentInterface {
	pub vtable: *const DocumentVtable,
}

/// `Scintilla::ILexer5`, in declaration order of `ILexer.h` (`ILexer4` first).
#[repr(C)]
pub struct LexerVtable {
	pub version: unsafe extern "system" fn(*const LexerInterface) -> c_int,
	pub release: unsafe extern "system" fn(*mut LexerInterface),
	pub property_names: unsafe extern "system" fn(*mut LexerInterface) -> *const c_char,
	pub property_type: unsafe extern "system" fn(*mut LexerInterface, *const c_char) -> c_int,
	pub describe_property:
		unsafe extern "system" fn(*mut LexerInterface, *const c_char) -> *const c_char,
	pub property_set:
		unsafe extern "system" fn(*mut LexerInterface, *const c_char, *const c_char) -> Position,
	pub describe_word_list_sets: unsafe extern "system" fn(*mut LexerInterface) -> *const c_char,
	pub word_list_set:
		unsafe extern "system" fn(*mut LexerInterface, c_int, *const c_char) -> Position,
	pub lex: unsafe extern "system" fn(
		*mut LexerInterface,
		usize,
		Position,
		c_int,
		*mut DocumentInterface,
	),
	pub fold: unsafe extern "system" fn(
		*mut LexerInterface,
		usize,
		Position,
		c_int,
		*mut DocumentInterface,
	),
	pub private_call:
		unsafe extern "system" fn(*mut LexerInterface, c_int, *mut c_void) -> *mut c_void,
	pub line_end_types_supported: unsafe extern "system" fn(*mut LexerInterface) -> c_int,
	pub allocate_sub_styles: unsafe extern "system" fn(*mut LexerInterface, c_int, c_int) -> c_int,
	pub sub_styles_start: unsafe extern "system" fn(*mut LexerInterface, c_int) -> c_int,
	pub sub_styles_length: unsafe extern "system" fn(*mut LexerInterface, c_int) -> c_int,
	pub style_from_sub_style: unsafe extern "system" fn(*mut LexerInterface, c_int) -> c_int,
	pub primary_style_from_style: unsafe extern "system" fn(*mut LexerInterface, c_int) -> c_int,
	pub free_sub_styles: unsafe extern "system" fn(*mut LexerInterface),
	pub set_identifiers: unsafe extern "system" fn(*mut LexerInterface, c_int, *const c_char),
	pub distance_to_secondary_styles: unsafe extern "system" fn(*mut LexerInterface) -> c_int,
	pub get_sub_style_bases: unsafe extern "system" fn(*mut LexerInterface) -> *const c_char,
	pub named_styles: unsafe extern "system" fn(*mut LexerInterface) -> c_int,
	pub name_of_style: unsafe extern "system" fn(*mut LexerInterface, c_int) -> *const c_char,
	pub tags_of_style: unsafe extern "system" fn(*mut LexerInterface, c_int) -> *const c_char,
	pub description_of_style:
		unsafe extern "system" fn(*mut LexerInterface, c_int) -> *const c_char,
	pub get_name: unsafe extern "system" fn(*mut LexerInterface) -> *const c_char,
	pub get_identifier: unsafe extern "system" fn(*mut LexerInterface) -> c_int,
	pub property_get:
		unsafe extern "system" fn(*mut LexerInterface, *const c_char) -> *const c_char,
}

/// A `Scintilla::ILexer5` object: its vtable pointer.
#[repr(C)]
pub struct LexerInterface {
	pub vtable: *const LexerVtable,
}

/// The document Scintilla passes to [`Lexer::lex`]. Only valid during that call.
#[derive(Clone, Copy)]
pub struct Document {
	raw: NonNull<DocumentInterface>,
}

impl Document {
	/// # Safety
	/// `raw` must be a valid `IDocument` for as long as the returned value is used.
	pub unsafe fn from_raw(raw: *mut DocumentInterface) -> Option<Self> {
		NonNull::new(raw).map(|raw| Self { raw })
	}

	pub fn raw(&self) -> *mut DocumentInterface {
		self.raw.as_ptr()
	}

	fn vtable(&self) -> &DocumentVtable {
		// SAFETY: A valid `IDocument` (see `from_raw`) starts with its vtable pointer.
		unsafe { &*(*self.raw.as_ptr()).vtable }
	}

	pub fn version(&self) -> c_int {
		// SAFETY: Calls the document's own method on itself (see `from_raw`).
		unsafe { (self.vtable().version)(self.raw()) }
	}

	pub fn length(&self) -> Position {
		// SAFETY: See `version`.
		unsafe { (self.vtable().length)(self.raw()) }
	}

	/// `SC_CP_UTF8`, a DBCS code page or 0 for single byte encodings.
	pub fn code_page(&self) -> c_int {
		// SAFETY: See `version`.
		unsafe { (self.vtable().code_page)(self.raw()) }
	}

	pub fn is_dbcs_lead_byte(&self, byte: u8) -> bool {
		// SAFETY: See `version`.
		unsafe { (self.vtable().is_dbcs_lead_byte)(self.raw(), byte as c_char) }
	}

	/// Appends the text of `range` to `buffer`. Unlike `BufferPointer`, this leaves the gap of
	/// Scintilla's buffer where it is, which would otherwise move the text after it twice per
	/// edit.
	pub fn read(&self, range: core::ops::Range<Position>, buffer: &mut Vec<u8>) {
		let start = range.start.clamp(0, self.length());
		let length = usize::try_from(range.end.min(self.length()) - start).unwrap_or(0);
		if length == 0 {
			return;
		}
		let old_length = buffer.len();
		buffer.resize(old_length + length, 0);
		// SAFETY: See `version`. The buffer has room for `length` bytes after `old_length`.
		unsafe {
			(self.vtable().get_char_range)(
				self.raw(),
				buffer.as_mut_ptr().add(old_length).cast(),
				start,
				length as Position,
			)
		};
	}

	/// Makes Scintilla style `range` again, when it is shown.
	pub fn change_lexer_state(&self, range: core::ops::Range<Position>) {
		// SAFETY: See `version`.
		unsafe { (self.vtable().change_lexer_state)(self.raw(), range.start, range.end) };
	}

	pub fn line_state(&self, line: Position) -> c_int {
		// SAFETY: See `version`.
		unsafe { (self.vtable().get_line_state)(self.raw(), line) }
	}

	pub fn set_line_state(&self, line: Position, state: c_int) {
		// SAFETY: See `version`.
		unsafe { (self.vtable().set_line_state)(self.raw(), line, state) };
	}

	pub fn style_at(&self, position: Position) -> u8 {
		// SAFETY: See `version`.
		unsafe { (self.vtable().style_at)(self.raw(), position) as u8 }
	}

	pub fn line_from_position(&self, position: Position) -> Position {
		// SAFETY: See `version`.
		unsafe { (self.vtable().line_from_position)(self.raw(), position) }
	}

	pub fn line_start(&self, line: Position) -> Position {
		// SAFETY: See `version`.
		unsafe { (self.vtable().line_start)(self.raw(), line) }
	}

	pub fn level(&self, line: Position) -> c_int {
		// SAFETY: See `version`.
		unsafe { (self.vtable().get_level)(self.raw(), line) }
	}

	pub fn set_level(&self, line: Position, level: c_int) -> c_int {
		// SAFETY: See `version`.
		unsafe { (self.vtable().set_level)(self.raw(), line, level) }
	}

	/// Moves the styling position. Scintilla takes it as the end of the styled text, so a lexer
	/// that styles out of order finishes with the end of everything it styled.
	pub fn start_styling(&self, position: Position) {
		// SAFETY: See `version`.
		unsafe { (self.vtable().start_styling)(self.raw(), position) }
	}

	pub fn set_style_for(&self, length: Position, style: u8) -> bool {
		// SAFETY: See `version`.
		unsafe { (self.vtable().set_style_for)(self.raw(), length, style as c_char) }
	}

	pub fn set_styles(&self, styles: &[u8]) -> bool {
		// SAFETY: See `version`. The length of the slice is passed.
		unsafe {
			(self.vtable().set_styles)(self.raw(), styles.len() as Position, styles.as_ptr().cast())
		}
	}
}

/// Name, tags and description of a lexer style for Scintilla's `SCI_NAMEOFSTYLE`,
/// `SCI_TAGSOFSTYLE` and `SCI_DESCRIPTIONOFSTYLE`.
#[derive(Clone, Copy, Debug)]
pub struct StyleDescription {
	pub name: &'static CStr,
	pub tags: &'static CStr,
	pub description: &'static CStr,
}

/// A lexer implemented in Rust. Scintilla calls it on the UI thread only.
pub trait Lexer {
	/// The lexer name Scintilla reports (`SCI_GETLEXERLANGUAGE`).
	fn name(&self) -> &'static CStr;

	/// The number of styles, numbered from 0, for the style metadata of Scintilla.
	fn style_count(&self) -> usize;

	fn style_description(&self, style: usize) -> Option<StyleDescription>;

	/// Styles at least `length` bytes from `start`. `initial_style` is the style before `start`.
	fn lex(&mut self, document: Document, start: usize, length: Position, initial_style: c_int);

	fn fold(
		&mut self,
		_document: Document,
		_start: usize,
		_length: Position,
		_initial_style: c_int,
	) {
	}
}

/// A Rust [`Lexer`] behind an `ILexer5` vtable. Scintilla owns it and calls `Release`.
#[repr(C)]
struct LexerObject {
	interface: LexerInterface,
	lexer: Box<dyn Lexer>,
}

/// Wraps `lexer` into an `ILexer5` object for Scintilla, which frees it with `Release`.
pub fn into_raw_lexer(lexer: Box<dyn Lexer>) -> *mut LexerInterface {
	let object =
		Box::new(LexerObject { interface: LexerInterface { vtable: &LEXER_VTABLE }, lexer });
	Box::into_raw(object).cast()
}

static LEXER_VTABLE: LexerVtable = LexerVtable {
	version: lexer_version,
	release: lexer_release,
	property_names: lexer_empty_string,
	property_type: lexer_property_type,
	describe_property: lexer_empty_string_for_name,
	property_set: lexer_property_set,
	describe_word_list_sets: lexer_empty_string,
	word_list_set: lexer_word_list_set,
	lex: lexer_lex,
	fold: lexer_fold,
	private_call: lexer_private_call,
	line_end_types_supported: lexer_line_end_types_supported,
	allocate_sub_styles: lexer_allocate_sub_styles,
	sub_styles_start: lexer_sub_styles_start,
	sub_styles_length: lexer_sub_styles_length,
	style_from_sub_style: lexer_same_style,
	primary_style_from_style: lexer_same_style,
	free_sub_styles: lexer_free_sub_styles,
	set_identifiers: lexer_set_identifiers,
	distance_to_secondary_styles: lexer_distance_to_secondary_styles,
	get_sub_style_bases: lexer_empty_string,
	named_styles: lexer_named_styles,
	name_of_style: lexer_name_of_style,
	tags_of_style: lexer_tags_of_style,
	description_of_style: lexer_description_of_style,
	get_name: lexer_get_name,
	get_identifier: lexer_get_identifier,
	property_get: lexer_empty_string_for_name,
};

/// # Safety
/// `this` must be a live object from [`into_raw_lexer`].
unsafe fn lexer_object<'a>(this: *mut LexerInterface) -> &'a mut LexerObject {
	// SAFETY: Guaranteed by the caller; the interface is the first field of the object.
	unsafe { &mut *this.cast::<LexerObject>() }
}

unsafe extern "system" fn lexer_version(_this: *const LexerInterface) -> c_int {
	LEXER_VERSION_5
}

unsafe extern "system" fn lexer_release(this: *mut LexerInterface) {
	// SAFETY: Scintilla releases the object from `into_raw_lexer` once and never uses it again.
	drop(unsafe { Box::from_raw(this.cast::<LexerObject>()) });
}

unsafe extern "system" fn lexer_empty_string(_this: *mut LexerInterface) -> *const c_char {
	c"".as_ptr()
}

unsafe extern "system" fn lexer_empty_string_for_name(
	_this: *mut LexerInterface,
	_name: *const c_char,
) -> *const c_char {
	c"".as_ptr()
}

unsafe extern "system" fn lexer_property_type(
	_this: *mut LexerInterface,
	_name: *const c_char,
) -> c_int {
	0
}

/// Returns -1: no property changes the styles, so nothing needs to be styled again.
unsafe extern "system" fn lexer_property_set(
	_this: *mut LexerInterface,
	_key: *const c_char,
	_value: *const c_char,
) -> Position {
	-1
}

unsafe extern "system" fn lexer_word_list_set(
	_this: *mut LexerInterface,
	_set: c_int,
	_words: *const c_char,
) -> Position {
	-1
}

unsafe extern "system" fn lexer_lex(
	this: *mut LexerInterface,
	start: usize,
	length: Position,
	initial_style: c_int,
	document: *mut DocumentInterface,
) {
	// SAFETY: Scintilla passes its live document for the duration of the call.
	let Some(document) = (unsafe { Document::from_raw(document) }) else { return };
	// SAFETY: Scintilla calls the methods of the object it got from `into_raw_lexer`.
	unsafe { lexer_object(this) }.lexer.lex(document, start, length, initial_style);
}

unsafe extern "system" fn lexer_fold(
	this: *mut LexerInterface,
	start: usize,
	length: Position,
	initial_style: c_int,
	document: *mut DocumentInterface,
) {
	// SAFETY: See `lexer_lex`.
	let Some(document) = (unsafe { Document::from_raw(document) }) else { return };
	// SAFETY: See `lexer_lex`.
	unsafe { lexer_object(this) }.lexer.fold(document, start, length, initial_style);
}

unsafe extern "system" fn lexer_private_call(
	_this: *mut LexerInterface,
	_operation: c_int,
	_pointer: *mut c_void,
) -> *mut c_void {
	core::ptr::null_mut()
}

unsafe extern "system" fn lexer_line_end_types_supported(_this: *mut LexerInterface) -> c_int {
	DEFAULT_LINE_END_TYPES
}

unsafe extern "system" fn lexer_allocate_sub_styles(
	_this: *mut LexerInterface,
	_style_base: c_int,
	_number_styles: c_int,
) -> c_int {
	-1
}

unsafe extern "system" fn lexer_sub_styles_start(
	_this: *mut LexerInterface,
	_style_base: c_int,
) -> c_int {
	-1
}

unsafe extern "system" fn lexer_sub_styles_length(
	_this: *mut LexerInterface,
	_style_base: c_int,
) -> c_int {
	0
}

unsafe extern "system" fn lexer_same_style(_this: *mut LexerInterface, style: c_int) -> c_int {
	style
}

unsafe extern "system" fn lexer_free_sub_styles(_this: *mut LexerInterface) {}

unsafe extern "system" fn lexer_set_identifiers(
	_this: *mut LexerInterface,
	_style: c_int,
	_identifiers: *const c_char,
) {
}

unsafe extern "system" fn lexer_distance_to_secondary_styles(_this: *mut LexerInterface) -> c_int {
	0
}

unsafe extern "system" fn lexer_named_styles(this: *mut LexerInterface) -> c_int {
	// SAFETY: See `lexer_lex`.
	unsafe { lexer_object(this) }.lexer.style_count() as c_int
}

/// # Safety
/// See [`lexer_object`].
unsafe fn style_description(this: *mut LexerInterface, style: c_int) -> Option<StyleDescription> {
	let style = usize::try_from(style).ok()?;
	// SAFETY: Guaranteed by the caller.
	unsafe { lexer_object(this) }.lexer.style_description(style)
}

unsafe extern "system" fn lexer_name_of_style(
	this: *mut LexerInterface,
	style: c_int,
) -> *const c_char {
	// SAFETY: See `lexer_lex`.
	unsafe { style_description(this, style) }.map_or(c"".as_ptr(), |style| style.name.as_ptr())
}

unsafe extern "system" fn lexer_tags_of_style(
	this: *mut LexerInterface,
	style: c_int,
) -> *const c_char {
	// SAFETY: See `lexer_lex`.
	unsafe { style_description(this, style) }.map_or(c"".as_ptr(), |style| style.tags.as_ptr())
}

unsafe extern "system" fn lexer_description_of_style(
	this: *mut LexerInterface,
	style: c_int,
) -> *const c_char {
	// SAFETY: See `lexer_lex`.
	unsafe { style_description(this, style) }
		.map_or(c"".as_ptr(), |style| style.description.as_ptr())
}

unsafe extern "system" fn lexer_get_name(this: *mut LexerInterface) -> *const c_char {
	// SAFETY: See `lexer_lex`.
	unsafe { lexer_object(this) }.lexer.name().as_ptr()
}

unsafe extern "system" fn lexer_get_identifier(_this: *mut LexerInterface) -> c_int {
	AUTOMATIC_LEXER_IDENTIFIER
}

/// A Lexilla lexer from `Notepad::create_lexer`, released when dropped.
pub struct ForeignLexer {
	raw: NonNull<LexerInterface>,
}

impl ForeignLexer {
	/// # Safety
	/// `raw` must be null or an `ILexer5` that the caller owns.
	pub unsafe fn from_raw(raw: *mut c_void) -> Option<Self> {
		NonNull::new(raw.cast::<LexerInterface>()).map(|raw| Self { raw })
	}

	fn vtable(&self) -> &LexerVtable {
		// SAFETY: An `ILexer5` (see `from_raw`) starts with its vtable pointer.
		unsafe { &*(*self.raw.as_ptr()).vtable }
	}

	/// Sets a lexer property such as `lexer.cpp.track.preprocessor`.
	pub fn set_property(&self, key: &str, value: &str) {
		let key = null_terminated(key);
		let value = null_terminated(value);
		// SAFETY: Both strings are NUL terminated and outlive the call.
		unsafe {
			(self.vtable().property_set)(
				self.raw.as_ptr(),
				key.as_ptr().cast(),
				value.as_ptr().cast(),
			)
		};
	}

	/// Sets keyword list `set` to the space separated `words`.
	pub fn set_word_list(&self, set: usize, words: &str) {
		let words = null_terminated(words);
		// SAFETY: The string is NUL terminated and outlives the call.
		unsafe {
			(self.vtable().word_list_set)(self.raw.as_ptr(), set as c_int, words.as_ptr().cast())
		};
	}

	/// Styles `length` bytes from `start` of `document`.
	///
	/// # Safety
	/// `document` must be a valid `IDocument` for the duration of the call.
	pub unsafe fn lex(
		&self,
		start: usize,
		length: Position,
		initial_style: c_int,
		document: *mut DocumentInterface,
	) {
		// SAFETY: The lexer is live and the document valid (guaranteed by the caller).
		unsafe { (self.vtable().lex)(self.raw.as_ptr(), start, length, initial_style, document) };
	}
}

impl Drop for ForeignLexer {
	fn drop(&mut self) {
		// SAFETY: The lexer is owned (see `from_raw`) and not used after this.
		unsafe { (self.vtable().release)(self.raw.as_ptr()) };
	}
}

fn null_terminated(text: &str) -> Vec<u8> {
	let mut bytes: Vec<u8> = text.bytes().filter(|&byte| byte != 0).collect();
	bytes.push(0);
	bytes
}

/// How a [`MappedDocument`] translates between a foreign lexer and the real document.
pub trait StyleMapping {
	/// The document style for the lexer's `style`.
	fn document_style(&mut self, style: u8) -> u8;
	/// The lexer's style for the document style `style`; 0 if it is none of the lexer's.
	fn lexer_style(&self, style: u8) -> u8;
	fn line_state(&self, line: Position) -> c_int;
	fn set_line_state(&mut self, line: Position, state: c_int);
}

/// An `IDocument` that shows a foreign lexer only a part of a real document (such as a code
/// block) as if it were the whole document, maps its styles and keeps line states of its own,
/// so that the lexer styles that part with styles that do not collide with the document's own
/// lexer. Positions and lines are relative to the start of the part, text outside of it reads as
/// zero bytes and styles outside of it are dropped. Lexers that go back to the start of a
/// statement (bash goes back to a line whose state starts a command) stop at the start of the
/// part instead of restyling the text before it. Fold levels and indicators are not passed on.
#[repr(C)]
pub struct MappedDocument<'a> {
	interface: DocumentInterface,
	inner: Document,
	/// The part in the real document.
	start: Position,
	length: Position,
	/// The line of `start` in the real document.
	first_line: Position,
	/// Where the next style goes, relative to `start`.
	styling_position: Position,
	mapping: &'a mut dyn StyleMapping,
}

impl<'a> MappedDocument<'a> {
	/// The lexer sees `part` of `inner`. Line states reach `mapping` with the lines of `inner`.
	pub fn new(
		inner: Document,
		part: core::ops::Range<Position>,
		mapping: &'a mut dyn StyleMapping,
	) -> Self {
		let start = part.start.clamp(0, inner.length());
		let end = part.end.clamp(start, inner.length());
		Self {
			interface: DocumentInterface { vtable: &MAPPED_DOCUMENT_VTABLE },
			inner,
			start,
			length: end - start,
			first_line: inner.line_from_position(start),
			styling_position: 0,
			mapping,
		}
	}

	/// The `IDocument` pointer to pass to a foreign lexer. Valid while `self` is borrowed.
	pub fn raw(&mut self) -> *mut DocumentInterface {
		core::ptr::from_mut(self).cast()
	}

	/// The real document's position of the part's `position`, clamped to the part.
	fn real_position(&self, position: Position) -> Position {
		self.start + position.clamp(0, self.length)
	}

	/// The part's position of the real `position`, clamped to the part.
	fn part_position(&self, position: Position) -> Position {
		(position - self.start).clamp(0, self.length)
	}

	/// The real document's line of the part's `line`, `None` before the part.
	fn real_line(&self, line: Position) -> Option<Position> {
		(line >= 0).then(|| self.first_line + line)
	}

	/// The real document's vtable and pointer.
	fn real(&self) -> (&DocumentVtable, *mut DocumentInterface) {
		(self.inner.vtable(), self.inner.raw())
	}
}

static MAPPED_DOCUMENT_VTABLE: DocumentVtable = DocumentVtable {
	version: mapped_version,
	set_error_status: mapped_set_error_status,
	length: mapped_length,
	get_char_range: mapped_get_char_range,
	style_at: mapped_style_at,
	line_from_position: mapped_line_from_position,
	line_start: mapped_line_start,
	get_level: mapped_get_level,
	set_level: mapped_set_level,
	get_line_state: mapped_get_line_state,
	set_line_state: mapped_set_line_state,
	start_styling: mapped_start_styling,
	set_style_for: mapped_set_style_for,
	set_styles: mapped_set_styles,
	decoration_set_current_indicator: mapped_decoration_set_current_indicator,
	decoration_fill_range: mapped_decoration_fill_range,
	change_lexer_state: mapped_change_lexer_state,
	code_page: mapped_code_page,
	is_dbcs_lead_byte: mapped_is_dbcs_lead_byte,
	buffer_pointer: mapped_buffer_pointer,
	get_line_indentation: mapped_get_line_indentation,
	line_end: mapped_line_end,
	get_relative_position: mapped_get_relative_position,
	get_character_and_width: mapped_get_character_and_width,
};

/// # Safety
/// `this` must point to a live [`MappedDocument`], which is how the vtable is only used.
unsafe fn mapped<'a, 'b>(this: *const DocumentInterface) -> &'a mut MappedDocument<'b> {
	// SAFETY: Guaranteed by the caller; the interface is the first field. The foreign lexer is
	// the only user of the document while it runs.
	unsafe { &mut *this.cast_mut().cast::<MappedDocument<'b>>() }
}

unsafe extern "system" fn mapped_version(this: *const DocumentInterface) -> c_int {
	// SAFETY: The vtable is only used by `MappedDocument` objects.
	unsafe { mapped(this) }.inner.version()
}

unsafe extern "system" fn mapped_set_error_status(this: *mut DocumentInterface, status: c_int) {
	// SAFETY: See `mapped_version`.
	let (vtable, real_document) = unsafe { mapped(this) }.real();
	// SAFETY: Forwards to the live inner document.
	unsafe { (vtable.set_error_status)(real_document, status) }
}

unsafe extern "system" fn mapped_length(this: *const DocumentInterface) -> Position {
	// SAFETY: See `mapped_version`.
	unsafe { mapped(this) }.length
}

unsafe extern "system" fn mapped_get_char_range(
	this: *const DocumentInterface,
	buffer: *mut c_char,
	position: Position,
	length: Position,
) {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	let Ok(buffer_length) = usize::try_from(length) else { return };
	if buffer.is_null() || buffer_length == 0 {
		return;
	}
	// SAFETY: The lexer passes a buffer for `length` bytes.
	unsafe { core::ptr::write_bytes(buffer, 0, buffer_length) };
	let start = position.max(0);
	let end = position.saturating_add(length).min(document.length);
	if start >= end {
		return;
	}
	let (vtable, real_document) = document.real();
	// SAFETY: `start - position` lies in the buffer and `end - start` bytes fit behind it. The
	// range lies inside the live inner document.
	unsafe {
		(vtable.get_char_range)(
			real_document,
			buffer.add((start - position) as usize),
			document.start + start,
			end - start,
		)
	}
}

unsafe extern "system" fn mapped_style_at(
	this: *const DocumentInterface,
	position: Position,
) -> c_char {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	if !(0..document.length).contains(&position) {
		return 0;
	}
	document.mapping.lexer_style(document.inner.style_at(document.start + position)) as c_char
}

unsafe extern "system" fn mapped_line_from_position(
	this: *const DocumentInterface,
	position: Position,
) -> Position {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	document.inner.line_from_position(document.real_position(position)) - document.first_line
}

unsafe extern "system" fn mapped_line_start(
	this: *const DocumentInterface,
	line: Position,
) -> Position {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	match line {
		..=0 => 0,
		_ => document.part_position(document.inner.line_start(document.first_line + line)),
	}
}

unsafe extern "system" fn mapped_get_level(
	this: *const DocumentInterface,
	line: Position,
) -> c_int {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	document.real_line(line).map_or(0, |line| document.inner.level(line))
}

unsafe extern "system" fn mapped_set_level(
	this: *mut DocumentInterface,
	line: Position,
	_level: c_int,
) -> c_int {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	document.real_line(line).map_or(0, |line| document.inner.level(line))
}

unsafe extern "system" fn mapped_get_line_state(
	this: *const DocumentInterface,
	line: Position,
) -> c_int {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	document.real_line(line).map_or(0, |line| document.mapping.line_state(line))
}

unsafe extern "system" fn mapped_set_line_state(
	this: *mut DocumentInterface,
	line: Position,
	state: c_int,
) -> c_int {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	if let Some(line) = document.real_line(line) {
		document.mapping.set_line_state(line, state);
	}
	state
}

unsafe extern "system" fn mapped_start_styling(this: *mut DocumentInterface, position: Position) {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	document.styling_position = position.clamp(0, document.length);
	document.inner.start_styling(document.start + document.styling_position);
}

unsafe extern "system" fn mapped_set_style_for(
	this: *mut DocumentInterface,
	length: Position,
	style: c_char,
) -> bool {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	let length = length.clamp(0, document.length - document.styling_position);
	document.styling_position += length;
	let style = document.mapping.document_style(style as u8);
	document.inner.set_style_for(length, style)
}

unsafe extern "system" fn mapped_set_styles(
	this: *mut DocumentInterface,
	length: Position,
	styles: *const c_char,
) -> bool {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	let Ok(length) = usize::try_from(length) else { return false };
	if styles.is_null() {
		return false;
	}
	// SAFETY: The lexer passes `length` styles.
	let styles = unsafe { core::slice::from_raw_parts(styles.cast::<u8>(), length) };
	let remaining = usize::try_from(document.length - document.styling_position).unwrap_or(0);
	let styles = &styles[..length.min(remaining)];
	document.styling_position += styles.len() as Position;
	let mut cache: [Option<u8>; 256] = [None; 256];
	let mapped_styles: Vec<u8> = styles
		.iter()
		.map(|&style| {
			*cache[usize::from(style)].get_or_insert_with(|| document.mapping.document_style(style))
		})
		.collect();
	document.inner.set_styles(&mapped_styles)
}

unsafe extern "system" fn mapped_decoration_set_current_indicator(
	_this: *mut DocumentInterface,
	_indicator: c_int,
) {
}

unsafe extern "system" fn mapped_decoration_fill_range(
	_this: *mut DocumentInterface,
	_position: Position,
	_value: c_int,
	_length: Position,
) {
}

unsafe extern "system" fn mapped_change_lexer_state(
	this: *mut DocumentInterface,
	start: Position,
	end: Position,
) {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	let (start, end) = (document.real_position(start), document.real_position(end));
	let (vtable, real_document) = document.real();
	// SAFETY: Forwards to the live inner document.
	unsafe { (vtable.change_lexer_state)(real_document, start, end) }
}

unsafe extern "system" fn mapped_code_page(this: *const DocumentInterface) -> c_int {
	// SAFETY: See `mapped_version`.
	unsafe { mapped(this) }.inner.code_page()
}

unsafe extern "system" fn mapped_is_dbcs_lead_byte(
	this: *const DocumentInterface,
	byte: c_char,
) -> bool {
	// SAFETY: See `mapped_version`.
	unsafe { mapped(this) }.inner.is_dbcs_lead_byte(byte as u8)
}

unsafe extern "system" fn mapped_buffer_pointer(this: *mut DocumentInterface) -> *const c_char {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	let (vtable, real_document) = document.real();
	// SAFETY: Forwards to the live inner document.
	let text = unsafe { (vtable.buffer_pointer)(real_document) };
	if text.is_null() {
		return text;
	}
	// SAFETY: The buffer holds the whole real document, which contains the part.
	unsafe { text.add(document.start as usize) }
}

unsafe extern "system" fn mapped_get_line_indentation(
	this: *mut DocumentInterface,
	line: Position,
) -> c_int {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	let Some(line) = document.real_line(line) else { return 0 };
	let (vtable, real_document) = document.real();
	// SAFETY: Forwards to the live inner document.
	unsafe { (vtable.get_line_indentation)(real_document, line) }
}

unsafe extern "system" fn mapped_line_end(
	this: *const DocumentInterface,
	line: Position,
) -> Position {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	let Some(line) = document.real_line(line) else { return 0 };
	let (vtable, real_document) = document.real();
	// SAFETY: Forwards to the live inner document.
	let end = unsafe { (vtable.line_end)(real_document, line) };
	document.part_position(end)
}

unsafe extern "system" fn mapped_get_relative_position(
	this: *const DocumentInterface,
	start: Position,
	character_offset: Position,
) -> Position {
	/// Scintilla's `INVALID_POSITION`.
	const INVALID_POSITION: Position = -1;
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	if !(0..=document.length).contains(&start) {
		return INVALID_POSITION;
	}
	let (vtable, real_document) = document.real();
	// SAFETY: Forwards to the live inner document.
	let position = unsafe {
		(vtable.get_relative_position)(real_document, document.start + start, character_offset)
	};
	match position - document.start {
		position if position >= 0 && position <= document.length => position,
		_ => INVALID_POSITION,
	}
}

unsafe extern "system" fn mapped_get_character_and_width(
	this: *const DocumentInterface,
	position: Position,
	width: *mut Position,
) -> c_int {
	// SAFETY: See `mapped_version`.
	let document = unsafe { mapped(this) };
	if !(0..document.length).contains(&position) {
		if !width.is_null() {
			// SAFETY: The lexer passes a valid out pointer or null.
			unsafe { *width = 1 };
		}
		return 0;
	}
	let (vtable, real_document) = document.real();
	// SAFETY: Forwards the lexer's out pointer to the live inner document.
	unsafe { (vtable.get_character_and_width)(real_document, document.start + position, width) }
}
