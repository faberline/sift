//! Assemble the control Raft adapter without crossing context layer rules.
use super::{interfaces::http::api_error::ApiError, service_state::ServiceState};
use crate::control::{
    application::service::ControlService,
    infrastructure::{raft_port::RaftControlPort, state_machine::ControlStateMachine},
};
use std::sync::Arc;

pub(crate) fn service(
    machine: Arc<ControlStateMachine>,
    raft: Option<Arc<raft_runtime::RaftHost>>,
    guard: Arc<crate::operations::domain::recovery_guard::RecoveryGuard>,
) -> Arc<ControlService> {
    Arc::new(ControlService::new(Arc::new(RaftControlPort {
        machine,
        raft,
        local: tokio::sync::Mutex::new(()),
        gate: Arc::new(move || guard.check(chrono::Utc::now().timestamp())),
    })))
}

impl ServiceState {
    pub(crate) fn control_service(&self) -> Result<&Arc<ControlService>, ApiError> {
        self.control.as_ref().ok_or_else(|| {
            ApiError::temporarily_unavailable(
                "this role has no control service; use the control endpoint",
            )
        })
    }
}
