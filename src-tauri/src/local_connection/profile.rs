use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PROFILE_FILE: &str = "connection-profile.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionMode {
    Local,
    OpenaiTunnel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub codex_home: PathBuf,
    pub adapter: PathBuf,
    pub install_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionSettings {
    pub schema_version: u32,
    pub mode: ConnectionMode,
    pub registration: Option<Registration>,
    pub auto_connect_enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyConnectionSettings {
    schema_version: u32,
    mode: ConnectionMode,
    registration: Option<Registration>,
}

impl ConnectionSettings {
    pub fn load(directory: &Path, _legacy_user: bool) -> io::Result<Self> {
        let path = directory.join(PROFILE_FILE);
        match fs::read(path) {
            Ok(bytes) => {
                let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid connection settings")
                })?;
                match value
                    .get("schema_version")
                    .and_then(serde_json::Value::as_u64)
                {
                    Some(1) => {
                        let legacy: LegacyConnectionSettings = serde_json::from_value(value)
                            .map_err(|_| {
                                io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "invalid legacy connection settings",
                                )
                            })?;
                        debug_assert_eq!(legacy.schema_version, 1);
                        let _previous_mode = legacy.mode;
                        Ok(Self {
                            schema_version: 2,
                            mode: ConnectionMode::Local,
                            registration: legacy.registration,
                            auto_connect_enabled: true,
                        })
                    }
                    Some(2) => serde_json::from_value(value).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid connection settings")
                    }),
                    _ => Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unsupported connection settings",
                    )),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self {
                schema_version: 2,
                mode: ConnectionMode::Local,
                registration: None,
                auto_connect_enabled: true,
            }),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, directory: &Path) -> io::Result<()> {
        if self.schema_version != 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported connection settings",
            ));
        }
        fs::create_dir_all(directory)?;
        let path = directory.join(PROFILE_FILE);
        let previous = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if let Some(bytes) = previous {
            let legacy = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|value| {
                    value
                        .get("schema_version")
                        .and_then(serde_json::Value::as_u64)
                })
                == Some(1);
            if legacy {
                match OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(directory.join("connection-profile.schema-1.bak"))
                {
                    Ok(mut backup) => {
                        backup.write_all(&bytes)?;
                        backup.sync_all()?;
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
            }
        }
        let temporary = directory.join(format!(
            "connection-profile-{}.tmp",
            crate::security::random_prefixed_id("")
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(self).map_err(io::Error::other)?)?;
        file.sync_all()?;
        drop(file);
        atomic_replace(&temporary, &path)
    }
}

#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, target: &Path) -> io::Result<()> {
    fs::rename(source, target)
}

pub fn installation_id(root: &Path) -> io::Result<String> {
    let root = root.canonicalize()?;
    Ok(format!(
        "{:x}",
        Sha256::digest(root.to_string_lossy().to_lowercase().as_bytes())
    ))
}

pub fn adapter_path(root: &Path) -> PathBuf {
    if cfg!(debug_assertions) {
        root.join("src-tauri/target/debug/localbridge-mcp.exe")
    } else {
        root.join("localbridge-mcp.exe")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_and_legacy_users_default_local_and_future_schema_fails_closed() {
        let root = std::env::temp_dir().join(crate::security::random_prefixed_id("local-profile-"));
        assert_eq!(
            ConnectionSettings::load(&root, false).unwrap().mode,
            ConnectionMode::Local
        );
        assert_eq!(
            ConnectionSettings::load(&root, true).unwrap().mode,
            ConnectionMode::Local
        );
        let mut settings = ConnectionSettings::load(&root, false).unwrap();
        settings.save(&root).unwrap();
        assert_eq!(ConnectionSettings::load(&root, true).unwrap(), settings);
        settings.schema_version = 99;
        assert!(settings.save(&root).is_err());
        fs::write(
            root.join(PROFILE_FILE),
            b"{\"schema_version\":2,\"mode\":\"local\",\"registration\":null}",
        )
        .unwrap();
        assert!(ConnectionSettings::load(&root, false).is_err());
    }

    #[test]
    fn legacy_tunnel_migrates_once_and_preserves_later_mode_and_disconnect() {
        let root =
            std::env::temp_dir().join(crate::security::random_prefixed_id("local-migration-"));
        fs::create_dir_all(&root).unwrap();
        let original = b"{\"schema_version\":1,\"mode\":\"openai_tunnel\",\"registration\":null}";
        fs::write(root.join(PROFILE_FILE), original).unwrap();
        let mut settings = ConnectionSettings::load(&root, true).unwrap();
        assert_eq!(settings.mode, ConnectionMode::Local);
        assert!(settings.auto_connect_enabled);
        settings.save(&root).unwrap();
        assert_eq!(
            fs::read(root.join("connection-profile.schema-1.bak")).unwrap(),
            original
        );
        settings.mode = ConnectionMode::OpenaiTunnel;
        settings.auto_connect_enabled = false;
        settings.save(&root).unwrap();
        assert_eq!(ConnectionSettings::load(&root, true).unwrap(), settings);
        assert_eq!(
            fs::read(root.join("connection-profile.schema-1.bak")).unwrap(),
            original
        );
    }
}
