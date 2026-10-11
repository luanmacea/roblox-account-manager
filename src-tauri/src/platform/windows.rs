use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, HWND, POINT, RECT,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CLASSES_ROOT, KEY_READ, REG_SZ,
};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, OpenProcess, ReleaseMutex, TerminateProcess, WaitForSingleObject,
    PROCESS_QUERY_INFORMATION, PROCESS_TERMINATE, PROCESS_VM_READ,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetAncestor, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, MoveWindow,
    SetForegroundWindow, ShowWindow, WindowFromPoint, GA_ROOT,
};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};

const WAIT_OBJECT_0: u32 = 0;
const WAIT_ABANDONED_0: u32 = 0x80;
const CREATE_NO_WINDOW: u32 = 0x08000000;
const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;
const SW_MINIMIZE: i32 = 6;
const SW_RESTORE: i32 = 9;
const MONITORINFOF_PRIMARY: u32 = 1;

include!("windows/core.rs");
include!("windows/process.rs");
include!("windows/singleton.rs");
include!("windows/launch.rs");
include!("windows/client_settings.rs");
include!("windows/settings_restore.rs");
include!("windows/optimization.rs");
include!("windows/focus_follow.rs");
include!("windows/live_audio.rs");
include!("windows/windowing.rs");
include!("windows/input.rs");
include!("windows/tracker.rs");
include!("windows/isolation.rs");
include!("windows/versions.rs");
include!("windows/external_clients.rs");
include!("windows/clipboard.rs");
include!("windows/power.rs");
