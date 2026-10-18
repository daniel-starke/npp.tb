//! The subset of the WinRT interfaces of `Windows.Web.UI.Interop.WebViewControl` (SDK headers
//! `windows.web.ui.interop.h`, `windows.web.ui.h` and `windows.foundation.h`) used by
//! [`super::EdgeView`].
//!
//! The `windows-core` interface macro cannot derive from `IInspectable`, so every interface derives
//! from `IUnknown` and starts with the three `IInspectable` slots. Interfaces that are only called
//! declare their methods up to the last one used, with placeholder signatures for methods that are
//! never called. The IIDs of parameterized interfaces are derived from their WinRT signature, given
//! in their documentation, and match the SDK headers. Delegates derive from `IUnknown` in WinRT.

#![allow(non_snake_case, non_camel_case_types, clippy::upper_case_acronyms)]

use core::ffi::c_void;

use windows_core::{GUID, HRESULT, HSTRING, IUnknown, interface};

/// WinRT `boolean`.
pub type BOOLEAN = u8;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct EventRegistrationToken {
	pub value: i64,
}

/// `Windows.Foundation.Rect`
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
	pub x: f32,
	pub y: f32,
	pub width: f32,
	pub height: f32,
}

/// `Windows.Web.UI.Interop.WebViewControlMoveFocusReason.Programmatic`
pub const MOVE_FOCUS_REASON_PROGRAMMATIC: i32 = 0;

/// `Windows.Foundation.AsyncStatus.Completed`
pub const ASYNC_STATUS_COMPLETED: i32 = 1;

#[interface("02c723ec-98d6-424a-b63e-c6136c36a0f2")]
pub unsafe trait IWebViewControlProcess: IUnknown {
	pub fn GetIids(&self) -> HRESULT;
	pub fn GetRuntimeClassName(&self) -> HRESULT;
	pub fn GetTrustLevel(&self) -> HRESULT;
	pub fn get_ProcessId(&self) -> HRESULT;
	pub fn get_EnterpriseId(&self) -> HRESULT;
	pub fn get_IsPrivateNetworkClientServerCapabilityEnabled(&self) -> HRESULT;
	pub fn CreateWebViewControlAsync(
		&self,
		host_window_handle: i64,
		bounds: Rect,
		operation: *mut Option<IAsyncOperation_WebViewControl>,
	) -> HRESULT;
	pub fn GetWebViewControls(&self) -> HRESULT;
	pub fn Terminate(&self) -> HRESULT;
}

/// `IAsyncOperation<Windows.Web.UI.Interop.WebViewControl>`, signature
/// `pinterface({9fc2b0bb-e446-44e2-aa61-9cab8f636af2};rc(Windows.Web.UI.Interop.WebViewControl;{3f921316-bc70-4bda-9136-c94370899fab}))`.
#[interface("ac3d28ac-8362-51c6-b2cc-16f3672758f1")]
pub unsafe trait IAsyncOperation_WebViewControl: IUnknown {
	pub fn GetIids(&self) -> HRESULT;
	pub fn GetRuntimeClassName(&self) -> HRESULT;
	pub fn GetTrustLevel(&self) -> HRESULT;
	/// `handler`: [`IAsyncOperationCompletedHandler_WebViewControl`].
	pub fn put_Completed(&self, handler: *mut c_void) -> HRESULT;
	pub fn get_Completed(&self) -> HRESULT;
	pub fn GetResults(&self, results: *mut Option<IWebViewControl>) -> HRESULT;
}

/// `AsyncOperationCompletedHandler<Windows.Web.UI.Interop.WebViewControl>`, signature
/// `pinterface({fcdcf02c-e5d8-4478-915a-4d90b74b83a5};rc(Windows.Web.UI.Interop.WebViewControl;{3f921316-bc70-4bda-9136-c94370899fab}))`.
#[interface("d61963d6-806d-50a8-a81c-75d9356ad5d7")]
pub unsafe trait IAsyncOperationCompletedHandler_WebViewControl: IUnknown {
	/// `operation`: [`IAsyncOperation_WebViewControl`].
	pub fn Invoke(&self, operation: *mut c_void, status: i32) -> HRESULT;
}

/// `Windows.Web.UI.IWebViewControl`, the default interface of `WebViewControl`.
#[interface("3f921316-bc70-4bda-9136-c94370899fab")]
pub unsafe trait IWebViewControl: IUnknown {
	pub fn GetIids(&self) -> HRESULT;
	pub fn GetRuntimeClassName(&self) -> HRESULT;
	pub fn GetTrustLevel(&self) -> HRESULT;
	pub fn get_Source(&self) -> HRESULT;
	pub fn put_Source(&self) -> HRESULT;
	pub fn get_DocumentTitle(&self) -> HRESULT;
	pub fn get_CanGoBack(&self) -> HRESULT;
	pub fn get_CanGoForward(&self) -> HRESULT;
	/// `color`: `Windows.UI.Color`, laid out like [`crate::webview2::Color`].
	pub fn put_DefaultBackgroundColor(&self, color: crate::webview2::Color) -> HRESULT;
	pub fn get_DefaultBackgroundColor(&self) -> HRESULT;
	pub fn get_ContainsFullScreenElement(&self) -> HRESULT;
	pub fn get_Settings(&self, settings: *mut Option<IWebViewControlSettings>) -> HRESULT;
	pub fn get_DeferredPermissionRequests(&self) -> HRESULT;
	pub fn GoForward(&self) -> HRESULT;
	pub fn GoBack(&self) -> HRESULT;
	pub fn Refresh(&self) -> HRESULT;
	pub fn Stop(&self) -> HRESULT;
	pub fn Navigate(&self) -> HRESULT;
	/// `text`: a borrowed `HSTRING`.
	pub fn NavigateToString(&self, text: *mut c_void) -> HRESULT;
	pub fn NavigateToLocalStreamUri(&self) -> HRESULT;
	pub fn NavigateWithHttpRequestMessage(&self) -> HRESULT;
	/// `script_name`: a borrowed `HSTRING`, `arguments`: [`IIterable_HSTRING`], `operation`:
	/// `IAsyncOperation<HSTRING>`.
	pub fn InvokeScriptAsync(
		&self,
		script_name: *mut c_void,
		arguments: *mut c_void,
		operation: *mut Option<IUnknown>,
	) -> HRESULT;
	pub fn CapturePreviewToStreamAsync(&self) -> HRESULT;
	pub fn CaptureSelectedContentToDataPackageAsync(&self) -> HRESULT;
	pub fn BuildLocalStreamUri(&self) -> HRESULT;
	pub fn GetDeferredPermissionRequestById(&self) -> HRESULT;
	/// `handler`: [`ITypedEventHandler_NavigationStarting`].
	pub fn add_NavigationStarting(
		&self,
		handler: *mut c_void,
		token: *mut EventRegistrationToken,
	) -> HRESULT;
	pub fn remove_NavigationStarting(&self) -> HRESULT;
	pub fn add_ContentLoading(&self) -> HRESULT;
	pub fn remove_ContentLoading(&self) -> HRESULT;
	pub fn add_DOMContentLoaded(&self) -> HRESULT;
	pub fn remove_DOMContentLoaded(&self) -> HRESULT;
	/// `handler`: [`ITypedEventHandler_NavigationCompleted`].
	pub fn add_NavigationCompleted(
		&self,
		handler: *mut c_void,
		token: *mut EventRegistrationToken,
	) -> HRESULT;
	pub fn remove_NavigationCompleted(&self) -> HRESULT;
	pub fn add_FrameNavigationStarting(&self) -> HRESULT;
	pub fn remove_FrameNavigationStarting(&self) -> HRESULT;
	pub fn add_FrameContentLoading(&self) -> HRESULT;
	pub fn remove_FrameContentLoading(&self) -> HRESULT;
	pub fn add_FrameDOMContentLoaded(&self) -> HRESULT;
	pub fn remove_FrameDOMContentLoaded(&self) -> HRESULT;
	pub fn add_FrameNavigationCompleted(&self) -> HRESULT;
	pub fn remove_FrameNavigationCompleted(&self) -> HRESULT;
	/// `handler`: [`ITypedEventHandler_ScriptNotify`].
	pub fn add_ScriptNotify(
		&self,
		handler: *mut c_void,
		token: *mut EventRegistrationToken,
	) -> HRESULT;
	pub fn remove_ScriptNotify(&self) -> HRESULT;
	pub fn add_LongRunningScriptDetected(&self) -> HRESULT;
	pub fn remove_LongRunningScriptDetected(&self) -> HRESULT;
	pub fn add_UnsafeContentWarningDisplaying(&self) -> HRESULT;
	pub fn remove_UnsafeContentWarningDisplaying(&self) -> HRESULT;
	pub fn add_UnviewableContentIdentified(&self) -> HRESULT;
	pub fn remove_UnviewableContentIdentified(&self) -> HRESULT;
	pub fn add_PermissionRequested(&self) -> HRESULT;
	pub fn remove_PermissionRequested(&self) -> HRESULT;
	pub fn add_UnsupportedUriSchemeIdentified(&self) -> HRESULT;
	pub fn remove_UnsupportedUriSchemeIdentified(&self) -> HRESULT;
	/// `handler`: [`ITypedEventHandler_NewWindowRequested`].
	pub fn add_NewWindowRequested(
		&self,
		handler: *mut c_void,
		token: *mut EventRegistrationToken,
	) -> HRESULT;
}

#[interface("133f47c6-12dc-4898-bd47-04967de648ba")]
pub unsafe trait IWebViewControlSite: IUnknown {
	pub fn GetIids(&self) -> HRESULT;
	pub fn GetRuntimeClassName(&self) -> HRESULT;
	pub fn GetTrustLevel(&self) -> HRESULT;
	pub fn get_Process(&self) -> HRESULT;
	pub fn put_Scale(&self) -> HRESULT;
	pub fn get_Scale(&self) -> HRESULT;
	pub fn put_Bounds(&self, bounds: Rect) -> HRESULT;
	pub fn get_Bounds(&self) -> HRESULT;
	pub fn put_IsVisible(&self, is_visible: BOOLEAN) -> HRESULT;
	pub fn get_IsVisible(&self) -> HRESULT;
	pub fn Close(&self) -> HRESULT;
	pub fn MoveFocus(&self, reason: i32) -> HRESULT;
}

#[interface("c9967fbf-5e98-4cfd-8cce-27b0911e3de8")]
pub unsafe trait IWebViewControlSettings: IUnknown {
	pub fn GetIids(&self) -> HRESULT;
	pub fn GetRuntimeClassName(&self) -> HRESULT;
	pub fn GetTrustLevel(&self) -> HRESULT;
	pub fn put_IsJavaScriptEnabled(&self, is_enabled: BOOLEAN) -> HRESULT;
	pub fn get_IsJavaScriptEnabled(&self) -> HRESULT;
	pub fn put_IsIndexedDBEnabled(&self, is_enabled: BOOLEAN) -> HRESULT;
	pub fn get_IsIndexedDBEnabled(&self) -> HRESULT;
	pub fn put_IsScriptNotifyAllowed(&self, is_allowed: BOOLEAN) -> HRESULT;
}

#[interface("0c9057c5-0a08-41c7-863b-71e3a9549137")]
pub unsafe trait IWebViewControlNavigationStartingEventArgs: IUnknown {
	pub fn GetIids(&self) -> HRESULT;
	pub fn GetRuntimeClassName(&self) -> HRESULT;
	pub fn GetTrustLevel(&self) -> HRESULT;
	pub fn get_Uri(&self, uri: *mut Option<IUriRuntimeClass>) -> HRESULT;
	pub fn get_Cancel(&self) -> HRESULT;
	pub fn put_Cancel(&self, cancel: BOOLEAN) -> HRESULT;
}

#[interface("3df44bbb-a124-46d5-a083-d02cacdff5ad")]
pub unsafe trait IWebViewControlNewWindowRequestedEventArgs: IUnknown {
	pub fn GetIids(&self) -> HRESULT;
	pub fn GetRuntimeClassName(&self) -> HRESULT;
	pub fn GetTrustLevel(&self) -> HRESULT;
	pub fn get_Uri(&self, uri: *mut Option<IUriRuntimeClass>) -> HRESULT;
	pub fn get_Referrer(&self) -> HRESULT;
	pub fn get_Handled(&self) -> HRESULT;
	pub fn put_Handled(&self, handled: BOOLEAN) -> HRESULT;
}

#[interface("491de57b-6f49-41bb-b591-51b85b817037")]
pub unsafe trait IWebViewControlScriptNotifyEventArgs: IUnknown {
	pub fn GetIids(&self) -> HRESULT;
	pub fn GetRuntimeClassName(&self) -> HRESULT;
	pub fn GetTrustLevel(&self) -> HRESULT;
	pub fn get_Uri(&self) -> HRESULT;
	pub fn get_Value(&self, value: *mut HSTRING) -> HRESULT;
}

/// `Windows.Foundation.IUriRuntimeClass`, the default interface of `Windows.Foundation.Uri`.
#[interface("9e365e57-48b2-4160-956f-c7385120bbfc")]
pub unsafe trait IUriRuntimeClass: IUnknown {
	pub fn GetIids(&self) -> HRESULT;
	pub fn GetRuntimeClassName(&self) -> HRESULT;
	pub fn GetTrustLevel(&self) -> HRESULT;
	pub fn get_AbsoluteUri(&self, uri: *mut HSTRING) -> HRESULT;
}

/// `TypedEventHandler<Windows.Web.UI.IWebViewControl, Windows.Web.UI.WebViewControlNavigationStartingEventArgs>`,
/// signature `pinterface({9de1c534-6ae1-11e0-84e1-18a905bcc53f};{3f921316-bc70-4bda-9136-c94370899fab};rc(Windows.Web.UI.WebViewControlNavigationStartingEventArgs;{0c9057c5-0a08-41c7-863b-71e3a9549137}))`.
#[interface("e92e0bcc-9ae9-5b9b-a684-83dd8ee57775")]
pub unsafe trait ITypedEventHandler_NavigationStarting: IUnknown {
	/// `arguments`: [`IWebViewControlNavigationStartingEventArgs`].
	pub fn Invoke(&self, sender: *mut c_void, arguments: *mut c_void) -> HRESULT;
}

/// `TypedEventHandler<Windows.Web.UI.IWebViewControl, Windows.Web.UI.WebViewControlNavigationCompletedEventArgs>`,
/// signature `pinterface({9de1c534-6ae1-11e0-84e1-18a905bcc53f};{3f921316-bc70-4bda-9136-c94370899fab};rc(Windows.Web.UI.WebViewControlNavigationCompletedEventArgs;{20409918-4a15-4c46-a55d-f79edb0bde8b}))`.
#[interface("57a87c53-47a5-5864-9881-fd4c00f230a9")]
pub unsafe trait ITypedEventHandler_NavigationCompleted: IUnknown {
	pub fn Invoke(&self, sender: *mut c_void, arguments: *mut c_void) -> HRESULT;
}

/// `TypedEventHandler<Windows.Web.UI.IWebViewControl, Windows.Web.UI.WebViewControlNewWindowRequestedEventArgs>`,
/// signature `pinterface({9de1c534-6ae1-11e0-84e1-18a905bcc53f};{3f921316-bc70-4bda-9136-c94370899fab};rc(Windows.Web.UI.WebViewControlNewWindowRequestedEventArgs;{3df44bbb-a124-46d5-a083-d02cacdff5ad}))`.
#[interface("317111df-10c6-559c-85a1-847eb0a1b2d5")]
pub unsafe trait ITypedEventHandler_NewWindowRequested: IUnknown {
	/// `arguments`: [`IWebViewControlNewWindowRequestedEventArgs`].
	pub fn Invoke(&self, sender: *mut c_void, arguments: *mut c_void) -> HRESULT;
}

/// `TypedEventHandler<Windows.Web.UI.IWebViewControl, Windows.Web.UI.WebViewControlScriptNotifyEventArgs>`,
/// signature `pinterface({9de1c534-6ae1-11e0-84e1-18a905bcc53f};{3f921316-bc70-4bda-9136-c94370899fab};rc(Windows.Web.UI.WebViewControlScriptNotifyEventArgs;{491de57b-6f49-41bb-b591-51b85b817037}))`.
#[interface("ee8b81d3-bbc2-55b0-877b-6ba86e3ad899")]
pub unsafe trait ITypedEventHandler_ScriptNotify: IUnknown {
	/// `arguments`: [`IWebViewControlScriptNotifyEventArgs`].
	pub fn Invoke(&self, sender: *mut c_void, arguments: *mut c_void) -> HRESULT;
}

/// `Windows.Foundation.Collections.IIterable<String>`, signature
/// `pinterface({faa585ea-6214-4217-afda-7f46de5869b3};string)`.
#[interface("e2fcc7c1-3bfc-5a0b-b2b0-72e769d1cb7e")]
pub unsafe trait IIterable_HSTRING: IUnknown {
	pub fn GetIids(&self, count: *mut u32, iids: *mut *mut GUID) -> HRESULT;
	pub fn GetRuntimeClassName(&self, name: *mut HSTRING) -> HRESULT;
	pub fn GetTrustLevel(&self, level: *mut i32) -> HRESULT;
	/// `iterator`: [`IIterator_HSTRING`].
	pub fn First(&self, iterator: *mut *mut c_void) -> HRESULT;
}

/// `Windows.Foundation.Collections.IIterator<String>`, signature
/// `pinterface({6a79e863-4300-459a-9966-cbb660963ee1};string)`.
#[interface("8c304ebb-6615-50a4-8829-879ecd443236")]
pub unsafe trait IIterator_HSTRING: IUnknown {
	pub fn GetIids(&self, count: *mut u32, iids: *mut *mut GUID) -> HRESULT;
	pub fn GetRuntimeClassName(&self, name: *mut HSTRING) -> HRESULT;
	pub fn GetTrustLevel(&self, level: *mut i32) -> HRESULT;
	pub fn get_Current(&self, current: *mut HSTRING) -> HRESULT;
	pub fn get_HasCurrent(&self, has_current: *mut BOOLEAN) -> HRESULT;
	pub fn MoveNext(&self, has_current: *mut BOOLEAN) -> HRESULT;
	pub fn GetMany(&self, capacity: u32, items: *mut HSTRING, count: *mut u32) -> HRESULT;
}
