use super::{
    error::{UiError, UiResult},
    ui,
};
use crate::app::{DesktopLifecycle, STARTUP_PROFILE_FILE_NAME, StartupProfileStore};
use crate::control_plane::convergence::{ConnectionProfile, ServiceIntent};
use crate::local_connection::{
    profile::{ConnectionMode, ConnectionSettings},
    registration, runtime,
};
use crate::settings::SettingsStore;
use crate::state::RuntimeState;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

// Serialize read-modify-write across windows and concurrent command invocations.
static CONNECTION_OPERATION: Mutex<()> = Mutex::new(());

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionState {
    mode: ConnectionMode,
    codex_detected: bool,
    service_ready: bool,
    configuration_complete: bool,
    connected_clients: usize,
    successful_calls: usize,
    affected_tasks: Vec<String>,
}

fn directory(app: &AppHandle) -> UiResult<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|_| UiError::from("无法定位连接设置"))
}

fn load(app: &AppHandle) -> UiResult<ConnectionSettings> {
    let directory = directory(app)?;
    let data = SettingsStore::new(directory.join("settings.json"))
        .load()
        .map_err(|_| "无法读取设置".to_string())?;
    let legacy = StartupProfileStore::new(directory.join(STARTUP_PROFILE_FILE_NAME))
        .load()
        .map_err(|_| "无法读取旧连接设置".to_string())?;
    ConnectionSettings::load(
        &directory,
        data.settings.onboarding_complete
            || legacy
                .validated_tunnel_id()
                .map_err(|_| "旧连接设置无效".to_string())?
                .is_some(),
    )
    .map_err(|_| UiError::from("连接模式设置损坏或版本不支持"))
}

pub(crate) fn ensure_idle(lifecycle: &DesktopLifecycle, confirmed_cancel: bool) -> UiResult<()> {
    let tasks = lifecycle.active_task_summaries();
    if !tasks.is_empty() && !confirmed_cancel {
        return Err(UiError::from("有运行或排队任务，请明确取消后再切换"));
    }
    if !tasks.is_empty() {
        lifecycle
            .stop_runtime_for_control_plane()
            .map_err(|_| "任务终止未确认，保持原连接设置".to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn get_connection_state(app: AppHandle) -> UiResult<ConnectionState> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<ConnectionState> {
        let settings = load(&app)?;
        let lifecycle = app.state::<DesktopLifecycle>();
        let ready = lifecycle
            .control_plane_snapshot()
            .runtime
            .ready_value()
            .is_some_and(|runtime| runtime.state == RuntimeState::Ready);
        Ok(ConnectionState {
            mode: settings.mode,
            codex_detected: registration::detect_codex().is_ok(),
            service_ready: ready,
            configuration_complete: registration::configured(&settings)
                .map_err(|_| "无法核验 Codex 配置".to_string())?,
            connected_clients: if ready && settings.mode == ConnectionMode::Local {
                runtime::connected_clients()
            } else {
                0
            },
            successful_calls: if ready && settings.mode == ConnectionMode::Local {
                runtime::successful_calls()
            } else {
                0
            },
            affected_tasks: lifecycle.active_task_summaries(),
        })
    })
    .await
    .map_err(|_| UiError::from("连接状态后台任务异常"))?
}

#[tauri::command]
pub async fn set_connection_mode(
    mode: ConnectionMode,
    confirmed_cancel: bool,
    app: AppHandle,
) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let _operation = CONNECTION_OPERATION
            .lock()
            .map_err(|_| UiError::from("连接设置正忙，请重试"))?;
        let mut settings = load(&app)?;
        if settings.mode == mode {
            return Ok(());
        }
        let lifecycle = app.state::<DesktopLifecycle>();
        let restart = lifecycle.desired_state().snapshot().state.services == ServiceIntent::Enabled;
        ensure_idle(&lifecycle, confirmed_cancel)?;
        lifecycle
            .stop_runtime_for_control_plane()
            .map_err(|_| "旧连接未完全停止".to_string())?;
        settings.mode = mode;
        settings
            .save(&directory(&app)?)
            .map_err(|_| "无法保存连接模式".to_string())?;
        let connection = match mode {
            ConnectionMode::Local => Some(ConnectionProfile::local()),
            ConnectionMode::OpenaiTunnel => {
                StartupProfileStore::new(directory(&app)?.join(STARTUP_PROFILE_FILE_NAME))
                    .load()
                    .map_err(|_| "无法读取旧 Tunnel 设置".to_string())?
                    .validated_tunnel_id()
                    .map_err(|_| "Tunnel ID 无效".to_string())?
                    .map(|id| ConnectionProfile::new(id, 0))
            }
        };
        lifecycle.set_desired_connection(connection);
        ui::refresh_settings_snapshot(&app, &lifecycle)?;
        if restart
            && (mode == ConnectionMode::Local
                || lifecycle
                    .desired_state()
                    .snapshot()
                    .state
                    .connection
                    .is_some())
        {
            let data = SettingsStore::new(directory(&app)?.join("settings.json"))
                .load()
                .map_err(|_| "无法读取项目".to_string())?;
            lifecycle.set_desired_services(ServiceIntent::Enabled);
            lifecycle
                .start_production_runtime(ui::production_runtime_config_for_active_workspace(
                    &app, &data,
                )?)
                .map_err(|_| "模式已保存，本地服务启动失败，请重试".to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|_| UiError::from("模式切换后台任务异常"))?
}

#[tauri::command]
pub async fn connect_codex(app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let _operation = CONNECTION_OPERATION
            .lock()
            .map_err(|_| UiError::from("连接设置正忙，请重试"))?;
        let mut settings = load(&app)?;
        if settings.mode != ConnectionMode::Local {
            return Err(UiError::from("请先选择本地连接模式"));
        }
        let lifecycle = app.state::<DesktopLifecycle>();
        if !lifecycle
            .control_plane_snapshot()
            .runtime
            .ready_value()
            .is_some_and(|runtime| runtime.state == RuntimeState::Ready)
        {
            return Err(UiError::from("请先选择项目并启动 LocalBridge 服务"));
        }
        registration::connect(
            &directory(&app)?,
            &ui::production_install_root()?,
            &mut settings,
        )
        .map_err(|error| UiError::from(error.to_string()))
    })
    .await
    .map_err(|_| UiError::from("Codex 接入后台任务异常"))?
}

#[tauri::command]
pub async fn disconnect_codex(confirmed_cancel: bool, app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let _operation = CONNECTION_OPERATION
            .lock()
            .map_err(|_| UiError::from("连接设置正忙，请重试"))?;
        let mut settings = load(&app)?;
        let lifecycle = app.state::<DesktopLifecycle>();
        if settings.mode == ConnectionMode::Local {
            ensure_idle(&lifecycle, confirmed_cancel)?;
            lifecycle
                .stop_runtime_for_control_plane()
                .map_err(|_| "活动连接尚未关闭".to_string())?;
        }
        registration::disconnect(
            &directory(&app)?,
            &ui::production_install_root()?,
            &mut settings,
        )
        .map_err(|error| UiError::from(error.to_string()))
    })
    .await
    .map_err(|_| UiError::from("Codex 断开后台任务异常"))?
}
