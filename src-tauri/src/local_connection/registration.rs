use super::profile::{ConnectionSettings, Registration, adapter_path, installation_id};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

static DETECTED_CODEX: Mutex<Option<PathBuf>> = Mutex::new(None);
static REGISTRATION_LOCK: Mutex<()> = Mutex::new(());
pub const SERVER_NAME: &str = "localbridge";

pub fn codex_home() -> io::Result<PathBuf> {
    if let Some(home) = std::env::var_os("CODEX_HOME").filter(|home| !home.is_empty()) {
        return Ok(PathBuf::from(home));
    }
    std::env::var_os("USERPROFILE")
        .map(|home| PathBuf::from(home).join(".codex"))
        .ok_or_else(|| io::Error::other("Codex configuration directory unavailable"))
}

pub fn detect_codex() -> io::Result<PathBuf> {
    let mut cached = DETECTED_CODEX
        .lock()
        .map_err(|_| io::Error::other("client detection busy"))?;
    if let Some(path) = cached.as_ref().filter(|path| path.is_file()) {
        return Ok(path.clone());
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("Codex desktop not found"))?
        .join("OpenAI/Codex/bin");
    let mut candidates: Vec<_> = fs::read_dir(base)?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("codex.exe"))
        .filter(|path| path.is_file())
        .collect();
    candidates.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    for candidate in candidates.into_iter().rev() {
        if execute(
            &candidate,
            &codex_home()?,
            &[
                OsString::from("mcp"),
                OsString::from("add"),
                OsString::from("--help"),
            ],
        )
        .is_ok()
        {
            let path = candidate.canonicalize()?;
            *cached = Some(path.clone());
            return Ok(path);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Codex desktop command unavailable",
    ))
}

fn execute(codex: &Path, home: &Path, args: &[OsString]) -> io::Result<()> {
    let mut child = Command::new(codex)
        .args(args)
        .env("CODEX_HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(io::Error::other("Codex configuration command failed"))
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Codex configuration command timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn config(home: &Path) -> io::Result<toml::Value> {
    match fs::read_to_string(home.join("config.toml")) {
        Ok(text) => toml::from_str(&text)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid Codex configuration")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(toml::Value::Table(Default::default()))
        }
        Err(error) => Err(error),
    }
}

fn server(config: &toml::Value) -> Option<&toml::Value> {
    config.get("mcp_servers")?.get(SERVER_NAME)
}

pub fn owns_entry(value: &toml::Value, registration: &Registration) -> bool {
    value.get("command").and_then(toml::Value::as_str) == registration.adapter.to_str()
        && value
            .get("args")
            .and_then(toml::Value::as_array)
            .is_some_and(|args| {
                args.len() == 2
                    && args[0].as_str() == Some("--install-id")
                    && args[1].as_str() == Some(&registration.install_id)
            })
        && value.get("url").is_none()
}

pub fn configured(settings: &ConnectionSettings) -> io::Result<bool> {
    let Some(registration) = &settings.registration else {
        return Ok(false);
    };
    if registration.codex_home != codex_home()?.canonicalize().unwrap_or(codex_home()?) {
        return Ok(false);
    }
    Ok(
        server(&config(&registration.codex_home)?).is_some_and(|entry| {
            owns_entry(entry, registration)
                && entry.get("enabled").and_then(toml::Value::as_bool) != Some(false)
        }),
    )
}

fn unrelated(mut value: toml::Value) -> toml::Value {
    if let Some(servers) = value
        .get_mut("mcp_servers")
        .and_then(toml::Value::as_table_mut)
    {
        servers.remove(SERVER_NAME);
        if servers.is_empty() {
            value
                .as_table_mut()
                .expect("TOML root")
                .remove("mcp_servers");
        }
    }
    value
}

fn backup(home: &Path, bytes: &[u8]) -> io::Result<()> {
    let path = home.join(format!(
        "config.toml.localbridge-{}.bak",
        crate::security::random_prefixed_id("")
    ));
    let mut output = OpenOptions::new().create_new(true).write(true).open(path)?;
    output.write_all(bytes)?;
    output.sync_all()
}

// The official CLI operates on a retained staging copy while the destination
// denies concurrent writes/deletes. Only a verified result reaches config.toml.
fn update_config<F>(
    directory: &Path,
    codex: &Path,
    home: &Path,
    args: &[OsString],
    verify: F,
) -> io::Result<()>
where
    F: FnOnce(&toml::Value, &toml::Value) -> io::Result<()>,
{
    let mut destination = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(FILE_SHARE_READ)
        .open(home.join("config.toml"))?;
    let mut original = Vec::new();
    destination.read_to_end(&mut original)?;
    let before: toml::Value = toml::from_str(
        std::str::from_utf8(&original)
            .map_err(|_| io::Error::other("invalid configuration encoding"))?,
    )
    .map_err(|_| io::Error::other("invalid Codex configuration"))?;
    backup(home, &original)?;
    let staging = directory.join(format!(
        "codex-registration-{}",
        crate::security::random_prefixed_id("")
    ));
    fs::create_dir_all(&staging)?;
    let mut source = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(staging.join("config.toml"))?;
    source.write_all(&original)?;
    source.sync_all()?;
    drop(source);
    execute(codex, &staging, args)?;
    let result = fs::read(staging.join("config.toml"))?;
    let after: toml::Value = toml::from_str(
        std::str::from_utf8(&result)
            .map_err(|_| io::Error::other("invalid configuration encoding"))?,
    )
    .map_err(|_| io::Error::other("invalid Codex configuration"))?;
    verify(&before, &after)?;
    destination.seek(SeekFrom::Start(0))?;
    destination.write_all(&result)?;
    destination.set_len(result.len() as u64)?;
    destination.sync_all()?;
    if config(home)? != after {
        return Err(io::Error::other(
            "configuration read-back failed; backup retained",
        ));
    }
    Ok(())
}

pub fn connect(
    directory: &Path,
    install_root: &Path,
    settings: &mut ConnectionSettings,
) -> io::Result<()> {
    let _lock = REGISTRATION_LOCK
        .lock()
        .map_err(|_| io::Error::other("registration busy"))?;
    let codex = detect_codex()?;
    let adapter = adapter_path(install_root).canonicalize()?;
    let home = codex_home()?;
    fs::create_dir_all(&home)?;
    let home = home.canonicalize()?;
    let registration = Registration {
        install_id: installation_id(
            adapter
                .parent()
                .ok_or_else(|| io::Error::other("adapter directory unavailable"))?,
        )?,
        adapter,
        codex_home: home.clone(),
    };
    if let Some(previous) = &settings.registration {
        if previous.codex_home != home
            && server(&config(&previous.codex_home)?)
                .is_some_and(|entry| owns_entry(entry, previous))
        {
            return Err(io::Error::other(
                "CODEX_HOME 已改变，请先断开原目录中的本安装条目，再接入新目录",
            ));
        }
    }
    let before = config(&home)?;
    if let Some(entry) = server(&before) {
        if settings.registration.as_ref() == Some(&registration) && owns_entry(entry, &registration)
        {
            if entry.get("enabled").and_then(toml::Value::as_bool) == Some(false) {
                return Err(io::Error::other(
                    "本安装条目已在 Codex 中停用，请先断开接入再重新连接",
                ));
            }
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "同名 LocalBridge MCP 配置不属于本安装，请先处理冲突",
        ));
    }
    // Persist the ownership intent before invoking the official CLI, so a failed
    // post-write check can be retried without claiming somebody else's entry.
    settings.registration = Some(registration.clone());
    settings.save(directory)?;
    update_config(
        directory,
        &codex,
        &home,
        &[
            "mcp".into(),
            "add".into(),
            SERVER_NAME.into(),
            "--".into(),
            registration.adapter.as_os_str().to_owned(),
            "--install-id".into(),
            registration.install_id.clone().into(),
        ],
        |before, after| {
            if server(before).is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "同名配置在接入期间出现，拒绝覆盖",
                ));
            }
            if !server(after).is_some_and(|entry| owns_entry(entry, &registration))
                || unrelated(before.clone()) != unrelated(after.clone())
            {
                return Err(io::Error::other("配置核验失败；保留原配置和备份"));
            }
            Ok(())
        },
    )?;
    Ok(())
}

pub fn disconnect(
    directory: &Path,
    install_root: &Path,
    settings: &mut ConnectionSettings,
) -> io::Result<()> {
    let _lock = REGISTRATION_LOCK
        .lock()
        .map_err(|_| io::Error::other("registration busy"))?;
    let Some(registration) = settings.registration.clone() else {
        return Ok(());
    };
    let current = adapter_path(install_root).canonicalize()?;
    if current != registration.adapter
        || installation_id(
            current
                .parent()
                .ok_or_else(|| io::Error::other("adapter directory unavailable"))?,
        )? != registration.install_id
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "连接记录属于另一安装，拒绝移除配置",
        ));
    }
    let before = config(&registration.codex_home)?;
    if let Some(entry) = server(&before) {
        if !owns_entry(entry, &registration) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "配置归属已变化，拒绝移除",
            ));
        }
        update_config(
            directory,
            &detect_codex()?,
            &registration.codex_home,
            &["mcp".into(), "remove".into(), SERVER_NAME.into()],
            |before, after| {
                if !server(before).is_some_and(|entry| owns_entry(entry, &registration)) {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "配置归属已变化，拒绝移除",
                    ));
                }
                if server(after).is_some() || unrelated(before.clone()) != unrelated(after.clone())
                {
                    return Err(io::Error::other("断开核验失败，保留原配置"));
                }
                Ok(())
            },
        )?;
    }
    settings.registration = None;
    settings.save(directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ownership_requires_exact_install_and_preserves_unrelated_configuration() {
        let registration = Registration {
            adapter: PathBuf::from("D:/中文 space/localbridge-mcp.exe"),
            codex_home: PathBuf::from("D:/codex"),
            install_id: "abc".into(),
        };
        let good: toml::Value = toml::from_str(
            "command = 'D:/中文 space/localbridge-mcp.exe'\nargs = ['--install-id','abc']",
        )
        .unwrap();
        assert!(owns_entry(&good, &registration));
        let mut other = good.clone();
        other["args"] = toml::Value::Array(vec!["--install-id".into(), "other".into()]);
        assert!(!owns_entry(&other, &registration));
        let config: toml::Value = toml::from_str("model='existing'\n[mcp_servers.other]\ncommand='keep'\n[mcp_servers.localbridge]\ncommand='remove'").unwrap();
        let remaining = unrelated(config);
        assert_eq!(remaining["model"].as_str(), Some("existing"));
        assert_eq!(
            remaining["mcp_servers"]["other"]["command"].as_str(),
            Some("keep")
        );
    }
    #[test]
    #[allow(clippy::permissions_set_readonly_false)] // Clears a Windows file attribute in a retained fixture.
    fn missing_client_and_readonly_config_never_change_existing_configuration() {
        let root = std::env::temp_dir().join(crate::security::random_prefixed_id(
            "registration-中文 space-",
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let bytes = b"model='existing'\n[mcp_servers.other]\ncommand='keep'\n";
        fs::write(&path, bytes).unwrap();
        let result = update_config(
            &root,
            &root.join("missing-client.exe"),
            &root,
            &[],
            |_, _| panic!("missing client cannot reach verification"),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();
        let result = update_config(
            &root,
            &root.join("missing-client.exe"),
            &root,
            &[],
            |_, _| panic!("readonly config cannot reach verification"),
        );
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
