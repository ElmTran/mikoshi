//! Typing an auto-type sequence into another window (features.md §3.10).
//!
//! Everything that can be checked is checked before the first key: the
//! sequence parses, every field it names exists, the window is still there
//! and is not elevated above us. Typing then waits for the user to let go of
//! the hotkey's modifiers, brings the window to the front, switches it to
//! a keyboard without an input method (a Chinese IME in Chinese mode would
//! otherwise turn the keys into pinyin), and stops as soon as another window
//! comes to the front.
//!
//! Characters go in as Unicode (`KEYEVENTF_UNICODE`), independent of the
//! keyboard layout; Tab, Enter and the other named keys as virtual keys.
//! The values typed are held in `Zeroizing` buffers and logged nowhere.

use std::time::{Duration, Instant};

use mikoshi_core::Vault;
use mikoshi_core::autotype::{self, DateTime, Derived, Key, Mods, SequenceError, Step};
use mikoshi_core::model::fields;
use mikoshi_core::totp::TotpError;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Diagnostics::Debug::Beep;
use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::Ime::ImmGetDefaultIMEWnd;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyboardLayout, GetKeyboardLayoutList, HKL, INPUT, INPUT_0,
    INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_ADD, VK_APPS, VK_BACK, VK_CANCEL, VK_CAPITAL,
    VK_CONTROL, VK_DELETE, VK_DIVIDE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_HELP, VK_HOME,
    VK_INSERT, VK_LEFT, VK_LWIN, VK_MENU, VK_MULTIPLY, VK_NEXT, VK_NUMLOCK, VK_NUMPAD0, VK_PRIOR,
    VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU, VK_RWIN, VK_SCROLL, VK_SHIFT, VK_SNAPSHOT,
    VK_SPACE, VK_SUBTRACT, VK_TAB, VK_UP, VkKeyScanW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindow,
    PostMessageW, SMTO_ABORTIFHUNG, SendMessageTimeoutW, SetForegroundWindow, WM_IME_CONTROL,
    WM_INPUTLANGCHANGEREQUEST,
};
use zeroize::Zeroizing;

/// Why auto-type did not start or did not finish. No user data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeError {
    /// the sequence has something that cannot be typed
    BadSequence,
    /// `{S:name}` names a field the entry does not have
    MissingField,
    /// `{TOTP}` but the entry has no usable TOTP settings
    NoTotp,
    /// the window to type into is gone
    NoWindow,
    /// the window runs as administrator; Windows blocks our keys (UIPI)
    Elevated,
    /// the hotkey's modifier keys were still held after a few seconds
    KeysHeld,
    /// another window came to the front before or while typing
    FocusLost,
    /// Windows refused the keys
    Rejected,
}

/// One thing to do, with field values already filled in.
enum Action {
    /// typed as Unicode, independent of the keyboard layout
    Text(Zeroizing<String>),
    /// `{MODE=VIRTUAL}`: characters pressed as keys of the layout; each is a
    /// `VkKeyScanW` result (key in the low byte, Shift/Ctrl/Alt in the high)
    LayoutKeys(Zeroizing<Vec<u16>>),
    Key(Key, Mods),
    Delay(u32),
    KeyDelay(u32),
    Beep {
        hz: u32,
        ms: u32,
    },
}

/// A sequence ready to type: all values fetched, nothing left to fail on
/// except the window itself.
pub struct Plan {
    target: isize,
    actions: Vec<Action>,
}

pub fn sequence_error(e: SequenceError) -> TypeError {
    log::warn!("auto-type sequence cannot be typed: {e:?}");
    TypeError::BadSequence
}

/// Parses `sequence` and fetches every value it needs from the entry.
pub fn plan(
    vault: &Vault,
    entry_id: &str,
    sequence: &str,
    target: isize,
) -> Result<Plan, TypeError> {
    if crate::windows::dev_quiet() {
        return Err(TypeError::NoWindow);
    }
    let steps = autotype::parse(sequence).map_err(sequence_error)?;
    let mut actions = Vec::with_capacity(steps.len());
    let mut layout_keys = false;
    for step in steps {
        let value = match step {
            Step::Text(t) => Zeroizing::new(t),
            Step::Field(name) => {
                let value = vault
                    .copy_value(entry_id, &name)
                    .ok_or(TypeError::MissingField)?
                    .map_err(|_| TypeError::MissingField)?
                    .0;
                Zeroizing::new(value.expose().to_string())
            }
            Step::Totp => {
                let code = vault
                    .copy_value(entry_id, fields::TOTP)
                    .ok_or(TypeError::NoTotp)?
                    .map_err(|_: TotpError| TypeError::NoTotp)?
                    .0;
                Zeroizing::new(code.expose().to_string())
            }
            Step::Derived(d) => Zeroizing::new(derived(vault, entry_id, d)?),
            Step::Date { part, utc } => Zeroizing::new(part.format(now(utc))),
            Step::VirtualMode => {
                layout_keys = true;
                continue;
            }
            Step::Key(k, m) => {
                actions.push(Action::Key(k, m));
                continue;
            }
            Step::Delay(ms) => {
                actions.push(Action::Delay(ms));
                continue;
            }
            Step::KeyDelay(ms) => {
                actions.push(Action::KeyDelay(ms));
                continue;
            }
            Step::Beep { hz, ms } => {
                actions.push(Action::Beep { hz, ms });
                continue;
            }
        };
        actions.push(if layout_keys {
            Action::LayoutKeys(on_layout(&value)?)
        } else {
            Action::Text(value)
        });
    }
    check_window(target)?;
    Ok(Plan { target, actions })
}

/// `{GROUP}`, `{GROUP_PATH}`, `{UUID}`, `{URL:…}` for the entry.
fn derived(vault: &Vault, entry_id: &str, d: Derived) -> Result<String, TypeError> {
    let entry = vault.entry(entry_id).ok_or(TypeError::MissingField)?;
    Ok(match d {
        Derived::Group => entry.group_path.last().cloned().unwrap_or_default(),
        // like KeePass: without the root group, unless the entry is in it
        Derived::GroupPath => match entry.group_path.as_slice() {
            [root] => root.clone(),
            [_, below @ ..] => below.join("."),
            [] => String::new(),
        },
        Derived::Uuid => entry.id.replace('-', "").to_uppercase(),
        Derived::Url(part) => part.of(&entry.url),
    })
}

/// The date and time now, local or UTC.
fn now(utc: bool) -> DateTime {
    // SAFETY: plain queries
    let t = unsafe { if utc { GetSystemTime() } else { GetLocalTime() } };
    DateTime {
        year: t.wYear,
        month: t.wMonth,
        day: t.wDay,
        hour: t.wHour,
        minute: t.wMinute,
        second: t.wSecond,
    }
}

/// The keys that make each character on the current keyboard layout. A
/// character the layout cannot make fails the whole sequence: nothing is
/// typed rather than half a password.
fn on_layout(text: &str) -> Result<Zeroizing<Vec<u16>>, TypeError> {
    let mut keys = Zeroizing::new(Vec::with_capacity(text.len()));
    for unit in text.encode_utf16() {
        // SAFETY: plain query; -1 means no key on this layout makes it
        let scan = unsafe { VkKeyScanW(unit) };
        if scan == -1 {
            log::warn!(
                "auto-type: {{MODE=VIRTUAL}} text has a character the keyboard layout cannot make"
            );
            return Err(TypeError::BadSequence);
        }
        keys.push(scan as u16);
    }
    Ok(keys)
}

/// The window exists and Windows will let us type into it.
pub fn check_window(target: isize) -> Result<(), TypeError> {
    let hwnd = hwnd(target);
    // SAFETY: IsWindow accepts any value
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err(TypeError::NoWindow);
    }
    if is_elevated_above_us(hwnd) {
        return Err(TypeError::Elevated);
    }
    Ok(())
}

/// The title of a window, for matching. Not logged anywhere.
pub fn window_title(target: isize) -> String {
    let hwnd = hwnd(target);
    // SAFETY: plain queries with a buffer of the reported length
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

/// The window in front, unless it is Mikoshi's own.
pub fn foreground_window() -> Option<isize> {
    // SAFETY: no preconditions
    let hwnd = unsafe { GetForegroundWindow() };
    (!hwnd.is_invalid() && !crate::windows::is_ours(hwnd)).then_some(hwnd.0 as isize)
}

fn hwnd(h: isize) -> HWND {
    HWND(h as *mut core::ffi::c_void)
}

/// Types the plan. Blocks for as long as typing takes: call it off the
/// main thread.
pub fn run(plan: Plan, key_delay_ms: u32) -> Result<(), TypeError> {
    let target = hwnd(plan.target);
    mask_menu_key();
    wait_for_modifiers_released()?;
    bring_to_front(target)?;
    let layout = LayoutGuard::switch(target);
    // for programs that still go through the old IME interface
    let ime = ImeGuard::close(target);
    let mut delay = Duration::from_millis(u64::from(key_delay_ms));
    let mut keys = 0usize;
    let result = (|| {
        for action in &plan.actions {
            match action {
                Action::Text(text) => {
                    for unit in text.encode_utf16() {
                        ensure_front(target)?;
                        send(&[unicode(unit, false), unicode(unit, true)])?;
                        keys += 1;
                        std::thread::sleep(delay);
                    }
                }
                Action::Key(key, mods) => {
                    ensure_front(target)?;
                    press(*key, *mods)?;
                    keys += 1;
                    std::thread::sleep(delay);
                }
                Action::LayoutKeys(scans) => {
                    for &scan in scans.iter() {
                        ensure_front(target)?;
                        let [vk, state] = scan.to_le_bytes();
                        let mods = Mods {
                            shift: state & 1 != 0,
                            ctrl: state & 2 != 0,
                            alt: state & 4 != 0,
                        };
                        press_vk(VIRTUAL_KEY(u16::from(vk)), false, mods)?;
                        keys += 1;
                        std::thread::sleep(delay);
                    }
                }
                Action::Beep { hz, ms } => {
                    // SAFETY: plain call; blocks for the tone's length
                    let _ = unsafe { Beep(*hz, *ms) };
                }
                Action::Delay(ms) => std::thread::sleep(Duration::from_millis(u64::from(*ms))),
                Action::KeyDelay(ms) => delay = Duration::from_millis(u64::from(*ms)),
            }
        }
        Ok(())
    })();
    drop(ime);
    drop(layout);
    // counts only, never what was typed
    log::debug!("auto-type: {keys} keys sent, result {result:?}");
    result
}

/// An Alt (or Win) released with no other key since it went down opens the
/// window's menu bar (or the Start menu). The hotkey's own key never reaches
/// the window, so the window would see exactly that. A press of an unassigned
/// key while the modifier is still down makes the release harmless: the
/// trick KeePass and AutoHotkey use.
fn mask_menu_key() {
    // SAFETY: plain queries
    let held = [VK_MENU, VK_LWIN, VK_RWIN]
        .iter()
        .any(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } < 0);
    if held {
        const UNASSIGNED: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);
        let _ = send(&[
            keyboard(UNASSIGNED, 0, KEYBD_EVENT_FLAGS(0)),
            keyboard(UNASSIGNED, 0, KEYEVENTF_KEYUP),
        ]);
    }
}

/// Primary languages whose keyboards are input methods (zh, ja, ko).
fn is_ime_language(hkl: HKL) -> bool {
    matches!(hkl.0 as usize & 0x3ff, 0x04 | 0x11 | 0x12)
}

/// Switches the target window to a keyboard layout without an input method
/// for the time of typing, and back afterwards. Microsoft Pinyin and other
/// TSF input methods ignore the old "IME off" message, so this is what keeps
/// a Chinese input method from catching the keys. Prefers English (US); does
/// nothing when the window already has a plain layout or none is installed.
struct LayoutGuard {
    target: HWND,
    thread: u32,
    before: HKL,
    now: HKL,
}

impl LayoutGuard {
    fn switch(target: HWND) -> Option<LayoutGuard> {
        // SAFETY: plain queries
        let thread = unsafe { GetWindowThreadProcessId(target, None) };
        let before = unsafe { GetKeyboardLayout(thread) };
        log::debug!(
            "auto-type: target keyboard layout {:08x}",
            before.0 as usize
        );
        if before.is_invalid() || !is_ime_language(before) {
            return None;
        }
        let mut list = [HKL::default(); 32];
        // SAFETY: the buffer's length is passed
        let n = unsafe { GetKeyboardLayoutList(Some(&mut list)) }.clamp(0, 32) as usize;
        let plain: Vec<HKL> = list[..n]
            .iter()
            .copied()
            .filter(|h| !is_ime_language(*h))
            .collect();
        let Some(now) = plain
            .iter()
            .copied()
            .find(|h| h.0 as usize & 0xffff == 0x0409)
            .or_else(|| plain.first().copied())
        else {
            log::warn!("auto-type: no keyboard layout without an input method is installed");
            return None;
        };
        log::debug!(
            "auto-type: keyboard layout {:08x} -> {:08x}",
            before.0 as usize,
            now.0 as usize
        );
        request_layout(target, now);
        let guard = LayoutGuard {
            target,
            thread,
            before,
            now,
        };
        // the window switches when it handles the message
        let deadline = Instant::now() + Duration::from_millis(500);
        // SAFETY: plain query
        while unsafe { GetKeyboardLayout(thread) } != now && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(50));
        // SAFETY: plain query
        let got = unsafe { GetKeyboardLayout(thread) };
        log::debug!("auto-type: keyboard layout is now {:08x}", got.0 as usize);
        Some(guard)
    }
}

impl Drop for LayoutGuard {
    fn drop(&mut self) {
        // only if nobody changed it in the meantime
        // SAFETY: plain query
        if unsafe { GetKeyboardLayout(self.thread) } == self.now {
            request_layout(self.target, self.before);
        }
    }
}

fn request_layout(target: HWND, hkl: HKL) {
    // SAFETY: a posted message; the window decides whether to switch
    unsafe {
        let _ = PostMessageW(
            Some(target),
            WM_INPUTLANGCHANGEREQUEST,
            WPARAM(0),
            LPARAM(hkl.0 as isize),
        );
    }
}

/// Typing while Ctrl or Alt from the hotkey is still down would turn the
/// characters into shortcuts.
fn wait_for_modifiers_released() -> Result<(), TypeError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let held = || {
        [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
            .iter()
            // SAFETY: plain query; the high bit means "down now"
            .any(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } < 0)
    };
    while held() {
        if Instant::now() > deadline {
            return Err(TypeError::KeysHeld);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // let the target see the key-ups before the first key
    std::thread::sleep(Duration::from_millis(50));
    Ok(())
}

fn bring_to_front(target: HWND) -> Result<(), TypeError> {
    // SAFETY: plain calls; failing to get the focus is checked below
    unsafe {
        if GetForegroundWindow() != target {
            let _ = SetForegroundWindow(target);
        }
    }
    let deadline = Instant::now() + Duration::from_millis(1000);
    // SAFETY: no preconditions
    while unsafe { GetForegroundWindow() } != target {
        if Instant::now() > deadline {
            return Err(TypeError::FocusLost);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // the window needs a moment to put the caret back after activation
    std::thread::sleep(Duration::from_millis(100));
    Ok(())
}

fn ensure_front(target: HWND) -> Result<(), TypeError> {
    // SAFETY: no preconditions
    if unsafe { GetForegroundWindow() } == target {
        Ok(())
    } else {
        Err(TypeError::FocusLost)
    }
}

fn send(inputs: &[INPUT]) -> Result<(), TypeError> {
    // SAFETY: `inputs` are fully initialised keyboard inputs
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(TypeError::Rejected)
    }
}

fn keyboard(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn unicode(unit: u16, up: bool) -> INPUT {
    let flags = if up {
        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
    } else {
        KEYEVENTF_UNICODE
    };
    keyboard(VIRTUAL_KEY(0), unit, flags)
}

/// The virtual key for a named key, and whether it is an "extended" key
/// (the arrow block, the Windows keys, numpad `/`), which needs a flag to
/// not be read as its twin on the numpad or the left side.
fn virtual_key(key: Key) -> Result<(VIRTUAL_KEY, bool), TypeError> {
    Ok(match key {
        Key::Tab => (VK_TAB, false),
        Key::Enter => (VK_RETURN, false),
        Key::Space => (VK_SPACE, false),
        Key::Backspace => (VK_BACK, false),
        Key::Escape => (VK_ESCAPE, false),
        Key::Delete => (VK_DELETE, true),
        Key::Insert => (VK_INSERT, true),
        Key::Home => (VK_HOME, true),
        Key::End => (VK_END, true),
        Key::PageUp => (VK_PRIOR, true),
        Key::PageDown => (VK_NEXT, true),
        Key::Up => (VK_UP, true),
        Key::Down => (VK_DOWN, true),
        Key::Left => (VK_LEFT, true),
        Key::Right => (VK_RIGHT, true),
        Key::F(n) => (VIRTUAL_KEY(VK_F1.0 + u16::from(n) - 1), false),
        Key::Win => (VK_LWIN, true),
        Key::RightWin => (VK_RWIN, true),
        Key::Apps => (VK_APPS, true),
        Key::CapsLock => (VK_CAPITAL, false),
        Key::NumLock => (VK_NUMLOCK, true),
        Key::ScrollLock => (VK_SCROLL, false),
        Key::PrintScreen => (VK_SNAPSHOT, true),
        // as KeePass: Ctrl+Break's key
        Key::Break => (VK_CANCEL, true),
        Key::Help => (VK_HELP, false),
        Key::NumAdd => (VK_ADD, false),
        Key::NumSubtract => (VK_SUBTRACT, false),
        Key::NumMultiply => (VK_MULTIPLY, false),
        Key::NumDivide => (VK_DIVIDE, true),
        Key::Numpad(n) => (VIRTUAL_KEY(VK_NUMPAD0.0 + u16::from(n)), false),
        Key::Vk { code, extended } => {
            let vk = VIRTUAL_KEY(u16::from(code));
            (vk, extended.unwrap_or_else(|| is_extended(vk)))
        }
        Key::Char(c) => {
            let mut buf = [0u16; 2];
            let units = c.encode_utf16(&mut buf);
            if units.len() != 1 {
                return Err(TypeError::BadSequence);
            }
            // SAFETY: plain query; -1 means no key on this layout makes it
            let scan = unsafe { VkKeyScanW(units[0]) };
            if scan == -1 {
                return Err(TypeError::BadSequence);
            }
            (VIRTUAL_KEY((scan & 0xff) as u16), false)
        }
    })
}

/// Keys that need the extended flag, for `{VKEY n}` without `-EX`/`-NX`.
fn is_extended(vk: VIRTUAL_KEY) -> bool {
    [
        VK_INSERT,
        VK_DELETE,
        VK_HOME,
        VK_END,
        VK_PRIOR,
        VK_NEXT,
        VK_UP,
        VK_DOWN,
        VK_LEFT,
        VK_RIGHT,
        VK_LWIN,
        VK_RWIN,
        VK_APPS,
        VK_NUMLOCK,
        VK_SNAPSHOT,
        VK_CANCEL,
        VK_DIVIDE,
        VK_RCONTROL,
        VK_RMENU,
    ]
    .contains(&vk)
}

/// A named key, with its modifiers held around it.
fn press(key: Key, mods: Mods) -> Result<(), TypeError> {
    let (vk, extended) = virtual_key(key)?;
    press_vk(vk, extended, mods)
}

/// A virtual key, with its modifiers held around it.
fn press_vk(vk: VIRTUAL_KEY, extended: bool, mods: Mods) -> Result<(), TypeError> {
    let held: Vec<VIRTUAL_KEY> = [
        (mods.ctrl, VK_CONTROL),
        (mods.alt, VK_MENU),
        (mods.shift, VK_SHIFT),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, vk)| *vk)
    .collect();
    let ext = if extended {
        KEYEVENTF_EXTENDEDKEY
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    let mut inputs: Vec<INPUT> = held
        .iter()
        .map(|m| keyboard(*m, 0, KEYBD_EVENT_FLAGS(0)))
        .collect();
    inputs.push(keyboard(vk, 0, ext));
    inputs.push(keyboard(vk, 0, ext | KEYEVENTF_KEYUP));
    inputs.extend(held.iter().rev().map(|m| keyboard(*m, 0, KEYEVENTF_KEYUP)));
    send(&inputs)
}

/// `WM_IME_CONTROL` sub-commands (imm.h).
const IMC_GETOPENSTATUS: usize = 0x0005;
const IMC_SETOPENSTATUS: usize = 0x0006;

/// Turns the target window's input method off for the time of typing and
/// back on afterwards, if it was on. A Chinese IME in Chinese mode would
/// otherwise take the keys as pinyin.
struct ImeGuard {
    ime: HWND,
    was_open: bool,
}

impl ImeGuard {
    fn close(target: HWND) -> Option<ImeGuard> {
        // SAFETY: plain query; null when the window has no input context
        let ime = unsafe { ImmGetDefaultIMEWnd(target) };
        if ime.is_invalid() {
            return None;
        }
        let was_open = ime_control(ime, IMC_GETOPENSTATUS, 0).is_some_and(|r| r != 0);
        if was_open {
            ime_control(ime, IMC_SETOPENSTATUS, 0);
            // the IME switches asynchronously
            std::thread::sleep(Duration::from_millis(50));
        }
        Some(ImeGuard { ime, was_open })
    }
}

impl Drop for ImeGuard {
    fn drop(&mut self) {
        if self.was_open {
            ime_control(self.ime, IMC_SETOPENSTATUS, 1);
        }
    }
}

fn ime_control(ime: HWND, command: usize, value: isize) -> Option<usize> {
    let mut result = 0usize;
    // SAFETY: a message to another process's IME window, with a timeout so
    // a hung program cannot hang us
    let ok = unsafe {
        SendMessageTimeoutW(
            ime,
            WM_IME_CONTROL,
            WPARAM(command),
            LPARAM(value),
            SMTO_ABORTIFHUNG,
            200,
            Some(&mut result),
        )
    };
    (ok.0 != 0).then_some(result)
}

/// Windows silently drops our keys into a window of a higher integrity level
/// (one run as administrator) when we are not elevated ourselves.
fn is_elevated_above_us(target: HWND) -> bool {
    // SAFETY: the current process pseudo-handle needs no closing
    if process_elevated(unsafe { GetCurrentProcess() }) == Some(true) {
        return false;
    }
    let mut pid = 0;
    // SAFETY: plain query
    unsafe { GetWindowThreadProcessId(target, Some(&mut pid)) };
    if pid == 0 {
        return false;
    }
    // SAFETY: the handle is closed below
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
    else {
        return false;
    };
    // An elevated process does not let a normal one read its token: that
    // refusal is the answer too.
    let elevated = process_elevated(process).unwrap_or(true);
    // SAFETY: opened above
    unsafe {
        let _ = CloseHandle(process);
    }
    elevated
}

/// `None` when the token cannot be read.
fn process_elevated(process: HANDLE) -> Option<bool> {
    let mut token = HANDLE::default();
    // SAFETY: the token handle is closed below
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.ok()?;
    let mut info = TOKEN_ELEVATION::default();
    let mut len = 0;
    // SAFETY: `info` is the size passed
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut info as *mut _ as *mut core::ffi::c_void),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
    };
    // SAFETY: opened above
    unsafe {
        let _ = CloseHandle(token);
    }
    ok.ok()?;
    Some(info.TokenIsElevated != 0)
}
