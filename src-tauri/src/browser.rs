//! What can be read from the window in front, through UI Automation, the
//! interface screen readers use.
//!
//! The address of the page, for auto-type (features.md §3.10) and quick
//! save: when the window belongs to a browser, its address bar is read.
//! Chromium browsers (Edge, Chrome, Brave, Vivaldi) expose it as
//! `OmniboxViewViews`, Firefox as `urlbar-input`. The address is matched
//! against entry URLs and never logged.
//!
//! The user name already typed into a login form, for quick save (§3.12).
//! Only ordinary text fields are read: a password field gives out nothing
//! but dots (browsers and Windows make sure of that), and is never asked.

use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::System::Variant::{VARIANT, VT_BSTR, VT_I4, VariantClear};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationValuePattern,
    TreeScope_Descendants, UIA_AutomationIdPropertyId, UIA_ClassNamePropertyId,
    UIA_ControlTypePropertyId, UIA_EditControlTypeId, UIA_ValuePatternId,
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

/// A `VT_I4` VARIANT, for a control type condition. Owns nothing.
fn i4_variant(value: i32) -> VARIANT {
    let mut v = VARIANT::default();
    // SAFETY: a zeroed VARIANT set to VT_I4 with its integer
    unsafe {
        let inner = &mut *v.Anonymous.Anonymous;
        inner.vt = VT_I4;
        inner.Anonymous.lVal = value;
    }
    v
}

/// Runs `f` with UI Automation on a thread of its own (COM in its own
/// apartment), giving up after `timeout`: a hung program must not hold
/// anything up.
fn with_uia<T: Send + 'static>(
    timeout: Duration,
    f: impl FnOnce(&IUIAutomation) -> Option<T> + Send + 'static,
) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("ui-automation".into())
        .spawn(move || {
            // SAFETY: COM is initialised for this thread and uninitialised at
            // the end; every interface is released before that
            unsafe {
                if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                    let _ = tx.send(None);
                    return;
                }
                let out = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                    .ok()
                    .and_then(|uia: IUIAutomation| f(&uia));
                let _ = tx.send(out);
                CoUninitialize();
            }
        })
        .ok()?;
    rx.recv_timeout(timeout).ok().flatten()
}

/// How a text field looks to quick save.
enum Field {
    /// the address bar, a password field, or not a text field at all
    Other,
    Password,
    /// an ordinary text field and what is in it (possibly nothing)
    Text(String),
}

/// What `e` is. Only a text field's value is read, never a password's.
fn field_of(e: &IUIAutomationElement) -> Field {
    // SAFETY: plain property queries on a live element
    unsafe {
        if e.CurrentControlType().ok() != Some(UIA_EditControlTypeId) {
            return Field::Other;
        }
        if e.CurrentIsPassword().is_ok_and(|p| p.as_bool()) {
            return Field::Password;
        }
        let class = e
            .CurrentClassName()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let id = e
            .CurrentAutomationId()
            .map(|s| s.to_string())
            .unwrap_or_default();
        if class == "OmniboxViewViews" || id == "urlbar-input" {
            return Field::Other;
        }
        let value = e
            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            .and_then(|v| v.CurrentValue())
            .map(|v| v.to_string())
            .unwrap_or_default();
        Field::Text(value)
    }
}

/// Longest text taken for a user name: anything longer is not one.
const MAX_USERNAME: usize = 128;

/// A plausible user name: one short line.
fn username_from(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty() && text.chars().count() <= MAX_USERNAME && !text.contains(['\n', '\r']))
        .then(|| text.to_string())
}

/// The text in the field that has the keyboard focus, when it is an
/// ordinary one-line text field in another program (not Mikoshi, not an
/// address bar, not a password field). Call it while that program is in
/// front, before the quick search box takes the focus.
pub fn focused_username() -> Option<String> {
    with_uia(TIMEOUT, |uia| {
        // SAFETY: plain queries on a live element
        let e = unsafe { uia.GetFocusedElement().ok()? };
        let pid = unsafe { e.CurrentProcessId().ok()? };
        if pid as u32 == unsafe { GetCurrentProcessId() } {
            return None;
        }
        match field_of(&e) {
            Field::Text(t) => username_from(&t),
            _ => None,
        }
    })
}

/// The user name on a login form in `target`: the text field just before
/// the first password field, if something is typed in it. A browser builds
/// the page's accessibility tree only when first asked, so an empty first
/// look is tried once more.
pub fn form_username(target: isize) -> Option<String> {
    with_uia(TIMEOUT, move |uia| {
        for attempt in 0..2 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(300));
            }
            // SAFETY: plain queries; the condition's VARIANT is plain data
            let found = unsafe {
                let window = uia.ElementFromHandle(HWND(target as *mut _)).ok()?;
                let edits = uia
                    .CreatePropertyCondition(
                        UIA_ControlTypePropertyId,
                        &i4_variant(UIA_EditControlTypeId.0),
                    )
                    .ok()?;
                let all = window.FindAll(TreeScope_Descendants, &edits).ok()?;
                let mut last: Option<String> = None;
                let mut found = None;
                for i in 0..all.Length().unwrap_or(0) {
                    let Ok(e) = all.GetElement(i) else { continue };
                    match field_of(&e) {
                        Field::Password => {
                            found = Some(last.take());
                            break;
                        }
                        Field::Text(t) => last = Some(t),
                        Field::Other => {}
                    }
                }
                found
            };
            // a password field was found: whatever came before it decides
            if let Some(before) = found {
                return before.and_then(|t| username_from(&t));
            }
        }
        None
    })
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
