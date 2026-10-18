//! The ways to show the rendered document: WebView2 or EdgeHTML with the HTML of `markdown_html`,
//! or RichEdit with the RTF of `markdown_rtf`.

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;

use npp::edge_html::{EdgeView, create_edge_view};
use npp::rich_edit::RichEdit;
use npp::webview2::{
	Color, HRESULT, WebView, WebViewEvents, WebViewOptions, create_webview, find_runtime,
};
use windows_sys::Win32::Foundation::{HWND, RECT};

use super::window::{self, ContextMenu, ViewerMessage};
use crate::image_embedding::{EmbeddedImage, ImageEmbedding};
use crate::markdown_html::{HighlightedSpan, ImageSyntax, RenderOptions, Styling, render_markdown};
use crate::markdown_rtf::{DARK_THEME, LIGHT_THEME, RtfDocument, RtfOptions, RtfTheme, render_rtf};
use crate::svg_animation::css_animated_image;
use crate::viewer_page::{
	CLEAR_SELECTION_SCRIPT, CONTEXT_MENU_SCRIPT, ContextMenuRequest, LINK_HOVER_SCRIPT, LinkTarget,
	SELECT_ALL_SCRIPT, anchor_script, content_script, link_location, link_target,
	parse_hovered_link, position_of_line, positions_of_lines, shell_page, status_text_link,
	sync_script, target_location,
};

/// Control ID of the RichEdit in the host window.
pub const RICH_EDIT_ID: usize = 100;

/// The engine selected with the `Engine` setting. The browsers fall back to RichEdit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineKind {
	WebView2,
	RichEdit,
	EdgeHtml,
}

impl EngineKind {
	pub fn from_setting(value: u32) -> Self {
		match value {
			1 => EngineKind::RichEdit,
			2 => EngineKind::EdgeHtml,
			_ => EngineKind::WebView2,
		}
	}
}

/// Local images of the current document by their source.
#[derive(Default)]
pub struct ImageCache {
	images: HashMap<String, Option<Rc<EmbeddedImage>>>,
	data_uris: HashMap<String, Option<Rc<str>>>,
}

impl ImageCache {
	pub fn clear(&mut self) {
		self.images.clear();
		self.data_uris.clear();
	}

	fn load(&mut self, embedding: &ImageEmbedding, source: &str) -> Option<Rc<EmbeddedImage>> {
		let image = self.images.entry(source.to_owned());
		image.or_insert_with(|| embedding.load(source).map(Rc::new)).clone()
	}

	/// The `data:` URI of the local image `source`. `css_animations` converts the SMIL animations of
	/// SVG images for EdgeHTML. It must not change while the cache is filled.
	fn data_uri(
		&mut self,
		embedding: &ImageEmbedding,
		source: &str,
		css_animations: bool,
	) -> Option<Rc<str>> {
		if let Some(uri) = self.data_uris.get(source) {
			return uri.clone();
		}
		let uri = self.load(embedding, source).map(|image| {
			let converted = css_animations.then(|| css_animated_image(&image)).flatten();
			Rc::from(converted.as_ref().unwrap_or(&image).data_uri())
		});
		self.data_uris.insert(source.to_owned(), uri.clone());
		uri
	}
}

pub struct RenderInput<'a> {
	pub markdown: &'a str,
	pub embedding: ImageEmbedding<'a>,
	pub images: &'a mut ImageCache,
	pub highlight_code: &'a mut dyn FnMut(&str, &str) -> Vec<HighlightedSpan>,
	/// For relative links in the browsers.
	pub base_url: String,
}

pub enum Engine {
	WebView(WebViewEngine),
	RichEdit(RichEditEngine),
	/// Neither a browser nor RichEdit could be created.
	Unavailable,
}

/// A browser control showing the [`shell_page`], whose content the viewer replaces by script.
pub enum BrowserView {
	WebView2(WebView),
	EdgeHtml(EdgeView),
}

pub struct WebViewEngine {
	host: HWND,
	/// EdgeHTML plays no SMIL animations.
	is_edge_html: bool,
	/// `None` until the browser reported the creation.
	view: Option<BrowserView>,
	is_dark: bool,
	is_page_loaded: bool,
	body: String,
	base_url: String,
	/// The content shown, to skip unchanged updates.
	shown: Option<(String, String)>,
	/// Repeated after the page is loaded.
	last_sync: Option<(usize, Option<Range<usize>>)>,
}

pub struct RichEditEngine {
	control: RichEdit,
	is_dark: bool,
	document: RtfDocument,
}

fn theme_background(is_dark: bool) -> [u8; 3] {
	theme(is_dark).background
}

fn theme(is_dark: bool) -> &'static RtfTheme {
	match is_dark {
		true => &DARK_THEME,
		false => &LIGHT_THEME,
	}
}

impl Engine {
	pub fn create(host: HWND, kind: EngineKind, is_dark: bool) -> Self {
		if kind != EngineKind::RichEdit
			&& let Some(engine) = WebViewEngine::create(host, kind, is_dark)
		{
			return Engine::WebView(engine);
		}
		Self::rich_edit(host, is_dark)
	}

	fn rich_edit(host: HWND, is_dark: bool) -> Self {
		match RichEdit::create(host, RICH_EDIT_ID) {
			Some(control) => {
				let engine = RichEditEngine { control, is_dark, document: RtfDocument::default() };
				engine.apply_background();
				Engine::RichEdit(engine)
			}
			None => Engine::Unavailable,
		}
	}

	pub fn rich_edit_control(&self) -> Option<&RichEdit> {
		match self {
			Engine::RichEdit(engine) => Some(&engine.control),
			_ => None,
		}
	}

	pub fn set_bounds(&self, bounds: RECT) {
		match self {
			Engine::WebView(engine) => {
				if let Some(view) = &engine.view {
					view.set_bounds(bounds);
				}
			}
			Engine::RichEdit(engine) => window::move_window(engine.control.window(), bounds),
			Engine::Unavailable => {}
		}
	}

	/// The host moved into another window. EdgeHTML follows its host window by itself.
	pub fn parent_changed(&self) {
		if let Engine::WebView(WebViewEngine {
			view: Some(BrowserView::WebView2(view)),
			host,
			..
		}) = self
		{
			view.set_parent(*host);
		}
	}

	/// The window containing the host moved.
	pub fn parent_moved(&self) {
		if let Engine::WebView(WebViewEngine { view: Some(BrowserView::WebView2(view)), .. }) = self
		{
			view.notify_parent_moved();
		}
	}

	pub fn focus(&self) {
		match self {
			Engine::WebView(WebViewEngine { view: Some(view), .. }) => view.focus(),
			Engine::RichEdit(engine) => window::focus(engine.control.window()),
			_ => {}
		}
	}

	/// The browser finished creating the view. A failure falls back to RichEdit. Returns whether
	/// the document needs to be rendered again.
	pub fn webview_created(&mut self, result: Result<BrowserView, HRESULT>) -> bool {
		let Engine::WebView(engine) = self else { return false };
		match result {
			Ok(view) => {
				view.set_visible(true);
				view.navigate_to_string(&shell_page(engine.is_dark));
				engine.view = Some(view);
				false
			}
			Err(_) => {
				*self = Self::rich_edit(engine.host, engine.is_dark);
				true
			}
		}
	}

	/// The shell page loaded, so the content can be shown.
	pub fn document_loaded(&mut self) {
		if let Engine::WebView(engine) = self {
			if let Some(view @ BrowserView::EdgeHtml(_)) = &engine.view {
				view.execute_script(CONTEXT_MENU_SCRIPT);
				view.execute_script(LINK_HOVER_SCRIPT);
			}
			engine.is_page_loaded = true;
			engine.shown = None;
			engine.push_content();
			if let Some((caret_line, selected_lines)) = engine.last_sync.clone() {
				engine.sync(caret_line, selected_lines);
			}
		}
	}

	pub fn set_dark(&mut self, is_dark: bool) {
		match self {
			Engine::WebView(engine) if engine.is_dark != is_dark => {
				engine.is_dark = is_dark;
				engine.is_page_loaded = false;
				if let Some(view) = &engine.view {
					view.set_background(webview_color(is_dark));
					view.navigate_to_string(&shell_page(is_dark));
				}
			}
			Engine::RichEdit(engine) => {
				engine.is_dark = is_dark;
				engine.apply_background();
			}
			_ => {}
		}
	}

	pub fn show(&mut self, input: &mut RenderInput) {
		match self {
			Engine::WebView(engine) => {
				let css_animations = engine.is_edge_html;
				let mut replace_image_source = |_: ImageSyntax, source: &str| {
					let uri = input.images.data_uri(&input.embedding, source, css_animations);
					uri.map(|uri| uri.to_string())
				};
				engine.body = render_markdown(
					input.markdown,
					&mut RenderOptions {
						styling: Styling::StyleSheet,
						replace_image_source: &mut replace_image_source,
						highlight_code: input.highlight_code,
						annotates_source_lines: true,
						is_dark: engine.is_dark,
					},
				);
				engine.base_url = input.base_url.clone();
				engine.push_content();
			}
			Engine::RichEdit(engine) => {
				let mut load_image = |source: &str| input.images.load(&input.embedding, source);
				engine.document = render_rtf(
					input.markdown,
					&mut RtfOptions {
						theme: theme(engine.is_dark),
						page_width: engine.control.text_width_in_twips(),
						pixels_per_inch: engine.control.dpi(),
						load_image: &mut load_image,
						highlight_code: input.highlight_code,
					},
				);
				// Notepad++'s dark mode theming may have set its own background.
				engine.apply_background();
				engine.control.set_rtf(&engine.document.rtf);
			}
			Engine::Unavailable => {}
		}
	}

	pub fn sync(&mut self, caret_line: usize, selected_lines: Option<Range<usize>>) {
		match self {
			Engine::WebView(engine) => engine.sync(caret_line, selected_lines),
			Engine::RichEdit(engine) => {
				let blocks = &engine.document.blocks;
				let length = engine.control.text_length();
				let selection =
					selected_lines.and_then(|lines| positions_of_lines(blocks, lines, length));
				if let Some(position) = position_of_line(blocks, caret_line) {
					engine.control.scroll_to(position, 1.0 / 3.0);
				}
				match selection {
					Some(selection) => engine.control.select(selection),
					None => {
						let position = position_of_line(blocks, caret_line).unwrap_or(0);
						engine.control.select(position..position);
					}
				}
			}
			Engine::Unavailable => {}
		}
	}

	pub fn clear_selection(&mut self) {
		match self {
			Engine::WebView(engine) => {
				if let Some(view) = &engine.view {
					view.execute_script(CLEAR_SELECTION_SCRIPT);
				}
				engine.last_sync = None;
			}
			Engine::RichEdit(engine) => engine.control.select(0..0),
			Engine::Unavailable => {}
		}
	}

	/// The target of a link the browser reported.
	pub fn resolved_link_target(&self, uri: &str) -> Option<LinkTarget> {
		match self {
			Engine::WebView(engine) => {
				Some(crate::viewer_page::resolved_link_target(uri, &engine.base_url))
			}
			_ => None,
		}
	}

	/// What "Copy Link" copies for a link the browser reported.
	pub fn link_location(&self, uri: &str) -> Option<String> {
		match self {
			Engine::WebView(engine) => link_location(uri, &engine.base_url),
			_ => None,
		}
	}

	/// What "Copy Link" copies for the RichEdit link at `characters`.
	pub fn rich_edit_link_location_at(
		&self,
		characters: Range<usize>,
		directory: Option<&Path>,
	) -> Option<String> {
		target_location(self.link_target_at(characters, directory)?)
	}

	/// What "Copy Link" copies for the RichEdit link at the screen position `point`.
	pub fn rich_edit_link_location(
		&self,
		point: windows_sys::Win32::Foundation::POINT,
		directory: Option<&Path>,
	) -> Option<String> {
		let character = self.rich_edit_control()?.character_at(point)?;
		self.rich_edit_link_location_at(character..character + 1, directory)
	}

	pub fn select_all_in_browser(&self) {
		if let Engine::WebView(WebViewEngine { view: Some(view), .. }) = self {
			view.execute_script(SELECT_ALL_SCRIPT);
		}
	}

	/// The target of the RichEdit link at `characters`.
	pub fn link_target_at(
		&self,
		characters: Range<usize>,
		directory: Option<&Path>,
	) -> Option<LinkTarget> {
		let Engine::RichEdit(engine) = self else { return None };
		// RichEdit reports the destination of the field instruction, but accept the text too.
		let end = characters.end.max(characters.start + 1);
		let overlaps = |range: &Range<usize>| range.start < end && characters.start < range.end;
		let link = engine
			.document
			.links
			.iter()
			.find(|link| overlaps(&link.target_range) || overlaps(&link.range))?;
		Some(link_target(&link.target, directory))
	}

	pub fn scroll_to_anchor(&self, anchor: &str) {
		match self {
			Engine::WebView(WebViewEngine { view: Some(view), .. }) => {
				view.execute_script(&anchor_script(anchor));
			}
			Engine::RichEdit(engine) => {
				let found = engine.document.anchors.iter().find(|(id, _)| id == anchor);
				if let Some(&(_, position)) = found {
					engine.control.scroll_to(position, 0.0);
				}
			}
			_ => {}
		}
	}
}

fn webview_color(is_dark: bool) -> Color {
	let [red, green, blue] = theme_background(is_dark);
	Color { alpha: 255, red, green, blue }
}

impl BrowserView {
	fn set_bounds(&self, bounds: RECT) {
		match self {
			BrowserView::WebView2(view) => view.set_bounds(bounds),
			BrowserView::EdgeHtml(view) => view.set_bounds(bounds),
		}
	}

	fn set_visible(&self, is_visible: bool) {
		match self {
			BrowserView::WebView2(view) => view.set_visible(is_visible),
			BrowserView::EdgeHtml(view) => view.set_visible(is_visible),
		}
	}

	fn focus(&self) {
		match self {
			BrowserView::WebView2(view) => view.focus(),
			BrowserView::EdgeHtml(view) => view.focus(),
		}
	}

	fn set_background(&self, color: Color) {
		match self {
			BrowserView::WebView2(view) => view.set_background(color),
			BrowserView::EdgeHtml(view) => view.set_background(color),
		}
	}

	fn navigate_to_string(&self, html: &str) {
		match self {
			BrowserView::WebView2(view) => view.navigate_to_string(html),
			BrowserView::EdgeHtml(view) => view.navigate_to_string(html),
		}
	}

	fn execute_script(&self, script: &str) {
		match self {
			BrowserView::WebView2(view) => view.execute_script(script),
			BrowserView::EdgeHtml(view) => view.execute_script(script),
		}
	}
}

impl WebViewEngine {
	fn create(host: HWND, kind: EngineKind, is_dark: bool) -> Option<Self> {
		let events = WebViewEvents {
			link_activated: Box::new(move |uri| {
				window::post(host, ViewerMessage::LinkActivated(uri.to_owned()));
			}),
			document_loaded: Box::new(move || window::post(host, ViewerMessage::DocumentLoaded)),
		};
		if kind == EngineKind::EdgeHtml {
			let created = Box::new(move |result: Result<EdgeView, HRESULT>| {
				let result = result.map(BrowserView::EdgeHtml);
				window::post(host, ViewerMessage::WebViewCreated(result));
			});
			// The bounds follow with the first layout after the creation.
			let bounds = RECT { left: 0, top: 0, right: 0, bottom: 0 };
			let script_notified = Box::new(move |message: &str| {
				if let Some(request) = ContextMenuRequest::parse(message) {
					window::post(host, ViewerMessage::ContextMenu(ContextMenu::EdgeHtml(request)));
				} else if let Some(link) = parse_hovered_link(message) {
					window::post(host, ViewerMessage::LinkHovered(link));
				}
			});
			let background = webview_color(is_dark);
			create_edge_view(host, bounds, background, events, script_notified, created).ok()?;
		} else {
			let runtime = find_runtime(npp::plugin::module_directory().as_deref())?;
			let user_data_folder = std::env::temp_dir().join("NppToolBox.WebView2");
			let options = WebViewOptions {
				runtime: &runtime,
				user_data_folder: &user_data_folder,
				background: webview_color(is_dark),
			};
			let created = Box::new(move |result: Result<WebView, HRESULT>| {
				let result = result.map(BrowserView::WebView2);
				window::post(host, ViewerMessage::WebViewCreated(result));
			});
			let context_menu_requested = Box::new(move |request| {
				window::post(host, ViewerMessage::ContextMenu(ContextMenu::WebView2(request)));
			});
			let status_bar_text_changed = Box::new(move |text: &str| {
				window::post(host, ViewerMessage::LinkHovered(status_text_link(text)));
			});
			create_webview(
				host,
				&options,
				events,
				context_menu_requested,
				status_bar_text_changed,
				created,
			)
			.ok()?;
		}
		Some(Self {
			host,
			is_edge_html: kind == EngineKind::EdgeHtml,
			view: None,
			is_dark,
			is_page_loaded: false,
			body: String::new(),
			base_url: String::new(),
			shown: None,
			last_sync: None,
		})
	}

	fn push_content(&mut self) {
		let Some(view) = self.view.as_ref().filter(|_| self.is_page_loaded) else { return };
		let content = (self.body.clone(), self.base_url.clone());
		if self.shown.as_ref() == Some(&content) {
			return;
		}
		view.execute_script(&content_script(&content.0, &content.1));
		self.shown = Some(content);
	}

	fn sync(&mut self, caret_line: usize, selected_lines: Option<Range<usize>>) {
		if let Some(view) = self.view.as_ref().filter(|_| self.is_page_loaded) {
			view.execute_script(&sync_script(caret_line, selected_lines.clone()));
		}
		self.last_sync = Some((caret_line, selected_lines));
	}
}

impl RichEditEngine {
	fn apply_background(&self) {
		let [red, green, blue] = theme_background(self.is_dark);
		self.control.set_background(u32::from(red) | u32::from(green) << 8 | u32::from(blue) << 16);
	}
}
