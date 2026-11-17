//! The Windows clipboard (features.md §3.5).
//!
//! - Secrets (passwords, protected fields, TOTP codes) are put on the
//!   clipboard with the formats that keep them out of the Win+V history and
//!   the cloud clipboard.
//! - A copied secret is cleared after the configured time, on lock and on
//!   exit, but **only if the clipboard still holds it**: the clipboard
//!   sequence number, which Windows bumps on every change, must still be the
//!   one we got right after copying. Whatever the user copied since stays.

use std::sync::Mutex;
use std::time::Duration;

use mikoshi_core::Secret;
use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::core::{PCWSTR, w};
use zeroize::Zeroizing;

use crate::AppState;
use crate::error::{AppError, AppResult};

/// Formats that tell Windows to keep the content out of clipboard history
/// and cloud sync. The first takes any data, the other two a DWORD 0.
const PRIVATE_FORMATS: [PCWSTR; 3] = [
    w!("ExcludeClipboardContentFromMonitorProcessing"),
    w!("CanIncludeInClipboardHistory"),
    w!("CanUploadToCloudClipboard"),
];

/// What we last put on the clipboard.
#[derive(Clone, Copy)]
struct Copied {
    sequence: u32,
    secret: bool,
}

#[derive(Default)]
pub struct Tracker {
    last: Mutex<Option<Copied>>,
}

/// The clipboard is open while this lives.
struct Open;

impl Open {
    /// Another program may hold the clipboard for a moment: retry briefly.
    fn new(owner: Option<HWND>) -> Option<Open> {
        for _ in 0..10 {
            // SAFETY: plain Win32 call; closed again by Drop
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Some(Open);
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        None
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: we opened it
        let _ = unsafe { CloseClipboard() };
    }
}

/// Copies `bytes` into global memory and hands it to the clipboard, which
/// then owns it. The clipboard must be open.
fn put(format: u32, bytes: &[u8]) -> bool {
    // SAFETY: standard SetClipboardData protocol; on failure we free the
    // memory ourselves, on success the system owns it.
    unsafe {
        let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) else {
            return false;
        };
        let p = GlobalLock(mem).cast::<u8>();
        if p.is_null() {
            let _ = GlobalFree(Some(mem));
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        // returns an "error" once the lock count reaches zero; that is fine
        let _ = GlobalUnlock(mem);
        if SetClipboardData(format, Some(HANDLE(mem.0))).is_err() {
            let _ = GlobalFree(Some(mem));
            return false;
        }
        true
    }
}

/// Puts text on the clipboard; `private` keeps it out of history and cloud
/// sync. Returns the clipboard sequence number of our content.
fn set_text(owner: Option<HWND>, text: &str, private: bool) -> Option<u32> {
    let wide: Zeroizing<Vec<u16>> = Zeroizing::new(text.encode_utf16().chain(Some(0)).collect());
    let bytes: Zeroizing<Vec<u8>> =
        Zeroizing::new(wide.iter().flat_map(|u| u.to_le_bytes()).collect());
    let open = Open::new(owner)?;
    // SAFETY: the clipboard is open
    unsafe { EmptyClipboard() }.ok()?;
    let mut ok = put(u32::from(CF_UNICODETEXT.0), &bytes);
    if private {
        for name in PRIVATE_FORMATS {
            // SAFETY: a static, NUL-terminated name
            let format = unsafe { RegisterClipboardFormatW(name) };
            ok = ok && format != 0 && put(format, &0u32.to_le_bytes());
        }
    }
    if !ok {
        // never leave a secret on the clipboard without its exclusions
        // SAFETY: the clipboard is still open
        let _ = unsafe { EmptyClipboard() };
        return None;
    }
    drop(open);
    // SAFETY: no preconditions
    Some(unsafe { GetClipboardSequenceNumber() })
}

/// Empties the clipboard if it has not changed since `sequence`.
fn clear_if_unchanged(owner: Option<HWND>, sequence: u32) -> bool {
    // SAFETY: no preconditions
    if unsafe { GetClipboardSequenceNumber() } != sequence {
        return false;
    }
    let Some(_open) = Open::new(owner) else {
        return false;
    };
    // check again now that nobody else can change it
    // SAFETY: the clipboard is open
    unsafe { GetClipboardSequenceNumber() == sequence && EmptyClipboard().is_ok() }
}

/// The main window owns what we put on the clipboard. The handle is passed
/// around as a number: `HWND` is not `Send`.
fn owner(app: &AppHandle) -> Option<isize> {
    let w = app.get_webview_window("main")?;
    w.hwnd().ok().map(|h| h.0 as isize)
}

fn hwnd(owner: Option<isize>) -> Option<HWND> {
    owner.map(|h| HWND(h as *mut core::ffi::c_void))
}

/// Copies a value. Secrets are kept out of clipboard history and, if
/// `clear_after` is set, removed again after that many seconds unless the
/// user has copied something else in the meantime.
pub fn copy(
    app: &AppHandle,
    value: &Secret,
    secret: bool,
    clear_after: Option<u32>,
) -> AppResult<()> {
    let window = owner(app);
    let sequence = set_text(hwnd(window), value.expose(), secret).ok_or_else(|| {
        log::warn!("the clipboard could not be written");
        AppError::Clipboard
    })?;
    if let Ok(mut last) = app.state::<AppState>().clipboard.last.lock() {
        *last = Some(Copied { sequence, secret });
    }
    if let Some(seconds) = clear_after {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(u64::from(seconds)));
            if clear_if_unchanged(hwnd(window), sequence) {
                log::debug!("clipboard cleared after {seconds} s");
            }
        });
    }
    Ok(())
}

/// On lock and exit: removes a secret we copied if it is still there.
pub fn clear_secret(app: &AppHandle) {
    let last = app
        .state::<AppState>()
        .clipboard
        .last
        .lock()
        .ok()
        .and_then(|mut l| l.take());
    if let Some(c) = last.filter(|c| c.secret)
        && clear_if_unchanged(hwnd(owner(app)), c.sequence)
    {
        log::debug!("clipboard cleared");
    }
}
