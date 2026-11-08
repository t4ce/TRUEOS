//! Kernel access-key policy for captured app principals and native file operations.
//!
//! AppDB contains executable bytes; this database contains permission relations.
//! Trusted workspace provisioning installs persisted owner grants before launch.
//! Only trusted kernel account-management code may configure policy. There is
//! deliberately no guest ABI for choosing an account or changing this database.

use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use core::sync::atomic::{AtomicU64, Ordering};
use trueos_crypto::AccountId;
use trueos_fs::{
    RecordKey,
    access::{AccessDenied, AccessKeyDb, AppId, ContainerId, ContainerPolicy, GrantScope},
};

static ACCESS_KEY_DB: Mutex<AccessKeyDb> = Mutex::new(AccessKeyDb::new());
const MAX_WARNINGS_PER_VM: usize = 32;

/// FFA is the public/default namespace. A keyed record requires the captured
/// launch account's grant; a different frontend's current login is irrelevant.
pub(crate) fn authorize_record(vm_id: u8, key: RecordKey) -> Result<(), AccessDenied> {
    if key == RecordKey::Ffa { return Ok(()); }
    let subject = principal(vm_id).ok_or(AccessDenied::MissingIdentity)?;
    ACCESS_KEY_DB.lock().check_file(subject.account, &subject.container, key)
}

/// Install only a durably provisioned owner/key relation. This trusted entry
/// point changes permission state; binding another VM never manufactures it.
pub(crate) fn grant_owned_workspace(
    archive: &str, workspace: &str, account: AccountId, key: RecordKey,
) -> Result<(), AccessDenied> {
    let container = container_for_workspace(archive, workspace)
        .ok_or(AccessDenied::UnknownContainer)?;
    if key == RecordKey::Ffa { return Err(AccessDenied::UnknownKey); }
    crate::allocators::with_host_alloc_domain_strong(|| {
        let mut workspaces = OWNED_WORKSPACES.lock();
        if workspaces.iter().any(|(id, owner, stored)| id == &container && (*owner != account || *stored != key)) {
            return Err(AccessDenied::PrincipalMismatch);
        }
        let mut database = ACCESS_KEY_DB.lock();
        match database.register_container(container.clone(), ContainerPolicy::Private { owner: account }) {
            Ok(()) | Err(trueos_fs::access::PolicyError::ContainerAlreadyRegistered) => {}
            Err(_) => return Err(AccessDenied::UnknownContainer),
        }
        database.add_member(&container, account).map_err(|_| AccessDenied::NotMember)?;
        database.register_key(key);
        database.grant(account, GrantScope::App(container.app().clone()), key)
            .map_err(|_| AccessDenied::MissingGrant)?;
        if !workspaces.iter().any(|(id, _, _)| id == &container) {
            workspaces.push((container, account, key));
        }
        Ok(())
    })
}

static OWNED_WORKSPACES: Mutex<Vec<(ContainerId, AccountId, RecordKey)>> = Mutex::new(Vec::new());

pub(crate) fn workspace_for_vm(vm_id: u8) -> Option<String> {
    workspace_for_vm_at(vm_id, principal_epoch(vm_id)?)
}

/// A legacy checkpoint/replication export has no private-key envelope.
pub(crate) fn is_protected_vm(vm_id: u8) -> bool {
    principal(vm_id).is_some_and(|subject| {
        OWNED_WORKSPACES.lock().iter().any(|(id, _, _)| id == &subject.container)
            || SQLITE_CONTAINERS.lock().contains(&subject.container)
    })
}

/// Retained protected runs can only be controlled by their captured account.
/// Public development apps keep their existing control behavior.
pub(crate) fn authorize_vm_control(vm_id: u8, actor: Option<AccountId>) -> Result<(), AccessDenied> {
    let Some(subject) = principal(vm_id) else { return Ok(()); };
    if !is_protected_vm(vm_id) { return Ok(()); }
    ACCESS_KEY_DB.lock().check_resume(actor, subject.account, &subject.container)
}

/// Credential paths must use this launch's exact persisted owner/key binding.
pub(crate) fn authorize_protected_workspace(vm_id: u8, root_key: RecordKey) -> Result<(), AccessDenied> {
    authorize_protected_workspace_at(vm_id, principal_epoch(vm_id).ok_or(AccessDenied::MissingIdentity)?, root_key)
}

#[derive(Clone)]
struct Principal {
    account: Option<AccountId>,
    container: ContainerId,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct Warning {
    actor: Option<AccountId>,
    key: Option<RecordKey>,
    operation: &'static str,
    reason: AccessDenied,
}

struct Binding {
    epoch: u64,
    principal: Principal,
    warnings: Vec<Warning>,
}

static NEXT_PRINCIPAL_EPOCH: AtomicU64 = AtomicU64::new(1);

static VM_BINDINGS: [Mutex<Option<Binding>>; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { Mutex::new(None) }; crate::allcaps::hv::VM_ID_LIMIT];

/// Future built-in permission management enters here after obtaining explicit
/// user consent. This is a trusted host API, never a guest-controlled request.
/// The policy is boot-local until durable storage and account UX are implemented.
#[allow(dead_code)]
pub(crate) fn configure_policy(update: impl FnOnce(&mut AccessKeyDb)) {
    crate::allocators::with_host_alloc_domain_strong(|| {
        update(&mut ACCESS_KEY_DB.lock());
        for slot in &VM_BINDINGS {
            if let Some(binding) = slot.lock().as_mut() {
                binding.warnings.clear();
            }
        }
    });
}

/// Resolve the loader's path to an app-prefixed container identity. The bare
/// app directory is container 0; a flat `apps/name_id` directory is another
/// container ID. A physical `apps/name_0` is not an alias for container 0.
pub(crate) fn container_for_workspace(archive: &str, workspace: &str) -> Option<ContainerId> {
    let app_root = crate::hv::blueprint::app_fs_root_for_archive(archive, &[]);
    let app = AppId::new(app_root.strip_prefix("apps/").unwrap_or(&app_root));
    if workspace == app_root {
        return Some(ContainerId::default_for_app(app));
    }
    let prefix = alloc::format!("{app_root}_");
    let id = workspace.strip_prefix(&prefix)?;
    if id.is_empty() || matches!(id, "0" | "." | "..") || id.contains('/') {
        return None;
    }
    Some(ContainerId::new(app, id))
}

/// Bind a runtime slot to the container and account selected by the host launch
/// path. Default launches select container 0, independently of runtime VM IDs.
pub(crate) fn bind_launch(vm_id: u8, archive: &str, workspace: &str, account: Option<AccountId>) {
    let Some(slot) = VM_BINDINGS.get(vm_id as usize) else {
        return;
    };
    let principal = crate::allocators::with_host_alloc_domain_strong(|| {
        // Use exactly the loader's app-name normalization. AppDB replacement
        // hashes and runtime instance UUIDs must not change app-wide grants.
        let container = container_for_workspace(archive, workspace)?;
        let principal = Principal { account, container };
        let epoch = NEXT_PRINCIPAL_EPOCH.fetch_add(1, Ordering::Relaxed);
        assert!(epoch != 0, "access principal epoch exhausted");
        *slot.lock() = Some(Binding {
            epoch,
            principal: principal.clone(),
            warnings: Vec::new(),
        });
        Some(principal)
    });
    let Some(principal) = principal else {
        clear_vm(vm_id);
        crate::log_warn!(target: "filesystem";
            "accesskeydb: vm={} account={:?} op=launch reason=invalid-container-path decision=would-deny mode=warn-only\n",
            vm_id, account.map(AccountId::raw));
        return;
    };
    let decision = {
        let mut database = ACCESS_KEY_DB.lock();
        // The existing Ffa header tag names the standard key. Registering its
        // identity grants neither membership nor file access to anyone.
        crate::allocators::with_host_alloc_domain_strong(|| database.register_key(RecordKey::Ffa));
        database.check_launch(principal.account, &principal.container)
    };
    if let Err(reason) = decision {
        warn_denial(vm_id, &principal, None, "launch", reason);
    }
}

fn principal(vm_id: u8) -> Option<Principal> {
    crate::allocators::with_host_alloc_domain_strong(|| {
        VM_BINDINGS
            .get(vm_id as usize)?
            .lock()
            .as_ref()
            .map(|binding| binding.principal.clone())
    })
}

/// Long-lived host work captures this token before queuing. Reusing a VM slot
/// never lends its next principal to the previous run's unfinished work.
pub(crate) fn principal_epoch(vm_id: u8) -> Option<u64> {
    Some(VM_BINDINGS.get(vm_id as usize)?.lock().as_ref()?.epoch)
}

fn principal_at(vm_id: u8, epoch: u64) -> Option<Principal> {
    crate::allocators::with_host_alloc_domain_strong(|| {
        let binding = VM_BINDINGS.get(vm_id as usize)?.lock();
        let binding = binding.as_ref()?;
        (binding.epoch == epoch).then(|| binding.principal.clone())
    })
}

/// Borrow an exact live launch for a synchronous, allocation-free check.
/// Callers may inspect policy while holding this binding, but must not reenter
/// the binding table or retain the borrow across an await. Policy updates drop
/// ACCESS_KEY_DB before visiting bindings, preserving binding -> policy order.
fn with_principal_at<T>(vm_id: u8, epoch: u64, check: impl FnOnce(&Principal) -> T) -> Option<T> {
    let binding = VM_BINDINGS.get(vm_id as usize)?.lock();
    let binding = binding.as_ref()?;
    (binding.epoch == epoch).then(|| check(&binding.principal))
}

fn authorize_principal_record(subject: &Principal, key: RecordKey) -> Result<(), AccessDenied> {
    if key == RecordKey::Ffa { return Ok(()); }
    ACCESS_KEY_DB.lock().check_file(subject.account, &subject.container, key)
}

pub(crate) fn authorize_record_at(vm_id: u8, epoch: u64, key: RecordKey) -> Result<(), AccessDenied> {
    with_principal_at(vm_id, epoch, |subject| authorize_principal_record(subject, key))
        .ok_or(AccessDenied::MissingIdentity)?
}

pub(crate) fn workspace_for_vm_at(vm_id: u8, epoch: u64) -> Option<String> {
    let subject = principal_at(vm_id, epoch)?;
    if SQLITE_DELEGATES.lock().iter().any(|d| d.vm == vm_id && d.epoch == epoch && Some(d.owner) == subject.account) {
        return Some(String::from("apps/codex"));
    }
    Some(subject.container.workspace_root())
}

pub(crate) fn authorize_protected_workspace_at(vm_id: u8, epoch: u64, root_key: RecordKey) -> Result<(), AccessDenied> {
    let subject = principal_at(vm_id, epoch).ok_or(AccessDenied::MissingIdentity)?;
    let account = subject.account.ok_or(AccessDenied::MissingIdentity)?;
    if SQLITE_DELEGATES.lock().iter().any(|d| d.vm == vm_id && d.epoch == epoch && d.owner == account && d.key == root_key) {
        return authorize_record_at(vm_id, epoch, root_key);
    }
    if !OWNED_WORKSPACES.lock().iter().any(|(id, owner, key)| {
        id == &subject.container && *owner == account && *key == root_key
    }) { return Err(AccessDenied::MissingGrant); }
    ACCESS_KEY_DB.lock().check_file(Some(account), &subject.container, root_key)
}

/// Child launches inherit the parent's frozen principal, never a global login.
pub(crate) fn principal_for_vm(vm_id: u8) -> Option<AccountId> {
    VM_BINDINGS
        .get(vm_id as usize)?
        .lock()
        .as_ref()?
        .principal
        .account
}

pub(crate) fn clear_vm(vm_id: u8) {
    if let Some(slot) = VM_BINDINGS.get(vm_id as usize) {
        slot.lock().take();
    }
    SQLITE_DELEGATES.lock().retain(|delegate| delegate.vm != vm_id);
}

/// The current Codex dependency is one named service, not another executable
/// installation. Reserve even before provisioning so a public listener cannot
/// impersonate it while Codex starts. This is port_for("codex") in the service.
pub(crate) const CODEX_SQLITE_PORT: u16 = 42116;
pub(crate) const CODEX_SQLITE_STORAGE: &str = "/apps/codex/.codex/sqlite-service/codex";
pub(crate) const CODEX_SQLITE_ENDPOINT: &str = "http://127.0.0.1:42116";
#[derive(Clone)]
struct SqliteReservation { owner: AccountId, key: RecordKey }
#[derive(Clone, Copy)]
struct SqliteDelegate { vm: u8, epoch: u64, owner: AccountId, key: RecordKey }
static SQLITE_RESERVATION: Mutex<Option<SqliteReservation>> = Mutex::new(None);
static SQLITE_CONTAINERS: Mutex<Vec<ContainerId>> = Mutex::new(Vec::new());
static SQLITE_DELEGATES: Mutex<Vec<SqliteDelegate>> = Mutex::new(Vec::new());

/// Called only after the persisted owner/key is loaded and the root stamped.
pub(crate) fn reserve_codex_sqlite(workspace: &str, owner: AccountId, key: RecordKey) -> Result<(), AccessDenied> {
    if workspace != "apps/codex" { return Ok(()); }
    if key == RecordKey::Ffa { return Err(AccessDenied::UnknownKey); }
    let mut reservation = SQLITE_RESERVATION.lock();
    if reservation.as_ref().is_some_and(|old| old.owner != owner || old.key != key) {
        return Err(AccessDenied::PrincipalMismatch);
    }
    *reservation = Some(SqliteReservation { owner, key });
    Ok(())
}

/// Explicit trusted dependency admission lends this app the owner's existing
/// key. No key is generated, no new AppDB row is created, and no other user's
/// grant is inferred from a display name or a loopback address.
pub(crate) fn grant_codex_sqlite(archive: &str, workspace: &str, account: Option<AccountId>) -> Result<(), AccessDenied> {
    let owner = account.ok_or(AccessDenied::MissingIdentity)?;
    let service = container_for_workspace(archive, workspace).ok_or(AccessDenied::UnknownContainer)?;
    if service.app().as_str() != "sqlite" { return Err(AccessDenied::UnknownContainer); }
    let reservation = SQLITE_RESERVATION.lock().clone().ok_or(AccessDenied::MissingGrant)?;
    if reservation.owner != owner { return Err(AccessDenied::PrincipalMismatch); }
    let codex = ContainerId::default_for_app(AppId::new("codex"));
    if !OWNED_WORKSPACES.lock().iter().any(|(id, user, key)| id == &codex && *user == owner && *key == reservation.key) {
        return Err(AccessDenied::MissingGrant);
    }
    crate::allocators::with_host_alloc_domain_strong(|| {
        let mut database = ACCESS_KEY_DB.lock();
        match database.register_container(service.clone(), ContainerPolicy::Private { owner }) {
            Ok(()) | Err(trueos_fs::access::PolicyError::ContainerAlreadyRegistered) => {}
            Err(_) => return Err(AccessDenied::UnknownContainer),
        }
        database.add_member(&service, owner).map_err(|_| AccessDenied::NotMember)?;
        database.grant(owner, GrantScope::Container(service.clone()), reservation.key)
            .map_err(|_| AccessDenied::MissingGrant)?;
        let mut containers = SQLITE_CONTAINERS.lock();
        if !containers.contains(&service) { containers.push(service); }
        Ok(())
    })
}

/// Attach the admitted purpose to this exact launch epoch, before guest entry.
pub(crate) fn bind_codex_sqlite(vm: u8) -> Result<(), AccessDenied> {
    let epoch = principal_epoch(vm).ok_or(AccessDenied::MissingIdentity)?;
    let subject = principal_at(vm, epoch).ok_or(AccessDenied::MissingIdentity)?;
    let reservation = SQLITE_RESERVATION.lock().clone().ok_or(AccessDenied::MissingGrant)?;
    if subject.account != Some(reservation.owner) || !SQLITE_CONTAINERS.lock().contains(&subject.container) {
        return Err(AccessDenied::PrincipalMismatch);
    }
    authorize_record_at(vm, epoch, reservation.key)?;
    crate::allocators::with_host_alloc_domain_strong(|| {
        let mut delegates = SQLITE_DELEGATES.lock();
        delegates.retain(|old| old.vm != vm);
        delegates.push(SqliteDelegate { vm, epoch, owner: reservation.owner, key: reservation.key });
    });
    Ok(())
}

fn sqlite_actor(actor: crate::r::fs::trueosfs::authorized::Actor, reservation: &SqliteReservation, service: bool) -> Result<(), ()> {
    let crate::r::fs::trueosfs::authorized::Actor::Vm(vm, epoch) = actor else { return Err(()); };
    // Network polling rechecks each live SQLite connection even while idle.
    // Borrow once rather than cloning both container strings twice per actor;
    // policy and launch-epoch revocation remain visible on every check.
    with_principal_at(vm, epoch, |subject| {
        if subject.account != Some(reservation.owner) { return Err(()); }
        authorize_principal_record(subject, reservation.key).map_err(|_| ())
    }).ok_or(())??;
    if service && !SQLITE_DELEGATES.lock().iter().any(|d| d.vm == vm && d.epoch == epoch && d.owner == reservation.owner && d.key == reservation.key) {
        return Err(());
    }
    Ok(())
}

pub(crate) fn authorize_tcp_listen(actor: crate::r::fs::trueosfs::authorized::Actor, local: core::net::SocketAddr) -> Result<(), ()> {
    if local.port() != CODEX_SQLITE_PORT { return Ok(()); }
    if local.ip() != core::net::IpAddr::V4(core::net::Ipv4Addr::LOCALHOST) { return Err(()); }
    let reservation = SQLITE_RESERVATION.lock().clone().ok_or(())?;
    sqlite_actor(actor, &reservation, true)
}

/// Called before connection establishment and before each oriented transfer.
pub(crate) fn authorize_tcp_connect(client: crate::r::fs::trueosfs::authorized::Actor, server: crate::r::fs::trueosfs::authorized::Actor, port: u16) -> Result<(), ()> {
    if port != CODEX_SQLITE_PORT { return Ok(()); }
    let reservation = SQLITE_RESERVATION.lock().clone().ok_or(())?;
    sqlite_actor(client, &reservation, false)?;
    sqlite_actor(server, &reservation, true)
}

fn warn_denial(
    vm_id: u8,
    principal: &Principal,
    key: Option<RecordKey>,
    operation: &'static str,
    reason: AccessDenied,
) {
    warn_denial_as(vm_id, principal, principal.account, key, operation, reason);
}

fn warn_denial_as(
    vm_id: u8,
    principal: &Principal,
    actor: Option<AccountId>,
    key: Option<RecordKey>,
    operation: &'static str,
    reason: AccessDenied,
) {
    let warning = Warning {
        actor,
        key,
        operation,
        reason,
    };
    let emit = crate::allocators::with_host_alloc_domain_strong(|| {
        let Some(slot) = VM_BINDINGS.get(vm_id as usize) else {
            return false;
        };
        let mut slot = slot.lock();
        let Some(binding) = slot.as_mut() else {
            return false;
        };
        if binding.principal.account != principal.account
            || binding.principal.container != principal.container
            || binding.warnings.contains(&warning)
            || binding.warnings.len() >= MAX_WARNINGS_PER_VM
        {
            return false;
        }
        binding.warnings.push(warning);
        true
    });
    if emit {
        crate::log_warn!(target: "filesystem";
            "accesskeydb: vm={} container={:?} account={:?} actor={:?} op={} key={:?} reason={:?} decision=would-deny mode=warn-only\n",
            vm_id, principal.container, principal.account.map(AccountId::raw), actor.map(AccountId::raw), operation, key, reason);
    }
}

fn audit_for_principal(vm_id: u8, principal: &Principal, key: RecordKey, operation: &'static str) {
    let decision = ACCESS_KEY_DB
        .lock()
        .check_file(principal.account, &principal.container, key);
    if let Err(reason) = decision {
        warn_denial(vm_id, principal, Some(key), operation, reason);
    }
}

/// A retained run cannot acquire a different account's identity through resume.
/// Keep the binding unchanged and report the initiating actor's policy failure.
pub(crate) fn audit_resume(vm_id: u8, actor: Option<AccountId>) {
    let Some(principal) = principal(vm_id) else {
        return;
    };
    let decision =
        ACCESS_KEY_DB
            .lock()
            .check_resume(actor, principal.account, &principal.container);
    if let Err(reason) = decision {
        warn_denial_as(vm_id, &principal, actor, None, "resume", reason);
    }
}

/// This deliberately has no permission result for the caller to enforce.
#[allow(dead_code)] // Entry point for services that already have record metadata.
pub(crate) fn audit_key(vm_id: u8, key: RecordKey, operation: &'static str) {
    if let Some(principal) = principal(vm_id) {
        audit_for_principal(vm_id, &principal, key, operation);
    }
}

/// Audit the committed record's one key, or the standard key for a new object.
/// Missing files and diagnostic metadata errors retain the operation's existing
/// behavior. No locks are held across storage awaits.
pub(crate) async fn audit_path(
    vm_id: u8,
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    operation: &'static str,
    create: bool,
) {
    if path.is_empty() {
        return;
    } // The synthetic volume root has no file header.
    let Some(principal) = principal(vm_id) else {
        return;
    };
    match crate::r::fs::trueosfs::node_info_async(disk, path).await {
        Ok(Some(info)) => audit_for_principal(vm_id, &principal, info.record_key, operation),
        Ok(None) if create => audit_for_principal(vm_id, &principal, RecordKey::Ffa, operation),
        Ok(None) => {}
        Err(error) => crate::log_warn!(target: "filesystem";
            "accesskeydb: vm={} op={} reason=metadata-unavailable error={:?} mode=warn-only\n", vm_id, operation, error),
    }
}
