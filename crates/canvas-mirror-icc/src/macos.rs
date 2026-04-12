use std::{ffi::c_void, ptr::NonNull};

use objc2::{msg_send, runtime::AnyObject, MainThreadMarker};
use raw_window_handle::{AppKitWindowHandle, RawWindowHandle};

use crate::{icc_profile_display_name, DisplayIccProfile};

type CGDirectDisplayID = u32;
type CGColorSpaceRef = *mut c_void;
type CFDataRef = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataGetBytePtr(the_data: CFDataRef) -> *const u8;
    fn CFDataGetLength(the_data: CFDataRef) -> isize;
    fn CFRelease(cf: *const c_void);
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGColorSpaceCopyICCData(space: CGColorSpaceRef) -> CFDataRef;
    fn CGDisplayCopyColorSpace(display: CGDirectDisplayID) -> CGColorSpaceRef;
    fn CGGetActiveDisplayList(
        max_displays: u32,
        active_displays: *mut CGDirectDisplayID,
        display_count: *mut u32,
    ) -> i32;
    fn CGMainDisplayID() -> CGDirectDisplayID;
}

pub(super) fn list_display_icc_profiles() -> Option<Vec<DisplayIccProfile>> {
    let mut display_count = 0;
    let status = unsafe { CGGetActiveDisplayList(0, std::ptr::null_mut(), &mut display_count) };
    if status != 0 {
        return None;
    }

    let mut displays = vec![0; usize::try_from(display_count).ok()?];
    let status =
        unsafe { CGGetActiveDisplayList(display_count, displays.as_mut_ptr(), &mut display_count) };
    if status != 0 {
        return None;
    }

    displays.truncate(usize::try_from(display_count).ok()?);
    let main_display_id = unsafe { CGMainDisplayID() };

    Some(
        displays
            .into_iter()
            .filter_map(|display_id| {
                let icc_profile = icc_profile_for_display(display_id)?;
                let display_name = icc_profile_display_name(&icc_profile).unwrap_or_else(|| {
                    if display_id == main_display_id {
                        format!("Main Display ({display_id})")
                    } else {
                        format!("Display {display_id}")
                    }
                });
                Some(DisplayIccProfile {
                    display_id: display_id.to_string(),
                    display_name,
                    is_primary: display_id == main_display_id,
                    icc_profile,
                })
            })
            .collect(),
    )
}

fn icc_profile_for_display(display_id: CGDirectDisplayID) -> Option<Vec<u8>> {
    let color_space = NonNull::new(unsafe { CGDisplayCopyColorSpace(display_id) })?;
    let icc_data = unsafe { CGColorSpaceCopyICCData(color_space.as_ptr()) };
    unsafe { CFRelease(color_space.as_ptr()) };

    let icc_data = NonNull::new(icc_data as *mut c_void)?;
    let bytes = cf_data_to_vec(icc_data.as_ptr() as CFDataRef);
    unsafe { CFRelease(icc_data.as_ptr()) };
    bytes
}

pub(super) fn window_display_icc_profile(handle: RawWindowHandle) -> Option<Vec<u8>> {
    let RawWindowHandle::AppKit(handle) = handle else {
        return None;
    };

    icc_profile_for_appkit_window(handle)
}

fn icc_profile_for_appkit_window(handle: AppKitWindowHandle) -> Option<Vec<u8>> {
    // AppKit screen/color-space queries are main-thread-only.
    let _main_thread = MainThreadMarker::new()?;
    let ns_view = unsafe { &*(handle.ns_view.as_ptr() as *mut AnyObject) };

    let ns_window: *mut AnyObject = unsafe { msg_send![ns_view, window] };
    let ns_window = NonNull::new(ns_window)?;
    let ns_screen: *mut AnyObject = unsafe { msg_send![ns_window.as_ptr(), screen] };
    let ns_screen = NonNull::new(ns_screen)?;
    let color_space: *mut AnyObject = unsafe { msg_send![ns_screen.as_ptr(), colorSpace] };
    let color_space = NonNull::new(color_space)?;
    let icc_data: *mut AnyObject = unsafe { msg_send![color_space.as_ptr(), ICCProfileData] };
    let icc_data = NonNull::new(icc_data)?;

    ns_data_to_vec(icc_data.as_ptr())
}

fn cf_data_to_vec(data: CFDataRef) -> Option<Vec<u8>> {
    let length = unsafe { CFDataGetLength(data) };
    if length <= 0 {
        return None;
    }

    let length = usize::try_from(length).ok()?;
    let bytes = unsafe { CFDataGetBytePtr(data) };
    let bytes = NonNull::new(bytes as *mut u8)?;

    Some(unsafe { std::slice::from_raw_parts(bytes.as_ptr(), length) }.to_vec())
}

fn ns_data_to_vec(data: *mut AnyObject) -> Option<Vec<u8>> {
    let length: usize = unsafe { msg_send![data, length] };
    if length == 0 {
        return None;
    }

    let bytes: *const u8 = unsafe { msg_send![data, bytes] };
    let bytes = NonNull::new(bytes as *mut u8)?;

    Some(unsafe { std::slice::from_raw_parts(bytes.as_ptr(), length) }.to_vec())
}
