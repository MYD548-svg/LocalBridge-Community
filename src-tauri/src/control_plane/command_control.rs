use crate::domain::{
    ExecutionRecord, ExecutionState, ExecutionTerminal, PublicSessionId, RpcRequestId, TaskId,
    TerminalOutcome,
};
use crate::execution::output_handles::OutputReferences;

use super::execution_registry::{ExecutionRegistry, ExecutionRegistryError};

pub(crate) const COMMAND_CONTROL_TRANSPORT_HEADROOM_MS: u64 = 1_000;
pub(crate) const COMMAND_CONTROL_UPSTREAM_HEADROOM_MS: u64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandControlAction {
    Poll,
    Write,
    Kill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandKillSignal {
    Term,
    Kill,
    Interrupt,
}

impl CommandKillSignal {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Term => "TERM",
            Self::Kill => "KILL",
            Self::Interrupt => "INT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCommandStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Lost,
}

impl RuntimeCommandStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Lost => "lost",
        }
    }

    const fn terminal_outcome(self) -> Option<TerminalOutcome> {
        match self {
            Self::Running => None,
            Self::Completed => Some(TerminalOutcome::Completed),
            Self::Failed => Some(TerminalOutcome::Failed),
            Self::Cancelled => Some(TerminalOutcome::Cancelled),
            Self::TimedOut => Some(TerminalOutcome::TimedOut),
            Self::Lost => Some(TerminalOutcome::Lost),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeCommandRequest {
    pub(crate) runtime_handle: String,
    pub(crate) action: CommandControlAction,
    pub(crate) chars: Option<String>,
    pub(crate) signal: Option<CommandKillSignal>,
    pub(crate) wait_ms: u64,
    pub(crate) request_id: RpcRequestId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeCommandObservation {
    pub(crate) status: RuntimeCommandStatus,
    pub(crate) exit_code: Option<i64>,
    pub(crate) signal: Option<String>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) truncated: Option<bool>,
    pub(crate) output_incomplete: bool,
    pub(crate) output_refs: OutputReferences,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandControlRequest {
    pub(crate) action: CommandControlAction,
    pub(crate) chars: Option<String>,
    pub(crate) signal: Option<CommandKillSignal>,
    pub(crate) wait_ms: u64,
    pub(crate) request_id: RpcRequestId,
    pub(crate) public_session_id: PublicSessionId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCommandControlError {
    InvalidRequest,
    SessionUnavailable,
    CapabilityMismatch,
    TimedOut,
    Unavailable,
}

pub(crate) trait RuntimeCommandControl {
    fn control_command(
        &self,
        request: &RuntimeCommandRequest,
    ) -> Result<RuntimeCommandObservation, RuntimeCommandControlError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandControlResult {
    pub(crate) status: RuntimeCommandStatus,
    pub(crate) public_session_id: PublicSessionId,
    pub(crate) task_id: TaskId,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) elapsed_ms: u64,
    pub(crate) exit_code: Option<i64>,
    pub(crate) signal: Option<String>,
    pub(crate) truncated: Option<bool>,
    pub(crate) output_incomplete: bool,
    pub(crate) output_refs: OutputReferences,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandControlError {
    InvalidRequest,
    SessionUnavailable,
    RuntimeUnavailable,
    RuntimeCapabilityMismatch,
    OperationTimedOut,
    ExecutionConflict,
}

pub(crate) fn control_command_during_work(
    request: CommandControlRequest,
    executions: &ExecutionRegistry,
    runtime: &dyn RuntimeCommandControl,
) -> Result<CommandControlResult, CommandControlError> {
    let mut result = collect_command_during_work(request, executions, runtime)?;
    result.output_refs = executions.command_output_refs(&result.public_session_id);
    Ok(result)
}

fn collect_command_during_work(
    request: CommandControlRequest,
    executions: &ExecutionRegistry,
    runtime: &dyn RuntimeCommandControl,
) -> Result<CommandControlResult, CommandControlError> {
    if request.action == CommandControlAction::Write
        && request.chars.as_deref().is_none_or(str::is_empty)
    {
        return Err(CommandControlError::InvalidRequest);
    }
    let output = executions
        .command_output(&request.public_session_id)
        .ok_or(CommandControlError::SessionUnavailable)?;
    let collection = output.begin();
    let execution = executions
        .execution_for_public_session(&request.public_session_id)
        .ok_or(CommandControlError::SessionUnavailable)?;
    if let ExecutionState::Terminal(terminal) = &execution.state {
        return if request.action == CommandControlAction::Poll {
            drop(collection);
            deliver_output(
                result_from_terminal(&execution, terminal),
                &output,
                request.action,
            )
        } else {
            Err(CommandControlError::SessionUnavailable)
        };
    }
    let runtime_handle = execution
        .runtime_handle
        .as_ref()
        .ok_or(CommandControlError::SessionUnavailable)?;
    if request.action == CommandControlAction::Kill {
        executions
            .request_cancellation(
                &request.public_session_id,
                request.signal.unwrap_or(CommandKillSignal::Term).as_str(),
            )
            .map_err(map_execution_error)?;
    }
    let observation = match runtime.control_command(&RuntimeCommandRequest {
        runtime_handle: runtime_handle.as_str().to_string(),
        action: request.action,
        chars: request.chars,
        signal: request.signal,
        wait_ms: request.wait_ms.min(30_000),
        request_id: request.request_id,
    }) {
        Ok(observation) => observation,
        Err(error) => {
            let cancellation_signal = executions.cancellation_signal(&request.public_session_id);
            if error == RuntimeCommandControlError::SessionUnavailable
                && cancellation_signal.is_some()
            {
                let terminal = ExecutionTerminal {
                    outcome: TerminalOutcome::Cancelled,
                    exit_code: None,
                    signal: cancellation_signal,
                    output_refs: executions
                        .command_output_refs(&request.public_session_id)
                        .values(),
                    error_code: Some("ProcessCancelled".to_string()),
                    completed_at_ms: unix_time_ms(),
                };
                match executions.finish(&execution.id, terminal) {
                    Ok(()) | Err(ExecutionRegistryError::AlreadyTerminal { .. }) => {}
                    Err(error) => return Err(map_execution_error(error)),
                }
                let settled = executions
                    .execution_for_public_session(&request.public_session_id)
                    .ok_or(CommandControlError::SessionUnavailable)?;
                let ExecutionState::Terminal(terminal) = &settled.state else {
                    return Err(CommandControlError::ExecutionConflict);
                };
                drop(collection);
                return deliver_output(
                    result_from_terminal(&settled, terminal),
                    &output,
                    request.action,
                );
            }
            if matches!(
                error,
                RuntimeCommandControlError::InvalidRequest
                    | RuntimeCommandControlError::CapabilityMismatch
            ) {
                executions.clear_cancellation(&request.public_session_id);
            }
            return Err(map_runtime_error(error));
        }
    };

    executions
        .register_command_output_refs(&request.public_session_id, &observation.output_refs)
        .map_err(map_execution_error)?;
    let stderr = output.filter_stderr(&observation.stderr);
    let combined = [observation.stdout.as_str(), stderr.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(if observation.stdout.is_empty() || stderr.is_empty() {
            ""
        } else {
            "\n"
        });
    output.append(&combined);
    output.annotate(
        observation.truncated.unwrap_or(false),
        observation.output_incomplete,
    );
    let cancellation_signal = executions.cancellation_signal(&request.public_session_id);
    if request.action == CommandControlAction::Kill
        && observation.status == RuntimeCommandStatus::Running
    {
        // The cancellation request is accepted and remains owned by the
        // ExecutionRegistry, but no terminal process fact was observed within
        // this call's wait budget. Returning `running` would make the kill
        // response contradict that accepted intent and skip the caller's
        // terminal poll path.
        return Err(CommandControlError::OperationTimedOut);
    }
    let terminal_outcome = observation.status.terminal_outcome().map(|outcome| {
        if cancellation_signal.is_some() {
            TerminalOutcome::Cancelled
        } else {
            outcome
        }
    });
    if let Some(outcome) = terminal_outcome {
        let finish_result = executions.finish(
            &execution.id,
            ExecutionTerminal {
                outcome,
                exit_code: observation.exit_code,
                signal: observation
                    .signal
                    .clone()
                    .or_else(|| cancellation_signal.clone()),
                output_refs: executions
                    .command_output_refs(&request.public_session_id)
                    .values(),
                error_code: terminal_error_code(outcome).map(str::to_string),
                completed_at_ms: unix_time_ms(),
            },
        );
        executions
            .enrich_terminal_output_refs(&execution.id)
            .map_err(map_execution_error)?;
        match finish_result {
            Ok(()) => {}
            Err(ExecutionRegistryError::AlreadyTerminal { .. }) => {
                // A concurrent control call observed the same process exit and
                // finalized this Execution first — the durable terminal of this
                // same execution is already recorded, so the poll answers with
                // that recorded outcome instead of a fabricated terminal-state
                // conflict. The observation still carries the incremental
                // output this caller has not seen yet, so the replay reports
                // the durable outcome together with the observed output rather
                // than an output-less envelope.
                let settled = executions
                    .execution_for_public_session(&request.public_session_id)
                    .ok_or(CommandControlError::SessionUnavailable)?;
                let ExecutionState::Terminal(terminal) = &settled.state else {
                    return Err(CommandControlError::ExecutionConflict);
                };
                let replayed = result_from_terminal(&settled, terminal);
                drop(collection);
                return deliver_output(replayed, &output, request.action);
            }
            Err(error) => return Err(map_execution_error(error)),
        }
    }

    drop(collection);
    deliver_output(
        CommandControlResult {
            status: if terminal_outcome == Some(TerminalOutcome::Cancelled) {
                RuntimeCommandStatus::Cancelled
            } else {
                observation.status
            },
            public_session_id: request.public_session_id,
            task_id: execution.task_id,
            stdout: String::new(),
            stderr: String::new(),
            elapsed_ms: unix_time_ms().saturating_sub(execution.started_at_ms),
            exit_code: observation.exit_code,
            signal: observation.signal.or(cancellation_signal),
            truncated: observation.truncated,
            output_incomplete: false,
            output_refs: OutputReferences::default(),
        },
        &output,
        request.action,
    )
}

fn deliver_output(
    mut result: CommandControlResult,
    output: &super::command_output::CommandOutput,
    action: CommandControlAction,
) -> Result<CommandControlResult, CommandControlError> {
    let delivery = output.take();
    if delivery.collecting && action == CommandControlAction::Kill {
        return Err(CommandControlError::OperationTimedOut);
    }
    result.stdout = delivery.output;
    result.stderr.clear();
    result.truncated = Some(delivery.truncated);
    result.output_incomplete = delivery.incomplete;
    if delivery.collecting {
        result.status = RuntimeCommandStatus::Running;
        result.exit_code = None;
        result.signal = None;
    }
    Ok(result)
}

fn result_from_terminal(
    execution: &ExecutionRecord,
    terminal: &ExecutionTerminal,
) -> CommandControlResult {
    CommandControlResult {
        status: match terminal.outcome {
            TerminalOutcome::Completed => RuntimeCommandStatus::Completed,
            TerminalOutcome::Failed | TerminalOutcome::Blocked => RuntimeCommandStatus::Failed,
            TerminalOutcome::Cancelled => RuntimeCommandStatus::Cancelled,
            TerminalOutcome::TimedOut => RuntimeCommandStatus::TimedOut,
            TerminalOutcome::Lost => RuntimeCommandStatus::Lost,
        },
        public_session_id: execution.public_session_id.clone(),
        task_id: execution.task_id.clone(),
        stdout: String::new(),
        stderr: String::new(),
        elapsed_ms: terminal
            .completed_at_ms
            .saturating_sub(execution.started_at_ms),
        exit_code: terminal.exit_code,
        signal: terminal.signal.clone(),
        truncated: None,
        output_incomplete: false,
        output_refs: OutputReferences::default(),
    }
}

fn terminal_error_code(outcome: TerminalOutcome) -> Option<&'static str> {
    match outcome {
        TerminalOutcome::Completed => None,
        TerminalOutcome::Cancelled => Some("ProcessCancelled"),
        TerminalOutcome::TimedOut => Some("ProcessTimedOut"),
        TerminalOutcome::Lost => Some("SessionUnavailable"),
        TerminalOutcome::Failed | TerminalOutcome::Blocked => Some("ProcessFailed"),
    }
}

fn map_runtime_error(error: RuntimeCommandControlError) -> CommandControlError {
    match error {
        RuntimeCommandControlError::InvalidRequest => CommandControlError::InvalidRequest,
        RuntimeCommandControlError::SessionUnavailable => CommandControlError::SessionUnavailable,
        RuntimeCommandControlError::CapabilityMismatch => {
            CommandControlError::RuntimeCapabilityMismatch
        }
        RuntimeCommandControlError::TimedOut => CommandControlError::OperationTimedOut,
        RuntimeCommandControlError::Unavailable => CommandControlError::RuntimeUnavailable,
    }
}

fn map_execution_error(error: ExecutionRegistryError) -> CommandControlError {
    match error {
        ExecutionRegistryError::AlreadyTerminal { .. } => CommandControlError::ExecutionConflict,
        ExecutionRegistryError::UnknownExecution(_) => CommandControlError::SessionUnavailable,
        _ => CommandControlError::RuntimeUnavailable,
    }
}

fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::control_plane::execution_registry::ExecutionRegistry;
    use crate::domain::{McpSessionId, RuntimeCommandHandle};

    #[derive(Debug)]
    struct FakeRuntime(Mutex<Option<RuntimeCommandObservation>>);

    impl RuntimeCommandControl for FakeRuntime {
        fn control_command(
            &self,
            _request: &RuntimeCommandRequest,
        ) -> Result<RuntimeCommandObservation, RuntimeCommandControlError> {
            self.0
                .lock()
                .expect("fake runtime lock")
                .take()
                .ok_or(RuntimeCommandControlError::Unavailable)
        }
    }

    #[derive(Debug)]
    struct TimedOutRuntime;

    impl RuntimeCommandControl for TimedOutRuntime {
        fn control_command(
            &self,
            _request: &RuntimeCommandRequest,
        ) -> Result<RuntimeCommandObservation, RuntimeCommandControlError> {
            Err(RuntimeCommandControlError::TimedOut)
        }
    }

    #[derive(Debug)]
    struct DisappearedRuntime;

    impl RuntimeCommandControl for DisappearedRuntime {
        fn control_command(
            &self,
            _request: &RuntimeCommandRequest,
        ) -> Result<RuntimeCommandObservation, RuntimeCommandControlError> {
            Err(RuntimeCommandControlError::SessionUnavailable)
        }
    }

    #[test]
    fn collection_blocks_terminal_delivery_without_consuming_bytes_or_blocking_kill() {
        let output = super::super::command_output::CommandOutput::default();
        let collection = output.begin();
        output.append("UNDELIVERED");
        output.annotate(false, true);
        let terminal = CommandControlResult {
            status: RuntimeCommandStatus::Cancelled,
            public_session_id: PublicSessionId::new("public-collection"),
            task_id: TaskId::new("task-collection"),
            stdout: String::new(),
            stderr: String::new(),
            elapsed_ms: 0,
            exit_code: Some(0),
            signal: Some("KILL".into()),
            truncated: None,
            output_incomplete: false,
            output_refs: OutputReferences::default(),
        };
        let polled = deliver_output(terminal.clone(), &output, CommandControlAction::Poll).unwrap();
        assert_eq!(polled.status, RuntimeCommandStatus::Running);
        assert!(polled.stdout.is_empty());
        assert_eq!(polled.exit_code, None);
        assert_eq!(
            deliver_output(terminal.clone(), &output, CommandControlAction::Kill),
            Err(CommandControlError::OperationTimedOut)
        );
        drop(collection);
        let final_output =
            deliver_output(terminal.clone(), &output, CommandControlAction::Poll).unwrap();
        assert_eq!(final_output.status, RuntimeCommandStatus::Cancelled);
        assert_eq!(final_output.stdout, "UNDELIVERED");
        assert!(final_output.output_incomplete);
        assert_eq!(final_output.truncated, Some(true));
        let replay = deliver_output(terminal, &output, CommandControlAction::Poll).unwrap();
        assert!(replay.stdout.is_empty());
        assert!(replay.output_incomplete);
    }

    #[test]
    fn terminal_observation_is_committed_by_control_plane_owner() {
        let root = std::env::temp_dir().join(format!(
            "localbridge-command-control-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        std::fs::create_dir_all(&root).expect("test workspace");
        let registry =
            ExecutionRegistry::open_at(root.join("executions.json")).expect("execution registry");
        let public_session = PublicSessionId::new("public-1");
        let execution_id = registry
            .start(TaskId::new("task-1"), public_session.clone())
            .expect("start execution");
        registry
            .bind_owner(&execution_id, McpSessionId::new("mcp-1"))
            .expect("bind owner");
        registry
            .bind_runtime_handle(&execution_id, RuntimeCommandHandle::new("private-1"))
            .expect("bind runtime handle");
        let runtime = FakeRuntime(Mutex::new(Some(RuntimeCommandObservation {
            // Windows KILL may be reported by the runtime as a non-zero process
            // exit. The ControlPlane-owned cancellation intent, not that adapter
            // spelling, determines the domain terminal outcome.
            status: RuntimeCommandStatus::Failed,
            exit_code: Some(1),
            signal: None,
            stdout: String::new(),
            stderr: String::new(),
            truncated: Some(false),
            output_incomplete: false,
            output_refs: OutputReferences::default(),
        })));

        let result = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Kill,
                chars: None,
                signal: Some(CommandKillSignal::Kill),
                wait_ms: 100,
                request_id: RpcRequestId::String("private-request".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &runtime,
        )
        .expect("control result");
        assert_eq!(result.status, RuntimeCommandStatus::Cancelled);
        assert!(matches!(
            registry
                .execution_for_public_session(&public_session)
                .expect("execution")
                .state,
            ExecutionState::Terminal(ExecutionTerminal {
                outcome: TerminalOutcome::Cancelled,
                ..
            })
        ));
        eprintln!("TEST_WORKSPACE_RETAINED path={}", root.display());
    }

    #[test]
    fn kill_timeout_preserves_intent_until_poll_observes_one_cancelled_terminal() {
        let root = std::env::temp_dir().join(format!(
            "localbridge-command-control-timeout-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        std::fs::create_dir_all(&root).expect("test workspace");
        let registry =
            ExecutionRegistry::open_at(root.join("executions.json")).expect("execution registry");
        let public_session = PublicSessionId::new("public-timeout");
        let execution_id = registry
            .start(TaskId::new("task-timeout"), public_session.clone())
            .expect("start execution");
        registry
            .bind_runtime_handle(&execution_id, RuntimeCommandHandle::new("private-timeout"))
            .expect("bind runtime handle");

        let timed_out = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Kill,
                chars: None,
                signal: Some(CommandKillSignal::Kill),
                wait_ms: 0,
                request_id: RpcRequestId::String("kill-timeout".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &TimedOutRuntime,
        );
        assert_eq!(timed_out, Err(CommandControlError::OperationTimedOut));
        assert_eq!(
            registry.cancellation_signal(&public_session).as_deref(),
            Some("KILL")
        );

        let polled = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Poll,
                chars: None,
                signal: None,
                wait_ms: 0,
                request_id: RpcRequestId::String("poll-after-timeout".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &FakeRuntime(Mutex::new(Some(RuntimeCommandObservation {
                status: RuntimeCommandStatus::Failed,
                exit_code: Some(1),
                signal: None,
                stdout: String::new(),
                stderr: String::new(),
                truncated: Some(false),
                output_incomplete: false,
                output_refs: OutputReferences::default(),
            }))),
        )
        .expect("poll result");
        assert_eq!(polled.status, RuntimeCommandStatus::Cancelled);
        assert!(matches!(
            registry
                .execution_for_public_session(&public_session)
                .expect("execution")
                .state,
            ExecutionState::Terminal(ExecutionTerminal {
                outcome: TerminalOutcome::Cancelled,
                ..
            })
        ));
        assert_eq!(registry.cancellation_signal(&public_session), None);
        eprintln!("TEST_WORKSPACE_RETAINED path={}", root.display());
    }

    #[test]
    fn accepted_kill_without_terminal_observation_returns_timeout_not_running() {
        let root = std::env::temp_dir().join(format!(
            "localbridge-command-control-running-kill-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        std::fs::create_dir_all(&root).expect("test workspace");
        let registry =
            ExecutionRegistry::open_at(root.join("executions.json")).expect("execution registry");
        let public_session = PublicSessionId::new("public-running-kill");
        let execution_id = registry
            .start(TaskId::new("task-running-kill"), public_session.clone())
            .expect("start execution");
        registry
            .bind_runtime_handle(
                &execution_id,
                RuntimeCommandHandle::new("private-running-kill"),
            )
            .expect("bind runtime handle");

        let result = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Kill,
                chars: None,
                signal: Some(CommandKillSignal::Kill),
                wait_ms: 0,
                request_id: RpcRequestId::String("kill-running".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &FakeRuntime(Mutex::new(Some(RuntimeCommandObservation {
                status: RuntimeCommandStatus::Running,
                exit_code: None,
                signal: None,
                stdout: String::new(),
                stderr: String::new(),
                truncated: Some(false),
                output_incomplete: false,
                output_refs: OutputReferences::default(),
            }))),
        );

        assert_eq!(result, Err(CommandControlError::OperationTimedOut));
        assert_eq!(
            registry.cancellation_signal(&public_session).as_deref(),
            Some("KILL")
        );
        assert!(matches!(
            registry
                .execution_for_public_session(&public_session)
                .expect("execution")
                .state,
            ExecutionState::Running
        ));
        eprintln!("TEST_WORKSPACE_RETAINED path={}", root.display());
    }

    #[test]
    fn accepted_cancellation_wins_when_the_runtime_session_disappears_before_poll() {
        let root = std::env::temp_dir().join(format!(
            "localbridge-command-control-disappeared-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        std::fs::create_dir_all(&root).expect("test workspace");
        let registry =
            ExecutionRegistry::open_at(root.join("executions.json")).expect("execution registry");
        let public_session = PublicSessionId::new("public-disappeared");
        let execution_id = registry
            .start(TaskId::new("task-disappeared"), public_session.clone())
            .expect("start execution");
        registry
            .bind_runtime_handle(
                &execution_id,
                RuntimeCommandHandle::new("private-disappeared"),
            )
            .expect("bind runtime handle");
        registry
            .request_cancellation(&public_session, "KILL")
            .expect("accept cancellation intent");

        let polled = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Poll,
                chars: None,
                signal: None,
                wait_ms: 0,
                request_id: RpcRequestId::String("poll-disappeared".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &DisappearedRuntime,
        )
        .expect("a disappeared cancelled session has one terminal outcome");

        assert_eq!(polled.status, RuntimeCommandStatus::Cancelled);
        assert_eq!(polled.signal.as_deref(), Some("KILL"));
        assert!(matches!(
            registry
                .execution_for_public_session(&public_session)
                .expect("execution")
                .state,
            ExecutionState::Terminal(ExecutionTerminal {
                outcome: TerminalOutcome::Cancelled,
                ..
            })
        ));
        assert_eq!(registry.cancellation_signal(&public_session), None);
        eprintln!("TEST_WORKSPACE_RETAINED path={}", root.display());
    }

    /// Deterministic reproduction of the terminal-state finish race: a
    /// concurrent control call finalizes the Execution between this poll's
    /// registry read and its own `finish`, so this poll's `finish` hits
    /// `AlreadyTerminal`. The poll must answer with the durable terminal of
    /// the same execution — never a fabricated conflict, never the racing
    /// observation's conflicting outcome.
    #[derive(Debug)]
    struct ConcurrentFinalizerRuntime {
        registry: ExecutionRegistry,
        public_session: PublicSessionId,
    }

    impl RuntimeCommandControl for ConcurrentFinalizerRuntime {
        fn control_command(
            &self,
            _request: &RuntimeCommandRequest,
        ) -> Result<RuntimeCommandObservation, RuntimeCommandControlError> {
            let execution = self
                .registry
                .execution_for_public_session(&self.public_session)
                .expect("racing execution still registered");
            self.registry
                .finish(
                    &execution.id,
                    ExecutionTerminal {
                        outcome: TerminalOutcome::Cancelled,
                        exit_code: None,
                        signal: Some("KILL".into()),
                        output_refs: Vec::new(),
                        error_code: Some("ProcessCancelled".into()),
                        completed_at_ms: unix_time_ms(),
                    },
                )
                .expect("the concurrent observer wins the finish race");
            Ok(RuntimeCommandObservation {
                status: RuntimeCommandStatus::Failed,
                exit_code: Some(1),
                signal: None,
                stdout: "RACING_OBSERVED_OUTPUT".to_string(),
                stderr: String::new(),
                truncated: Some(false),
                output_incomplete: false,
                output_refs: OutputReferences {
                    primary: Some("late-stderr".into()),
                    stdout: Some("late-stdout".into()),
                    stderr: Some("late-stderr".into()),
                },
            })
        }
    }

    #[test]
    fn poll_returns_the_durable_terminal_when_a_concurrent_call_won_the_finish_race() {
        let root = std::env::temp_dir().join(format!(
            "localbridge-command-control-finish-race-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        std::fs::create_dir_all(&root).expect("test workspace");
        let registry =
            ExecutionRegistry::open_at(root.join("executions.json")).expect("execution registry");
        let public_session = PublicSessionId::new("public-finish-race");
        let execution_id = registry
            .start(TaskId::new("task-finish-race"), public_session.clone())
            .expect("start execution");
        registry
            .bind_runtime_handle(
                &execution_id,
                RuntimeCommandHandle::new("private-finish-race"),
            )
            .expect("bind runtime handle");
        registry
            .request_cancellation(&public_session, "KILL")
            .expect("accept cancellation intent");

        let polled = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Poll,
                chars: None,
                signal: None,
                wait_ms: 0,
                request_id: RpcRequestId::String("poll-finish-race".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &ConcurrentFinalizerRuntime {
                registry: registry.clone(),
                public_session: public_session.clone(),
            },
        )
        .expect("a poll that loses the finish race replays the durable terminal");

        assert_eq!(polled.status, RuntimeCommandStatus::Cancelled);
        // The replay must carry the observed incremental output: the caller has
        // not seen it yet, and dropping it breaks output-accumulating consumers
        // (the schema27/r1 terminal-poll failures on 37659cc).
        assert_eq!(polled.stdout, "RACING_OBSERVED_OUTPUT");
        assert!(
            polled
                .output_refs
                .stderr
                .as_deref()
                .unwrap()
                .starts_with("lb-output-")
        );
        assert_eq!(polled.output_refs.primary, polled.output_refs.stderr);
        let saved = registry
            .execution_for_public_session(&public_session)
            .unwrap();
        let ExecutionState::Terminal(saved) = saved.state else {
            panic!("terminal");
        };
        assert!(
            saved
                .output_refs
                .contains(polled.output_refs.stderr.as_ref().unwrap())
        );
        let replay = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Poll,
                chars: None,
                signal: None,
                wait_ms: 0,
                request_id: RpcRequestId::String("replay".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &TimedOutRuntime,
        )
        .unwrap();
        assert_eq!(replay.output_refs, polled.output_refs);
        assert!(replay.stdout.is_empty());
        assert!(matches!(
            registry
                .execution_for_public_session(&public_session)
                .expect("execution")
                .state,
            ExecutionState::Terminal(ExecutionTerminal {
                outcome: TerminalOutcome::Cancelled,
                ..
            })
        ));
        eprintln!("TEST_WORKSPACE_RETAINED path={}", root.display());
    }

    /// The PEP maps transport outages (connection refused and friends) to
    /// `RuntimeCommandControlError::Unavailable`. A kill whose upstream call
    /// fails that way must NOT combine with the recorded cancellation intent
    /// into any terminal: intent, delivery and process termination are
    /// different facts. The intent survives the outage and the next real
    /// observation resolves the terminal per contract.
    #[derive(Debug)]
    struct TransportOutageRuntime;

    impl RuntimeCommandControl for TransportOutageRuntime {
        fn control_command(
            &self,
            _request: &RuntimeCommandRequest,
        ) -> Result<RuntimeCommandObservation, RuntimeCommandControlError> {
            Err(RuntimeCommandControlError::Unavailable)
        }
    }

    #[test]
    fn kill_intent_survives_a_transport_outage_until_the_runtime_reports_the_real_terminal() {
        let root = std::env::temp_dir().join(format!(
            "localbridge-command-control-outage-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        std::fs::create_dir_all(&root).expect("test workspace");
        let registry =
            ExecutionRegistry::open_at(root.join("executions.json")).expect("execution registry");
        let public_session = PublicSessionId::new("public-transport-outage");
        let execution_id = registry
            .start(TaskId::new("task-transport-outage"), public_session.clone())
            .expect("start execution");
        registry
            .bind_runtime_handle(
                &execution_id,
                RuntimeCommandHandle::new("private-transport-outage"),
            )
            .expect("bind runtime handle");
        registry
            .request_cancellation(&public_session, "KILL")
            .expect("record the cancellation intent before the outage");

        let killed = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Kill,
                chars: None,
                signal: Some(CommandKillSignal::Kill),
                wait_ms: 0,
                request_id: RpcRequestId::String("kill-outage".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &TransportOutageRuntime,
        );
        assert_eq!(
            killed,
            Err(CommandControlError::RuntimeUnavailable),
            "a transport outage must surface as the retryable error, not a fabricated terminal"
        );
        assert_eq!(
            registry.cancellation_signal(&public_session).as_deref(),
            Some("KILL"),
            "the outage must not consume the recorded cancel intent"
        );
        assert!(matches!(
            registry
                .execution_for_public_session(&public_session)
                .expect("execution")
                .state,
            ExecutionState::Running
        ));

        // The outage passes; the next poll observes the process's REAL exit.
        // The accepted cancellation intent maps the outcome to Cancelled, and
        // the observed output must ride along instead of being lost.
        let polled = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Poll,
                chars: None,
                signal: None,
                wait_ms: 0,
                request_id: RpcRequestId::String("poll-after-outage".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &FakeRuntime(Mutex::new(Some(RuntimeCommandObservation {
                status: RuntimeCommandStatus::Failed,
                exit_code: Some(1),
                signal: None,
                stdout: "REAL_TERMINAL_OUTPUT".to_string(),
                stderr: String::new(),
                truncated: Some(false),
                output_incomplete: false,
                output_refs: OutputReferences::default(),
            }))),
        )
        .expect("the recovered poll resolves the real terminal");
        assert_eq!(polled.status, RuntimeCommandStatus::Cancelled);
        assert_eq!(polled.stdout, "REAL_TERMINAL_OUTPUT");
        assert_eq!(polled.exit_code, Some(1));

        // Later replays answer from the durable terminal without re-appending
        // the observation output.
        let replay = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Poll,
                chars: None,
                signal: None,
                wait_ms: 0,
                request_id: RpcRequestId::String("replay-after-outage".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &TransportOutageRuntime,
        )
        .expect("the durable terminal replays");
        assert_eq!(replay.status, RuntimeCommandStatus::Cancelled);
        assert_eq!(replay.stdout, String::new());

        // A non-poll action on the now-terminal execution reports the
        // contract error and must not overwrite the recorded terminal.
        let late_kill = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Kill,
                chars: None,
                signal: Some(CommandKillSignal::Kill),
                wait_ms: 0,
                request_id: RpcRequestId::String("kill-after-terminal".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &TransportOutageRuntime,
        );
        assert_eq!(
            late_kill,
            Err(CommandControlError::SessionUnavailable),
            "killing a terminal execution surfaces the contract error"
        );
        assert!(matches!(
            registry
                .execution_for_public_session(&public_session)
                .expect("execution")
                .state,
            ExecutionState::Terminal(ExecutionTerminal {
                outcome: TerminalOutcome::Cancelled,
                ..
            })
        ));
        eprintln!("TEST_WORKSPACE_RETAINED path={}", root.display());
    }
    #[test]
    fn direct_observations_publish_stable_references_for_every_terminal_status() {
        for (index, status) in [
            RuntimeCommandStatus::Completed,
            RuntimeCommandStatus::Failed,
            RuntimeCommandStatus::Cancelled,
            RuntimeCommandStatus::TimedOut,
        ]
        .into_iter()
        .enumerate()
        {
            let root = std::env::temp_dir().join(format!(
                "lb-direct-refs-{}-{}-{index}",
                std::process::id(),
                unix_time_ms()
            ));
            std::fs::create_dir_all(&root).unwrap();
            let registry = ExecutionRegistry::open_at(root.join("executions.json")).unwrap();
            let public = PublicSessionId::new("public-refs");
            let id = registry.start(TaskId::new("task"), public.clone()).unwrap();
            registry
                .bind_runtime_handle(&id, RuntimeCommandHandle::new("private-session"))
                .unwrap();
            let request = CommandControlRequest {
                action: CommandControlAction::Poll,
                chars: None,
                signal: None,
                wait_ms: 0,
                request_id: RpcRequestId::String("first".into()),
                public_session_id: public.clone(),
            };
            let first = control_command_during_work(
                request.clone(),
                &registry,
                &FakeRuntime(Mutex::new(Some(RuntimeCommandObservation {
                    status,
                    exit_code: Some(if status == RuntimeCommandStatus::Completed {
                        0
                    } else {
                        1
                    }),
                    signal: None,
                    stdout: "once".into(),
                    stderr: String::new(),
                    truncated: Some(true),
                    output_incomplete: true,
                    output_refs: OutputReferences {
                        primary: Some("private-stderr".into()),
                        stdout: Some("private-stdout".into()),
                        stderr: Some("private-stderr".into()),
                    },
                }))),
            )
            .unwrap();
            let replay = control_command_during_work(request, &registry, &TimedOutRuntime).unwrap();
            assert_eq!(first.status, status);
            assert_eq!(first.stdout, "once");
            assert!(replay.stdout.is_empty());
            assert_eq!(first.output_refs, replay.output_refs);
            assert_eq!(first.output_refs.primary, first.output_refs.stderr);
            assert_eq!(
                registry
                    .output_handles()
                    .stream(first.output_refs.stderr.as_ref().unwrap())
                    .as_deref(),
                Some("stderr")
            );
            assert!(replay.output_incomplete);
            let ExecutionState::Terminal(saved) = registry
                .execution_for_public_session(&public)
                .unwrap()
                .state
            else {
                panic!("terminal");
            };
            assert_eq!(saved.output_refs, first.output_refs.values());
            eprintln!("TEST_WORKSPACE_RETAINED path={}", root.display());
        }
    }
}
