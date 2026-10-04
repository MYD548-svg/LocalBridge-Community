use super::error::{UiError, UiResult};
use crate::browser_connection::{authority, package, service};
use crate::local_connection::runtime;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleState {
    available: bool,
    version: Option<String>,
    message: String,
}

fn bundle_directory(app: &AppHandle) -> UiResult<PathBuf> {
    let installed = app
        .path()
        .resource_dir()
        .map_err(|_| UiError::from("无法定位安装资源"))?
        .join("browser-extension");
    #[cfg(debug_assertions)]
    if !installed.join("bundle.json").exists() {
        return Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/browser-extension-stage"));
    }
    Ok(installed)
}

fn bundle_state(app: &AppHandle) -> BundleState {
    match bundle_directory(app)
        .and_then(|path| package::bundled(&path).map_err(|error| UiError::from(error.to_string())))
    {
        Ok(bundle) => BundleState {
            available: true,
            version: Some(bundle.extension.version),
            message: "安装包内置扩展已校验，可直接准备。".into(),
        },
        Err(error) => BundleState {
            available: false,
            version: None,
            message: format!(
                "内置扩展不可用：{}。请下载本项目配套安装包，或手动导入同次构建的 ZIP。",
                error.message
            ),
        },
    }
}

static IMPORT_OPERATION: Mutex<()> = Mutex::new(());
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserState {
    protocol: u32,
    extension_id: String,
    directory: String,
    prepared_version: Option<String>,
    application_version: String,
    repository: String,
    bundle: BundleState,
    service_ready: bool,
    connected_browsers: usize,
    connected_chats: usize,
    successful_calls: usize,
    pairings: Vec<authority::PairingSummary>,
}
#[tauri::command]
pub async fn get_browser_connection_state(app: AppHandle) -> UiResult<BrowserState> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<BrowserState> {
        let _snapshot = IMPORT_OPERATION
            .lock()
            .map_err(|_| UiError::from("扩展准备状态正忙，请稍后刷新"))?;
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
            application_version: env!("CARGO_PKG_VERSION").into(),
            repository: crate::domain::distribution::configuration()
                .repository
                .clone(),
            bundle: bundle_state(&app),
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
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareResult {
    version: String,
    already_prepared: bool,
}

fn prepare(source: PathBuf, confirmed_disabled: bool, bundled: bool) -> UiResult<PrepareResult> {
    let _operation = IMPORT_OPERATION
        .lock()
        .map_err(|_| UiError::from("扩展准备正忙"))?;
    let (metadata, already_prepared) = package::prepare(
        &source,
        &package::directory().map_err(|error| UiError::from(error.to_string()))?,
        confirmed_disabled,
        service::connected_browsers() + runtime::browser_connected_clients(),
        bundled,
    )
    .map_err(|error| UiError::from(error.to_string()))?;
    Ok(PrepareResult {
        version: metadata.version,
        already_prepared,
    })
}

#[tauri::command]
pub async fn import_browser_extension(
    path: String,
    confirmed_disabled: bool,
) -> UiResult<PrepareResult> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<PrepareResult> {
        prepare(PathBuf::from(path), confirmed_disabled, false)
    })
    .await
    .map_err(|_| UiError::from("扩展导入后台任务异常"))?
}

#[tauri::command]
pub async fn prepare_bundled_browser_extension(
    app: AppHandle,
    confirmed_disabled: bool,
) -> UiResult<PrepareResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let directory = bundle_directory(&app)?;
        package::bundled(&directory)
            .map_err(|error| UiError::from(format!("内置扩展不可用：{error}")))?;
        prepare(directory.join("extension.zip"), confirmed_disabled, true)
    })
    .await
    .map_err(|_| UiError::from("内置扩展准备后台任务异常"))?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionDownload {
    state: &'static str,
    message: String,
    repository: String,
    application_version: String,
    extension: Option<crate::domain::distribution::ReleaseExtension>,
    url: Option<String>,
    tag: Option<String>,
}

fn extension_download() -> ExtensionDownload {
    let repository = crate::domain::GitHubRepository::official();
    let mut info = ExtensionDownload {
        state: "unavailable",
        message: String::new(),
        repository: repository.as_str().into(),
        application_version: env!("CARGO_PKG_VERSION").into(),
        extension: None,
        url: None,
        tag: None,
    };
    match crate::app::GitHubReleaseSource::default().current_extension() {
        Ok(manifest) => {
            info.state = "available";
            info.message = "配套发行 ZIP 可下载；下载后请在应用内导入。".into();
            info.url = Some(manifest.extension_url(&repository));
            info.tag = Some(manifest.tag);
            info.extension = Some(manifest.extension);
        }
        Err(error) => {
            use crate::app::UpdateFetchError;
            info.state = match error {
                UpdateFetchError::NoPublishedRelease => "no_release",
                UpdateFetchError::MissingAsset => "missing_asset",
                UpdateFetchError::Incompatible => "incompatible",
                UpdateFetchError::Timeout
                | UpdateFetchError::Transport
                | UpdateFetchError::RateLimited => "network_error",
                _ => "invalid_release",
            };
            info.message = error.user_message().into();
        }
    }
    info
}

#[tauri::command]
pub async fn get_browser_extension_download() -> UiResult<ExtensionDownload> {
    tauri::async_runtime::spawn_blocking(extension_download)
        .await
        .map_err(|_| UiError::from("读取配套下载后台任务异常"))
}

#[tauri::command]
pub async fn open_browser_extension_download() -> UiResult<ExtensionDownload> {
    tauri::async_runtime::spawn_blocking(|| -> UiResult<ExtensionDownload> {
        let info = extension_download();
        if let Some(url) = &info.url {
            super::ui::open_system_url(url)?;
        }
        Ok(info)
    })
    .await
    .map_err(|_| UiError::from("打开配套下载后台任务异常"))?
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
