use super::error::{UiError, UiResult};
use crate::browser_connection::{authority, package, service};
use crate::local_connection::runtime;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;

static IMPORT_OPERATION: Mutex<()> = Mutex::new(());
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserState {
    protocol: u32,
    extension_id: String,
    directory: String,
    prepared_version: Option<String>,
    service_ready: bool,
    connected_browsers: usize,
    connected_chats: usize,
    successful_calls: usize,
    pairings: Vec<authority::PairingSummary>,
}
#[tauri::command]
pub async fn get_browser_connection_state(app: AppHandle) -> UiResult<BrowserState> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<BrowserState> {
        let directory = package::directory().map_err(|_| UiError::from("无法定位扩展目录"))?;
        let prepared =
            package::installed(&directory).map_err(|error| UiError::from(error.to_string()))?;
        let authority = authority::global().ok_or_else(|| {
            UiError::from("浏览器接入控制不可用，请重启 LocalBridge 并检查安装完整性")
        })?;
        Ok(BrowserState {
            protocol: 1,
            extension_id: crate::browser_connection::EXTENSION_ID.trim().into(),
            directory: directory.to_string_lossy().into_owned(),
            prepared_version: prepared.map(|metadata| metadata.version),
            service_ready: service::workspace(&app).is_some(),
            connected_browsers: service::connected_browsers(),
            connected_chats: runtime::browser_connected_clients(),
            successful_calls: runtime::browser_successful_calls(),
            pairings: authority
                .summaries()
                .map_err(|error| UiError::from(error.to_string()))?,
        })
    })
    .await
    .map_err(|_| UiError::from("浏览器状态后台任务异常"))?
}
#[tauri::command]
pub async fn import_browser_extension(path: String, confirmed_disabled: bool) -> UiResult<String> {
    if !confirmed_disabled {
        return Err(UiError::from(
            "请先停止工具并关闭浏览器扩展，再明确确认导入",
        ));
    }
    tauri::async_runtime::spawn_blocking(move || -> UiResult<String> {
        let _operation = IMPORT_OPERATION
            .lock()
            .map_err(|_| UiError::from("扩展准备正忙"))?;
        if runtime::browser_connected_clients() != 0 {
            return Err(UiError::from("仍有浏览器会话，请先在扩展中停止并断开"));
        }
        let metadata = package::import(
            &PathBuf::from(path),
            &package::directory().map_err(|error| UiError::from(error.to_string()))?,
        )
        .map_err(|error| UiError::from(error.to_string()))?;
        Ok(metadata.version)
    })
    .await
    .map_err(|_| UiError::from("扩展导入后台任务异常"))?
}
#[tauri::command]
pub async fn approve_browser_pairing(instance: String, context: String) -> UiResult<()> {
    authority::global()
        .ok_or_else(|| UiError::from("浏览器控制不可用"))?
        .approve(&instance, &context)
        .map_err(|error| UiError::from(error.to_string()))
}
#[tauri::command]
pub async fn revoke_browser_pairing(instance: String) -> UiResult<()> {
    authority::global()
        .ok_or_else(|| UiError::from("浏览器控制不可用"))?
        .revoke(&instance)
        .map_err(|error| UiError::from(error.to_string()))
}
#[tauri::command]
pub async fn open_browser_extension_directory() -> UiResult<()> {
    let directory = package::directory().map_err(|error| UiError::from(error.to_string()))?;
    if !directory.is_dir() {
        return Err(UiError::from("请先导入扩展 ZIP"));
    }
    super::ui::open_system_url(&directory.to_string_lossy())
}
#[tauri::command]
pub async fn choose_browser_extension(folder: bool) -> UiResult<Option<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        if folder {
            super::onboarding::pick_windows_workspace_folder()
        } else {
            choose_zip()
        }
    })
    .await
    .map_err(|_| UiError::from("选择扩展包后台任务异常"))?
}
fn choose_zip() -> UiResult<Option<String>> {
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    let mut buffer = vec![0_u16; 32768];
    let filter: Vec<u16> = "LocalBridge 扩展 ZIP\0*.zip\0\0".encode_utf16().collect();
    let title: Vec<u16> = "选择 LocalBridge 扩展压缩包"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: filter.as_ptr(),
        lpstrFile: buffer.as_mut_ptr(),
        nMaxFile: buffer.len() as u32,
        lpstrTitle: title.as_ptr(),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..unsafe { std::mem::zeroed() }
    };
    if unsafe { GetOpenFileNameW(&mut dialog) } == 0 {
        return Ok(None);
    }
    let end = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    Ok(Some(String::from_utf16_lossy(&buffer[..end])))
}
