use localbridge_lib::settings::{CURRENT_SETTINGS_SCHEMA_VERSION, MigrationError, SettingsStore};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_file(name: &str, contents: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "localbridge-lb003-migration-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, contents).unwrap();
    (dir, path)
}

#[test]
fn v1_migrates_sequentially_to_current_registry_and_active_reference() {
    let (dir, path) = temp_file("settings.json", include_str!("v1-single-workspace.json"));
    let data = SettingsStore::new(&path).load().unwrap();
    assert_eq!(data.schema_version, CURRENT_SETTINGS_SCHEMA_VERSION);
    assert_eq!(data.workspace.registry.entries().len(), 1);
    assert_eq!(
        data.workspace.active_entry().unwrap().workspace_id.as_str(),
        "legacy-one"
    );
    assert!(SettingsStore::new(&path).backup_path().exists());
    eprintln!("TEST_WORKSPACE_RETAINED path={}", dir.display());
}

#[test]
fn v2_migrates_to_current_without_skipping_version_contract() {
    let (dir, path) = temp_file("settings.json", include_str!("v2-single-workspace.json"));
    let data = SettingsStore::new(&path).load().unwrap();
    assert_eq!(data.schema_version, CURRENT_SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        data.workspace
            .active_entry()
            .unwrap()
            .validated_identity
            .as_str(),
        "validated:v2"
    );
    eprintln!("TEST_WORKSPACE_RETAINED path={}", dir.display());
}

#[test]
fn v3_migrates_close_window_policy_to_safe_continue_running_default() {
    let (dir, path) = temp_file("settings.json", include_str!("v3-close-policy.json"));
    let data = SettingsStore::new(&path).load().unwrap();
    assert_eq!(data.schema_version, CURRENT_SETTINGS_SCHEMA_VERSION);
    assert!(data.settings.close_window_continue_running);
    assert!(SettingsStore::new(&path).backup_path().exists());
    eprintln!("TEST_WORKSPACE_RETAINED path={}", dir.display());
}

#[test]
fn unvalidated_historical_workspace_is_preserved_as_pending_but_not_authorized() {
    let (dir, path) = temp_file(
        "settings.json",
        include_str!("v2-unvalidated-workspace.json"),
    );
    let data = SettingsStore::new(&path).load().unwrap();
    assert!(data.workspace.registry.entries().is_empty());
    assert!(data.workspace.active_workspace_id.is_none());
    assert!(data.workspace.pending_workspace_confirmation.is_some());
    eprintln!("TEST_WORKSPACE_RETAINED path={}", dir.display());
}

#[test]
fn future_schema_fails_safely_and_preserves_original_bytes() {
    let fixture = include_str!("future-v99.json");
    let (dir, path) = temp_file("settings.json", fixture);
    let before = fs::read(&path).unwrap();
    let error = SettingsStore::new(&path).load().unwrap_err();
    assert!(matches!(
        error,
        localbridge_lib::settings::SettingsStoreError::Migration(
            MigrationError::ConfigurationVersionUnsupported { found: 99, .. }
        )
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    eprintln!("TEST_WORKSPACE_RETAINED path={}", dir.display());
}

#[test]
fn failed_migration_preserves_original_data_and_does_not_reset() {
    let fixture = include_str!("invalid-v2.json");
    let (dir, path) = temp_file("settings.json", fixture);
    let before = fs::read(&path).unwrap();
    assert!(SettingsStore::new(&path).load().is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!SettingsStore::new(&path).backup_path().exists());
    eprintln!("TEST_WORKSPACE_RETAINED path={}", dir.display());
}

#[test]
fn v4_project_names_are_filled_once_without_changing_ids_paths_or_selection() {
    let fixture = serde_json::json!({
        "schema_version": 4,
        "settings": { "permission_mode": "Edit", "auto_start_services": false,
            "close_window_continue_running": true, "onboarding_complete": false },
        "workspace": { "registry": { "entries": [
            { "workspace_id": "one", "display_path": "D:/项目/毕业论文", "validated_identity": "identity-one", "last_opened_at": 1 },
            { "workspace_id": "two", "display_path": "D:/项目/软件", "validated_identity": "identity-two", "last_opened_at": 2 }
        ] }, "active_workspace_id": "one" }
    }).to_string();
    let (dir, path) = temp_file("settings.json", &fixture);
    let store = SettingsStore::new(&path);
    let mut data = store.load().unwrap();
    let one = data.workspace.active_entry().unwrap();
    assert_eq!(one.workspace_id.as_str(), "one");
    assert_eq!(one.display_path, PathBuf::from("D:/项目/毕业论文"));
    assert_eq!(one.display_name, "毕业论文");
    let original_backup = dir.join("settings.json.schema-4.bak");
    assert_eq!(fs::read_to_string(&original_backup).unwrap(), fixture);
    let id = one.workspace_id.clone();
    data.workspace.registry.rename(&id, "  论文研究  ").unwrap();
    store.save(&data).unwrap();
    assert_eq!(
        store
            .load()
            .unwrap()
            .workspace
            .active_entry()
            .unwrap()
            .display_name,
        "论文研究"
    );
    assert_eq!(fs::read_to_string(original_backup).unwrap(), fixture);
    eprintln!("TEST_WORKSPACE_RETAINED path={}", dir.display());
}
