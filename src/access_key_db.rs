//! Kernel adapter for AccessKeyDB. Policy failures are diagnostic during bring-up.
//!
//! AppDB contains executable bytes; this database contains permission relations.
//! Nothing here grants a key or container membership as a side effect of launch.
//! Only trusted kernel account-management code may configure policy. There is
//! deliberately no guest ABI for choosing an account or changing this database.

use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use trueos_crypto::AccountId;
use trueos_fs::{
    RecordKey,
    access::{AccessDenied, AccessKeyDb, AppId, ContainerId, ContainerPolicy, GrantScope},
};

static ACCESS_KEY_DB: Mutex<AccessKeyDb> = Mutex::new(AccessKeyDb::new());
static WORKSPACE_PREPARE: embassy_sync::mutex::Mutex<crate::wait::EmbassySpinRawMutex, ()> =
    embassy_sync::mutex::Mutex::new(());
const MAX_WARNINGS_PER_VM: usize = 32;
/// Reserved shared configuration image beside Cry's `users/<name>` profiles.
/// AccessKeyDB belongs in that Redb image, separately from volatile `app.db`.
/// Storage and account-profile migration are intentionally not activated yet.
#[allow(dead_code)]
pub(crate) const CONFIGURATION_DB_PATH: &str = "users/configuration.redb";

/// FFA is the public/default namespace. A keyed record requires the captured
/// launch account's grant; a different frontend's current login is irrelevant.
pub(crate) fn authorize_record(vm_id: u8, key: RecordKey) -> Result<(), AccessDenied> {
    if key == RecordKey::Ffa { return Ok(()); }
    let subject = principal(vm_id).ok_or(AccessDenied::MissingIdentity)?;
    ACCESS_KEY_DB.lock().check_file(subject.account, &subject.container, key)
}

/// Trusted host provisioning, before Codex executes. Persist ownership before
/// granting access; a failed write never manufactures a boot-local permission.
pub(crate) async fn prepare_owned_workspace(
    archive: &str,
    workspace: &str,
    account: Option<AccountId>,
) -> Result<RecordKey, crate::r::io::kfs::FsError> {
    use crate::r::io::kfs::FsError;
    use crate::r::fs::trueosfs as fs;
    use sha2::{Digest, Sha256};
    let account = account.ok_or(FsError::PermissionDenied)?;
    let container = container_for_workspace(archive, workspace).ok_or(FsError::BadPath)?;
    let _prepare = WORKSPACE_PREPARE.lock().await;
    let disk = fs::primary_root_handle().ok_or(FsError::NoRoot)?;
    let image = fs::file_out_async(disk, CONFIGURATION_DB_PATH).await
        .map_err(|_| FsError::Io)?;
    let mut handle = [0u8; 32];
    crate::crypt::fill_access_key_handle(&mut handle).map_err(|_| FsError::Io)?;
    let candidate = crate::access_key_store::software_key(handle).map_err(|_| FsError::Io)?;
    let update = crate::access_key_store::ensure_workspace(
        image.as_deref().unwrap_or_default(), workspace, account, candidate,
    ).map_err(|error| match error {
        crate::access_key_store::StoreError::ForeignOwner => FsError::PermissionDenied,
        _ => FsError::Io,
    })?;
    if let Some(next) = update.image {
        if !fs::dir_create_all_async(disk, "users").await.map_err(|_| FsError::Io)? {
            return Err(FsError::NoSpace);
        }
        let digest = image.as_ref().map(|bytes| <[u8;32]>::from(Sha256::digest(bytes)));
        fs::file_in_typed_checked_async(
            disk, CONFIGURATION_DB_PATH, &next, fs::ContentTypeId::BLOB,
            image.is_none(), digest, false,
        ).await.map_err(|_| FsError::Io)?.then_some(()).ok_or(FsError::NoSpace)?;
        let saved = fs::file_out_async(disk, CONFIGURATION_DB_PATH).await.map_err(|_| FsError::Io)?;
        if saved.as_deref() != Some(next.as_slice()) { return Err(FsError::Io); }
    }
    let key = update.record.key;
    if !fs::dir_create_all_async(disk, workspace).await.map_err(|_| FsError::Io)?
        || !fs::stamp_directory_key_async(disk, workspace, key).await.map_err(|_| FsError::PermissionDenied)? {
        return Err(FsError::NoSpace);
    }
    crate::allocators::with_host_alloc_domain_strong(|| {
        let mut database = ACCESS_KEY_DB.lock();
        match database.register_container(container.clone(), ContainerPolicy::Private { owner: account }) {
            Ok(()) | Err(trueos_fs::access::PolicyError::ContainerAlreadyRegistered) => {}
            Err(_) => return Err(FsError::PermissionDenied),
        }
        database.add_member(&container, account).map_err(|_| FsError::PermissionDenied)?;
        database.register_key(key);
        database.grant(account, GrantScope::App(container.app().clone()), key)
            .map_err(|_| FsError::PermissionDenied)?;
        Ok(())
    })?;
    Ok(key)
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
    principal: Principal,
    warnings: Vec<Warning>,
}

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
fn container_for_workspace(archive: &str, workspace: &str) -> Option<ContainerId> {
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
        *slot.lock() = Some(Binding {
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
