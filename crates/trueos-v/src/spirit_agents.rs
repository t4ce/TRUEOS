//! Read-only agent observations for resident Spirit presentation.
//!
//! Lifecycle and runtime/attention remain separate observations of the existing
//! controller. Publishing does not start, stop, acknowledge, or control agents.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

pub const SNAPSHOT_VERSION: u16 = 1;
pub const MAX_AGENTS: usize = 32;
pub const MAX_SNAPSHOT_BYTES: usize = 64 * 1024;
pub const MAX_ID_BYTES: usize = 64;
pub const MAX_LABEL_BYTES: usize = 96;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentLifecycle {
    PendingInit,
    Running,
    Interrupted,
    Completed,
    Errored,
    Shutdown,
    NotFound,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntime {
    NotLoaded,
    Idle,
    Active,
    SystemError,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentEntry {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: Option<String>,
    pub role: Option<String>,
    pub lifecycle: Option<AgentLifecycle>,
    pub runtime: Option<AgentRuntime>,
    pub waiting_approval: bool,
    pub waiting_input: bool,
    pub loaded: bool,
}

/// One producer's complete view, or an explicitly partial update.
///
/// The kernel obtains the sender's identity from execution context. Epochs are
/// monotonically increasing producer generations within that Blueprint run;
/// revisions increase within each epoch. A new epoch requires a full snapshot.
/// Root agents are represented by Lilly; entries describe their subagents.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSnapshot {
    pub version: u16,
    pub epoch: u64,
    pub revision: u64,
    pub complete: bool,
    pub connected: bool,
    pub agents: Vec<AgentEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    Invalid,
    Capacity,
}

impl AgentSnapshot {
    pub fn validate(&self) -> Result<(), SnapshotError> {
        if self.version != SNAPSHOT_VERSION || self.epoch == 0 || self.revision == 0 {
            return Err(SnapshotError::Invalid);
        }
        if self.agents.len() > MAX_AGENTS {
            return Err(SnapshotError::Capacity);
        }
        for (index, agent) in self.agents.iter().enumerate() {
            let valid_id = |id: &str| {
                !id.is_empty()
                    && id.len() <= MAX_ID_BYTES
                    && id
                        .bytes()
                        .all(|ch| ch.is_ascii_alphanumeric() || b"-_.:/".contains(&ch))
            };
            if !valid_id(&agent.id)
                || agent
                    .parent_id
                    .as_deref()
                    .is_none_or(|parent| !valid_id(parent) || parent == agent.id)
                || self.agents[..index]
                    .iter()
                    .any(|previous| previous.id == agent.id)
            {
                return Err(SnapshotError::Invalid);
            }
            if agent
                .name
                .as_ref()
                .is_some_and(|name| name.len() > MAX_LABEL_BYTES)
                || agent
                    .role
                    .as_ref()
                    .is_some_and(|role| role.len() > MAX_LABEL_BYTES)
            {
                return Err(SnapshotError::Capacity);
            }
        }
        Ok(())
    }
}

pub fn encode(snapshot: &AgentSnapshot) -> Result<Vec<u8>, SnapshotError> {
    snapshot.validate()?;
    let bytes = serde_json::to_vec(snapshot).map_err(|_| SnapshotError::Invalid)?;
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(SnapshotError::Capacity);
    }
    Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> Result<AgentSnapshot, SnapshotError> {
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(SnapshotError::Capacity);
    }
    let snapshot: AgentSnapshot =
        serde_json::from_slice(bytes).map_err(|_| SnapshotError::Invalid)?;
    snapshot.validate()?;
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{string::ToString, vec};

    fn snapshot() -> AgentSnapshot {
        AgentSnapshot {
            version: SNAPSHOT_VERSION,
            epoch: 1,
            revision: 1,
            complete: true,
            connected: true,
            agents: vec![AgentEntry {
                id: "child-1".into(),
                parent_id: Some("root".into()),
                name: Some("worker".into()),
                role: None,
                lifecycle: Some(AgentLifecycle::Running),
                runtime: Some(AgentRuntime::Active),
                waiting_approval: true,
                waiting_input: false,
                loaded: true,
            }],
        }
    }

    #[test]
    fn preserves_lifecycle_and_attention_as_independent_facts() {
        let original = snapshot();
        assert_eq!(original.validate(), Ok(()));
        let bytes = serde_json::to_vec(&original).unwrap();
        assert_eq!(
            serde_json::from_slice::<AgentSnapshot>(&bytes).unwrap(),
            original
        );
    }

    #[test]
    fn invalid_identity_duplicate_version_and_capacity_reject_whole_snapshot() {
        let mut value = snapshot();
        value.agents.push(value.agents[0].clone());
        assert_eq!(value.validate(), Err(SnapshotError::Invalid));
        value = snapshot();
        value.agents[0].id = "bad\nidentity".into();
        assert_eq!(value.validate(), Err(SnapshotError::Invalid));
        value = snapshot();
        value.version = SNAPSHOT_VERSION + 1;
        assert_eq!(value.validate(), Err(SnapshotError::Invalid));
        value = snapshot();
        value.agents.resize(MAX_AGENTS + 1, value.agents[0].clone());
        assert_eq!(value.validate(), Err(SnapshotError::Capacity));
        value = snapshot();
        value.agents[0].name = Some("a".repeat(MAX_LABEL_BYTES + 1));
        assert_eq!(value.validate(), Err(SnapshotError::Capacity));
        value = snapshot();
        value.agents[0].parent_id = None;
        assert_eq!(value.validate(), Err(SnapshotError::Invalid));
        value = snapshot();
        value.agents[0].parent_id = Some(value.agents[0].id.to_string());
        assert_eq!(value.validate(), Err(SnapshotError::Invalid));
    }
}
