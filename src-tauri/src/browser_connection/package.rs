use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

const MAX_PACKAGE: u64 = 32 * 1024 * 1024;
const MAX_FILES: usize = 64;
const METADATA: &str = "localbridge-extension.json";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Metadata {
    pub schema_version: u32,
    pub version: String,
    pub protocol: u32,
    pub application: String,
    pub extension_id: String,
    pub files: BTreeMap<String, String>,
}
pub fn directory() -> io::Result<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|root| root.join("LocalBridge/browser-extension/current"))
        .ok_or_else(|| io::Error::other("无法定位扩展安装目录"))
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 180
        && !name.contains('\\')
        && !name.contains(':')
        && !name
            .chars()
            .any(|ch| ch.is_control() || "<>\"|*?".contains(ch))
        && name.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with([' ', '.'])
                && ![
                    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6",
                    "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7",
                    "LPT8", "LPT9",
                ]
                .contains(
                    &part
                        .split('.')
                        .next()
                        .unwrap_or("")
                        .to_ascii_uppercase()
                        .as_str(),
                )
        })
        && Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}
fn safe_existing_path(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors() {
        if let Ok(metadata) = fs::symlink_metadata(ancestor) {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(io::Error::other("扩展路径包含链接或重解析点"));
                }
            }
            if metadata.file_type().is_symlink() {
                return Err(io::Error::other("扩展路径包含链接"));
            }
        }
    }
    Ok(())
}
fn validate(files: &BTreeMap<String, Vec<u8>>) -> io::Result<Metadata> {
    let bytes = files
        .get(METADATA)
        .ok_or_else(|| io::Error::other("请选择发行扩展 ZIP，源码包不能直接加载"))?;
    let metadata: Metadata =
        serde_json::from_slice(bytes).map_err(|_| io::Error::other("扩展包说明损坏"))?;
    let app = semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(io::Error::other)?;
    let compatible = semver::VersionReq::parse(&metadata.application).map_err(io::Error::other)?;
    if metadata.schema_version != 1
        || metadata.protocol != super::PROTOCOL_VERSION
        || !compatible.matches(&app)
        || metadata.extension_id != super::EXTENSION_ID.trim()
        || metadata.application != ">=0.1.5, <0.2.0"
        || metadata.files.len() > MAX_FILES
        || metadata.files.len() + 1 != files.len()
    {
        return Err(io::Error::other("扩展版本、身份或文件清单不兼容"));
    }
    for (name, digest) in &metadata.files {
        if !safe_name(name)
            || digest.len() != 64
            || !digest.bytes().all(|b| b.is_ascii_hexdigit())
            || files.get(name).is_none_or(|bytes| hash(bytes) != *digest)
        {
            return Err(io::Error::other("扩展文件校验失败"));
        }
    }
    let manifest: Value = serde_json::from_slice(
        files
            .get("manifest.json")
            .ok_or_else(|| io::Error::other("缺少 manifest.json"))?,
    )
    .map_err(io::Error::other)?;
    let identity: Value = serde_json::from_str(super::IDENTITY).map_err(io::Error::other)?;
    if manifest["manifest_version"] != 3
        || manifest["key"] != identity["key"]
        || manifest["version"] != metadata.version
    {
        return Err(io::Error::other("扩展 manifest 身份或版本错误"));
    }
    if manifest["content_scripts"][0]["matches"] != serde_json::json!(["https://chatgpt.com/*"])
        || manifest.get("externally_connectable").is_some()
        || manifest["host_permissions"] != serde_json::json!(["https://chatgpt.com/*"])
        || manifest["permissions"]
            != serde_json::json!(["nativeMessaging", "storage", "clipboardWrite"])
        || manifest["content_scripts"]
            .as_array()
            .is_none_or(|scripts| scripts.len() != 1)
        || manifest["content_scripts"][0]["all_frames"] != false
    {
        return Err(io::Error::other("扩展网页权限不兼容"));
    }
    for entry in [
        manifest["background"]["service_worker"].as_str(),
        manifest["action"]["default_popup"].as_str(),
        Some("popup.js"),
        Some("popup.css"),
        Some("INSTALL.html"),
        Some("INSTALL.svg"),
    ] {
        if entry.is_none_or(|name| !files.contains_key(name)) {
            return Err(io::Error::other("扩展引用文件缺失"));
        }
    }
    let scripts = manifest["content_scripts"][0]["js"]
        .as_array()
        .ok_or_else(|| io::Error::other("扩展入口无效"))?;
    if scripts.is_empty()
        || scripts
            .iter()
            .any(|name| name.as_str().is_none_or(|name| !files.contains_key(name)))
    {
        return Err(io::Error::other("扩展脚本入口缺失"));
    }
    Ok(metadata)
}
fn read_package(source: &Path) -> io::Result<BTreeMap<String, Vec<u8>>> {
    safe_existing_path(source)?;
    let mut files = BTreeMap::new();
    let mut total = 0_u64;
    if source.is_file() {
        if fs::metadata(source)?.len() > MAX_PACKAGE {
            return Err(io::Error::other("扩展压缩包过大"));
        }
        let mut archive =
            zip::ZipArchive::new(fs::File::open(source)?).map_err(io::Error::other)?;
        if archive.len() > MAX_FILES + 1 {
            return Err(io::Error::other("扩展文件数量超限"));
        }
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(io::Error::other)?;
            let name = entry.name().to_owned();
            if entry
                .unix_mode()
                .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000))
            {
                return Err(io::Error::other("压缩包包含链接或特殊文件"));
            }
            if entry.is_dir() {
                if !safe_name(name.trim_end_matches('/')) {
                    return Err(io::Error::other("压缩包包含不安全目录"));
                }
                continue;
            }
            if !safe_name(&name)
                || entry
                    .unix_mode()
                    .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err(io::Error::other("压缩包包含不安全路径或链接"));
            }
            total = total
                .checked_add(entry.size())
                .ok_or_else(|| io::Error::other("扩展大小溢出"))?;
            if total > MAX_PACKAGE {
                return Err(io::Error::other("扩展解压大小超限"));
            }
            let mut bytes = Vec::new();
            entry
                .by_ref()
                .take(MAX_PACKAGE + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_PACKAGE || files.insert(name, bytes).is_some() {
                return Err(io::Error::other("扩展重复文件或大小超限"));
            }
        }
    } else if source.is_dir() {
        safe_existing_path(&source.join(METADATA))?;
        if fs::metadata(source.join(METADATA))?.len() > 64 * 1024 {
            return Err(io::Error::other("扩展说明文件过大"));
        }
        let metadata_bytes = fs::read(source.join(METADATA))?;
        let metadata: Metadata =
            serde_json::from_slice(&metadata_bytes).map_err(io::Error::other)?;
        if metadata.files.len() > MAX_FILES {
            return Err(io::Error::other("扩展文件数量超限"));
        }
        files.insert(METADATA.into(), metadata_bytes);
        for name in metadata.files.keys() {
            if !safe_name(name) {
                return Err(io::Error::other("扩展路径无效"));
            }
            let path = source.join(name);
            safe_existing_path(&path)?;
            total += fs::metadata(&path)?.len();
            if total > MAX_PACKAGE {
                return Err(io::Error::other("扩展大小超限"));
            }
            files.insert(name.clone(), fs::read(path)?);
        }
    } else {
        return Err(io::Error::other("扩展包或目录不存在"));
    }
    // Windows path aliases must not overwrite one another.
    let mut aliases = std::collections::HashSet::new();
    for name in files.keys() {
        if !aliases.insert(name.to_lowercase()) {
            return Err(io::Error::other("扩展存在大小写重复路径"));
        }
    }
    Ok(files)
}
pub fn installed(target: &Path) -> io::Result<Option<Metadata>> {
    safe_existing_path(target)?;
    if !target.join(METADATA).exists() {
        return Ok(None);
    }
    let files = read_package(target)?;
    validate(&files).map(Some)
}
fn transaction(target: &Path, value: &Value) -> io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::other("扩展目录无效"))?;
    let staged = parent
        .join(crate::security::random_prefixed_id("import-"))
        .with_extension("tmp");
    fs::write(
        &staged,
        serde_json::to_vec(value).map_err(io::Error::other)?,
    )?;
    crate::local_connection::profile::atomic_replace(
        &staged,
        &parent.join("import-transaction.json"),
    )
}
fn recover(target: &Path) -> io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::other("扩展目录无效"))?;
    let path = parent.join("import-transaction.json");
    safe_existing_path(&path)?;
    if !path.exists() {
        return Ok(());
    }
    let marker: Value = serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)?;
    if marker["state"] != "active" {
        return Ok(());
    }
    let name = marker["backup"]
        .as_str()
        .ok_or_else(|| io::Error::other("扩展恢复说明无效"))?;
    if !name.starts_with("backup-") || !safe_name(name) || name.contains('/') {
        return Err(io::Error::other("扩展恢复路径无效"));
    }
    let backup = parent.join(name);
    if marker["hadPrevious"] == true {
        let files = read_package(&backup)?;
        validate(&files)?;
        for (name, bytes) in files {
            let destination = target.join(name);
            safe_existing_path(&destination)?;
            let staged = parent
                .join(crate::security::random_prefixed_id("restore-"))
                .with_extension("tmp");
            fs::write(&staged, bytes)?;
            crate::local_connection::profile::atomic_replace(&staged, &destination)?;
        }
        installed(target)?.ok_or_else(|| io::Error::other("恢复后的扩展校验失败"))?;
    }
    transaction(
        target,
        &serde_json::json!({"version":1,"state":"rolledBack","backup":name}),
    )
}
pub fn import(source: &Path, target: &Path) -> io::Result<Metadata> {
    safe_existing_path(target)?;
    recover(target)?;
    let files = read_package(source)?;
    let metadata = validate(&files)?;
    let previous = installed(target)?;
    if target.exists() && previous.is_none() && fs::read_dir(target)?.next().is_some() {
        return Err(io::Error::other(
            "加载目录已有未知文件，已保留；请手动处理后重试",
        ));
    }
    let managed: Vec<String> = previous
        .as_ref()
        .map(|old| {
            old.files
                .keys()
                .cloned()
                .chain(Some(METADATA.into()))
                .collect()
        })
        .unwrap_or_default();
    for name in files.keys() {
        let destination = target.join(name);
        safe_existing_path(&destination)?;
        if destination.exists()
            && name != METADATA
            && previous
                .as_ref()
                .is_none_or(|old| !old.files.contains_key(name))
        {
            return Err(io::Error::other("目标文件不属于已管理扩展，已保留"));
        }
    }
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::other("扩展目录无效"))?;
    let backup = parent.join(crate::security::random_prefixed_id("backup-"));
    fs::create_dir_all(&backup)?;
    fs::create_dir_all(target)?;
    let mut existing = Vec::new();
    for name in &managed {
        let destination = target.join(name);
        if destination.is_file() {
            let saved = backup.join(name);
            fs::create_dir_all(saved.parent().unwrap())?;
            fs::copy(&destination, &saved)?;
            existing.push(name.clone());
        }
    }
    // Preserve the transaction and all backups. No recursive cleanup.
    fs::write(backup.join("transaction.json"), serde_json::to_vec(&serde_json::json!({
        "version":1,"target":target,"replaced":files.keys().collect::<Vec<_>>(),"existing":existing
    })).map_err(io::Error::other)?)?;
    transaction(
        target,
        &serde_json::json!({"version":1,"state":"active","backup":backup.file_name().unwrap().to_string_lossy(),"hadPrevious":previous.is_some()}),
    )?;
    let mut names: Vec<String> = files
        .keys()
        .filter(|name| *name != "manifest.json" && *name != METADATA)
        .cloned()
        .collect();
    names.push("manifest.json".into());
    names.push(METADATA.into());
    let result = (|| -> io::Result<()> {
        for name in names {
            let destination = target.join(&name);
            fs::create_dir_all(destination.parent().unwrap())?;
            let staged = parent
                .join(crate::security::random_prefixed_id("extension-file-"))
                .with_extension("tmp");
            fs::write(&staged, &files[&name])?;
            crate::local_connection::profile::atomic_replace(&staged, &destination)?;
        }
        installed(target)?.ok_or_else(|| io::Error::other("扩展准备校验失败"))?;
        Ok(())
    })();
    if let Err(error) = result {
        let restored = recover(target).is_ok();
        return Err(io::Error::other(format!(
            "扩展更新未完成；备份已保留，恢复状态={restored}：{error}"
        )));
    }
    transaction(
        target,
        &serde_json::json!({"version":1,"state":"committed","backup":backup.file_name().unwrap().to_string_lossy()}),
    )?;
    Ok(metadata)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsafe_paths_are_never_accepted() {
        for name in [
            "../x",
            "C:/x",
            "/x",
            "a\\b",
            "a//b",
            "a/../b",
            "manifest.json.",
            "x:stream",
        ] {
            assert!(!safe_name(name), "{name}");
        }
        assert!(safe_name("assets/图解.svg"));
    }
    #[test]
    fn arbitrary_source_archive_is_rejected() {
        assert!(validate(&BTreeMap::from([("manifest.json".into(), b"{}".to_vec())])).is_err());
    }
    fn fixture(content: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(crate::security::random_prefixed_id("extension-package-"));
        fs::create_dir_all(&directory).unwrap();
        let identity: Value = serde_json::from_str(super::super::IDENTITY).unwrap();
        let manifest = serde_json::json!({"manifest_version":3,"key":identity["key"],"version":"0.1.5","permissions":["nativeMessaging","storage","clipboardWrite"],"host_permissions":["https://chatgpt.com/*"],"background":{"service_worker":"background.js"},"action":{"default_popup":"popup.html"},"content_scripts":[{"matches":["https://chatgpt.com/*"],"js":["content.js"],"all_frames":false}]});
        let mut files = BTreeMap::from([(
            "manifest.json".to_owned(),
            serde_json::to_vec(&manifest).unwrap(),
        )]);
        for name in [
            "background.js",
            "content.js",
            "popup.js",
            "popup.html",
            "popup.css",
            "INSTALL.html",
            "INSTALL.svg",
        ] {
            files.insert(name.into(), content.as_bytes().to_vec());
        }
        let metadata = Metadata {
            schema_version: 1,
            version: "0.1.5".into(),
            protocol: 1,
            application: ">=0.1.5, <0.2.0".into(),
            extension_id: super::super::EXTENSION_ID.trim().into(),
            files: files
                .iter()
                .map(|(name, bytes)| (name.clone(), hash(bytes)))
                .collect(),
        };
        for (name, bytes) in files {
            fs::write(directory.join(name), bytes).unwrap();
        }
        fs::write(
            directory.join(METADATA),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
        directory
    }
    #[test]
    fn managed_import_hashes_upgrade_backups_and_interruption_recovery() {
        let old = fixture("old");
        let target = old
            .parent()
            .unwrap()
            .join(crate::security::random_prefixed_id("managed-"))
            .join("current");
        import(&old, &target).unwrap();
        let new = fixture("new");
        import(&new, &target).unwrap();
        assert_eq!(fs::read(target.join("background.js")).unwrap(), b"new");
        let backup = target.parent().unwrap().join("backup-recovery");
        fs::create_dir_all(&backup).unwrap();
        let managed = read_package(&target).unwrap();
        for (name, bytes) in managed {
            fs::write(backup.join(name), bytes).unwrap();
        }
        transaction(
            &target,
            &serde_json::json!({"state":"active","backup":"backup-recovery","hadPrevious":true}),
        )
        .unwrap();
        fs::write(target.join("background.js"), "interrupted").unwrap();
        fs::write(new.join("background.js"), "tampered").unwrap();
        assert!(import(&new, &target).is_err());
        assert_eq!(fs::read(target.join("background.js")).unwrap(), b"new");
        assert!(installed(&target).unwrap().is_some());
        assert!(backup.join("background.js").exists());
        fs::write(target.join("private-notes.txt"), "preserved").unwrap();
        import(&old, &target).unwrap();
        assert_eq!(
            fs::read(target.join("private-notes.txt")).unwrap(),
            b"preserved"
        );
    }
    #[test]
    fn zip_path_traversal_and_unmanaged_directory_are_preserved_and_rejected() {
        use std::io::Write;
        let source = fixture("safe");
        let zip_path = source.join("unsafe.zip");
        let mut archive = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
        archive
            .start_file("../escape", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"unsafe").unwrap();
        archive.finish().unwrap();
        assert!(read_package(&zip_path).is_err());
        let target = source.join("unmanaged");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("notes.txt"), "keep").unwrap();
        assert!(import(&source, &target).is_err());
        assert_eq!(fs::read(target.join("notes.txt")).unwrap(), b"keep");
    }
}
