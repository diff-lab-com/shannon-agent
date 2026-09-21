//! Windows platform integration for the desktop-control family.
//!
//! Gives the model first-class Windows surfaces the pixel loop can't offer:
//!
//! - **DPI awareness** — per-monitor-v2 declaration so xcap captures and
//!   enigo's `SetCursorPos` share one physical-pixel coordinate space
//!   (without it, Windows virtualizes coordinates and clicks drift under
//!   display scaling — the #1 Windows pixel-automation failure).
//! - **Foreground-window context** — every `computer` action records the
//!   window title + owning process it landed on, so event-sourced sessions
//!   show *what* was driven, not just *that* something was clicked.
//! - **UIA (UI Automation)** — structured read of the foreground window's
//!   control tree (`ui_tree`) and click-by-name (`ui_click`) with element
//!   rectangles, the semantic alternative to screenshot-diff clicking.
//! - **Window management** — list top-level windows, focus by title.
//! - **Clipboard** — read/write `CF_UNICODETEXT`.
//! - **App launch** — ShellExecuteW `open` (cross-platform tool).
//!
//! # Platform / feature gates
//!
//! The module compiles everywhere so tool registration stays stable, but
//! the `windows` crate is only linked under
//! `all(target_os = "windows", feature = "computer-use")` — the same shape
//! the tools require. Everything else returns an honest error (same
//! pattern as the `computer` and `applescript` tools).

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;

/// Why a desktop-control surface is unavailable in this build/session.
fn unavailable_reason(surface: &str) -> String {
    if !cfg!(target_os = "windows") {
        format!(
            "{surface} is only implemented on Windows. On this platform use the \
             platform-native route instead (macOS: `applescript` tool / Accessibility; \
             Linux: computer-use X11/Wayland backends)."
        )
    } else if !cfg!(feature = "computer-use") {
        format!(
            "{surface} requires the `computer-use` feature, which is not enabled in \
             this Shannon build. Windows release bundles ship with it enabled; when \
             building from source use `cargo build --features computer-use`."
        )
    } else {
        format!("{surface} is unavailable in this session.")
    }
}

/// Combined gate used by every Windows-only execution path.
#[cfg(all(target_os = "windows", feature = "computer-use"))]
const WINDOWS_DESKTOP_CONTROL: bool = true;
#[cfg(not(all(target_os = "windows", feature = "computer-use")))]
const WINDOWS_DESKTOP_CONTROL: bool = false;

// ===========================================================================
// Helpers consumed by `computer_use.rs` (safe no-ops off the gated shape)
// ===========================================================================

/// Declare per-monitor-v2 DPI awareness (idempotent, once per process).
///
/// Must happen before the first capture/click: with awareness unset, Windows
/// reports virtualized (scaled) coordinates to `SetCursorPos` while xcap
/// returns physical pixels, so every model-computed click lands off-target
/// on any display scaling ≠ 100%. Best-effort: failures are logged and
/// ignored (the call fails harmlessly if awareness was already set).
#[cfg_attr(
    not(all(target_os = "windows", feature = "computer-use")),
    allow(dead_code)
)]
pub fn ensure_dpi_awareness() {
    #[cfg(all(target_os = "windows", feature = "computer-use"))]
    imp::ensure_dpi_awareness();
}

/// `(window title, process name)` of the foreground window, for attaching
/// provenance to `computer` action results (P2 security story: event logs
/// show which app each action hit). `None` when unavailable (non-Windows,
/// query failure, or the desktop shell's own windows).
#[cfg_attr(
    not(all(target_os = "windows", feature = "computer-use")),
    allow(dead_code)
)]
pub fn foreground_window_context() -> Option<(String, String)> {
    if WINDOWS_DESKTOP_CONTROL {
        #[cfg(all(target_os = "windows", feature = "computer-use"))]
        return imp::foreground_window_context();
    }
    None
}

/// Attach foreground-window provenance to a tool output's metadata.
///
/// Only invoked from the gated `computer` execution paths
/// (`#[cfg(feature = "computer-use")]`); the bare lib build never reaches
/// the callsites, so allow dead-code on the stub side.
#[cfg_attr(
    not(all(target_os = "windows", feature = "computer-use")),
    allow(dead_code)
)]
pub(crate) fn attach_window_context(metadata: &mut HashMap<String, serde_json::Value>) {
    if let Some((title, process)) = foreground_window_context() {
        metadata.insert("window_title".to_string(), json!(title));
        metadata.insert("window_process".to_string(), json!(process));
    }
}

/// Structured UIA tree of a window (foreground when `window` is empty).
/// Error (not panic) on every non-gated shape.
#[cfg_attr(
    not(all(target_os = "windows", feature = "computer-use")),
    allow(dead_code)
)]
pub fn ui_tree(window: &str) -> Result<String, String> {
    #[cfg(all(target_os = "windows", feature = "computer-use"))]
    return imp::ui_tree(window);
    #[cfg(not(all(target_os = "windows", feature = "computer-use")))]
    {
        let _ = window;
        Err(unavailable_reason("ui_tree (UI Automation)"))
    }
}

/// Resolve a UIA element by name substring and return its bounding-rect
/// center in physical pixels plus a description.
#[cfg_attr(
    not(all(target_os = "windows", feature = "computer-use")),
    allow(dead_code)
)]
pub fn ui_click_center(
    window: &str,
    element: &str,
    index: usize,
) -> Result<((i32, i32), String), String> {
    #[cfg(all(target_os = "windows", feature = "computer-use"))]
    return imp::find_element_center(window, element, index);
    #[cfg(not(all(target_os = "windows", feature = "computer-use")))]
    {
        let _ = (window, element, index);
        Err(unavailable_reason("ui_click (UI Automation)"))
    }
}

// ===========================================================================
// imp — real Win32/UIA implementation
// ===========================================================================

#[cfg(all(target_os = "windows", feature = "computer-use"))]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};

    use windows::Win32::Foundation::{CloseHandle, HGLOBAL, HWND, LPARAM, RECT};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::CF_UNICODETEXT;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker,
    };
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_MENU, keybd_event,
    };
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, IsWindowVisible, SW_RESTORE, SW_SHOWNORMAL, SetForegroundWindow,
        ShowWindow,
    };
    use windows::core::{BOOL, PCWSTR, PWSTR};

    static DPI_SET: AtomicBool = AtomicBool::new(false);

    /// Per-Monitor v2 declaration. Windows applies the first successful call
    /// per process; later calls fail harmlessly (E_ACCESSDENIED) — ignored.
    pub fn ensure_dpi_awareness() {
        if DPI_SET.swap(true, Ordering::Relaxed) {
            return;
        }
        // SAFETY: no parameters; process-wide UIPI setting.
        if let Err(e) =
            unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
        {
            tracing::debug!(
                error = %e,
                "SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2) failed — \
                 DPI awareness may already be set via manifest"
            );
        }
    }

    fn hwnd_text(hwnd: HWND) -> String {
        // SAFETY: hwnd comes from the OS window list; length probe + fill is
        // the documented GetWindowText pattern.
        unsafe {
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return String::new();
            }
            let mut buf = vec![0u16; len as usize + 1];
            let copied = GetWindowTextW(hwnd, &mut buf);
            if copied <= 0 {
                return String::new();
            }
            String::from_utf16_lossy(&buf[..copied as usize])
        }
    }

    fn process_image_name(hwnd: HWND) -> String {
        // SAFETY: pid/handle pairing from GetWindowThreadProcessId/OpenProcess
        // with QUERY_LIMITED rights; buffer sized per the documented pattern.
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return String::new();
            }
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return String::new();
            };
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let name = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .ok()
            .map(|_| String::from_utf16_lossy(&buf[..len as usize]))
            .unwrap_or_default();
            let _ = CloseHandle(handle);
            // Leaf name only — the model wants "notepad.exe", not a path.
            name.rsplit(['\\', '/']).next().unwrap_or(&name).to_string()
        }
    }

    pub fn foreground_window_context() -> Option<(String, String)> {
        // SAFETY: no side effects — reads the foreground window.
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() {
            return None;
        }
        let title = hwnd_text(hwnd);
        let process = process_image_name(hwnd);
        if title.is_empty() && process.is_empty() {
            return None;
        }
        Some((title, process))
    }

    // ── window enumeration / focus ──────────────────────────────────────

    #[derive(Debug, Clone)]
    pub struct WindowInfo {
        pub hwnd: isize,
        pub title: String,
        pub process: String,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam carries a &mut Vec<WindowInfo> installed by
        // enumerate_windows; EnumWindows is synchronous.
        unsafe {
            let list = &mut *(lparam.0 as *mut Vec<WindowInfo>);
            if IsWindowVisible(hwnd).as_bool() {
                let title = hwnd_text(hwnd);
                // Skip untitled tool windows (cycles noise from the shell).
                if !title.trim().is_empty() {
                    list.push(WindowInfo {
                        hwnd: hwnd.0 as isize,
                        title,
                        process: process_image_name(hwnd),
                    });
                }
            }
        }
        true.into()
    }

    pub fn enumerate_windows() -> Vec<WindowInfo> {
        let mut out: Vec<WindowInfo> = Vec::new();
        // SAFETY: synchronous enumeration with our own collector pinned in
        // lparam; no reentrancy.
        unsafe {
            let _ = EnumWindows(
                Some(enum_proc),
                LPARAM(&mut out as *mut Vec<WindowInfo> as isize),
            );
        }
        out
    }

    fn find_window(title_substring: &str) -> Option<WindowInfo> {
        let needle = title_substring.to_lowercase();
        enumerate_windows().into_iter().find(|w| {
            w.title.to_lowercase().contains(&needle) || w.process.to_lowercase().contains(&needle)
        })
    }

    pub fn focus_window(title_substring: &str) -> Result<(isize, String, String), String> {
        let info = find_window(title_substring)
            .ok_or_else(|| format!("no visible window matching {title_substring:?}"))?;
        let hwnd = HWND(info.hwnd as *mut _);
        // SAFETY: hwnd from EnumWindows; the synthetic-ALT preamble is the
        // standard workaround for Windows' foreground-rights restriction —
        // without it SetForegroundWindow is silently dropped when the
        // calling process is not itself foreground.
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
            keybd_event(
                VK_MENU.0 as u8,
                0,
                KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP,
                0,
            );
            let ok = SetForegroundWindow(hwnd).as_bool();
            if !ok {
                return Err(format!(
                    "SetForegroundWindow failed for {info:?} — the window may \
                     be on another virtual desktop"
                ));
            }
        }
        Ok((info.hwnd, info.title, info.process))
    }

    // ── clipboard (CF_UNICODETEXT) ──────────────────────────────────────

    fn open_clipboard_retry() -> Result<(), String> {
        // The clipboard is a single-user global: browsers/IMEs hold it in
        // bursts. Retry ~2s before giving up.
        for _ in 0..10 {
            // SAFETY: None = associate with current process; paired with
            // CloseClipboard on every path.
            match unsafe { OpenClipboard(None) } {
                Ok(()) => return Ok(()),
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
        Err("OpenClipboard timed out — the clipboard is held by another process".into())
    }

    pub fn clipboard_get() -> Result<String, String> {
        open_clipboard_retry()?;
        // SAFETY: standard CF_UNICODETEXT read; GlobalLock/Unlock bracket the
        // buffer, and CloseClipboard runs on every return path.
        let result = unsafe {
            let h = GetClipboardData(u32::from(CF_UNICODETEXT.0)).map_err(|e| e.to_string())?;
            let hglobal = HGLOBAL(h.0);
            let ptr = GlobalLock(hglobal) as *const u16;
            if ptr.is_null() {
                let _ = CloseClipboard();
                return Err("GlobalLock failed on clipboard data".into());
            }
            // Text is NUL-terminated; cap the scan defensively.
            let mut len = 0usize;
            while len < 8 * 1024 * 1024 && *ptr.add(len) != 0 {
                len += 1;
            }
            let text = std::slice::from_raw_parts(ptr, len);
            let s = String::from_utf16_lossy(text);
            let _ = GlobalUnlock(hglobal);
            let _ = CloseClipboard();
            Ok(s)
        };
        result
    }

    pub fn clipboard_set(text: &str) -> Result<(), String> {
        open_clipboard_retry()?;
        // SAFETY: GMEM_MOVEABLE buffer handed to the clipboard, which owns it
        // after SetClipboardData; EmptyClipboard discards our previous value.
        let outcome = unsafe {
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            wide.push(0);
            let outcome = (|| -> Result<(), String> {
                EmptyClipboard().map_err(|e| e.to_string())?;
                let h = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).map_err(|e| e.to_string())?;
                let dst = GlobalLock(h) as *mut u16;
                if dst.is_null() {
                    return Err("GlobalLock failed while setting clipboard".into());
                }
                std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
                let _ = GlobalUnlock(h);
                SetClipboardData(
                    u32::from(CF_UNICODETEXT.0),
                    Some(windows::Win32::Foundation::HANDLE(h.0)),
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            })();
            let _ = CloseClipboard();
            outcome
        };
        outcome
    }

    // ── app launch ──────────────────────────────────────────────────────

    pub fn open_path(target: &str) -> Result<(), String> {
        let mut wide: Vec<u16> = target.encode_utf16().collect();
        wide.push(0);
        // SAFETY: ShellExecuteW with a NUL-terminated literal verb and path;
        // return value only signals success (<=32 on failure).
        let hinst = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(windows::core::w!("open").as_ptr()),
                PCWSTR(wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if (hinst.0 as isize) <= 32 {
            return Err(format!(
                "ShellExecuteW failed (code {:#x})",
                hinst.0 as isize
            ));
        }
        Ok(())
    }

    // ── UI Automation (UIA) ─────────────────────────────────────────────

    /// Depth/width caps so a pathological UIA tree can't flood the context:
    /// 14 levels, 240 elements, 90 chars per name.
    const UIA_MAX_DEPTH: usize = 14;
    const UIA_MAX_ELEMENTS: usize = 240;
    const UIA_MAX_NAME: usize = 90;

    fn automation() -> Result<IUIAutomation, String> {
        // SAFETY: COM init + in-proc UIAutomation creation; failure modes
        // mapped to strings for tool-error reporting.
        unsafe {
            // RPC_E_CHANGED_MODE: the thread already runs another apartment
            // (tokio worker that touched COM) — UIA still works via
            // marshalers, so tolerate that one failure code.
            const RPC_E_CHANGED_MODE: windows::core::HRESULT =
                windows::core::HRESULT(0x8001_0106_u32 as i32);
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if hr.is_err() && hr != RPC_E_CHANGED_MODE {
                return Err(format!("CoInitializeEx: {hr}"));
            }
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("create UIAutomation: {e}"))
        }
    }

    fn control_type_name(id: i32) -> &'static str {
        // Raw UIA_CONTROLTYPE_ID values from the UIA spec — mapped locally so
        // the module doesn't depend on per-crate constant exports.
        match id {
            50000 => "Button",
            50002 => "CheckBox",
            50004 => "ComboBox",
            50005 => "Edit",
            50007 => "Hyperlink",
            50009 => "ListItem",
            50010 => "List",
            50012 => "MenuBar",
            50016 => "Pane",
            50020 => "RadioButton",
            50025 => "Tab",
            50026 => "TabItem",
            50028 => "Text",
            50030 => "TitleBar",
            50031 => "ToolBar",
            50032 => "ToolTip",
            50033 => "Tree",
            50034 => "TreeItem",
            50035 => "Custom",
            50036 => "Group",
            50040 => "Window",
            50041 => "Document",
            50044 => "SplitButton",
            _ => "Control",
        }
    }

    struct UiaNode {
        role: &'static str,
        name: String,
        rect: RECT,
    }

    fn read_element(el: &IUIAutomationElement) -> Option<UiaNode> {
        // SAFETY: property reads on a live element — no side effects.
        unsafe {
            let role = control_type_name(el.CurrentControlType().ok()?.0);
            let name = el
                .CurrentName()
                .ok()
                .map(|b| {
                    let s = b.to_string();
                    let mut s = s.trim().to_string();
                    if s.chars().count() > UIA_MAX_NAME {
                        s = s.chars().take(UIA_MAX_NAME).collect::<String>() + "…";
                    }
                    s
                })
                .unwrap_or_default();
            let rect = el.CurrentBoundingRectangle().ok()?;
            Some(UiaNode { role, name, rect })
        }
    }

    fn walk(
        walker: &IUIAutomationTreeWalker,
        el: &IUIAutomationElement,
        depth: usize,
        lines: &mut Vec<String>,
        count: &mut usize,
    ) {
        if depth > UIA_MAX_DEPTH || *count >= UIA_MAX_ELEMENTS {
            return;
        }
        if let Some(node) = read_element(el) {
            *count += 1;
            lines.push(format!(
                "{}e{} {} {:?} [{},{})-({},{})",
                "  ".repeat(depth),
                *count,
                node.role,
                node.name,
                node.rect.left,
                node.rect.top,
                node.rect.right,
                node.rect.bottom,
            ));
            // SAFETY: walker navigation on a live tree; recursion bounded by
            // the caps above.
            unsafe {
                let mut child = walker.GetFirstChildElement(el);
                while let Ok(next) = child {
                    walk(walker, &next, depth + 1, lines, count);
                    if *count >= UIA_MAX_ELEMENTS {
                        break;
                    }
                    child = walker.GetNextSiblingElement(&next);
                }
            }
        }
    }

    /// Rendered UIA tree of one window. `title_substring` selects among
    /// visible top-level windows; empty selects the foreground window.
    pub fn ui_tree(title_substring: &str) -> Result<String, String> {
        let automation = automation()?;
        let target: (HWND, String, String) = if title_substring.trim().is_empty() {
            // SAFETY: read-only foreground query.
            let hwnd = unsafe { GetForegroundWindow() };
            if hwnd.0.is_null() {
                return Err("no foreground window".into());
            }
            (hwnd, hwnd_text(hwnd), process_image_name(hwnd))
        } else {
            let info = find_window(title_substring)
                .ok_or_else(|| format!("no visible window matching {title_substring:?}"))?;
            (HWND(info.hwnd as *mut _), info.title, info.process)
        };
        // SAFETY: ElementFromHandle on a live HWND; walking is read-only.
        let root = unsafe { automation.ElementFromHandle(target.0) }
            .map_err(|e| format!("ElementFromHandle: {e}"))?;
        let walker: IUIAutomationTreeWalker = unsafe { automation.ControlViewWalker() }
            .map_err(|e| format!("ControlViewWalker: {e}"))?;
        let mut lines = vec![format!(
            "[UIA tree — window {:?} (process {})] refs e1..eN usable with ui_click",
            target.1, target.2
        )];
        let mut count = 0usize;
        walk(&walker, &root, 0, &mut lines, &mut count);
        if count == 0 {
            lines.push("(no control-view elements — app may not expose UIA)".into());
        }
        Ok(lines.join("\n"))
    }

    /// Find the nth (0-based) control whose name contains `name_query`
    /// (case-insensitive) in the given window, returning its center point
    /// in physical pixels and a description.
    pub fn find_element_center(
        title_substring: &str,
        name_query: &str,
        index: usize,
    ) -> Result<((i32, i32), String), String> {
        let automation = automation()?;
        let hwnd = if title_substring.trim().is_empty() {
            // SAFETY: read-only foreground query.
            let hwnd = unsafe { GetForegroundWindow() };
            if hwnd.0.is_null() {
                return Err("no foreground window".into());
            }
            hwnd
        } else {
            let info = find_window(title_substring)
                .ok_or_else(|| format!("no visible window matching {title_substring:?}"))?;
            HWND(info.hwnd as *mut _)
        };
        // SAFETY: ElementFromHandle + read-only subtree scan.
        let root = unsafe { automation.ElementFromHandle(hwnd) }
            .map_err(|e| format!("ElementFromHandle: {e}"))?;
        let walker: IUIAutomationTreeWalker = unsafe { automation.ControlViewWalker() }
            .map_err(|e| format!("ControlViewWalker: {e}"))?;
        let needle = name_query.to_lowercase();
        let mut hits: Vec<(RECT, String)> = Vec::new();
        collect_matches(&walker, &root, &needle, &mut hits, 0);
        if hits.is_empty() {
            return Err(format!(
                "no UIA element named {name_query:?} in the window (call ui_tree first to inspect)"
            ));
        }
        if index >= hits.len() {
            return Err(format!(
                "index {index} out of range — {} element(s) match {name_query:?}; pass index 0..{}",
                hits.len(),
                hits.len() - 1
            ));
        }
        let (rect, _desc) = &hits[index];
        Ok((
            ((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2),
            hits[index].1.clone(),
        ))
    }

    fn collect_matches(
        walker: &IUIAutomationTreeWalker,
        el: &IUIAutomationElement,
        needle: &str,
        hits: &mut Vec<(RECT, String)>,
        depth: usize,
    ) {
        if depth > UIA_MAX_DEPTH || hits.len() >= 32 {
            return;
        }
        if let Some(node) = read_element(el) {
            if node.name.to_lowercase().contains(needle) {
                let desc = format!(
                    "{} {:?} [{},{})-({},{})",
                    node.role,
                    node.name,
                    node.rect.left,
                    node.rect.top,
                    node.rect.right,
                    node.rect.bottom
                );
                hits.push((node.rect, desc));
            }
            // SAFETY: bounded read-only navigation.
            unsafe {
                let mut child = walker.GetFirstChildElement(el);
                while let Ok(next) = child {
                    collect_matches(walker, &next, needle, hits, depth + 1);
                    if hits.len() >= 32 {
                        break;
                    }
                    child = walker.GetNextSiblingElement(&next);
                }
            }
        }
    }
}

// ===========================================================================
// Tools — window_list / window_focus / clipboard_read / clipboard_write /
// app_open (app_open is genuinely cross-platform)
// ===========================================================================

fn stub_output(surface: &str) -> ToolOutput {
    ToolOutput {
        content: unavailable_reason(surface),
        is_error: true,
        metadata: HashMap::new(),
    }
}

// ── window_list ─────────────────────────────────────────────────────────────
pub struct WindowListTool;

#[async_trait]
impl Tool for WindowListTool {
    fn name(&self) -> &str {
        "window_list"
    }
    fn description(&self) -> &str {
        "List visible top-level windows: title, owning process, and handle id. Use the title with `window_focus` or `computer` ui actions."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {}})
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, _input: serde_json::Value) -> ToolResult<ToolOutput> {
        #[cfg(all(target_os = "windows", feature = "computer-use"))]
        {
            let wins = imp::enumerate_windows();
            if wins.is_empty() {
                return Ok(ToolOutput {
                    content: "(no visible top-level windows)".to_string(),
                    is_error: false,
                    metadata: HashMap::new(),
                });
            }
            let lines: Vec<String> = wins
                .iter()
                .map(|w| format!("- [{}] {} ({})", w.hwnd, w.title, w.process))
                .collect();
            return Ok(ToolOutput {
                content: lines.join("\n"),
                is_error: false,
                metadata: HashMap::new(),
            });
        }
        Ok(stub_output("window_list"))
    }
}

// ── window_focus ────────────────────────────────────────────────────────────
pub struct WindowFocusTool;

#[async_trait]
impl Tool for WindowFocusTool {
    fn name(&self) -> &str {
        "window_focus"
    }
    fn description(&self) -> &str {
        "Bring a window to the foreground by title or process substring. Restores it if minimized."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "title": {"type": "string", "description": "Title or process-name substring, e.g. 'Notepad' or '报告.docx - Word'"}
            },
            "required": ["title"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_destructive(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let title = input["title"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing field \"title\"".into()))?;
        if WINDOWS_DESKTOP_CONTROL {
            #[cfg(all(target_os = "windows", feature = "computer-use"))]
            match imp::focus_window(title) {
                Ok((_hwnd, wtitle, process)) => {
                    return Ok(ToolOutput {
                        content: format!("focused window {wtitle:?} ({process})"),
                        is_error: false,
                        metadata: HashMap::new(),
                    });
                }
                Err(e) => {
                    return Ok(ToolOutput {
                        content: e,
                        is_error: true,
                        metadata: HashMap::new(),
                    });
                }
            }
        }
        // Gated arm ran; if we reach here the build was non-Windows or the
        // feature is off — drop the unused `title` binding so the stub
        // compiles under `-D unused-variables`.
        let _ = title;
        Ok(stub_output("window_focus"))
    }
}

// ── clipboard_read ──────────────────────────────────────────────────────────
pub struct ClipboardReadTool;

#[async_trait]
impl Tool for ClipboardReadTool {
    fn name(&self) -> &str {
        "clipboard_read"
    }
    fn description(&self) -> &str {
        "Read the current text (CF_UNICODETEXT) from the system clipboard. May contain secrets the user copied — never echo it verbatim unless asked."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {}})
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, _input: serde_json::Value) -> ToolResult<ToolOutput> {
        if WINDOWS_DESKTOP_CONTROL {
            #[cfg(all(target_os = "windows", feature = "computer-use"))]
            match imp::clipboard_get() {
                Ok(text) => {
                    return Ok(ToolOutput {
                        content: text,
                        is_error: false,
                        metadata: HashMap::new(),
                    });
                }
                Err(e) => {
                    return Ok(ToolOutput {
                        content: e,
                        is_error: true,
                        metadata: HashMap::new(),
                    });
                }
            }
        }
        Ok(stub_output("clipboard_read"))
    }
}

// ── clipboard_write ─────────────────────────────────────────────────────────
pub struct ClipboardWriteTool;

#[async_trait]
impl Tool for ClipboardWriteTool {
    fn name(&self) -> &str {
        "clipboard_write"
    }
    fn description(&self) -> &str {
        "Write text to the system clipboard (CF_UNICODETEXT), replacing whatever was there."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_destructive(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let text = input["text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing field \"text\"".into()))?;
        if WINDOWS_DESKTOP_CONTROL {
            #[cfg(all(target_os = "windows", feature = "computer-use"))]
            match imp::clipboard_set(text) {
                Ok(()) => {
                    return Ok(ToolOutput {
                        content: format!("clipboard set ({} chars)", text.chars().count()),
                        is_error: false,
                        metadata: HashMap::new(),
                    });
                }
                Err(e) => {
                    return Ok(ToolOutput {
                        content: e,
                        is_error: true,
                        metadata: HashMap::new(),
                    });
                }
            }
        }
        let _ = text;
        Ok(stub_output("clipboard_write"))
    }
}

// ── app_open ────────────────────────────────────────────────────────────────
/// Cross-platform `open`: Windows ShellExecuteW, macOS `open`,
/// Linux `xdg-open`. Launches the OS default handler for executables,
/// documents, folders, and URLs.
pub struct AppOpenTool;

#[async_trait]
impl Tool for AppOpenTool {
    fn name(&self) -> &str {
        "app_open"
    }
    fn description(&self) -> &str {
        "Open a file, folder, URL, or application with the OS default handler (Windows ShellExecuteW / macOS open / Linux xdg-open)."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "target": {"type": "string", "description": "Path, folder, URL, or app name, e.g. 'notepad', 'C:/report.xlsx', 'https://example.com'"}
            },
            "required": ["target"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_destructive(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let target = input["target"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing field \"target\"".into()))?
            .trim()
            .to_string();
        if target.is_empty() {
            return Err(ToolError::InvalidInput("target must not be empty".into()));
        }

        #[cfg(all(target_os = "windows", feature = "computer-use"))]
        if WINDOWS_DESKTOP_CONTROL {
            return match imp::open_path(&target) {
                Ok(()) => Ok(ToolOutput {
                    content: format!("opened {target:?}"),
                    is_error: false,
                    metadata: HashMap::new(),
                }),
                Err(e) => Ok(ToolOutput {
                    content: e,
                    is_error: true,
                    metadata: HashMap::new(),
                }),
            };
        }

        #[cfg(not(all(target_os = "windows", feature = "computer-use")))]
        {
            let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
                ("open", vec![&target])
            } else if cfg!(target_os = "linux") {
                ("xdg-open", vec![&target])
            } else {
                return Ok(stub_output("app_open"));
            };
            match std::process::Command::new(program).args(args).spawn() {
                Ok(_) => Ok(ToolOutput {
                    content: format!("opened {target:?} via {program}"),
                    is_error: false,
                    metadata: HashMap::new(),
                }),
                Err(e) => Ok(ToolOutput {
                    content: format!("{program} failed: {e}"),
                    is_error: true,
                    metadata: HashMap::new(),
                }),
            }
        }

        // Windows WITHOUT the feature reaches here — honest stub.
        #[cfg(all(target_os = "windows", feature = "computer-use"))]
        {
            let _ = &target;
            Ok(stub_output("app_open"))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_names_are_stable() {
        for (tool, expected) in [
            (&WindowListTool as &dyn Tool, "window_list"),
            (&WindowFocusTool, "window_focus"),
            (&ClipboardReadTool, "clipboard_read"),
            (&ClipboardWriteTool, "clipboard_write"),
            (&AppOpenTool, "app_open"),
        ] {
            assert_eq!(tool.name(), expected);
        }
    }

    #[test]
    fn test_window_list_is_read_only_and_serializable() {
        assert!(WindowListTool.is_read_only());
        assert!(WindowListTool.is_concurrency_safe());
        assert!(!ClipboardWriteTool.is_read_only());
        assert!(!ClipboardWriteTool.is_concurrency_safe());
    }

    #[test]
    fn test_schemas_require_fields() {
        assert_eq!(WindowFocusTool.input_schema()["required"], json!(["title"]));
        assert_eq!(
            ClipboardWriteTool.input_schema()["required"],
            json!(["text"])
        );
        assert_eq!(AppOpenTool.input_schema()["required"], json!(["target"]));
    }

    #[test]
    fn test_unavailable_reason_names_platform_or_feature() {
        let reason = unavailable_reason("window_focus");
        if !cfg!(target_os = "windows") {
            assert!(reason.contains("only implemented on Windows"));
        } else if !cfg!(feature = "computer-use") {
            assert!(reason.contains("computer-use"));
        }
    }
}
