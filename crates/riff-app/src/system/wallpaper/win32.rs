//! Win32 desktop capture, monitor layout, registry wallpaper style, and layered overlay fade.

use super::config::{MonitorTile, TransitionEasing};
use super::scheduler::ease;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{
  COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
  AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BitBlt, CreateCompatibleDC,
  CreateDIBSection, CreateSolidBrush, DIB_RGB_COLORS, DeleteDC, DeleteObject, EnumDisplayMonitors,
  FillRect, GetDC, GetMonitorInfoW, HDC, HGDIOBJ, HMONITOR, MONITORINFO, ReleaseDC, SRCCOPY,
  SelectObject, SetViewportOrgEx,
};
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
  HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ, REG_VALUE_TYPE, RegCloseKey,
  RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::UI::Shell::{
  QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{
  CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, FindWindowExW,
  FindWindowW, GW_HWNDPREV, GetClassNameW, GetForegroundWindow, GetSystemMetrics, GetWindow,
  GetWindowRect, HWND_TOP, IsIconic, IsWindow, MSG, PM_REMOVE, PeekMessageW, RegisterClassW,
  SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
  SM_YVIRTUALSCREEN, SPI_GETDESKWALLPAPER, SPI_SETDESKWALLPAPER, SPIF_SENDCHANGE,
  SPIF_UPDATEINIFILE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
  SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetWindowPos, ShowWindow, SystemParametersInfoW, ULW_ALPHA,
  UpdateLayeredWindow, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{BOOL, PCWSTR, w};

const MONITORINFOF_PRIMARY_FLAG: u32 = 1;
const OVERLAY_CLASS_NAME: PCWSTR = w!("RiffWallpaperSyncOverlay");
const DESKTOP_SUBKEY: PCWSTR = w!("Control Panel\\Desktop");

// Sentinel RGB(0xAB, 0x37, 0xF1) -> COLORREF 0x00F137AB, BGRA bytes [0xF1, 0x37, 0xAB].
const PROBE_SENTINEL_COLORREF: u32 = 0x00F1_37AB;
const PROBE_SENTINEL_B: u8 = 0xF1;
const PROBE_SENTINEL_G: u8 = 0x37;
const PROBE_SENTINEL_R: u8 = 0xAB;
const PROBE_COVERAGE_THRESHOLD: f64 = 0.99;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
  pub x: i32,
  pub y: i32,
  pub width: i32,
  pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
  pub rect: Rect,
  pub primary: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct FadeRunParams {
  pub duration: f64,
  pub fps: u32,
  pub easing: TransitionEasing,
  pub start_at: Option<Instant>,
  pub settle: f64,
}

fn wide_os(text: &OsStr) -> Vec<u16> {
  text.encode_wide().chain(std::iter::once(0)).collect()
}

fn wide_str(text: &str) -> Vec<u16> {
  text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide_buf(buf: &[u16]) -> Option<String> {
  let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
  let text = String::from_utf16_lossy(&buf[..len]);
  (!text.is_empty()).then_some(text)
}

/// Enumerates all active display monitors in virtual screen coordinates.
#[must_use]
pub fn enum_monitors() -> Vec<Monitor> {
  unsafe extern "system" fn enum_proc(
    hmon: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
  ) -> BOOL {
    // SAFETY: `lparam` points to our local `Vec<Monitor>` for the duration of `EnumDisplayMonitors`.
    let out = unsafe { &mut *(lparam.0 as *mut Vec<Monitor>) };
    let mut mi = MONITORINFO {
      cbSize: size_of::<MONITORINFO>() as u32,
      ..Default::default()
    };
    // SAFETY: `mi` has `cbSize` initialized and `hmon` was supplied by the OS callback.
    if unsafe { GetMonitorInfoW(hmon, &mut mi) }.as_bool() {
      let r = mi.rcMonitor;
      out.push(Monitor {
        rect: Rect {
          x: r.left,
          y: r.top,
          width: r.right - r.left,
          height: r.bottom - r.top,
        },
        primary: (mi.dwFlags & MONITORINFOF_PRIMARY_FLAG) != 0,
      });
    }
    BOOL(1)
  }

  let mut monitors = Vec::new();
  let ptr = (&raw mut monitors) as isize;
  // SAFETY: `enum_proc` only writes to `monitors` while `EnumDisplayMonitors` runs synchronously.
  let _ = unsafe { EnumDisplayMonitors(None, None, Some(enum_proc), LPARAM(ptr)) };
  monitors
}

/// Bounding rectangle of the full virtual desktop across all monitors.
#[must_use]
pub fn virtual_screen_rect() -> Rect {
  // SAFETY: `GetSystemMetrics` is stateless and safe to query for virtual screen dimensions.
  unsafe {
    Rect {
      x: GetSystemMetrics(SM_XVIRTUALSCREEN),
      y: GetSystemMetrics(SM_YVIRTUALSCREEN),
      width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
      height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
    }
  }
}

/// Primary monitor rectangle, falling back to `SM_CXSCREEN`/`SM_CYSCREEN`.
#[must_use]
pub fn primary_monitor_rect() -> Rect {
  let monitors = enum_monitors();
  if let Some(m) = monitors
    .iter()
    .find(|m| m.primary)
    .or_else(|| monitors.first())
  {
    return m.rect;
  }
  // SAFETY: `GetSystemMetrics` is safe to query on any thread.
  unsafe {
    Rect {
      x: 0,
      y: 0,
      width: GetSystemMetrics(SM_CXSCREEN),
      height: GetSystemMetrics(SM_CYSCREEN),
    }
  }
}

/// Computes `(canvas_rect, optional_span_tiles)` for the current monitor layout.
#[must_use]
pub fn compute_layout(span_requested: bool) -> (Rect, Option<Vec<MonitorTile>>) {
  let monitors = enum_monitors();
  if span_requested && monitors.len() >= 2 {
    let vrect = virtual_screen_rect();
    let tiles = monitors
      .iter()
      .filter(|m| m.rect.width > 0 && m.rect.height > 0)
      .map(|m| MonitorTile {
        x: m.rect.x - vrect.x,
        y: m.rect.y - vrect.y,
        width: m.rect.width as u32,
        height: m.rect.height as u32,
      })
      .collect();
    return (vrect, Some(tiles));
  }
  (primary_monitor_rect(), None)
}

fn window_class_name(hwnd: HWND) -> String {
  let mut buf = [0u16; 256];
  // SAFETY: `buf` is a stack buffer of length 256.
  let len = unsafe { GetClassNameW(hwnd, &mut buf) };
  if len <= 0 {
    return String::new();
  }
  String::from_utf16_lossy(&buf[..len as usize])
}

/// Returns `true` if a fullscreen app or foreground window completely covers `rect`.
#[must_use]
pub fn is_desktop_obscured(rect: Rect) -> bool {
  // SAFETY: `SHQueryUserNotificationState` takes a valid out-pointer.
  if let Ok(state) = unsafe { SHQueryUserNotificationState() }
    && matches!(
      state,
      QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE
    )
  {
    return true;
  }
  // SAFETY: `GetForegroundWindow` returns the current foreground HWND or null.
  let fg = unsafe { GetForegroundWindow() };
  if fg.0.is_null() {
    return false;
  }
  let class = window_class_name(fg);
  if matches!(
    class.as_str(),
    "Progman" | "WorkerW" | "Shell_TrayWnd" | "Windows.UI.Core.CoreWindow"
  ) {
    return false;
  }
  // SAFETY: `fg` is a non-null HWND.
  if unsafe { IsIconic(fg) }.as_bool() {
    return false;
  }
  let mut win_rect = RECT::default();
  // SAFETY: `win_rect` is a valid out-pointer.
  if unsafe { GetWindowRect(fg, &mut win_rect) }.is_err() {
    return false;
  }
  win_rect.left <= rect.x
    && win_rect.top <= rect.y
    && win_rect.right >= rect.x + rect.width
    && win_rect.bottom >= rect.y + rect.height
}

/// Reads the current Windows desktop wallpaper path via `SPI_GETDESKWALLPAPER`.
#[must_use]
pub fn get_wallpaper() -> Option<String> {
  let mut buf = [0u16; 520];
  // SAFETY: `buf` holds 520 `u16` elements matching the `uiparam` count.
  let ok = unsafe {
    SystemParametersInfoW(
      SPI_GETDESKWALLPAPER,
      buf.len() as u32,
      Some(buf.as_mut_ptr().cast()),
      SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    )
  }
  .is_ok();
  if !ok {
    return None;
  }
  from_wide_buf(&buf)
}

/// Applies `path` as the Windows desktop wallpaper via `SPI_SETDESKWALLPAPER`.
pub fn set_wallpaper(path: &Path, update_ini: bool) -> bool {
  let mut wide_path = wide_os(path.as_os_str());
  let flags = SPIF_SENDCHANGE
    | if update_ini {
      SPIF_UPDATEINIFILE
    } else {
      SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)
    };
  // SAFETY: `wide_path` is NUL-terminated for the duration of `SystemParametersInfoW`.
  unsafe {
    SystemParametersInfoW(
      SPI_SETDESKWALLPAPER,
      0,
      Some(wide_path.as_mut_ptr().cast()),
      flags,
    )
  }
  .is_ok()
}

/// Applies a raw path string as the Windows desktop wallpaper (used when restoring original).
pub fn set_wallpaper_raw(path: &str, update_ini: bool) -> bool {
  set_wallpaper(Path::new(path), update_ini)
}

/// Reads `HKCU\Control Panel\Desktop\WallpaperStyle`.
#[must_use]
pub fn get_wallpaper_style() -> Option<String> {
  let mut hkey = HKEY::default();
  // SAFETY: `hkey` is a valid out handle and closed below.
  if unsafe {
    RegOpenKeyExW(
      HKEY_CURRENT_USER,
      DESKTOP_SUBKEY,
      Some(0),
      KEY_READ,
      &mut hkey,
    )
  }
  .is_err()
  {
    return None;
  }
  let mut kind = REG_VALUE_TYPE::default();
  let mut buf = [0u8; 64];
  let mut size = buf.len() as u32;
  // SAFETY: `hkey` is open; `buf` and `size` are valid.
  let res = unsafe {
    RegQueryValueExW(
      hkey,
      w!("WallpaperStyle"),
      None,
      Some(&mut kind),
      Some(buf.as_mut_ptr()),
      Some(&mut size),
    )
  };
  // SAFETY: `hkey` was opened by `RegOpenKeyExW`.
  let _ = unsafe { RegCloseKey(hkey) };
  if res.is_err() {
    return None;
  }
  let u16_count = (size as usize / 2).min(buf.len() / 2);
  let words: Vec<u16> = buf[..u16_count * 2]
    .as_chunks::<2>()
    .0
    .iter()
    .map(|c| u16::from_le_bytes([c[0], c[1]]))
    .collect();
  from_wide_buf(&words)
}

/// Sets `HKCU\Control Panel\Desktop\WallpaperStyle` to `style` and `TileWallpaper` to `"0"`.
pub fn set_wallpaper_style(style: &str) -> bool {
  let mut hkey = HKEY::default();
  // SAFETY: `hkey` is a valid out handle and closed below.
  if unsafe {
    RegOpenKeyExW(
      HKEY_CURRENT_USER,
      DESKTOP_SUBKEY,
      Some(0),
      KEY_SET_VALUE,
      &mut hkey,
    )
  }
  .is_err()
  {
    return false;
  }
  let write_sz = |name: PCWSTR, val: &str| -> bool {
    let w = wide_str(val);
    let bytes: &[u8] =
      // SAFETY: `w` is a contiguous slice of `u16` whose byte length is `w.len() * 2`.
      unsafe { std::slice::from_raw_parts(w.as_ptr().cast::<u8>(), w.len() * 2) };
    // SAFETY: `hkey` is open with `KEY_SET_VALUE`, and `bytes` holds a NUL-terminated UTF-16 string.
    unsafe { RegSetValueExW(hkey, name, Some(0), REG_SZ, Some(bytes)) }.is_ok()
  };
  let ok = write_sz(w!("WallpaperStyle"), style) && write_sz(w!("TileWallpaper"), "0");
  // SAFETY: `hkey` was opened by `RegOpenKeyExW`.
  let _ = unsafe { RegCloseKey(hkey) };
  ok
}

fn find_def_view_in_workers() -> Option<HWND> {
  unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` points to `Option<HWND>` on the stack.
    let out = unsafe { &mut *(lparam.0 as *mut Option<HWND>) };
    // SAFETY: searching direct child `SHELLDLL_DefView` of top-level `hwnd`.
    if let Ok(def_view) = unsafe { FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), None) }
      && !def_view.0.is_null()
    {
      *out = Some(def_view);
      return BOOL(0);
    }
    BOOL(1)
  }

  let mut found: Option<HWND> = None;
  let ptr = (&raw mut found) as isize;
  // SAFETY: `enum_proc` writes to `found` synchronously during `EnumWindows`.
  let _ = unsafe { EnumWindows(Some(enum_proc), LPARAM(ptr)) };
  found
}

fn find_def_view() -> Option<HWND> {
  // SAFETY: querying `Progman` and its child `SHELLDLL_DefView`.
  if let Ok(progman) = unsafe { FindWindowW(w!("Progman"), None) }
    && !progman.0.is_null()
    && let Ok(def_view) =
      unsafe { FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), None) }
    && !def_view.0.is_null()
  {
    return Some(def_view);
  }
  find_def_view_in_workers()
}

fn progman_owns_def_view() -> bool {
  // SAFETY: read-only `FindWindowW` / `FindWindowExW` check; never sends 0x052C.
  unsafe {
    let Ok(progman) = FindWindowW(w!("Progman"), None) else {
      return false;
    };
    if progman.0.is_null() {
      return false;
    }
    FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), None).is_ok_and(|h| !h.0.is_null())
  }
}

/// Returns the desktop host window above which the fade overlay should sit.
/// Never sends `0x052C` to `Progman` so `SHELLDLL_DefView` continues painting wallpaper + icons.
fn desktop_host() -> Option<HWND> {
  // SAFETY: read-only window hierarchy queries.
  let progman = unsafe { FindWindowW(w!("Progman"), None) }
    .ok()
    .filter(|h| !h.0.is_null());
  if let Some(pm) = progman
    && unsafe { FindWindowExW(Some(pm), None, w!("SHELLDLL_DefView"), None) }
      .is_ok_and(|h| !h.0.is_null())
  {
    return Some(pm);
  }

  unsafe extern "system" fn worker_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` points to `Vec<HWND>` on the stack.
    let workers = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    // SAFETY: `FindWindowExW` is read-only.
    if unsafe { FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), None) }
      .is_ok_and(|h| !h.0.is_null())
      && let Ok(worker) = unsafe { FindWindowExW(None, Some(hwnd), w!("WorkerW"), None) }
      && !worker.0.is_null()
    {
      workers.push(worker);
    }
    BOOL(1)
  }

  let mut workers: Vec<HWND> = Vec::new();
  let ptr = (&raw mut workers) as isize;
  // SAFETY: `worker_proc` populates `workers` synchronously.
  let _ = unsafe { EnumWindows(Some(worker_proc), LPARAM(ptr)) };
  workers.into_iter().next().or(progman)
}

/// RAII wrapper around a screen-compatible memory DC and 32-bit top-down DIB section.
struct DibBuffer {
  width: i32,
  height: i32,
  screen_dc: HDC,
  dc: HDC,
  bitmap: HGDIOBJ,
  previous: HGDIOBJ,
  bits: *mut u8,
}

impl DibBuffer {
  fn new(width: i32, height: i32) -> Result<Self, String> {
    if width <= 0 || height <= 0 {
      return Err("invalid DIB dimensions".to_string());
    }
    // SAFETY: `GetDC(None)` returns the virtual screen DC, released in `Drop`.
    let screen_dc = unsafe { GetDC(None) };
    if screen_dc.0.is_null() {
      return Err("GetDC(NULL) failed".to_string());
    }
    // SAFETY: `CreateCompatibleDC` creates a memory DC compatible with `screen_dc`.
    let dc = unsafe { CreateCompatibleDC(Some(screen_dc)) };
    if dc.0.is_null() {
      // SAFETY: `screen_dc` was acquired above.
      unsafe { ReleaseDC(None, screen_dc) };
      return Err("CreateCompatibleDC failed".to_string());
    }
    let bmi = BITMAPINFO {
      bmiHeader: BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
      },
      ..Default::default()
    };
    let mut raw_bits: *mut core::ffi::c_void = ptr::null_mut();
    // SAFETY: `bmi` is initialized, `raw_bits` receives the DIB pixel buffer owned by `bitmap`.
    let bitmap =
      unsafe { CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut raw_bits, None, 0) }.map_err(
        |err| {
          // SAFETY: cleaning up `dc` and `screen_dc` on error.
          unsafe {
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen_dc);
          }
          format!("CreateDIBSection failed: {err}")
        },
      )?;
    if bitmap.is_invalid() || raw_bits.is_null() {
      // SAFETY: cleaning up partially constructed GDI resources.
      unsafe {
        if !bitmap.is_invalid() {
          let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen_dc);
      }
      return Err("CreateDIBSection returned null".to_string());
    }
    // SAFETY: `bitmap` is a valid GDI bitmap compatible with `dc`.
    let previous = unsafe { SelectObject(dc, HGDIOBJ(bitmap.0)) };
    Ok(Self {
      width,
      height,
      screen_dc,
      dc,
      bitmap: HGDIOBJ(bitmap.0),
      previous,
      bits: raw_bits.cast::<u8>(),
    })
  }

  fn pixels(&self) -> &[u8] {
    let len = self.width as usize * self.height as usize * 4;
    // SAFETY: `bits` is valid for `width * height * 4` bytes while `bitmap` lives.
    unsafe { std::slice::from_raw_parts(self.bits, len) }
  }
}

impl Drop for DibBuffer {
  fn drop(&mut self) {
    // SAFETY: restores previous GDI selection and frees owned bitmap, memory DC, and screen DC.
    unsafe {
      let _ = SelectObject(self.dc, self.previous);
      let _ = DeleteObject(self.bitmap);
      let _ = DeleteDC(self.dc);
      ReleaseDC(None, self.screen_dc);
    }
  }
}

fn capture_desktop(dc: HDC, x: i32, y: i32, w: i32, h: i32) -> bool {
  if progman_owns_def_view()
    && let Some(def_view) = find_def_view()
  {
    let mut view_rect = RECT::default();
    // SAFETY: `def_view` is a valid HWND and `view_rect` is a valid out-pointer.
    if unsafe { GetWindowRect(def_view, &mut view_rect) }.is_err() {
      return false;
    }
    let mut prev = POINT::default();
    // SAFETY: `dc` is a valid memory DC; viewport origin is restored before returning.
    if !unsafe { SetViewportOrgEx(dc, view_rect.left - x, view_rect.top - y, Some(&mut prev)) }
      .as_bool()
    {
      return false;
    }
    // SAFETY: `PrintWindow` renders `SHELLDLL_DefView` (wallpaper + desktop icons) into `dc` (`PW_RENDERFULLCONTENT = 2`).
    let ok = unsafe { PrintWindow(def_view, dc, PRINT_WINDOW_FLAGS(2)) }.as_bool();
    // SAFETY: restore original viewport origin.
    let _ = unsafe { SetViewportOrgEx(dc, prev.x, prev.y, None) };
    return ok;
  }

  // Fallback when another app already split Progman: capture from screen DC behind windows.
  // SAFETY: `GetDC(None)` is released before returning.
  let screen_dc = unsafe { GetDC(None) };
  if screen_dc.0.is_null() {
    return false;
  }
  // SAFETY: `dc` and `screen_dc` are valid DCs covering `(x, y, w, h)`.
  let ok = unsafe { BitBlt(dc, 0, 0, w, h, Some(screen_dc), x, y, SRCCOPY) }.is_ok();
  // SAFETY: release acquired screen DC.
  unsafe { ReleaseDC(None, screen_dc) };
  ok
}

/// Checks whether desktop capture yields a complete image (>= 99% non-sentinel pixels).
pub fn probe_desktop_capture() -> bool {
  static CACHED: OnceLock<Mutex<Option<bool>>> = OnceLock::new();
  let lock = CACHED.get_or_init(|| Mutex::new(None));
  if let Ok(guard) = lock.lock()
    && let Some(val) = *guard
  {
    return val;
  }

  let available = (|| -> bool {
    let Some(def_view) = find_def_view() else {
      return false;
    };
    let mut r = RECT::default();
    // SAFETY: `def_view` is valid and `r` is a valid out-pointer.
    if unsafe { GetWindowRect(def_view, &mut r) }.is_err() {
      return false;
    }
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    if w < 64 || h < 64 {
      return false;
    }
    let Ok(dib) = DibBuffer::new(w, h) else {
      return false;
    };
    // SAFETY: `CreateSolidBrush` creates a GDI brush deleted immediately after `FillRect`.
    let brush = unsafe { CreateSolidBrush(COLORREF(PROBE_SENTINEL_COLORREF)) };
    if brush.0.is_null() {
      return false;
    }
    let fill_rc = RECT {
      left: 0,
      top: 0,
      right: w,
      bottom: h,
    };
    // SAFETY: `dib.dc` and `brush` are valid GDI handles.
    unsafe { FillRect(dib.dc, &fill_rc, brush) };
    // SAFETY: `brush` is no longer selected or needed.
    let _ = unsafe { DeleteObject(HGDIOBJ(brush.0)) };
    if !capture_desktop(dib.dc, r.left, r.top, w, h) {
      return false;
    }
    let pixels = dib.pixels();
    let sentinel_count = pixels
      .as_chunks::<4>()
      .0
      .iter()
      .filter(|px| {
        px[0] == PROBE_SENTINEL_B && px[1] == PROBE_SENTINEL_G && px[2] == PROBE_SENTINEL_R
      })
      .count();
    let coverage = 1.0 - (sentinel_count as f64 / (w as f64 * h as f64));
    if coverage >= PROBE_COVERAGE_THRESHOLD {
      tracing::info!(
        coverage_pct = coverage * 100.0,
        "wallpaper-sync desktop capture active -> fade with icons preserved"
      );
      true
    } else {
      tracing::warn!(
        coverage_pct = coverage * 100.0,
        "wallpaper-sync desktop capture incomplete -> fade disabled"
      );
      false
    }
  })();

  if let Ok(mut guard) = lock.lock() {
    *guard = Some(available);
  }
  available
}

unsafe extern "system" fn overlay_wnd_proc(
  hwnd: HWND,
  msg: u32,
  wparam: WPARAM,
  lparam: LPARAM,
) -> LRESULT {
  // SAFETY: standard default window procedure for the overlay window.
  unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn ensure_overlay_class(hinstance: HINSTANCE) -> Result<(), String> {
  static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
  REGISTERED
    .get_or_init(|| {
      let wc = WNDCLASSW {
        lpfnWndProc: Some(overlay_wnd_proc),
        hInstance: hinstance,
        lpszClassName: OVERLAY_CLASS_NAME,
        ..Default::default()
      };
      // SAFETY: `wc` points to a static class name and `DefWindowProcW`.
      let atom = unsafe { RegisterClassW(&wc) };
      if atom == 0 {
        let err = std::io::Error::last_os_error();
        // ERROR_CLASS_ALREADY_EXISTS = 1410
        if err.raw_os_error() != Some(1410) {
          return Err(format!("RegisterClassW failed: {err}"));
        }
      }
      Ok(())
    })
    .clone()
}

/// Layered transparent click-through overlay stacked directly above the Windows desktop layer.
/// Must only be used on the thread that created it.
pub struct OverlayWindow {
  rect: Rect,
  hwnd: HWND,
  host: Option<HWND>,
}

impl OverlayWindow {
  pub fn new(rect: Rect) -> Result<Self, String> {
    // SAFETY: `GetModuleHandleW(None)` returns the current executable module handle.
    let hmod =
      unsafe { GetModuleHandleW(None) }.map_err(|err| format!("GetModuleHandleW failed: {err}"))?;
    let hinstance = HINSTANCE(hmod.0);
    ensure_overlay_class(hinstance)?;
    let host = desktop_host();
    // SAFETY: creates a popup layered transparent tool window owned by the current thread.
    let hwnd = unsafe {
      CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW,
        OVERLAY_CLASS_NAME,
        OVERLAY_CLASS_NAME,
        WS_POPUP,
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        None,
        None,
        Some(hinstance),
        None,
      )
    }
    .map_err(|err| format!("CreateWindowExW failed: {err}"))?;
    if hwnd.0.is_null() {
      return Err("CreateWindowExW returned null".to_string());
    }
    // SAFETY: shows without activating so focus never leaves the active window.
    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    let overlay = Self { rect, hwnd, host };
    overlay.place_above_desktop();
    Ok(overlay)
  }

  fn place_above_desktop(&self) {
    if let Some(host) = self.host {
      // SAFETY: `host` and `self.hwnd` are checked with `IsWindow`/`GetWindow` on this thread.
      // In Win32, `SetWindowPos(hwnd, insert_after, ...)` places `hwnd` immediately BELOW
      // `insert_after`, so passing the window preceding `host` (`GW_HWNDPREV`) stacks our
      // overlay directly ABOVE `host`.
      unsafe {
        if !IsWindow(Some(host)).as_bool() {
          return;
        }
        let insert_after = match GetWindow(host, GW_HWNDPREV) {
          Ok(prev) if prev == self.hwnd => return,
          Ok(prev) if !prev.0.is_null() => prev,
          _ => HWND_TOP,
        };
        let _ = SetWindowPos(
          self.hwnd,
          Some(insert_after),
          0,
          0,
          0,
          0,
          SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
      }
    }
  }

  pub fn snapshot(&self, alpha: u8) -> Result<(), String> {
    let dib = DibBuffer::new(self.rect.width, self.rect.height)?;
    if !capture_desktop(dib.dc, self.rect.x, self.rect.y, dib.width, dib.height) {
      return Err("desktop capture failed".to_string());
    }
    let pt_dst = POINT {
      x: self.rect.x,
      y: self.rect.y,
    };
    let size = SIZE {
      cx: dib.width,
      cy: dib.height,
    };
    let pt_src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
      BlendOp: AC_SRC_OVER as u8,
      BlendFlags: 0,
      SourceConstantAlpha: alpha,
      AlphaFormat: 0,
    };
    // SAFETY: uploads `dib.dc` pixels to the layered window at `alpha`.
    unsafe {
      UpdateLayeredWindow(
        self.hwnd,
        Some(dib.screen_dc),
        Some(&pt_dst),
        Some(&size),
        Some(dib.dc),
        Some(&pt_src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
      )
    }
    .map_err(|err| format!("UpdateLayeredWindow (upload) failed: {err}"))?;
    self.place_above_desktop();
    Ok(())
  }

  pub fn set_alpha(&self, alpha: u8) -> Result<(), String> {
    let blend = BLENDFUNCTION {
      BlendOp: AC_SRC_OVER as u8,
      BlendFlags: 0,
      SourceConstantAlpha: alpha,
      AlphaFormat: 0,
    };
    // SAFETY: updates constant alpha only (`hdcSrc = None`, zero pixel copy).
    unsafe {
      UpdateLayeredWindow(
        self.hwnd,
        None,
        None,
        None,
        None,
        None,
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
      )
    }
    .map_err(|err| format!("UpdateLayeredWindow (alpha) failed: {err}"))
  }

  pub fn pump(&self) {
    let mut msg = MSG::default();
    // SAFETY: drains pending window messages for `self.hwnd`.
    while unsafe { PeekMessageW(&mut msg, Some(self.hwnd), 0, 0, PM_REMOVE) }.as_bool() {
      // SAFETY: `msg` was populated by `PeekMessageW`.
      let _ = unsafe { DispatchMessageW(&msg) };
    }
  }
}

impl Drop for OverlayWindow {
  fn drop(&mut self) {
    if !self.hwnd.0.is_null() {
      // SAFETY: `self.hwnd` was created on this thread and is destroyed once.
      let _ = unsafe { DestroyWindow(self.hwnd) };
      self.hwnd = HWND(ptr::null_mut());
    }
  }
}

struct TimerPeriodGuard;

impl TimerPeriodGuard {
  fn new() -> Self {
    // SAFETY: requests 1ms Windows multimedia timer resolution for smooth 60/120fps sleep pacing.
    unsafe {
      let _ = timeBeginPeriod(1);
    }
    Self
  }
}

impl Drop for TimerPeriodGuard {
  fn drop(&mut self) {
    // SAFETY: pairs with `timeBeginPeriod(1)` in `new()`.
    unsafe {
      let _ = timeEndPeriod(1);
    }
  }
}

/// Manages a single frozen-desktop-snapshot crossfade transition.
pub struct FadeTransition {
  overlay: OverlayWindow,
  applied_at: Option<Instant>,
}

impl FadeTransition {
  pub fn new(rect: Rect) -> Result<Self, String> {
    let overlay = OverlayWindow::new(rect)?;
    overlay.snapshot(255)?;
    Ok(Self {
      overlay,
      applied_at: None,
    })
  }

  pub fn begin(&mut self, wallpaper_path: &Path) {
    set_wallpaper(wallpaper_path, true);
    self.applied_at = Some(Instant::now());
  }

  pub fn run(&mut self, params: FadeRunParams, cancel_check: impl Fn() -> bool) {
    let mut start_at = params.start_at;
    if let Some(applied_at) = self.applied_at
      && params.settle > 0.0
    {
      let settle_at = applied_at + Duration::from_secs_f64(params.settle);
      start_at = Some(match start_at {
        Some(prev) => prev.max(settle_at),
        None => settle_at,
      });
    }
    if let Some(target) = start_at {
      while !cancel_check() {
        let now = Instant::now();
        if now >= target {
          break;
        }
        std::thread::sleep((target - now).min(Duration::from_millis(15)));
      }
    }
    if cancel_check() {
      return;
    }

    let _timer_guard = TimerPeriodGuard::new();
    let frame_period = Duration::from_secs_f64(1.0 / f64::from(params.fps.max(1)));
    let started = Instant::now();
    let mut frames = 0u32;

    while !cancel_check() {
      let elapsed = started.elapsed().as_secs_f64();
      if elapsed >= params.duration {
        break;
      }
      let progress = if params.duration > 0.0 {
        elapsed / params.duration
      } else {
        1.0
      };
      let alpha = (255.0 * (1.0 - ease(progress, params.easing)))
        .round()
        .clamp(0.0, 255.0) as u8;
      if self.overlay.set_alpha(alpha).is_err() {
        return;
      }
      frames += 1;
      self.overlay.pump();
      let next_frame = started + frame_period.saturating_mul(frames + 1);
      let now = Instant::now();
      if next_frame > now {
        std::thread::sleep((next_frame - now).min(Duration::from_millis(20)));
      }
    }
    let _ = self.overlay.set_alpha(0);
  }
}
