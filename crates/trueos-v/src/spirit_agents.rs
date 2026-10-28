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
    /// Canonical human nickname, separate from the task path in `name`.
    /// Optional within v1 so snapshots from older producers still decode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
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
                    .nickname
                    .as_ref()
                    .is_some_and(|nickname| nickname.len() > MAX_LABEL_BYTES)
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
                name: Some("/root/worker".into()),
                nickname: Some("Scout".into()),
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
    fn nickname_is_optional_for_older_v1_snapshots_and_separate_from_task_path() {
        let original = snapshot();
        let decoded = decode(&encode(&original).unwrap()).unwrap();
        assert_eq!(decoded.agents[0].name.as_deref(), Some("/root/worker"));
        assert_eq!(decoded.agents[0].nickname.as_deref(), Some("Scout"));
        let mut value = serde_json::to_value(&original).unwrap();
        value["agents"][0]
            .as_object_mut()
            .unwrap()
            .remove("nickname");
        let decoded = decode(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(decoded.agents[0].nickname, None);
        assert_eq!(decoded.agents[0].name, original.agents[0].name);
        assert!(
            serde_json::to_value(&decoded).unwrap()["agents"][0]
                .as_object()
                .unwrap()
                .get("nickname")
                .is_none()
        );
    }

    #[test]
    fn nickname_has_the_same_byte_budget_as_other_labels() {
        let mut value = snapshot();
        value.agents[0].nickname = Some("é".repeat(MAX_LABEL_BYTES / 2));
        assert_eq!(value.validate(), Ok(()));
        value.agents[0].nickname.as_mut().unwrap().push('a');
        assert_eq!(value.validate(), Err(SnapshotError::Capacity));
        assert_eq!(
            decode(&serde_json::to_vec(&value).unwrap()),
            Err(SnapshotError::Capacity)
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
