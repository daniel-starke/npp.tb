//! The subset of the WebView2 COM interfaces (`WebView2.h` of the WebView2 SDK) used by
//! [`super::WebView`].
//!
//! Interfaces that are only called declare their methods up to the last one used. Methods that are
//! never called keep their vtable slot with a placeholder signature. The method names are those of
//! the SDK header so that they can be looked up there.

#![allow(non_snake_case, non_camel_case_types, clippy::upper_case_acronyms)]

use core::ffi::c_void;

use windows_core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface, interface};
use windows_sys::Win32::Foundation::{HWND, RECT};

pub type BOOL = i32;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct EventRegistrationToken {
	pub value: i64,
}

/// `COREWEBVIEW2_COLOR`
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Color {
	pub alpha: u8,
	pub red: u8,
	pub green: u8,
	pub blue: u8,
}

/// `COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC`
pub const MOVE_FOCUS_REASON_PROGRAMMATIC: i32 = 0;

#[interface("b96d755e-0319-4e92-a296-23436f46a1fc")]
pub unsafe trait ICoreWebView2Environment: IUnknown {
	/// `handler`: [`ICoreWebView2CreateCoreWebView2ControllerCompletedHandler`].
	pub fn CreateCoreWebView2Controller(&self, parent: HWND, handler: *mut c_void) -> HRESULT;
}

#[interface("4d00c0d1-9434-4eb6-8078-8697a560334f")]
pub unsafe trait ICoreWebView2Controller: IUnknown {
	pub fn get_IsVisible(&self) -> HRESULT;
	pub fn put_IsVisible(&self, is_visible: BOOL) -> HRESULT;
	pub fn get_Bounds(&self) -> HRESULT;
	pub fn put_Bounds(&self, bounds: RECT) -> HRESULT;
	pub fn get_ZoomFactor(&self) -> HRESULT;
	pub fn put_ZoomFactor(&self) -> HRESULT;
	pub fn add_ZoomFactorChanged(&self) -> HRESULT;
	pub fn remove_ZoomFactorChanged(&self) -> HRESULT;
	pub fn SetBoundsAndZoomFactor(&self) -> HRESULT;
	pub fn MoveFocus(&self, reason: i32) -> HRESULT;
	pub fn add_MoveFocusRequested(&self) -> HRESULT;
	pub fn remove_MoveFocusRequested(&self) -> HRESULT;
	pub fn add_GotFocus(&self) -> HRESULT;
	pub fn remove_GotFocus(&self) -> HRESULT;
	pub fn add_LostFocus(&self) -> HRESULT;
	pub fn remove_LostFocus(&self) -> HRESULT;
	pub fn add_AcceleratorKeyPressed(&self) -> HRESULT;
	pub fn remove_AcceleratorKeyPressed(&self) -> HRESULT;
	pub fn get_ParentWindow(&self) -> HRESULT;
	pub fn put_ParentWindow(&self, parent: HWND) -> HRESULT;
	pub fn NotifyParentWindowPositionChanged(&self) -> HRESULT;
	pub fn Close(&self) -> HRESULT;
	pub fn get_CoreWebView2(&self, webview: *mut Option<ICoreWebView2>) -> HRESULT;
}

#[interface("c979903e-d4ca-4228-92eb-47ee3fa96eab")]
pub unsafe trait ICoreWebView2Controller2: ICoreWebView2Controller {
	pub fn get_DefaultBackgroundColor(&self) -> HRESULT;
	pub fn put_DefaultBackgroundColor(&self, color: Color) -> HRESULT;
}

#[interface("76eceacb-0462-4d94-ac83-423a6793775e")]
pub unsafe trait ICoreWebView2: IUnknown {
	pub fn get_Settings(&self, settings: *mut Option<ICoreWebView2Settings>) -> HRESULT;
	pub fn get_Source(&self) -> HRESULT;
	pub fn Navigate(&self) -> HRESULT;
	pub fn NavigateToString(&self, html: *const u16) -> HRESULT;
	/// `handler`: [`ICoreWebView2NavigationStartingEventHandler`].
	pub fn add_NavigationStarting(
		&self,
		handler: *mut c_void,
		token: *mut EventRegistrationToken,
	) -> HRESULT;
	pub fn remove_NavigationStarting(&self) -> HRESULT;
	pub fn add_ContentLoading(&self) -> HRESULT;
	pub fn remove_ContentLoading(&self) -> HRESULT;
	pub fn add_SourceChanged(&self) -> HRESULT;
	pub fn remove_SourceChanged(&self) -> HRESULT;
	pub fn add_HistoryChanged(&self) -> HRESULT;
	pub fn remove_HistoryChanged(&self) -> HRESULT;
	/// `handler`: [`ICoreWebView2NavigationCompletedEventHandler`].
	pub fn add_NavigationCompleted(
		&self,
		handler: *mut c_void,
		token: *mut EventRegistrationToken,
	) -> HRESULT;
	pub fn remove_NavigationCompleted(&self) -> HRESULT;
	pub fn add_FrameNavigationStarting(&self) -> HRESULT;
	pub fn remove_FrameNavigationStarting(&self) -> HRESULT;
	pub fn add_FrameNavigationCompleted(&self) -> HRESULT;
	pub fn remove_FrameNavigationCompleted(&self) -> HRESULT;
	pub fn add_ScriptDialogOpening(&self) -> HRESULT;
	pub fn remove_ScriptDialogOpening(&self) -> HRESULT;
	pub fn add_PermissionRequested(&self) -> HRESULT;
	pub fn remove_PermissionRequested(&self) -> HRESULT;
	pub fn add_ProcessFailed(&self) -> HRESULT;
	pub fn remove_ProcessFailed(&self) -> HRESULT;
	pub fn AddScriptToExecuteOnDocumentCreated(&self) -> HRESULT;
	pub fn RemoveScriptToExecuteOnDocumentCreated(&self) -> HRESULT;
	/// `handler`: [`ICoreWebView2ExecuteScriptCompletedHandler`] or null.
	pub fn ExecuteScript(&self, script: *const u16, handler: *mut c_void) -> HRESULT;
	pub fn CapturePreview(&self) -> HRESULT;
	pub fn Reload(&self) -> HRESULT;
	pub fn PostWebMessageAsJson(&self) -> HRESULT;
	pub fn PostWebMessageAsString(&self) -> HRESULT;
	pub fn add_WebMessageReceived(&self) -> HRESULT;
	pub fn remove_WebMessageReceived(&self) -> HRESULT;
	pub fn CallDevToolsProtocolMethod(&self) -> HRESULT;
	pub fn get_BrowserProcessId(&self) -> HRESULT;
	pub fn get_CanGoBack(&self) -> HRESULT;
	pub fn get_CanGoForward(&self) -> HRESULT;
	pub fn GoBack(&self) -> HRESULT;
	pub fn GoForward(&self) -> HRESULT;
	pub fn GetDevToolsProtocolEventReceiver(&self) -> HRESULT;
	pub fn Stop(&self) -> HRESULT;
	/// `handler`: [`ICoreWebView2NewWindowRequestedEventHandler`].
	pub fn add_NewWindowRequested(
		&self,
		handler: *mut c_void,
		token: *mut EventRegistrationToken,
	) -> HRESULT;
}

/// Number of methods after `IUnknown` that precede `add_ContextMenuRequested`: 58 of
/// `ICoreWebView2`, those added by `ICoreWebView2_2` through `ICoreWebView2_10`, and
/// `ICoreWebView2_11::CallDevToolsProtocolMethodForSession`.
const METHODS_BEFORE_ADD_CONTEXT_MENU_REQUESTED: usize = 58 + 7 + 5 + 4 + 2 + 1 + 1 + 7 + 9 + 2 + 1;

/// `ICoreWebView2_11`, declared by hand: the macro would need all 100 inherited methods.
#[repr(transparent)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICoreWebView2_11(IUnknown);

// SAFETY: `ICoreWebView2_11` is a COM interface pointer with the IID and vtable layout of the SDK.
unsafe impl Interface for ICoreWebView2_11 {
	type Vtable = ICoreWebView2_11_Vtbl;
	const IID: GUID = GUID::from_u128(0x0be78e56_c193_4051_b943_23b460c08bdb);
}

#[repr(C)]
pub struct ICoreWebView2_11_Vtbl {
	pub base: IUnknown_Vtbl,
	inherited: [usize; METHODS_BEFORE_ADD_CONTEXT_MENU_REQUESTED],
	/// `handler`: [`ICoreWebView2ContextMenuRequestedEventHandler`].
	pub add_ContextMenuRequested: unsafe extern "system" fn(
		this: *mut c_void,
		handler: *mut c_void,
		token: *mut EventRegistrationToken,
	) -> HRESULT,
}

impl ICoreWebView2_11 {
	/// # Safety
	/// `handler` must be an `ICoreWebView2ContextMenuRequestedEventHandler`.
	pub unsafe fn add_ContextMenuRequested(
		&self,
		handler: *mut c_void,
		token: *mut EventRegistrationToken,
	) -> HRESULT {
		// SAFETY: The vtable matches the SDK declaration, the caller guarantees the handler.
		unsafe { (self.vtable().add_ContextMenuRequested)(self.as_raw(), handler, token) }
	}
}

/// `ICoreWebView2_12`, declared by hand like [`ICoreWebView2_11`]. Its first methods follow
/// `ICoreWebView2_11::add_ContextMenuRequested` and `remove_ContextMenuRequested`.
#[repr(transparent)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICoreWebView2_12(IUnknown);

// SAFETY: `ICoreWebView2_12` is a COM interface pointer with the IID and vtable layout of the SDK.
unsafe impl Interface for ICoreWebView2_12 {
	type Vtable = ICoreWebView2_12_Vtbl;
	const IID: GUID = GUID::from_u128(0x35d69927_bcfa_4566_9349_6b3e0d154cac);
}

#[repr(C)]
pub struct ICoreWebView2_12_Vtbl {
	pub base: IUnknown_Vtbl,
	inherited: [usize; METHODS_BEFORE_ADD_CONTEXT_MENU_REQUESTED + 2],
	/// `handler`: [`ICoreWebView2StatusBarTextChangedEventHandler`].
	pub add_StatusBarTextChanged: unsafe extern "system" fn(
		this: *mut c_void,
		handler: *mut c_void,
		token: *mut EventRegistrationToken,
	) -> HRESULT,
	remove_StatusBarTextChanged: usize,
	pub get_StatusBarText:
		unsafe extern "system" fn(this: *mut c_void, value: *mut *mut u16) -> HRESULT,
}

impl ICoreWebView2_12 {
	/// # Safety
	/// `handler` must be an `ICoreWebView2StatusBarTextChangedEventHandler`.
	pub unsafe fn add_StatusBarTextChanged(
		&self,
		handler: *mut c_void,
		token: *mut EventRegistrationToken,
	) -> HRESULT {
		// SAFETY: The vtable matches the SDK declaration, the caller guarantees the handler.
		unsafe { (self.vtable().add_StatusBarTextChanged)(self.as_raw(), handler, token) }
	}

	/// # Safety
	/// `value` must be valid for writes. It receives a string to free with `CoTaskMemFree`.
	pub unsafe fn get_StatusBarText(&self, value: *mut *mut u16) -> HRESULT {
		// SAFETY: The vtable matches the SDK declaration, the caller guarantees the pointer.
		unsafe { (self.vtable().get_StatusBarText)(self.as_raw(), value) }
	}
}

#[interface("e562e4f0-d7fa-43ac-8d71-c05150499f00")]
pub unsafe trait ICoreWebView2Settings: IUnknown {
	pub fn get_IsScriptEnabled(&self) -> HRESULT;
	pub fn put_IsScriptEnabled(&self, is_enabled: BOOL) -> HRESULT;
	pub fn get_IsWebMessageEnabled(&self) -> HRESULT;
	pub fn put_IsWebMessageEnabled(&self, is_enabled: BOOL) -> HRESULT;
	pub fn get_AreDefaultScriptDialogsEnabled(&self) -> HRESULT;
	pub fn put_AreDefaultScriptDialogsEnabled(&self, are_enabled: BOOL) -> HRESULT;
	pub fn get_IsStatusBarEnabled(&self) -> HRESULT;
	pub fn put_IsStatusBarEnabled(&self, is_enabled: BOOL) -> HRESULT;
	pub fn get_AreDevToolsEnabled(&self) -> HRESULT;
	pub fn put_AreDevToolsEnabled(&self, are_enabled: BOOL) -> HRESULT;
	pub fn get_AreDefaultContextMenusEnabled(&self) -> HRESULT;
	pub fn put_AreDefaultContextMenusEnabled(&self, are_enabled: BOOL) -> HRESULT;
	pub fn get_AreHostObjectsAllowed(&self) -> HRESULT;
	pub fn put_AreHostObjectsAllowed(&self, are_allowed: BOOL) -> HRESULT;
	pub fn get_IsZoomControlEnabled(&self) -> HRESULT;
	pub fn put_IsZoomControlEnabled(&self, is_enabled: BOOL) -> HRESULT;
	pub fn get_IsBuiltInErrorPageEnabled(&self) -> HRESULT;
	pub fn put_IsBuiltInErrorPageEnabled(&self, is_enabled: BOOL) -> HRESULT;
}

#[interface("ee9a0f68-f46c-4e32-ac23-ef8cac224d2a")]
pub unsafe trait ICoreWebView2Settings2: ICoreWebView2Settings {
	pub fn get_UserAgent(&self) -> HRESULT;
	pub fn put_UserAgent(&self) -> HRESULT;
}

#[interface("fdb5ab74-af33-4854-84f0-0a631deb5eba")]
pub unsafe trait ICoreWebView2Settings3: ICoreWebView2Settings2 {
	pub fn get_AreBrowserAcceleratorKeysEnabled(&self) -> HRESULT;
	pub fn put_AreBrowserAcceleratorKeysEnabled(&self, are_enabled: BOOL) -> HRESULT;
}

#[interface("5b495469-e119-438a-9b18-7604f25f2e49")]
pub unsafe trait ICoreWebView2NavigationStartingEventArgs: IUnknown {
	/// Allocated with `CoTaskMemAlloc`.
	pub fn get_Uri(&self, uri: *mut *mut u16) -> HRESULT;
	pub fn get_IsUserInitiated(&self) -> HRESULT;
	pub fn get_IsRedirected(&self) -> HRESULT;
	pub fn get_RequestHeaders(&self) -> HRESULT;
	pub fn get_Cancel(&self) -> HRESULT;
	pub fn put_Cancel(&self, cancel: BOOL) -> HRESULT;
}

#[interface("34acb11c-fc37-4418-9132-f9c21d1eafb9")]
pub unsafe trait ICoreWebView2NewWindowRequestedEventArgs: IUnknown {
	/// Allocated with `CoTaskMemAlloc`.
	pub fn get_Uri(&self, uri: *mut *mut u16) -> HRESULT;
	pub fn put_NewWindow(&self) -> HRESULT;
	pub fn get_NewWindow(&self) -> HRESULT;
	pub fn put_Handled(&self, handled: BOOL) -> HRESULT;
}

#[interface("a1d309ee-c03f-11eb-8529-0242ac130003")]
pub unsafe trait ICoreWebView2ContextMenuRequestedEventArgs: IUnknown {
	pub fn get_MenuItems(
		&self,
		items: *mut Option<ICoreWebView2ContextMenuItemCollection>,
	) -> HRESULT;
	pub fn get_ContextMenuTarget(
		&self,
		target: *mut Option<ICoreWebView2ContextMenuTarget>,
	) -> HRESULT;
	pub fn get_Location(&self) -> HRESULT;
	pub fn put_SelectedCommandId(&self, command_id: i32) -> HRESULT;
	pub fn get_SelectedCommandId(&self) -> HRESULT;
	pub fn put_Handled(&self, handled: BOOL) -> HRESULT;
	pub fn get_Handled(&self) -> HRESULT;
	pub fn GetDeferral(&self, deferral: *mut Option<ICoreWebView2Deferral>) -> HRESULT;
}

#[interface("b8611d99-eed6-4f3f-902c-a198502ad472")]
pub unsafe trait ICoreWebView2ContextMenuTarget: IUnknown {
	pub fn get_Kind(&self) -> HRESULT;
	pub fn get_IsEditable(&self) -> HRESULT;
	pub fn get_IsRequestedForMainFrame(&self) -> HRESULT;
	pub fn get_PageUri(&self) -> HRESULT;
	pub fn get_FrameUri(&self) -> HRESULT;
	pub fn get_HasLinkUri(&self, has_link_uri: *mut BOOL) -> HRESULT;
	/// Allocated with `CoTaskMemAlloc`.
	pub fn get_LinkUri(&self, uri: *mut *mut u16) -> HRESULT;
	pub fn get_HasLinkText(&self) -> HRESULT;
	pub fn get_LinkText(&self) -> HRESULT;
	pub fn get_HasSourceUri(&self) -> HRESULT;
	pub fn get_SourceUri(&self) -> HRESULT;
	pub fn get_HasSelection(&self, has_selection: *mut BOOL) -> HRESULT;
	/// Allocated with `CoTaskMemAlloc`.
	pub fn get_SelectionText(&self, text: *mut *mut u16) -> HRESULT;
}

#[interface("c10e7f7b-b585-46f0-a623-8befbf3e4ee0")]
pub unsafe trait ICoreWebView2Deferral: IUnknown {
	pub fn Complete(&self) -> HRESULT;
}

#[interface("f562a2f5-c415-45cf-b909-d4b7c1e276d3")]
pub unsafe trait ICoreWebView2ContextMenuItemCollection: IUnknown {
	pub fn get_Count(&self, count: *mut u32) -> HRESULT;
	pub fn GetValueAtIndex(
		&self,
		index: u32,
		item: *mut Option<ICoreWebView2ContextMenuItem>,
	) -> HRESULT;
}

#[interface("7aed49e3-a93f-497a-811c-749c6b6b6c65")]
pub unsafe trait ICoreWebView2ContextMenuItem: IUnknown {
	/// Allocated with `CoTaskMemAlloc`.
	pub fn get_Name(&self, name: *mut *mut u16) -> HRESULT;
	pub fn get_Label(&self) -> HRESULT;
	pub fn get_CommandId(&self, command_id: *mut i32) -> HRESULT;
	pub fn get_ShortcutKeyDescription(&self) -> HRESULT;
	pub fn get_Icon(&self) -> HRESULT;
	pub fn get_Kind(&self) -> HRESULT;
	pub fn put_IsEnabled(&self) -> HRESULT;
	pub fn get_IsEnabled(&self, is_enabled: *mut BOOL) -> HRESULT;
}

// Handlers implemented by the plugin. Interface arguments are raw borrowed pointers.

#[interface("4e8a3389-c9d8-4bd2-b6b5-124fee6cc14d")]
pub unsafe trait ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler:
	IUnknown
{
	pub fn Invoke(&self, result: HRESULT, environment: *mut c_void) -> HRESULT;
}

#[interface("6c4819f3-c9b7-4260-8127-c9f5bde7f68c")]
pub unsafe trait ICoreWebView2CreateCoreWebView2ControllerCompletedHandler:
	IUnknown
{
	pub fn Invoke(&self, result: HRESULT, controller: *mut c_void) -> HRESULT;
}

#[interface("9adbe429-f36d-432b-9ddc-f8881fbd76e3")]
pub unsafe trait ICoreWebView2NavigationStartingEventHandler: IUnknown {
	pub fn Invoke(&self, sender: *mut c_void, arguments: *mut c_void) -> HRESULT;
}

#[interface("d33a35bf-1c49-4f98-93ab-006e0533fe1c")]
pub unsafe trait ICoreWebView2NavigationCompletedEventHandler: IUnknown {
	pub fn Invoke(&self, sender: *mut c_void, arguments: *mut c_void) -> HRESULT;
}

#[interface("d4c185fe-c81c-4989-97af-2d3fa7ab5651")]
pub unsafe trait ICoreWebView2NewWindowRequestedEventHandler: IUnknown {
	pub fn Invoke(&self, sender: *mut c_void, arguments: *mut c_void) -> HRESULT;
}

#[interface("49511172-cc67-4bca-9923-137112f4c4cc")]
pub unsafe trait ICoreWebView2ExecuteScriptCompletedHandler: IUnknown {
	pub fn Invoke(&self, result: HRESULT, result_as_json: *const u16) -> HRESULT;
}

#[interface("04d3fe1d-ab87-42fb-a898-da241d35b63c")]
pub unsafe trait ICoreWebView2ContextMenuRequestedEventHandler: IUnknown {
	pub fn Invoke(&self, sender: *mut c_void, arguments: *mut c_void) -> HRESULT;
}

#[interface("a5e3b0d0-10df-4156-bfad-3b43867acac6")]
pub unsafe trait ICoreWebView2StatusBarTextChangedEventHandler: IUnknown {
	pub fn Invoke(&self, sender: *mut c_void, arguments: *mut c_void) -> HRESULT;
}
