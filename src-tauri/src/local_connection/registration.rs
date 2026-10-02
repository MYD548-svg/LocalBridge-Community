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

#[derive(Clone, Copy)]
enum ConfigChange<'a> {
    Add(&'a Registration),
    Remove(&'a Registration),
}

// The CLI may normalize unrelated/unknown MCP fields. Adopt only the verified
// owned entry into the original document, never its rewritten configuration.
fn configuration_candidate(
    original: &str,
    cli_result: &str,
    change: ConfigChange<'_>,
) -> io::Result<String> {
    let invalid = || io::Error::other("配置核验失败；保留原配置和备份");
    let before: toml::Value = toml::from_str(original).map_err(|_| invalid())?;
    let generated: toml::Value = toml::from_str(cli_result).map_err(|_| invalid())?;
    let mut document: toml_edit::Document = original.parse().map_err(|_| invalid())?;
    match change {
        ConfigChange::Add(registration) => {
            if server(&before).is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "同名配置拒绝覆盖",
                ));
            }
            if !server(&generated).is_some_and(|entry| {
                owns_entry(entry, registration)
                    && entry.get("enabled").and_then(toml::Value::as_bool) != Some(false)
            }) {
                return Err(invalid());
            }
            let generated_document: toml_edit::Document =
                cli_result.parse().map_err(|_| invalid())?;
            let mut entry = generated_document
                .get("mcp_servers")
                .and_then(|servers| servers.get(SERVER_NAME))
                .ok_or_else(invalid)?
                .clone();
            if !document.contains_key("mcp_servers") {
                let mut table = toml_edit::Table::new();
                table.set_implicit(true);
                document["mcp_servers"] = toml_edit::Item::Table(table);
            }
            let servers_item = document.get_mut("mcp_servers").ok_or_else(invalid)?;
            // Inline tables require a value; ordinary tables accept a table item.
            if servers_item.is_inline_table() {
                entry = toml_edit::Item::Value(entry.into_value().map_err(|_| invalid())?);
            }
            servers_item
                .as_table_like_mut()
                .ok_or_else(invalid)?
                .insert(SERVER_NAME, entry);
        }
        ConfigChange::Remove(registration) => {
            if !server(&before).is_some_and(|entry| owns_entry(entry, registration))
                || server(&generated).is_some()
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "配置归属或移除结果核验失败",
                ));
            }
            document
                .get_mut("mcp_servers")
                .and_then(toml_edit::Item::as_table_like_mut)
                .ok_or_else(invalid)?
                .remove(SERVER_NAME);
        }
    }
    let candidate = document.to_string();
    let after: toml::Value = toml::from_str(&candidate).map_err(|_| invalid())?;
    if unrelated(before) != unrelated(after) {
        return Err(invalid());
    }
    Ok(candidate)
}

// The official CLI operates on a retained staging copy while the destination
// denies concurrent writes/deletes. Only a verified result reaches config.toml.
fn update_config<F>(
    directory: &Path,
    codex: &Path,
    home: &Path,
    args: &[OsString],
    change: ConfigChange<'_>,
    verify: F,
) -> io::Result<()>
where
    F: FnOnce(&toml::Value, &toml::Value) -> io::Result<()>,
{
    update_config_using(directory, codex, home, args, change, verify, execute)
}

fn update_config_using<F, E>(
    directory: &Path,
    codex: &Path,
    home: &Path,
    args: &[OsString],
    change: ConfigChange<'_>,
    verify: F,
    run_cli: E,
) -> io::Result<()>
where
    F: FnOnce(&toml::Value, &toml::Value) -> io::Result<()>,
    E: FnOnce(&Path, &Path, &[OsString]) -> io::Result<()>,
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
    run_cli(codex, &staging, args)?;
    let generated = fs::read_to_string(staging.join("config.toml"))?;
    let result = configuration_candidate(
        std::str::from_utf8(&original)
            .map_err(|_| io::Error::other("invalid configuration encoding"))?,
        &generated,
        change,
    )?;
    let after: toml::Value =
        toml::from_str(&result).map_err(|_| io::Error::other("invalid Codex configuration"))?;
    verify(&before, &after)?;
    destination.seek(SeekFrom::Start(0))?;
    destination.write_all(result.as_bytes())?;
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
        ConfigChange::Add(&registration),
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
            ConfigChange::Remove(&registration),
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

    fn registration() -> Registration {
        Registration {
            adapter: PathBuf::from("D:/中文 space/localbridge-mcp.exe"),
            codex_home: PathBuf::from("D:/synthetic-codex"),
            install_id: "synthetic-install".into(),
        }
    }

    const ORIGINAL: &str = "# keep this comment\nmodel = 'existing' # model comment\nfuture_setting = { value = 'preserve' }\n[mcp_servers.fetch]\ntype = 'stdio' # keep type\ncommand = 'synthetic-fetch'\n[mcp_servers.time]\ntype = 'stdio'\ncommand = 'synthetic-time'\n[mcp_servers.'synthetic/paper-search']\ntype = 'stdio'\ncommand = 'synthetic-paper'\n[mcp_servers.node_repl]\ncommand = 'synthetic-node'\nargs = [] # keep explicit empty args\nunknown = 'preserve'\n";
    const CLI_ADDED: &str = "model = 'changed-by-cli'\n[mcp_servers.localbridge]\ncommand = 'D:/中文 space/localbridge-mcp.exe'\nargs = ['--install-id','synthetic-install']\n";

    #[test]
    fn transaction_preserves_bytes_on_cli_failure_invalid_output_and_conflict() {
        let registration = registration();
        for case in [
            "cli-failed",
            "invalid-output",
            "changed-identity",
            "conflict",
        ] {
            let root = std::env::temp_dir().join(crate::security::random_prefixed_id(
                "registration-中文 space-",
            ));
            fs::create_dir_all(&root).unwrap();
            let original = if case == "conflict" {
                CLI_ADDED
            } else {
                ORIGINAL
            };
            let path = root.join("config.toml");
            fs::write(&path, original).unwrap();
            let result = update_config_using(
                &root,
                &root.join("synthetic-cli.exe"),
                &root,
                &[],
                ConfigChange::Add(&registration),
                |_, _| panic!("invalid CLI result must not reach final verification"),
                |_, staging, _| {
                    // The destination denies concurrent writes while the CLI
                    // runs; this never touches the real user configuration.
                    assert!(OpenOptions::new().write(true).open(&path).is_err());
                    if case == "cli-failed" {
                        return Err(io::Error::other("synthetic CLI failed"));
                    }
                    let generated = if case == "invalid-output" {
                        "invalid = [".to_owned()
                    } else if case == "changed-identity" {
                        CLI_ADDED.replace("synthetic-install", "other-install")
                    } else {
                        CLI_ADDED.to_owned()
                    };
                    fs::write(staging.join("config.toml"), generated)
                },
            );
            assert!(result.is_err(), "{case}");
            assert_eq!(fs::read(&path).unwrap(), original.as_bytes(), "{case}");
            let backups: Vec<_> = fs::read_dir(&root)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".bak"))
                .collect();
            assert_eq!(backups.len(), 1);
            assert_eq!(fs::read(backups[0].path()).unwrap(), original.as_bytes());
        }
    }

    #[test]
    fn transaction_commits_only_the_owned_delta_and_reads_it_back() {
        let registration = registration();
        let root = std::env::temp_dir().join(crate::security::random_prefixed_id(
            "registration-中文 space-",
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        fs::write(&path, ORIGINAL).unwrap();
        update_config_using(
            &root,
            &root.join("synthetic-cli.exe"),
            &root,
            &[],
            ConfigChange::Add(&registration),
            |before, after| {
                assert_eq!(unrelated(before.clone()), unrelated(after.clone()));
                assert!(owns_entry(server(after).unwrap(), &registration));
                Ok(())
            },
            |_, staging, _| fs::write(staging.join("config.toml"), CLI_ADDED),
        )
        .unwrap();
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("args = [] # keep explicit empty args")
        );
        update_config_using(
            &root,
            &root.join("synthetic-cli.exe"),
            &root,
            &[],
            ConfigChange::Remove(&registration),
            |before, after| {
                assert!(owns_entry(server(before).unwrap(), &registration));
                assert!(server(after).is_none());
                Ok(())
            },
            |_, staging, _| fs::write(staging.join("config.toml"), "model='cli-rewritten'\n"),
        )
        .unwrap();
        assert_eq!(
            unrelated(toml::from_str(ORIGINAL).unwrap()),
            unrelated(config(&root).unwrap())
        );
    }

    #[test]
    fn cli_rewrites_are_not_adopted_for_add_or_remove() {
        let registration = registration();
        let candidate =
            configuration_candidate(ORIGINAL, CLI_ADDED, ConfigChange::Add(&registration)).unwrap();
        let before: toml::Value = toml::from_str(ORIGINAL).unwrap();
        let after: toml::Value = toml::from_str(&candidate).unwrap();
        assert_eq!(unrelated(before), unrelated(after.clone()));
        assert!(owns_entry(server(&after).unwrap(), &registration));
        for preserved in [
            "# keep this comment",
            "# model comment",
            "type = 'stdio' # keep type",
            "args = [] # keep explicit empty args",
            "unknown = 'preserve'",
            "future_setting = { value = 'preserve' }",
        ] {
            assert!(
                candidate.contains(preserved),
                "lost source text: {preserved}"
            );
        }
        let removed = configuration_candidate(
            &candidate,
            "model='changed-by-cli'",
            ConfigChange::Remove(&registration),
        )
        .unwrap();
        let removed_value: toml::Value = toml::from_str(&removed).unwrap();
        assert!(server(&removed_value).is_none());
        assert_eq!(
            unrelated(toml::from_str(ORIGINAL).unwrap()),
            unrelated(removed_value)
        );
        assert!(removed.contains("args = [] # keep explicit empty args"));
    }

    #[test]
    fn inline_servers_and_missing_servers_preserve_original_fields() {
        let registration = registration();
        for original in [
            "model='existing'\n",
            "mcp_servers = { other = { command = 'keep', args = [], unknown = 42 } } # inline comment\n",
        ] {
            let candidate =
                configuration_candidate(original, CLI_ADDED, ConfigChange::Add(&registration))
                    .unwrap();
            let after: toml::Value = toml::from_str(&candidate).unwrap();
            assert!(owns_entry(server(&after).unwrap(), &registration));
            assert_eq!(
                unrelated(toml::from_str(original).unwrap()),
                unrelated(after)
            );
            let removed =
                configuration_candidate(&candidate, "", ConfigChange::Remove(&registration))
                    .unwrap();
            assert_eq!(
                unrelated(toml::from_str(original).unwrap()),
                unrelated(toml::from_str(&removed).unwrap())
            );
            if original.contains("# inline comment") {
                assert!(removed.contains("# inline comment"));
            }
        }
    }

    #[test]
    fn conflicts_identity_changes_and_invalid_cli_results_are_rejected() {
        let registration = registration();
        assert!(
            configuration_candidate(CLI_ADDED, CLI_ADDED, ConfigChange::Add(&registration))
                .is_err()
        );
        for invalid in [
            CLI_ADDED.replace("synthetic-install", "other-install"),
            format!("{CLI_ADDED}enabled=false\n"),
            "invalid TOML = [".into(),
            "model='missing-owned-entry'".into(),
        ] {
            assert!(
                configuration_candidate(ORIGINAL, &invalid, ConfigChange::Add(&registration))
                    .is_err()
            );
        }
        let wrong_owner = CLI_ADDED.replace("synthetic-install", "other-install");
        assert!(
            configuration_candidate(&wrong_owner, "", ConfigChange::Remove(&registration)).is_err()
        );
        assert!(
            configuration_candidate(CLI_ADDED, CLI_ADDED, ConfigChange::Remove(&registration))
                .is_err()
        );
    }
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
        let registration = registration();
        let bytes = b"model='existing'\n[mcp_servers.other]\ncommand='keep'\n";
        fs::write(&path, bytes).unwrap();
        let result = update_config(
            &root,
            &root.join("missing-client.exe"),
            &root,
            &[],
            ConfigChange::Add(&registration),
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
            ConfigChange::Add(&registration),
            |_, _| panic!("readonly config cannot reach verification"),
        );
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
