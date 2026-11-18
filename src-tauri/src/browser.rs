//! The address of the page in front, for auto-type (features.md §3.10).
//!
//! When the window to type into belongs to a browser, its address bar is
//! read through UI Automation, the interface screen readers use: Chromium
//! browsers (Edge, Chrome, Brave, Vivaldi) expose it as `OmniboxViewViews`,
//! Firefox as `urlbar-input`. The address is matched against entry URLs and
//! never logged.

use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::System::Variant::{VARIANT, VT_BSTR, VariantClear};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationValuePattern, TreeScope_Descendants,
    UIA_AutomationIdPropertyId, UIA_ClassNamePropertyId, UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::core::{BSTR, PWSTR};

/// Programs whose address bar we know how to find.
const BROWSERS: &[&str] = &[
    "msedge.exe",
    "chrome.exe",
    "brave.exe",
    "vivaldi.exe",
    "firefox.exe",
];

/// How long a browser may take to answer; a hung one must not hold up
/// auto-type.
const TIMEOUT: Duration = Duration::from_millis(1500);

/// The address shown in the browser window `target`; `None` when it is not
/// a browser we know, or the address cannot be read.
pub fn page_url(target: isize) -> Option<String> {
    let exe = process_file_name(target)?;
    if !BROWSERS.contains(&exe.to_ascii_lowercase().as_str()) {
        return None;
    }
    // COM on a thread of its own, in its own apartment, with a time limit
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("page-url".into())
        .spawn(move || {
            let _ = tx.send(read_address_bar(target));
        })
        .ok()?;
    let url = rx.recv_timeout(TIMEOUT).ok().flatten();
    if url.is_none() {
        log::debug!("auto-type: the address of the {exe} window could not be read");
    }
    url
}

fn process_file_name(target: isize) -> Option<String> {
    let mut pid = 0;
    // SAFETY: plain query
    unsafe { GetWindowThreadProcessId(HWND(target as *mut _), Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    // SAFETY: the handle is closed below; the buffer's length is passed
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().map(str::to_string)
    }
}

/// A `VT_BSTR` VARIANT for a property condition, freed on drop.
struct BstrVariant(VARIANT);

impl BstrVariant {
    fn new(text: &str) -> BstrVariant {
        let mut v = VARIANT::default();
        // SAFETY: a zeroed VARIANT set to VT_BSTR with an owned BSTR, which
        // VariantClear frees
        unsafe {
            let inner = &mut *v.Anonymous.Anonymous;
            inner.vt = VT_BSTR;
            inner.Anonymous.bstrVal = std::mem::ManuallyDrop::new(BSTR::from(text));
        }
        BstrVariant(v)
    }
}

impl Drop for BstrVariant {
    fn drop(&mut self) {
        // SAFETY: initialised in `new`
        unsafe {
            let _ = VariantClear(&mut self.0);
        }
    }
}

fn read_address_bar(target: isize) -> Option<String> {
    // SAFETY: COM is initialised for this thread and uninitialised at the
    // end; every interface is released before that
    unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return None;
        }
        let url = (|| -> Option<String> {
            let uia: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let window = uia.ElementFromHandle(HWND(target as *mut _)).ok()?;
            let chromium = BstrVariant::new("OmniboxViewViews");
            let firefox = BstrVariant::new("urlbar-input");
            let condition = uia
                .CreateOrCondition(
                    &uia.CreatePropertyCondition(UIA_ClassNamePropertyId, &chromium.0)
                        .ok()?,
                    &uia.CreatePropertyCondition(UIA_AutomationIdPropertyId, &firefox.0)
                        .ok()?,
                )
                .ok()?;
            let bar = window.FindFirst(TreeScope_Descendants, &condition).ok()?;
            let value: IUIAutomationValuePattern =
                bar.GetCurrentPatternAs(UIA_ValuePatternId).ok()?;
            let text = value.CurrentValue().ok()?.to_string();
            let text = text.trim().to_string();
            (!text.is_empty()).then_some(text)
        })();
        CoUninitialize();
        url
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, IsWindowVisible};
    use windows::core::BOOL;

    /// Needs Chrome open on https://example.com/ (features.md
    /// §3.10, M5 acceptance). Reads the address bars only, types nothing;
    /// prints whether each window matched, never the address.
    /// `cargo test -p mikoshi browser -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn reads_example_com_from_open_browsers() {
        unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
            // SAFETY: `data` is the Vec passed below, alive for the call
            let found = unsafe { &mut *(data.0 as *mut Vec<(isize, String)>) };
            let mut buf = [0u16; 512];
            // SAFETY: plain queries into a buffer of the given length
            let len = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
            let title = String::from_utf16_lossy(&buf[..len]);
            if unsafe { IsWindowVisible(hwnd) }.as_bool() && title.contains("Example Domain") {
                found.push((hwnd.0 as isize, title));
            }
            true.into()
        }
        let mut found: Vec<(isize, String)> = Vec::new();
        // SAFETY: the callback only touches `found`
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut found as *mut _ as isize));
        }
        assert!(!found.is_empty(), "no window titled Example Domain");
        let mut browsers = Vec::new();
        for (hwnd, title) in &found {
            let exe = super::process_file_name(*hwnd).unwrap_or_default();
            let started = std::time::Instant::now();
            let url = super::page_url(*hwnd);
            let ms = started.elapsed().as_millis();
            let url = url.unwrap_or_else(|| panic!("{exe}: address not read ({title})"));
            let same = mikoshi_core::autotype::url_matches("https://example.com/login", &url);
            let sub = mikoshi_core::autotype::url_matches("example.com", &url);
            let other = mikoshi_core::autotype::url_matches("https://example.org", &url);
            let parent = mikoshi_core::autotype::url_matches("https://www.example.com", &url);
            println!(
                "{exe}: read in {ms} ms; example.com/login {same}, bare example.com {sub}, \
                 example.org {other}, entry www.example.com {parent}"
            );
            assert!(same && sub && !other, "{exe}: wrong match");
            browsers.push(exe.to_ascii_lowercase());
        }
        assert!(
            browsers.iter().any(|b| b == "chrome.exe"),
            "chrome.exe not checked"
        );
    }
}
