//! In-memory access policy for TRUEOSFS file identities.
//!
//! Every file header has exactly one [`RecordKey`]; a key can identify many
//! files. The database grants that key to an account and an app scope, rather
//! than adding more keys to the file. [`RecordKey::Ffa`] is the legacy standard
//! identity, not an implicit grant. Installation, container registration, and
//! membership never create key grants.
//!
//! The host supplies the authenticated account that actually launched the
//! container. This module neither authenticates users nor accepts an additional
//! "on behalf of" identity. Administrative mutations must be authorized by the
//! host. Persistence, cryptographic key material, file rekeying, and the choice
//! to enforce or only report a denied decision belong to other layers.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::String,
    vec::Vec,
};
use trueos_crypto::AccountId;

use crate::RecordKey;

/// Stable canonical app name, without the `apps/` workspace-path prefix.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AppId(String);

impl AppId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for AppId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for AppId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// An app's file container, independent of a VM slot or AppDB archive/version.
///
/// Every installation uses the same app + container ID model. The default ID is
/// `"0"`, stored at `apps/name`; other IDs are stored at `apps/name_id`. IDs are
/// opaque suffixes, including existing named suffixes. The host must validate
/// app names and IDs as canonical path components and resolve aliases before
/// constructing identities; this policy module does not resolve paths.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ContainerId {
    app: AppId,
    id: String,
}

impl ContainerId {
    pub fn new(app: impl Into<AppId>, id: impl Into<String>) -> Self {
        Self {
            app: app.into(),
            id: id.into(),
        }
    }

    pub fn default_for_app(app: impl Into<AppId>) -> Self {
        Self::new(app, "0")
    }

    pub fn app(&self) -> &AppId {
        &self.app
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn workspace_root(&self) -> String {
        if self.id == "0" {
            format!("apps/{}", self.app.as_str())
        } else {
            format!("apps/{}_{}", self.app.as_str(), self.id)
        }
    }
}

/// Sharing is chosen when the clean workspace is registered.
///
/// A private workspace can only admit its owner. Sharing it later requires a
/// separately registered, clean shared workspace; registration cannot convert
/// an existing private container into a shared one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContainerPolicy {
    Private { owner: AccountId },
    Shared,
}

/// A key grant always belongs to one account as well as one scope.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum GrantScope {
    /// Applies to every workspace for this app that the account may launch.
    App(AppId),
    /// Applies only to this exact app workspace.
    Container(ContainerId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyError {
    ContainerAlreadyRegistered,
    WorkspaceAlreadyRegistered,
    UnknownContainer,
    PrivateContainer,
    UnknownKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessDenied {
    MissingIdentity,
    PrincipalMismatch,
    UnknownContainer,
    NotMember,
    UnknownKey,
    MissingGrant,
}

#[derive(Debug)]
struct Container {
    policy: ContainerPolicy,
    members: BTreeSet<AccountId>,
}

#[derive(Debug, Default)]
struct AccountGrants {
    apps: BTreeMap<AppId, Vec<RecordKey>>,
    containers: BTreeMap<ContainerId, Vec<RecordKey>>,
}

/// Pure policy state. No entry grants access merely by being registered.
///
/// Keys are opaque identities, stored by equality so this module does not alter
/// the filesystem's record-key format or require key-provider ordering.
#[derive(Debug, Default)]
pub struct AccessKeyDb {
    keys: Vec<RecordKey>,
    containers: BTreeMap<ContainerId, Container>,
    grants: BTreeMap<AccountId, AccountGrants>,
}

#[derive(Clone, Debug, Default)]
pub struct PolicySnapshot {
    pub keys: Vec<RecordKey>,
    pub containers: Vec<(ContainerId, ContainerPolicy, Vec<AccountId>)>,
    pub grants: Vec<(AccountId, GrantScope, RecordKey)>,
}

impl AccessKeyDb {
    pub const fn new() -> Self {
        Self {
            keys: Vec::new(),
            containers: BTreeMap::new(),
            grants: BTreeMap::new(),
        }
    }

    /// Owned inspection data for trusted permission-management UIs.
    pub fn snapshot(&self) -> PolicySnapshot {
        PolicySnapshot {
            keys: self.keys.clone(),
            containers: self.containers.iter().map(|(id, c)|
                (id.clone(), c.policy, c.members.iter().copied().collect())).collect(),
            grants: self.grants.iter().flat_map(|(account, grants)| {
                grants.apps.iter().flat_map(move |(app, keys)| keys.iter().map(move |key|
                    (*account, GrantScope::App(app.clone()), *key)))
                    .chain(grants.containers.iter().flat_map(move |(id, keys)| keys.iter().map(move |key|
                        (*account, GrantScope::Container(id.clone()), *key))))
            }).collect(),
        }
    }

    /// Register an empty, clean workspace with no members or key grants.
    /// The host is responsible for ensuring that the workspace is clean.
    pub fn register_container(
        &mut self,
        id: ContainerId,
        policy: ContainerPolicy,
    ) -> Result<(), PolicyError> {
        if self.containers.contains_key(&id) {
            return Err(PolicyError::ContainerAlreadyRegistered);
        }
        let workspace = id.workspace_root();
        if self
            .containers
            .keys()
            .any(|registered| registered.workspace_root() == workspace)
        {
            return Err(PolicyError::WorkspaceAlreadyRegistered);
        }
        self.containers.insert(
            id,
            Container {
                policy,
                members: BTreeSet::new(),
            },
        );
        Ok(())
    }

    /// Admit a launch identity, without granting any file keys.
    pub fn add_member(&mut self, id: &ContainerId, account: AccountId) -> Result<(), PolicyError> {
        let container = self
            .containers
            .get_mut(id)
            .ok_or(PolicyError::UnknownContainer)?;
        if let ContainerPolicy::Private { owner } = container.policy {
            if account != owner {
                return Err(PolicyError::PrivateContainer);
            }
        }
        container.members.insert(account);
        Ok(())
    }

    /// Remove launch permission. Existing grants alone cannot authorize access.
    pub fn remove_member(
        &mut self,
        id: &ContainerId,
        account: AccountId,
    ) -> Result<bool, PolicyError> {
        let container = self
            .containers
            .get_mut(id)
            .ok_or(PolicyError::UnknownContainer)?;
        Ok(container.members.remove(&account))
    }

    /// Register an identity without making it accessible. Returns whether new.
    pub fn register_key(&mut self, key: RecordKey) -> bool {
        if self.keys.contains(&key) {
            return false;
        }
        self.keys.push(key);
        true
    }

    /// Grant an account one registered key at the requested app/workspace scope.
    /// Membership is independent and is still required when access is checked.
    pub fn grant(
        &mut self,
        account: AccountId,
        scope: GrantScope,
        key: RecordKey,
    ) -> Result<(), PolicyError> {
        if !self.keys.contains(&key) {
            return Err(PolicyError::UnknownKey);
        }
        if let GrantScope::Container(id) = &scope {
            if !self.containers.contains_key(id) {
                return Err(PolicyError::UnknownContainer);
            }
        }
        let grants = self.grants.entry(account).or_default();
        let keys = match scope {
            GrantScope::App(app) => grants.apps.entry(app).or_default(),
            GrantScope::Container(id) => grants.containers.entry(id).or_default(),
        };
        if !keys.contains(&key) {
            keys.push(key);
        }
        Ok(())
    }

    /// Revoke exactly this account/scope grant, preserving other grants.
    pub fn revoke(&mut self, account: AccountId, scope: &GrantScope, key: RecordKey) -> bool {
        let Some(grants) = self.grants.get_mut(&account) else {
            return false;
        };
        let Some(keys) = (match scope {
            GrantScope::App(app) => grants.apps.get_mut(app),
            GrantScope::Container(id) => grants.containers.get_mut(id),
        }) else {
            return false;
        };
        let Some(position) = keys.iter().position(|stored| *stored == key) else {
            return false;
        };
        keys.remove(position);
        if keys.is_empty() {
            match scope {
                GrantScope::App(app) => {
                    grants.apps.remove(app);
                }
                GrantScope::Container(id) => {
                    grants.containers.remove(id);
                }
            }
        }
        if grants.apps.is_empty() && grants.containers.is_empty() {
            self.grants.remove(&account);
        }
        true
    }

    /// Check the host-authenticated launch identity against exact membership.
    pub fn check_launch(
        &self,
        account: Option<AccountId>,
        id: &ContainerId,
    ) -> Result<(), AccessDenied> {
        let account = account.ok_or(AccessDenied::MissingIdentity)?;
        let container = self
            .containers
            .get(id)
            .ok_or(AccessDenied::UnknownContainer)?;
        if !container.members.contains(&account) {
            return Err(AccessDenied::NotMember);
        }
        Ok(())
    }

    /// Resume only under the original authenticated launch account.
    ///
    /// Shared workspace membership cannot authorize taking over another
    /// account's live run and its private in-memory state. `launched_as` comes
    /// from the host's immutable run binding, not a caller-selected identity.
    pub fn check_resume(
        &self,
        actor: Option<AccountId>,
        launched_as: Option<AccountId>,
        id: &ContainerId,
    ) -> Result<(), AccessDenied> {
        let actor = actor.ok_or(AccessDenied::MissingIdentity)?;
        if Some(actor) != launched_as {
            return Err(AccessDenied::PrincipalMismatch);
        }
        self.check_launch(Some(actor), id)
    }

    /// Check one file's identity using only this launch account's grants.
    pub fn check_file(
        &self,
        account: Option<AccountId>,
        id: &ContainerId,
        key: RecordKey,
    ) -> Result<(), AccessDenied> {
        self.check_launch(account, id)?;
        if !self.keys.contains(&key) {
            return Err(AccessDenied::UnknownKey);
        }
        // check_launch has already rejected a missing authenticated identity.
        let account = account.ok_or(AccessDenied::MissingIdentity)?;
        if let Some(grants) = self.grants.get(&account) {
            if grants
                .apps
                .get(&id.app)
                .is_some_and(|keys| keys.contains(&key))
                || grants
                    .containers
                    .get(id)
                    .is_some_and(|keys| keys.contains(&key))
            {
                return Ok(());
            }
        }
        Err(AccessDenied::MissingGrant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trueos_crypto::{KeyHandle, KeyRef, ProviderId};

    const ALICE: AccountId = AccountId::new(1);
    const BOB: AccountId = AccountId::new(2);

    fn key(value: u8) -> RecordKey {
        RecordKey::Key(KeyRef {
            provider: ProviderId::new([1; 16]).unwrap(),
            handle: KeyHandle::new([value; 32]).unwrap(),
        })
    }

    fn shared(db: &mut AccessKeyDb, app: &str, container_id: &str) -> ContainerId {
        let id = ContainerId::new(app, container_id);
        db.register_container(id.clone(), ContainerPolicy::Shared)
            .unwrap();
        db.add_member(&id, ALICE).unwrap();
        id
    }

    #[test]
    fn every_installation_has_an_app_and_container_id_including_default_zero() {
        let default = ContainerId::default_for_app("notes");
        assert_eq!(default, ContainerId::new("notes", "0"));
        assert_eq!(default.app().as_str(), "notes");
        assert_eq!(default.id(), "0");
        assert_eq!(default.workspace_root(), "apps/notes");
        let another = ContainerId::new("notes", "team");
        assert_eq!(another.workspace_root(), "apps/notes_team");
        assert_ne!(another, default);
        assert_ne!(another, ContainerId::new("editor", "team"));
    }

    #[test]
    fn distinct_container_identities_cannot_share_a_flat_workspace_path() {
        let named = ContainerId::new("notes", "team");
        let default = ContainerId::default_for_app("notes_team");
        assert_ne!(named, default);
        assert_eq!(named.workspace_root(), default.workspace_root());
        for (first, second) in [(named.clone(), default.clone()), (default, named)] {
            let mut db = AccessKeyDb::new();
            db.register_container(first.clone(), ContainerPolicy::Shared)
                .unwrap();
            assert_eq!(
                db.register_container(second, ContainerPolicy::Private { owner: ALICE }),
                Err(PolicyError::WorkspaceAlreadyRegistered)
            );
            assert_eq!(
                db.register_container(first, ContainerPolicy::Shared),
                Err(PolicyError::ContainerAlreadyRegistered)
            );
        }
    }

    #[test]
    fn registration_and_membership_never_grant_keys_including_legacy_default() {
        let mut db = AccessKeyDb::new();
        let id = ContainerId::new("editor", "0");
        db.register_container(id.clone(), ContainerPolicy::Private { owner: ALICE })
            .unwrap();
        assert_eq!(
            db.check_launch(Some(ALICE), &id),
            Err(AccessDenied::NotMember)
        );
        db.add_member(&id, ALICE).unwrap();
        assert_eq!(db.check_launch(Some(ALICE), &id), Ok(()));
        assert_eq!(
            db.check_file(Some(ALICE), &id, RecordKey::Ffa),
            Err(AccessDenied::UnknownKey)
        );
        assert!(db.register_key(RecordKey::Ffa));
        assert!(!db.register_key(RecordKey::Ffa));
        assert_eq!(
            db.check_file(Some(ALICE), &id, RecordKey::Ffa),
            Err(AccessDenied::MissingGrant)
        );
        db.grant(ALICE, GrantScope::App(AppId::new("editor")), RecordKey::Ffa)
            .unwrap();
        assert_eq!(db.check_file(Some(ALICE), &id, RecordKey::Ffa), Ok(()));
    }

    #[test]
    fn app_wide_and_container_grants_have_distinct_boundaries() {
        let mut db = AccessKeyDb::new();
        let first = shared(&mut db, "editor", "first");
        let second = shared(&mut db, "editor", "second");
        let other_app = shared(&mut db, "viewer", "0");
        let app_key = key(1);
        let private_key = key(2);
        db.register_key(app_key);
        db.register_key(private_key);
        db.grant(ALICE, GrantScope::App(AppId::new("editor")), app_key)
            .unwrap();
        db.grant(ALICE, GrantScope::Container(first.clone()), private_key)
            .unwrap();
        assert_eq!(db.check_file(Some(ALICE), &first, app_key), Ok(()));
        assert_eq!(db.check_file(Some(ALICE), &second, app_key), Ok(()));
        assert_eq!(
            db.check_file(Some(ALICE), &other_app, app_key),
            Err(AccessDenied::MissingGrant)
        );
        assert_eq!(db.check_file(Some(ALICE), &first, private_key), Ok(()));
        assert_eq!(
            db.check_file(Some(ALICE), &second, private_key),
            Err(AccessDenied::MissingGrant)
        );
        db.remove_member(&second, ALICE).unwrap();
        assert_eq!(
            db.check_file(Some(ALICE), &second, app_key),
            Err(AccessDenied::NotMember)
        );
    }

    #[test]
    fn shared_container_does_not_union_users_keychains() {
        let mut db = AccessKeyDb::new();
        let id = shared(&mut db, "notes", "team");
        db.add_member(&id, BOB).unwrap();
        let alice_key = key(1);
        let bob_key = key(2);
        db.register_key(alice_key);
        db.register_key(bob_key);
        db.grant(ALICE, GrantScope::App(id.app.clone()), alice_key)
            .unwrap();
        db.grant(BOB, GrantScope::Container(id.clone()), bob_key)
            .unwrap();
        assert_eq!(db.check_file(Some(ALICE), &id, alice_key), Ok(()));
        assert_eq!(db.check_file(Some(BOB), &id, bob_key), Ok(()));
        assert_eq!(
            db.check_file(Some(BOB), &id, alice_key),
            Err(AccessDenied::MissingGrant)
        );
        assert_eq!(
            db.check_file(Some(ALICE), &id, bob_key),
            Err(AccessDenied::MissingGrant)
        );
    }

    #[test]
    fn private_workspace_cannot_be_reassigned_or_retroactively_shared() {
        let mut db = AccessKeyDb::new();
        let private = ContainerId::new("notes", "private");
        db.register_container(private.clone(), ContainerPolicy::Private { owner: ALICE })
            .unwrap();
        assert_eq!(
            db.add_member(&private, BOB),
            Err(PolicyError::PrivateContainer)
        );
        assert_eq!(
            db.register_container(private.clone(), ContainerPolicy::Shared),
            Err(PolicyError::ContainerAlreadyRegistered)
        );
        db.add_member(&private, ALICE).unwrap();
        assert_eq!(db.check_launch(Some(ALICE), &private), Ok(()));
        assert_eq!(
            db.check_launch(Some(BOB), &private),
            Err(AccessDenied::NotMember)
        );
        let clean = shared(&mut db, "notes", "shared");
        db.add_member(&clean, BOB).unwrap();
        assert_eq!(db.check_launch(Some(BOB), &clean), Ok(()));
    }

    #[test]
    fn splitting_two_files_changes_one_file_identity_and_preserves_other_access() {
        let mut db = AccessKeyDb::new();
        let editor = shared(&mut db, "editor", "0");
        let viewer = shared(&mut db, "viewer", "0");
        let old = key(1);
        let new = key(2);
        db.register_key(old);
        let editor_scope = GrantScope::App(editor.app.clone());
        let viewer_scope = GrantScope::App(viewer.app.clone());
        db.grant(ALICE, editor_scope.clone(), old).unwrap();
        db.grant(ALICE, viewer_scope, old).unwrap();
        // Initially both files carry exactly the same identity.
        let first_file = old;
        let mut second_file = old;
        assert_eq!(db.check_file(Some(ALICE), &viewer, second_file), Ok(()));
        // Actual header rewriting is the filesystem's job, not DB policy.
        db.register_key(new);
        db.grant(ALICE, editor_scope, new).unwrap();
        second_file = new;
        assert_eq!(db.check_file(Some(ALICE), &editor, first_file), Ok(()));
        assert_eq!(db.check_file(Some(ALICE), &editor, second_file), Ok(()));
        assert_eq!(db.check_file(Some(ALICE), &viewer, first_file), Ok(()));
        assert_eq!(
            db.check_file(Some(ALICE), &viewer, second_file),
            Err(AccessDenied::MissingGrant)
        );
    }

    #[test]
    fn revoke_is_account_and_scope_specific() {
        let mut db = AccessKeyDb::new();
        let id = shared(&mut db, "editor", "0");
        db.add_member(&id, BOB).unwrap();
        let key = key(1);
        db.register_key(key);
        let app = GrantScope::App(id.app.clone());
        let exact = GrantScope::Container(id.clone());
        db.grant(ALICE, app.clone(), key).unwrap();
        db.grant(ALICE, exact.clone(), key).unwrap();
        db.grant(BOB, app.clone(), key).unwrap();
        assert!(db.revoke(ALICE, &app, key));
        assert!(!db.revoke(ALICE, &app, key));
        assert_eq!(db.check_file(Some(ALICE), &id, key), Ok(()));
        assert!(db.revoke(ALICE, &exact, key));
        assert_eq!(
            db.check_file(Some(ALICE), &id, key),
            Err(AccessDenied::MissingGrant)
        );
        assert_eq!(db.check_file(Some(BOB), &id, key), Ok(()));
    }

    #[test]
    fn missing_identity_unknown_container_and_unknown_keys_are_explicit() {
        let mut db = AccessKeyDb::new();
        let id = shared(&mut db, "editor", "0");
        assert_eq!(
            db.check_launch(None, &id),
            Err(AccessDenied::MissingIdentity)
        );
        assert_eq!(
            db.check_file(None, &id, RecordKey::Ffa),
            Err(AccessDenied::MissingIdentity)
        );
        assert_eq!(
            db.check_launch(Some(ALICE), &ContainerId::new("editor", "unknown")),
            Err(AccessDenied::UnknownContainer)
        );
        assert_eq!(
            db.grant(ALICE, GrantScope::App(id.app.clone()), key(1)),
            Err(PolicyError::UnknownKey)
        );
        db.register_key(key(1));
        assert_eq!(
            db.grant(
                ALICE,
                GrantScope::Container(ContainerId::new("editor", "missing")),
                key(1)
            ),
            Err(PolicyError::UnknownContainer)
        );
    }

    #[test]
    fn resume_keeps_original_actor_even_when_workspace_is_shared() {
        let mut db = AccessKeyDb::new();
        let id = shared(&mut db, "notes", "team");
        db.add_member(&id, BOB).unwrap();
        assert_eq!(db.check_resume(Some(ALICE), Some(ALICE), &id), Ok(()));
        assert_eq!(db.check_resume(Some(BOB), Some(BOB), &id), Ok(()));
        assert_eq!(
            db.check_resume(Some(BOB), Some(ALICE), &id),
            Err(AccessDenied::PrincipalMismatch)
        );
        assert_eq!(
            db.check_resume(Some(ALICE), Some(BOB), &id),
            Err(AccessDenied::PrincipalMismatch)
        );
        assert_eq!(
            db.check_resume(None, Some(ALICE), &id),
            Err(AccessDenied::MissingIdentity)
        );
        assert_eq!(
            db.check_resume(None, None, &id),
            Err(AccessDenied::MissingIdentity)
        );
        assert_eq!(
            db.check_resume(Some(ALICE), None, &id),
            Err(AccessDenied::PrincipalMismatch)
        );
        db.remove_member(&id, BOB).unwrap();
        assert_eq!(
            db.check_resume(Some(BOB), Some(BOB), &id),
            Err(AccessDenied::NotMember)
        );
    }
}
