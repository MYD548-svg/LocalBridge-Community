use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn durable_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceStamp {
    pub path: String,
    pub identity: String,
    pub permission: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pairing {
    instance: String,
    origin: String,
    secret_hash: String,
    workspace: WorkspaceStamp,
    approved: bool,
    revoked: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    state: String,
    instance: String,
    workspace: WorkspaceStamp,
    result_file: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Stored {
    version: u32,
    pairings: BTreeMap<String, Pairing>,
    requests: BTreeMap<String, Record>,
}
struct Binding {
    instance: String,
    chat: String,
    workspace: WorkspaceStamp,
    expires: u64,
    consumed: bool,
    busy: bool,
    last_seen: u64,
}
struct State {
    stored: Stored,
    grants: HashMap<String, Binding>,
    observed: Option<Option<WorkspaceStamp>>,
}
type WorkspaceSource = Arc<dyn Fn() -> Option<WorkspaceStamp> + Send + Sync>;
pub struct Authority {
    path: PathBuf,
    workspace: WorkspaceSource,
    state: Mutex<State>,
}
static AUTHORITY: OnceLock<Arc<Authority>> = OnceLock::new();
pub fn global() -> Option<&'static Arc<Authority>> {
    AUTHORITY.get()
}
pub fn install(authority: Arc<Authority>) -> io::Result<()> {
    AUTHORITY
        .set(authority)
        .map_err(|_| io::Error::other("browser authority already installed"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingSummary {
    pub instance: String,
    pub workspace: String,
    pub permission: String,
    pub approved: bool,
    pub revoked: bool,
    pub context: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observation {
    pub message: String,
    pub branch: String,
    pub index: u32,
}
pub enum Claim {
    New(String),
    Existing(Value),
}

impl Authority {
    pub fn open(path: PathBuf, workspace: WorkspaceSource) -> io::Result<Self> {
        let mut stored = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Stored>(&bytes).map_err(io::Error::other)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Stored {
                version: 1,
                ..Stored::default()
            },
            Err(error) => return Err(error),
        };
        if stored.version != 1 {
            return Err(io::Error::other("browser settings version unsupported"));
        }
        // Durable nonterminal entries are uncertain after a process restart.
        for record in stored.requests.values_mut() {
            if record.state == "submitted" || record.state == "running" {
                record.state = "unknown".into();
            }
        }
        let result = Self {
            path,
            workspace,
            state: Mutex::new(State {
                stored,
                grants: HashMap::new(),
                observed: None,
            }),
        };
        {
            let state = result.lock()?;
            result.save(&state)?;
        }
        Ok(result)
    }
    fn save(&self, state: &State) -> io::Result<()> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("browser state location"))?;
        fs::create_dir_all(parent)?;
        let staged = parent
            .join(crate::security::random_prefixed_id("browser-state-"))
            .with_extension("tmp");
        durable_write(
            &staged,
            &serde_json::to_vec(&state.stored).map_err(io::Error::other)?,
        )?;
        crate::local_connection::profile::atomic_replace(&staged, &self.path)
    }
    fn lock(&self) -> io::Result<std::sync::MutexGuard<'_, State>> {
        self.state
            .lock()
            .map_err(|_| io::Error::other("browser state lock"))
    }
    pub fn refresh_workspace(&self) -> io::Result<Option<WorkspaceStamp>> {
        let current = (self.workspace)();
        let mut state = self.lock()?;
        if state
            .observed
            .as_ref()
            .is_some_and(|previous| previous != &current)
        {
            state.grants.clear();
            for pairing in state.stored.pairings.values_mut() {
                pairing.approved = false;
            }
            self.save(&state)?;
        }
        state.observed = Some(current.clone());
        Ok(current)
    }
    pub fn pair(&self, instance: &str, secret: &str, origin: &str) -> io::Result<bool> {
        if instance.len() != 36
            || !instance.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
            || secret.len() != 64
            || !secret.bytes().all(|b| b.is_ascii_hexdigit())
            || !super::allowed_origin(origin)
        {
            return Err(io::Error::other("browser identity invalid"));
        }
        let workspace = self
            .refresh_workspace()?
            .ok_or_else(|| io::Error::other("local service or workspace not ready"))?;
        let mut state = self.lock()?;
        let digest = hash(secret.as_bytes());
        if let Some(pairing) = state.stored.pairings.get_mut(instance) {
            if pairing.secret_hash != digest || pairing.origin != origin {
                return Err(io::Error::other("pairing credential rejected"));
            }
            if pairing.revoked {
                return Err(io::Error::other("pairing revoked; reset in extension"));
            }
            if pairing.workspace != workspace {
                pairing.workspace = workspace;
                pairing.approved = false;
                self.save(&state)?;
            }
            return Ok(state.stored.pairings[instance].approved);
        }
        if state.stored.pairings.len() >= 16 {
            return Err(io::Error::other("pairing capacity"));
        }
        state.stored.pairings.insert(
            instance.into(),
            Pairing {
                instance: instance.into(),
                origin: origin.into(),
                secret_hash: digest,
                workspace,
                approved: false,
                revoked: false,
            },
        );
        self.save(&state)?;
        Ok(false)
    }
    pub fn approve(&self, instance: &str, context: &str) -> io::Result<()> {
        let workspace = self
            .refresh_workspace()?
            .ok_or_else(|| io::Error::other("local service not ready"))?;
        let mut state = self.lock()?;
        let pairing = state
            .stored
            .pairings
            .get_mut(instance)
            .ok_or_else(|| io::Error::other("pairing not found"))?;
        if pairing.revoked
            || pairing.workspace != workspace
            || context != hash(&serde_json::to_vec(&pairing.workspace).map_err(io::Error::other)?)
        {
            return Err(io::Error::other("pairing workspace changed"));
        }
        pairing.approved = true;
        self.save(&state)
    }
    pub fn revoke(&self, instance: &str) -> io::Result<()> {
        let mut state = self.lock()?;
        let pairing = state
            .stored
            .pairings
            .get_mut(instance)
            .ok_or_else(|| io::Error::other("pairing not found"))?;
        pairing.revoked = true;
        pairing.approved = false;
        state.grants.retain(|_, grant| grant.instance != instance);
        self.save(&state)
    }
    pub fn summaries(&self) -> io::Result<Vec<PairingSummary>> {
        let current = self.refresh_workspace()?;
        Ok(self
            .lock()?
            .stored
            .pairings
            .values()
            .map(|pair| PairingSummary {
                instance: pair.instance.clone(),
                workspace: pair.workspace.path.clone(),
                permission: pair.workspace.permission.clone(),
                approved: pair.approved && current.as_ref() == Some(&pair.workspace),
                revoked: pair.revoked,
                context: hash(
                    &serde_json::to_vec(&pair.workspace).expect("workspace stamp serialization"),
                ),
            })
            .collect())
    }
    pub fn grant(
        &self,
        instance: &str,
        secret: &str,
        origin: &str,
        chat: &str,
    ) -> io::Result<String> {
        if !self.pair(instance, secret, origin)? {
            return Err(io::Error::other("confirm pairing in LocalBridge"));
        }
        if chat.is_empty() || chat.len() > 200 {
            return Err(io::Error::other("chat identity invalid"));
        }
        let workspace = self
            .refresh_workspace()?
            .ok_or_else(|| io::Error::other("local service not ready"))?;
        let mut state = self.lock()?;
        let now = now_ms();
        state.grants.retain(|_, binding| {
            if binding.consumed {
                binding.busy || now.saturating_sub(binding.last_seen) < 600_000
            } else {
                binding.expires > now
            }
        });
        if state.grants.len() >= 4 {
            return Err(io::Error::other("browser session capacity"));
        }
        if state
            .grants
            .values()
            .any(|binding| binding.instance == instance && binding.chat == chat)
        {
            return Err(io::Error::other("this chat already owns a browser session"));
        }
        let token = crate::security::random_prefixed_id("browser-");
        state.grants.insert(
            token.clone(),
            Binding {
                instance: instance.into(),
                chat: chat.into(),
                workspace,
                expires: now + 30_000,
                consumed: false,
                busy: false,
                last_seen: now,
            },
        );
        Ok(token)
    }
    pub fn consume(&self, token: &str, workspace: &Path) -> io::Result<()> {
        let current = self
            .refresh_workspace()?
            .ok_or_else(|| io::Error::other("browser workspace not ready"))?;
        let mut state = self.lock()?;
        let binding = state
            .grants
            .get_mut(token)
            .ok_or_else(|| io::Error::other("browser grant rejected"))?;
        if binding.consumed
            || binding.expires < now_ms()
            || binding.workspace != current
            || Path::new(&current.path) != workspace
        {
            return Err(io::Error::other(
                "browser grant expired or workspace changed",
            ));
        }
        binding.consumed = true;
        Ok(())
    }
    pub fn valid(&self, token: &str, workspace: &Path) -> bool {
        let Ok(Some(current)) = self.refresh_workspace() else {
            return false;
        };
        let Ok(mut state) = self.lock() else {
            return false;
        };
        let Some(binding) = state.grants.get_mut(token) else {
            return false;
        };
        if !binding.consumed
            || binding.workspace != current
            || Path::new(&current.path) != workspace
        {
            return false;
        }
        binding.last_seen = now_ms();
        let instance = binding.instance.clone();
        state
            .stored
            .pairings
            .get(&instance)
            .is_some_and(|pair| pair.approved && !pair.revoked && pair.workspace == current)
    }
    pub fn valid_current(&self, token: &str) -> bool {
        self.refresh_workspace()
            .ok()
            .flatten()
            .is_some_and(|workspace| self.valid(token, Path::new(&workspace.path)))
    }
    pub fn claim(
        &self,
        token: &str,
        observation: &Observation,
        request: &Value,
    ) -> io::Result<Claim> {
        if observation.message.is_empty()
            || observation.message.len() > 200
            || observation.branch.is_empty()
            || observation.branch.len() > 200
            || observation.index >= 4
        {
            return Err(io::Error::other(
                "stable assistant message identity required",
            ));
        }
        let current = self
            .refresh_workspace()?
            .ok_or_else(|| io::Error::other("browser workspace not ready"))?;
        if !self.valid(token, Path::new(&current.path)) {
            return Err(io::Error::other("browser authorization revoked"));
        }
        let mut state = self.lock()?;
        let binding = state
            .grants
            .get(token)
            .ok_or_else(|| io::Error::other("browser session closed"))?;
        let instance = binding.instance.clone();
        let key = hash(
            serde_json::to_string(&json!([
                instance,
                current,
                binding.chat,
                observation.branch,
                observation.message,
                observation.index,
                request.get("params")
            ]))
            .map_err(io::Error::other)?
            .as_bytes(),
        );
        if let Some(record) = state.stored.requests.get(&key) {
            let result = record
                .result_file
                .as_ref()
                .and_then(|name| {
                    let safe = name.len() == 69
                        && name.ends_with(".json")
                        && name[..64].bytes().all(|b| b.is_ascii_hexdigit());
                    safe.then(|| {
                        self.path
                            .parent()
                            .unwrap()
                            .join("browser-results")
                            .join(name)
                    })
                })
                .and_then(|path| fs::read(path).ok())
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
            return Ok(Claim::Existing(
                json!({"state":record.state,"result":result}),
            ));
        }
        if binding.busy {
            return Err(io::Error::other("one tool call per chat"));
        }
        if state.stored.requests.len() >= 10_000 {
            return Err(io::Error::other("request journal capacity"));
        }
        state.grants.get_mut(token).unwrap().busy = true;
        state.stored.requests.insert(
            key.clone(),
            Record {
                state: "submitted".into(),
                instance,
                workspace: current,
                result_file: None,
            },
        );
        if let Err(error) = self.save(&state) {
            state.grants.get_mut(token).unwrap().busy = false;
            // Keep the uncertain in-memory entry; never execute without durable admission.
            return Err(error);
        }
        Ok(Claim::New(key))
    }
    pub fn finish(&self, token: &str, key: &str, result: &Value) -> io::Result<()> {
        let directory = self.path.parent().unwrap().join("browser-results");
        fs::create_dir_all(&directory)?;
        let file = format!("{key}.json");
        let staged = directory
            .join(crate::security::random_prefixed_id("result-"))
            .with_extension("tmp");
        durable_write(
            &staged,
            &serde_json::to_vec(result).map_err(io::Error::other)?,
        )?;
        crate::local_connection::profile::atomic_replace(&staged, &directory.join(&file))?;
        let mut state = self.lock()?;
        if let Some(binding) = state.grants.get_mut(token) {
            binding.busy = false;
        }
        let record = state
            .stored
            .requests
            .get_mut(key)
            .ok_or_else(|| io::Error::other("request journal missing"))?;
        record.state = if result.get("error").is_some()
            || result.pointer("/result/isError") == Some(&Value::Bool(true))
        {
            "failed"
        } else {
            "succeeded"
        }
        .into();
        record.result_file = Some(file);
        self.save(&state)
    }
    pub fn unknown(&self, token: &str, key: &str) {
        if let Ok(mut state) = self.lock() {
            if let Some(binding) = state.grants.get_mut(token) {
                binding.busy = false;
            }
            if let Some(record) = state.stored.requests.get_mut(key) {
                record.state = "unknown".into();
            }
            let _ = self.save(&state);
        }
    }
    pub fn close(&self, token: &str) {
        if let Ok(mut state) = self.lock() {
            state.grants.remove(token);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Authority, WorkspaceStamp) {
        let workspace = WorkspaceStamp {
            path: "C:\\测试".into(),
            identity: "volume-object".into(),
            permission: "edit".into(),
        };
        let stamp = workspace.clone();
        let path = std::env::temp_dir()
            .join(crate::security::random_prefixed_id("browser-authority-"))
            .join("state.json");
        (
            Authority::open(path, Arc::new(move || Some(stamp.clone()))).unwrap(),
            workspace,
        )
    }
    #[test]
    fn pairing_single_use_grant_revocation_and_durable_deduplication() {
        let (authority, workspace) = fixture();
        let instance = "11111111-1111-1111-1111-111111111111";
        let secret = "a".repeat(64);
        let origin = format!("chrome-extension://{}/", super::super::EXTENSION_ID.trim());
        assert!(!authority.pair(instance, &secret, &origin).unwrap());
        assert!(authority.grant(instance, &secret, &origin, "chat").is_err());
        authority
            .approve(instance, &authority.summaries().unwrap()[0].context)
            .unwrap();
        let token = authority.grant(instance, &secret, &origin, "chat").unwrap();
        authority
            .consume(&token, Path::new(&workspace.path))
            .unwrap();
        assert!(
            authority
                .consume(&token, Path::new(&workspace.path))
                .is_err()
        );
        let observation = Observation {
            message: "assistant".into(),
            branch: "root".into(),
            index: 0,
        };
        let request = json!({"params":{"name":"filesystem","arguments":{"action":"write"}}});
        let Claim::New(key) = authority.claim(&token, &observation, &request).unwrap() else {
            panic!("new request expected")
        };
        assert!(matches!(
            authority.claim(&token, &observation, &request).unwrap(),
            Claim::Existing(_)
        ));
        authority
            .finish(&token, &key, &json!({"result":{"isError":false}}))
            .unwrap();
        assert!(matches!(
            authority.claim(&token, &observation, &request).unwrap(),
            Claim::Existing(_)
        ));
        authority.revoke(instance).unwrap();
        assert!(!authority.valid(&token, Path::new(&workspace.path)));
        assert!(authority.claim(&token, &observation, &request).is_err());
    }
    #[test]
    fn workspace_change_stop_and_return_never_restore_old_grants() {
        let stamp = WorkspaceStamp {
            path: "C:\\one".into(),
            identity: "one".into(),
            permission: "edit".into(),
        };
        let source = Arc::new(Mutex::new(Some(stamp.clone())));
        let observed = source.clone();
        let path = std::env::temp_dir()
            .join(crate::security::random_prefixed_id("browser-epoch-"))
            .join("state.json");
        let authority =
            Authority::open(path, Arc::new(move || observed.lock().unwrap().clone())).unwrap();
        let instance = "11111111-1111-1111-1111-111111111111";
        let secret = "a".repeat(64);
        let origin = format!("chrome-extension://{}/", super::super::EXTENSION_ID.trim());
        authority.pair(instance, &secret, &origin).unwrap();
        authority
            .approve(instance, &authority.summaries().unwrap()[0].context)
            .unwrap();
        let token = authority.grant(instance, &secret, &origin, "chat").unwrap();
        authority.consume(&token, Path::new(&stamp.path)).unwrap();
        *source.lock().unwrap() = None;
        authority.refresh_workspace().unwrap();
        *source.lock().unwrap() = Some(stamp.clone());
        assert!(!authority.valid(&token, Path::new(&stamp.path)));
        assert!(authority.grant(instance, &secret, &origin, "chat").is_err());
        authority
            .approve(instance, &authority.summaries().unwrap()[0].context)
            .unwrap();
        let token = authority.grant(instance, &secret, &origin, "chat").unwrap();
        assert!(authority.grant(instance, &secret, &origin, "chat").is_err());
        assert!(authority.pair(instance, &"b".repeat(64), &origin).is_err());
        assert!(
            authority
                .pair(instance, &secret, "chrome-extension://wrong/")
                .is_err()
        );
        authority.close(&token);
    }
    #[test]
    fn restart_marks_unfinished_records_unknown_and_isolates_chats() {
        let (authority, workspace) = fixture();
        let instance = "11111111-1111-1111-1111-111111111111";
        let secret = "a".repeat(64);
        let origin = format!("chrome-extension://{}/", super::super::EXTENSION_ID.trim());
        authority.pair(instance, &secret, &origin).unwrap();
        authority
            .approve(instance, &authority.summaries().unwrap()[0].context)
            .unwrap();
        let observation = Observation {
            message: "assistant".into(),
            branch: "user".into(),
            index: 0,
        };
        let request = json!({"params":{"name":"filesystem","arguments":{"action":"read"}}});
        let mut tokens = Vec::new();
        for chat in ["a", "b", "c", "d"] {
            let token = authority.grant(instance, &secret, &origin, chat).unwrap();
            authority
                .consume(&token, Path::new(&workspace.path))
                .unwrap();
            assert!(matches!(
                authority.claim(&token, &observation, &request).unwrap(),
                Claim::New(_)
            ));
            tokens.push(token);
        }
        assert!(
            authority
                .grant(instance, &secret, &origin, "fifth")
                .is_err()
        );
        let path = authority.path.clone();
        drop(authority);
        let stamp = workspace.clone();
        let reopened = Authority::open(path, Arc::new(move || Some(stamp.clone()))).unwrap();
        let token = reopened.grant(instance, &secret, &origin, "a").unwrap();
        reopened
            .consume(&token, Path::new(&workspace.path))
            .unwrap();
        let Claim::Existing(previous) = reopened.claim(&token, &observation, &request).unwrap()
        else {
            panic!("restart must not replay");
        };
        assert_eq!(previous["state"], "unknown");
        assert!(previous["result"].is_null());
        let different = Observation {
            message: "other".into(),
            branch: "user".into(),
            index: 0,
        };
        assert!(matches!(
            reopened.claim(&token, &different, &request).unwrap(),
            Claim::New(_)
        ));
        let third = Observation {
            message: "third".into(),
            branch: "user".into(),
            index: 0,
        };
        assert!(reopened.claim(&token, &third, &request).is_err());
    }
}
