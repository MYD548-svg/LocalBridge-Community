use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::domain::McpSessionId;
pub(crate) const MAX_LOCAL_RETAINED_OUTPUT_HANDLES: usize = 8;
pub(crate) const MAX_PRIVATE_RETAINED_OUTPUT_HANDLES: usize = 256;

#[derive(Debug, Clone)]
enum OutputHandle {
    Private {
        private_output_ref: String,
        owner_public_session_id: String,
        stream: String,
    },
    Local {
        owner_session: McpSessionId,
        stream: String,
        content: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OutputOwner {
    PublicSession(String),
    McpSession(McpSessionId),
}

#[derive(Debug, Default)]
struct OutputHandleState {
    handles: HashMap<String, OutputHandle>,
    private_to_public: HashMap<(String, String, String), String>,
    private_order: VecDeque<String>,
    local_order: VecDeque<String>,
    primary: HashMap<String, String>,
}

impl OutputHandleState {
    pub(crate) fn public_for_private(
        &mut self,
        private_output_ref: &str,
        owner_public_session_id: &str,
        stream: &str,
    ) -> String {
        if let Some(public) = self.private_to_public.get(&(
            owner_public_session_id.into(),
            stream.into(),
            private_output_ref.into(),
        )) {
            return public.clone();
        }
        while self.private_order.len() >= MAX_PRIVATE_RETAINED_OUTPUT_HANDLES {
            if let Some(expired) = self.private_order.pop_front() {
                self.remove(&expired);
            }
        }
        let public = next_output_handle();
        self.private_to_public.insert(
            (
                owner_public_session_id.into(),
                stream.into(),
                private_output_ref.into(),
            ),
            public.clone(),
        );
        self.handles.insert(
            public.clone(),
            OutputHandle::Private {
                private_output_ref: private_output_ref.to_owned(),
                owner_public_session_id: owner_public_session_id.to_owned(),
                stream: stream.to_owned(),
            },
        );
        self.private_order.push_back(public.clone());
        public
    }

    pub(crate) fn retain_local(
        &mut self,
        owner_session: McpSessionId,
        stream: &str,
        content: String,
    ) -> String {
        while self.local_order.len() >= MAX_LOCAL_RETAINED_OUTPUT_HANDLES {
            if let Some(expired) = self.local_order.pop_front() {
                self.remove(&expired);
            }
        }
        let public = next_output_handle();
        self.handles.insert(
            public.clone(),
            OutputHandle::Local {
                owner_session,
                stream: stream.to_owned(),
                content,
            },
        );
        self.local_order.push_back(public.clone());
        public
    }

    pub(crate) fn private(&self, public_output_ref: &str) -> Option<String> {
        match self.handles.get(public_output_ref)? {
            OutputHandle::Private {
                private_output_ref, ..
            } => Some(private_output_ref.clone()),
            OutputHandle::Local { .. } => None,
        }
    }

    pub(crate) fn local(&self, public_output_ref: &str) -> Option<(String, String)> {
        match self.handles.get(public_output_ref)? {
            OutputHandle::Private { .. } => None,
            OutputHandle::Local {
                stream, content, ..
            } => Some((stream.clone(), content.clone())),
        }
    }

    pub(crate) fn stream(&self, public_output_ref: &str) -> Option<String> {
        match self.handles.get(public_output_ref)? {
            OutputHandle::Private { stream, .. } | OutputHandle::Local { stream, .. } => {
                Some(stream.clone())
            }
        }
    }

    pub(crate) fn owner(&self, public_output_ref: &str) -> Option<OutputOwner> {
        match self.handles.get(public_output_ref)? {
            OutputHandle::Private {
                owner_public_session_id,
                ..
            } => Some(OutputOwner::PublicSession(owner_public_session_id.clone())),
            OutputHandle::Local { owner_session, .. } => {
                Some(OutputOwner::McpSession(owner_session.clone()))
            }
        }
    }

    pub(crate) fn reap_owned_by(&mut self, public_sessions: &[String]) {
        let expired = self
            .handles
            .iter()
            .filter(|(_, handle)| match handle {
                OutputHandle::Private {
                    owner_public_session_id,
                    ..
                } => public_sessions.contains(owner_public_session_id),
                OutputHandle::Local { .. } => false,
            })
            .map(|(public, _)| public.clone())
            .collect::<Vec<_>>();
        for public in &expired {
            self.remove(public);
        }
        self.private_order
            .retain(|public| !expired.contains(public));
    }

    fn remove(&mut self, public_output_ref: &str) {
        if let Some(OutputHandle::Private {
            private_output_ref,
            owner_public_session_id,
            stream,
        }) = self.handles.remove(public_output_ref)
        {
            self.private_to_public
                .remove(&(owner_public_session_id, stream, private_output_ref));
            self.primary.retain(|_, value| value != public_output_ref);
        }
    }
}

/// Public and private references use the same typed shape, but only public values
/// leave the control plane. Clones share one bounded mapping registry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OutputReferences {
    pub(crate) primary: Option<String>,
    pub(crate) stdout: Option<String>,
    pub(crate) stderr: Option<String>,
}
impl OutputReferences {
    pub(crate) fn values(&self) -> Vec<String> {
        let mut values = Vec::new();
        for value in [&self.primary, &self.stdout, &self.stderr]
            .into_iter()
            .flatten()
        {
            if !values.contains(value) {
                values.push(value.clone());
            }
        }
        values
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct OutputHandleRegistry(Arc<Mutex<OutputHandleState>>);
impl OutputHandleRegistry {
    #[cfg(test)]
    pub(crate) fn public_for_private(&self, private: &str, owner: &str, stream: &str) -> String {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .public_for_private(private, owner, stream)
    }
    pub(crate) fn register(&self, owner: &str, refs: &OutputReferences) -> OutputReferences {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stdout = refs
            .stdout
            .as_deref()
            .map(|value| state.public_for_private(value, owner, "stdout"));
        let stderr = refs
            .stderr
            .as_deref()
            .map(|value| state.public_for_private(value, owner, "stderr"));
        // Adding the second stream can evict an older first-stream handle.
        // Never publish a reference that the same registration just expired.
        let stdout = stdout.filter(|value| state.handles.contains_key(value));
        let stderr = stderr.filter(|value| state.handles.contains_key(value));
        let primary = refs.primary.as_ref().and_then(|value| {
            if Some(value) == refs.stderr.as_ref() {
                stderr.clone()
            } else if Some(value) == refs.stdout.as_ref() {
                stdout.clone()
            } else {
                None
            }
        });
        if let Some(primary) = &primary {
            state.primary.insert(owner.into(), primary.clone());
        }
        OutputReferences {
            primary,
            stdout,
            stderr,
        }
    }
    pub(crate) fn for_session(&self, owner: &str) -> OutputReferences {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut refs = OutputReferences::default();
        for public in &state.private_order {
            if let Some(OutputHandle::Private {
                owner_public_session_id,
                stream,
                ..
            }) = state.handles.get(public)
            {
                if owner_public_session_id == owner {
                    match stream.as_str() {
                        "stdout" => refs.stdout = Some(public.clone()),
                        "stderr" => refs.stderr = Some(public.clone()),
                        _ => {}
                    }
                }
            }
        }
        refs.primary = state
            .primary
            .get(owner)
            .filter(|value| state.handles.contains_key(*value))
            .cloned()
            .or_else(|| refs.stdout.clone())
            .or_else(|| refs.stderr.clone());
        refs
    }
    pub(crate) fn retain_local(
        &self,
        owner: McpSessionId,
        stream: &str,
        content: String,
    ) -> String {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain_local(owner, stream, content)
    }
    pub(crate) fn private(&self, public: &str) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .private(public)
    }
    pub(crate) fn local(&self, public: &str) -> Option<(String, String)> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .local(public)
    }
    pub(crate) fn stream(&self, public: &str) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stream(public)
    }
    pub(crate) fn owner(&self, public: &str) -> Option<OutputOwner> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .owner(public)
    }
    pub(crate) fn reap_owned_by(&self, sessions: &[String]) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reap_owned_by(sessions);
    }
}

fn next_output_handle() -> String {
    crate::security::random_prefixed_id("lb-output-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_and_private_handles_have_independent_bounded_fifo_retention() {
        let registry = OutputHandleRegistry::default();
        let private_first = registry.public_for_private("private-0", "session-a", "stdout");
        for index in 1..=MAX_PRIVATE_RETAINED_OUTPUT_HANDLES {
            registry.public_for_private(&format!("private-{index}"), "session-a", "stdout");
        }
        assert!(registry.private(&private_first).is_none());
        assert_eq!(
            registry.0.lock().unwrap().private_order.len(),
            MAX_PRIVATE_RETAINED_OUTPUT_HANDLES
        );

        let local_first =
            registry.retain_local(McpSessionId::new("owner"), "stdout", "first".into());
        for index in 1..=MAX_LOCAL_RETAINED_OUTPUT_HANDLES {
            registry.retain_local(
                McpSessionId::new("owner"),
                "stderr",
                format!("local-{index}"),
            );
        }
        assert!(registry.local(&local_first).is_none());
        assert_eq!(
            registry.0.lock().unwrap().local_order.len(),
            MAX_LOCAL_RETAINED_OUTPUT_HANDLES
        );
    }

    #[test]
    fn private_handles_reap_with_their_public_session_owner() {
        let registry = OutputHandleRegistry::default();
        let a = registry.public_for_private("private-a", "session-a", "stdout");
        let b = registry.public_for_private("private-b", "session-b", "stderr");
        registry.reap_owned_by(&["session-a".into()]);
        assert!(registry.private(&a).is_none());
        assert_eq!(registry.private(&b).as_deref(), Some("private-b"));
    }
    #[test]
    fn concurrent_registration_shares_handles_and_retains_primary_stream() {
        let registry = OutputHandleRegistry::default();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let shared = registry.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    shared.register(
                        "owner",
                        &OutputReferences {
                            primary: Some("err".into()),
                            stdout: Some("out".into()),
                            stderr: Some("err".into()),
                        },
                    )
                })
            })
            .collect();
        barrier.wait();
        let mut results = workers.into_iter().map(|worker| worker.join().unwrap());
        let first = results.next().unwrap();
        assert_eq!(Some(first.clone()), results.next());
        assert_eq!(first.primary, first.stderr);
        assert_eq!(registry.for_session("owner"), first);
        assert_eq!(registry.for_session("owner"), first); // metadata is never consumed
        let other = registry.register(
            "another-owner",
            &OutputReferences {
                stderr: Some("err".into()),
                ..OutputReferences::default()
            },
        );
        assert_ne!(first.stderr, other.stderr);
        for index in 0..MAX_PRIVATE_RETAINED_OUTPUT_HANDLES {
            registry.register(
                "filler",
                &OutputReferences {
                    stdout: Some(format!("out-{index}")),
                    ..OutputReferences::default()
                },
            );
        }
        assert_eq!(registry.for_session("owner"), OutputReferences::default());
        assert_eq!(registry.for_session("owner"), OutputReferences::default());
        assert!(registry.private(first.stderr.as_ref().unwrap()).is_none());
        registry.reap_owned_by(&["filler".into()]);
        assert_eq!(registry.for_session("filler"), OutputReferences::default());
    }
    #[test]
    fn registering_a_second_stream_cannot_publish_an_evicted_primary() {
        let registry = OutputHandleRegistry::default();
        let old = registry.register(
            "owner",
            &OutputReferences {
                primary: Some("out".into()),
                stdout: Some("out".into()),
                stderr: None,
            },
        );
        for index in 1..MAX_PRIVATE_RETAINED_OUTPUT_HANDLES {
            registry.public_for_private(&format!("filler-{index}"), "filler", "stdout");
        }
        let added = registry.register(
            "owner",
            &OutputReferences {
                primary: Some("out".into()),
                stdout: Some("out".into()),
                stderr: Some("err".into()),
            },
        );
        assert!(added.stdout.is_none());
        assert!(added.primary.is_none());
        assert!(registry.private(old.primary.as_ref().unwrap()).is_none());
        let replay = registry.for_session("owner");
        assert!(replay.stdout.is_none());
        assert_eq!(replay.primary, replay.stderr);
    }
}
