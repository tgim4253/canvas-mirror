use std::{
    ffi::{c_void, OsString},
    fs, mem,
    os::windows::ffi::OsStringExt,
    ptr,
};

use raw_window_handle::RawWindowHandle;
use windows_sys::core::BOOL;
use windows_sys::Win32::{
    Foundation::{LPARAM, RECT},
    Graphics::Gdi::{
        CreateDCW, DeleteDC, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW,
        MonitorFromWindow, DISPLAY_DEVICEW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
        MONITOR_DEFAULTTONEAREST,
    },
    UI::ColorSystem::GetICMProfileW,
};

use crate::{icc_profile_display_name, DisplayIccProfile};

const DISPLAY_DRIVER: &[u16] = &[68, 73, 83, 80, 76, 65, 89, 0];
const INITIAL_PROFILE_PATH_CAPACITY: u32 = 260;
const DISPLAY_DEVICE_PRIMARY_DEVICE: u32 = 0x0000_0004;

pub(super) fn list_display_icc_profiles() -> Option<Vec<DisplayIccProfile>> {
    let mut monitors = Vec::<HMONITOR>::new();
    let ok = unsafe {
        EnumDisplayMonitors(
            ptr::null_mut(),
            ptr::null(),
            Some(collect_monitor),
            &mut monitors as *mut Vec<HMONITOR> as LPARAM,
        )
    };
    if ok == 0 {
        return None;
    }

    Some(
        monitors
            .into_iter()
            .filter_map(display_profile_for_monitor)
            .collect(),
    )
}

pub(super) fn window_display_icc_profile(handle: RawWindowHandle) -> Option<Vec<u8>> {
    let RawWindowHandle::Win32(handle) = handle else {
        return None;
    };

    let hwnd = handle.hwnd.get() as *mut c_void;
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    display_profile_for_monitor(monitor).map(|profile| profile.icc_profile)
}

fn display_profile_for_monitor(monitor: HMONITOR) -> Option<DisplayIccProfile> {
    if monitor.is_null() {
        return None;
    }

    let mut monitor_info = MONITORINFOEXW::default();
    monitor_info.monitorInfo.cbSize = u32::try_from(mem::size_of::<MONITORINFOEXW>()).ok()?;

    let ok = unsafe {
        GetMonitorInfoW(
            monitor,
            &mut monitor_info as *mut MONITORINFOEXW as *mut MONITORINFO,
        )
    };
    if ok == 0 {
        return None;
    }

    let display_id = wide_array_to_string(&monitor_info.szDevice);
    let (fallback_display_name, is_primary) = display_name_for_device(&monitor_info.szDevice)
        .map(|(name, is_primary)| (name, is_primary))
        .unwrap_or_else(|| (display_id.clone(), false));

    let hdc = unsafe {
        CreateDCW(
            DISPLAY_DRIVER.as_ptr(),
            monitor_info.szDevice.as_ptr(),
            ptr::null(),
            ptr::null(),
        )
    };
    if hdc.is_null() {
        return None;
    }

    let profile = icc_profile_for_dc(hdc);
    unsafe { DeleteDC(hdc) };
    let icc_profile = profile?;
    let display_name = icc_profile_display_name(&icc_profile).unwrap_or(fallback_display_name);

    Some(DisplayIccProfile {
        display_id,
        display_name,
        is_primary,
        icc_profile,
    })
}

fn icc_profile_for_dc(hdc: HDC) -> Option<Vec<u8>> {
    let mut required = INITIAL_PROFILE_PATH_CAPACITY;

    loop {
        let mut buffer = vec![0_u16; usize::try_from(required).ok()?];
        let mut actual = required;
        let ok = unsafe { GetICMProfileW(hdc, &mut actual, buffer.as_mut_ptr()) };
        if ok != 0 {
            let length = buffer
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(buffer.len());
            let path = OsString::from_wide(&buffer[..length]);
            return fs::read(path).ok();
        }

        let next = usize::try_from(actual).ok()?;
        if next == 0 || next <= buffer.len() {
            return None;
        }
        required = actual;
    }
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let monitors = unsafe { &mut *(data as *mut Vec<HMONITOR>) };
    monitors.push(monitor);
    1
}

fn display_name_for_device(device_name: &[u16; 32]) -> Option<(String, bool)> {
    let mut device = DISPLAY_DEVICEW::default();
    device.cb = u32::try_from(mem::size_of::<DISPLAY_DEVICEW>()).ok()?;

    let ok = unsafe { EnumDisplayDevicesW(device_name.as_ptr(), 0, &mut device, 0) };
    if ok == 0 {
        return None;
    }

    let display_name = wide_array_to_string(&device.DeviceString);
    let is_primary = (device.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE) != 0;
    Some((display_name, is_primary))
}

fn wide_array_to_string<const N: usize>(input: &[u16; N]) -> String {
    let length = input
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(input.len());
    OsString::from_wide(&input[..length])
        .to_string_lossy()
        .into_owned()
}
