//! Bounded routing between an Edit workspace owner and independent VMX clients.
//!
//! The owner retains all source, editor, and build semantics. This registry only
//! binds opaque endpoints and request tickets to host-observed VM incarnations.
//! Reads are immediate and return an empty body when no frame is available.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use spin::Mutex;

use crate::r::cabi_codes::{
    FS_ERR_ALREADY_EXISTS, FS_ERR_BAD_PARAM, FS_ERR_BAD_PATH, FS_ERR_NO_SPACE, FS_ERR_NOT_FOUND,
    FS_ERR_TOO_LARGE,
};

pub(crate) const VFILE_PREFIX: &str = "vFile:ide/";
pub(crate) const MAX_FRAME_BYTES: usize = 64 * 1024;
const MAX_ENDPOINTS: usize = 16;
const QUEUE_LIMIT: usize = 16;
const VM_LIMIT: usize = crate::allcaps::hv::VM_ID_LIMIT;

pub(crate) fn is_read(path: &str) -> bool {
    matches!(
        path,
        "vFile:ide/discover" | "vFile:ide/poll" | "vFile:ide/reply"
    )
}

pub(crate) fn is_write(path: &str) -> bool {
    matches!(
        path,
        "vFile:ide/register"
            | "vFile:ide/request"
            | "vFile:ide/reply"
            | "vFile:ide/cancel"
            | "vFile:ide/unregister"
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Identity {
    vm: u8,
    run: u64,
    instance: [u8; 16],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    version: u32,
    project: String,
    database: String,
}

#[derive(Clone, Serialize)]
struct Descriptor {
    endpoint: u64,
    project: String,
    database: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    matrix_slot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    matrix_lifetime: Option<u64>,
}

const fn version_one() -> u32 {
    1
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(default = "version_one")]
    version: u32,
    endpoint: u64,
    request_id: u64,
    command: Value,
}

#[derive(Serialize)]
struct Delivery {
    endpoint: u64,
    client_token: u64,
    request_id: u64,
    command: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    #[serde(default = "version_one", skip_serializing)]
    version: u32,
    endpoint: u64,
    client_token: u64,
    request_id: u64,
    reply: Value,
}

struct Queued {
    client_token: u64,
    bytes: Vec<u8>,
}

struct Endpoint {
    identity: Identity,
    descriptor: Descriptor,
    database_key: String,
    queue: VecDeque<Queued>,
}

struct Pending {
    identity: Identity,
    endpoint: u64,
    client_token: u64,
    request_id: u64,
    delivered: bool,
    reply: Option<Vec<u8>>,
}

struct Registry {
    endpoints: [Option<Endpoint>; VM_LIMIT],
    pending: [Option<Pending>; VM_LIMIT],
    next_token: u64,
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>, i32> {
    let bytes = serde_json::to_vec(value).map_err(|_| FS_ERR_BAD_PARAM)?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(FS_ERR_TOO_LARGE);
    }
    Ok(bytes)
}

impl Registry {
    const fn new() -> Self {
        Self {
            endpoints: [const { None }; VM_LIMIT],
            pending: [const { None }; VM_LIMIT],
            next_token: 1,
        }
    }

    fn token(&mut self) -> Result<u64, i32> {
        let token = self.next_token;
        self.next_token = token.checked_add(1).ok_or(FS_ERR_NO_SPACE)?;
        Ok(token)
    }

    fn retire_endpoint(&mut self, index: usize) {
        let Some(endpoint) = self.endpoints[index].take() else {
            return;
        };
        for pending in self.pending.iter_mut().flatten() {
            if pending.endpoint == endpoint.descriptor.endpoint && pending.reply.is_none() {
                // This fixed failure envelope is far below MAX_FRAME_BYTES.
                pending.reply = Some(
                    encode(&Reply {
                        version: 1,
                        endpoint: pending.endpoint,
                        client_token: pending.client_token,
                        request_id: pending.request_id,
                        reply: serde_json::json!({
                            "status": "error", "code": "unavailable",
                            "message": "The IDE workspace owner is no longer available."
                        }),
                    })
                    .expect("fixed IDE failure envelope exceeds limit"),
                );
            }
        }
    }

    fn cancel(&mut self, identity: Identity) {
        let index = identity.vm as usize;
        if !self.pending[index]
            .as_ref()
            .is_some_and(|pending| pending.identity == identity)
        {
            return;
        }
        let pending = self.pending[index]
            .take()
            .expect("matching pending request");
        for endpoint in self.endpoints.iter_mut().flatten() {
            endpoint
                .queue
                .retain(|queued| queued.client_token != pending.client_token);
        }
    }

    fn release_run(&mut self, vm: u8, run: u64) {
        let index = vm as usize;
        if index >= VM_LIMIT {
            return;
        }
        if self.endpoints[index]
            .as_ref()
            .is_some_and(|endpoint| endpoint.identity.run == run)
        {
            self.retire_endpoint(index);
        }
        if let Some(identity) = self.pending[index]
            .as_ref()
            .filter(|pending| pending.identity.run == run)
            .map(|pending| pending.identity)
        {
            self.cancel(identity);
        }
    }

    fn reap(&mut self, identities: &[Option<Identity>; VM_LIMIT]) {
        for (index, identity) in identities.iter().enumerate() {
            if self.endpoints[index]
                .as_ref()
                .is_some_and(|endpoint| Some(endpoint.identity) != *identity)
            {
                self.retire_endpoint(index);
            }
            if let Some(old) = self.pending[index]
                .as_ref()
                .filter(|pending| Some(pending.identity) != *identity)
                .map(|pending| pending.identity)
            {
                self.cancel(old);
            }
        }
    }

    fn register(
        &mut self,
        identity: Identity,
        registration: Registration,
        database_key: String,
        matrix: Option<(String, u64)>,
    ) -> Result<(), i32> {
        if registration.version != 1
            || registration.project.is_empty()
            || registration.project.len() > 128
            || registration.project.contains('\0')
        {
            return Err(FS_ERR_BAD_PARAM);
        }
        if self
            .endpoints
            .iter()
            .flatten()
            .any(|endpoint| endpoint.database_key == database_key && endpoint.identity != identity)
        {
            return Err(FS_ERR_ALREADY_EXISTS);
        }
        let index = identity.vm as usize;
        if let Some(endpoint) = self.endpoints[index].as_ref() {
            if endpoint.identity == identity
                && endpoint.database_key == database_key
                && endpoint.descriptor.project == registration.project
            {
                return Ok(());
            }
        } else if self.endpoints.iter().flatten().count() >= MAX_ENDPOINTS {
            return Err(FS_ERR_NO_SPACE);
        }
        let token = self.token()?;
        // A different active project is a new endpoint, even in the same VM.
        // Already accepted requests must never silently follow that switch.
        self.retire_endpoint(index);
        self.endpoints[index] = Some(Endpoint {
            identity,
            descriptor: Descriptor {
                endpoint: token,
                project: registration.project,
                database: registration.database,
                matrix_slot: matrix.as_ref().map(|(slot, _)| slot.clone()),
                matrix_lifetime: matrix.map(|(_, lifetime)| lifetime),
            },
            database_key,
            queue: VecDeque::new(),
        });
        Ok(())
    }

    fn discover(&self, available: &[bool; VM_LIMIT]) -> Result<Vec<u8>, i32> {
        let descriptors: Vec<_> = self
            .endpoints
            .iter()
            .flatten()
            .filter(|endpoint| available[endpoint.identity.vm as usize])
            .map(|endpoint| &endpoint.descriptor)
            .collect();
        encode(&descriptors)
    }

    fn request(
        &mut self,
        identity: Identity,
        request: Request,
        available: &[bool; VM_LIMIT],
    ) -> Result<(), i32> {
        if request.version != 1 || request.endpoint == 0 || request.request_id == 0 {
            return Err(FS_ERR_BAD_PARAM);
        }
        if self.pending[identity.vm as usize].is_some() {
            return Err(FS_ERR_ALREADY_EXISTS);
        }
        let index = self
            .endpoints
            .iter()
            .position(|entry| {
                entry.as_ref().is_some_and(|endpoint| {
                    endpoint.descriptor.endpoint == request.endpoint
                        && available[endpoint.identity.vm as usize]
                })
            })
            .ok_or(FS_ERR_NOT_FOUND)?;
        if self.endpoints[index]
            .as_ref()
            .expect("selected endpoint")
            .queue
            .len()
            >= QUEUE_LIMIT
        {
            return Err(FS_ERR_NO_SPACE);
        }
        // A token identifies this one accepted request, not merely its client.
        // Cancel/retry remains safe even if the client reuses request_id.
        let client_token = self.token()?;
        let delivery = Delivery {
            endpoint: request.endpoint,
            client_token,
            request_id: request.request_id,
            command: request.command,
        };
        let bytes = encode(&delivery)?;
        self.pending[identity.vm as usize] = Some(Pending {
            identity,
            endpoint: request.endpoint,
            client_token,
            request_id: request.request_id,
            delivered: false,
            reply: None,
        });
        self.endpoints[index]
            .as_mut()
            .expect("selected endpoint")
            .queue
            .push_back(Queued {
                client_token,
                bytes,
            });
        Ok(())
    }

    fn poll(&mut self, identity: Identity) -> Result<Vec<u8>, i32> {
        let endpoint = self.endpoints[identity.vm as usize]
            .as_mut()
            .filter(|endpoint| endpoint.identity == identity)
            .ok_or(FS_ERR_NOT_FOUND)?;
        while let Some(queued) = endpoint.queue.pop_front() {
            if let Some(pending) = self.pending.iter_mut().flatten().find(|pending| {
                pending.client_token == queued.client_token && pending.reply.is_none()
            }) {
                pending.delivered = true;
                return Ok(queued.bytes);
            }
        }
        Ok(Vec::new())
    }

    fn reply(&mut self, identity: Identity, reply: Reply) -> Result<(), i32> {
        if reply.version != 1 {
            return Err(FS_ERR_BAD_PARAM);
        }
        if !self.endpoints[identity.vm as usize]
            .as_ref()
            .is_some_and(|endpoint| {
                endpoint.identity == identity && endpoint.descriptor.endpoint == reply.endpoint
            })
        {
            return Err(FS_ERR_NOT_FOUND);
        }
        let pending = self
            .pending
            .iter_mut()
            .flatten()
            .find(|pending| {
                pending.endpoint == reply.endpoint
                    && pending.client_token == reply.client_token
                    && pending.request_id == reply.request_id
                    && pending.delivered
            })
            .ok_or(FS_ERR_NOT_FOUND)?;
        if pending.reply.is_some() {
            return Err(FS_ERR_ALREADY_EXISTS);
        }
        pending.reply = Some(encode(&reply)?);
        Ok(())
    }

    fn take_reply(&mut self, identity: Identity) -> Vec<u8> {
        let slot = &mut self.pending[identity.vm as usize];
        let Some(pending) = slot.as_mut().filter(|pending| pending.identity == identity) else {
            return Vec::new();
        };
        let Some(bytes) = pending.reply.take() else {
            return Vec::new();
        };
        *slot = None;
        bytes
    }

    fn unregister(&mut self, identity: Identity) {
        if self.endpoints[identity.vm as usize]
            .as_ref()
            .is_some_and(|endpoint| endpoint.identity == identity)
        {
            self.retire_endpoint(identity.vm as usize);
        }
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());

fn current_identity(vm: u8) -> Option<Identity> {
    let run = crate::hv::vm_run_generation(vm).filter(|run| *run != 0)?;
    let instance = crate::hv::blueprint_instance_identity(vm)?.instance;
    (crate::hv::vm_run_generation(vm) == Some(run)).then_some(Identity { vm, run, instance })
}

fn available(vm: u8) -> bool {
    let state = crate::hv::vm_state(vm);
    state.supported
        && state.running
        && !state.starting
        && !state.stop_requested
        && !state.pause_requested
        && !state.paused
        && !state.restore_inflight
}

fn caller(owner: u32) -> Result<Identity, i32> {
    let vm = owner & !0x8000_0000;
    if owner & 0x8000_0000 == 0 || vm >= VM_LIMIT as u32 || !available(vm as u8) {
        return Err(FS_ERR_NOT_FOUND);
    }
    current_identity(vm as u8).ok_or(FS_ERR_NOT_FOUND)
}

// Observe host identity without holding REGISTRY: lifecycle and Matrix locks
// must never be acquired underneath the routing registry lock.
fn snapshot() -> ([Option<Identity>; VM_LIMIT], [bool; VM_LIMIT]) {
    let identities = core::array::from_fn(|index| current_identity(index as u8));
    let available = core::array::from_fn(|index| available(index as u8));
    (identities, available)
}

fn with_registry<T>(
    identity: Identity,
    action: impl FnOnce(&mut Registry, &[bool; VM_LIMIT]) -> Result<T, i32>,
) -> Result<T, i32> {
    loop {
        // Never reap an endpoint installed after the host identity snapshot.
        // New endpoints and request tickets both advance this sequence.
        let observed = REGISTRY.lock().next_token;
        let (identities, available) = snapshot();
        let mut registry = REGISTRY.lock();
        if registry.next_token != observed {
            drop(registry);
            continue;
        }
        registry.reap(&identities);
        if identities[identity.vm as usize] != Some(identity) || !available[identity.vm as usize] {
            return Err(FS_ERR_NOT_FOUND);
        }
        return action(&mut registry, &available);
    }
}

/// Async-FS must reserve a result slot before calling this consuming read.
pub(crate) fn read_for_owner(owner: u32, path: &str) -> Result<Vec<u8>, i32> {
    let identity = caller(owner)?;
    crate::allocators::with_host_alloc_domain_strong(|| {
        with_registry(identity, |registry, available| match path {
            "vFile:ide/discover" => registry.discover(available),
            "vFile:ide/poll" => registry.poll(identity),
            "vFile:ide/reply" => Ok(registry.take_reply(identity)),
            _ => Err(FS_ERR_BAD_PARAM),
        })
    })
}

pub(crate) fn write_for_owner(owner: u32, path: &str, bytes: &[u8]) -> Result<(), i32> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(FS_ERR_TOO_LARGE);
    }
    crate::allocators::with_host_alloc_domain_strong(|| write_inner(owner, path, bytes))
}

fn write_inner(owner: u32, path: &str, bytes: &[u8]) -> Result<(), i32> {
    let identity = caller(owner)?;
    let registration = if path == "vFile:ide/register" {
        let registration: Registration =
            serde_json::from_slice(bytes).map_err(|_| FS_ERR_BAD_PARAM)?;
        if registration.database.is_empty()
            || registration.database.len() > 1024
            || registration.database.starts_with("vFile:")
        {
            return Err(FS_ERR_BAD_PATH);
        }
        let resolved = crate::hv::with_guest_broker_context(identity.vm, || {
            crate::r::io::env::resolve_fs_path(&registration.database, false)
        })
        .ok_or(FS_ERR_BAD_PATH)?;
        let database_key = crate::r::io::async_fs_cabi::canonical_file_key(&resolved)?;
        let matrix = crate::hv::blueprint_console_target(identity.vm).map(|target| {
            let lease = crate::shell3::matrix_target_slot_lease(&target);
            (String::from(lease.name()), lease.lifetime_generation())
        });
        Some((registration, database_key, matrix))
    } else {
        None
    };
    with_registry(identity, |registry, available| match path {
        "vFile:ide/register" => {
            let (registration, database_key, matrix) = registration.expect("parsed registration");
            registry.register(identity, registration, database_key, matrix)
        }
        "vFile:ide/request" => registry.request(
            identity,
            serde_json::from_slice(bytes).map_err(|_| FS_ERR_BAD_PARAM)?,
            available,
        ),
        "vFile:ide/reply" => registry.reply(
            identity,
            serde_json::from_slice(bytes).map_err(|_| FS_ERR_BAD_PARAM)?,
        ),
        "vFile:ide/cancel" | "vFile:ide/unregister" if bytes.is_empty() => {
            if path == "vFile:ide/cancel" {
                registry.cancel(identity);
            } else {
                registry.unregister(identity);
            }
            Ok(())
        }
        _ => Err(FS_ERR_BAD_PARAM),
    })
}

/// Called after pending async-FS work drains, before this VM run is reclaimed.
/// A retained pause keeps its writer lease; it is only hidden from discovery.
pub(crate) fn release_vm(vm: u8, run: u64) {
    crate::allocators::with_host_alloc_domain_strong(|| REGISTRY.lock().release_run(vm, run));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(vm: u8, run: u64) -> Identity {
        Identity {
            vm,
            run,
            instance: [run as u8; 16],
        }
    }
    fn register(registry: &mut Registry, owner: Identity) -> u64 {
        registry
            .register(
                owner,
                Registration {
                    version: 1,
                    project: "demo".into(),
                    database: "common/ide/projects.redb".into(),
                },
                "apps/common/ide/projects.redb".into(),
                None,
            )
            .unwrap();
        registry.endpoints[owner.vm as usize]
            .as_ref()
            .unwrap()
            .descriptor
            .endpoint
    }
    fn request(registry: &mut Registry, client: Identity, endpoint: u64) -> Result<(), i32> {
        registry.request(
            client,
            Request {
                version: 1,
                endpoint,
                request_id: 7,
                command: serde_json::json!({"op":"context"}),
            },
            &[true; VM_LIMIT],
        )
    }
    fn reply(delivery: &[u8]) -> Reply {
        let frame: Value = serde_json::from_slice(delivery).unwrap();
        Reply {
            version: 1,
            endpoint: frame["endpoint"].as_u64().unwrap(),
            client_token: frame["client_token"].as_u64().unwrap(),
            request_id: 7,
            reply: serde_json::json!({"status":"ok"}),
        }
    }

    #[test]
    fn writer_lease_is_exclusive_and_registration_is_idempotent() {
        let mut registry = Registry::new();
        let owner = identity(0, 1);
        let endpoint = register(&mut registry, owner);
        assert_eq!(register(&mut registry, owner), endpoint);
        assert_eq!(
            registry.register(
                identity(1, 1),
                Registration {
                    version: 1,
                    project: "other".into(),
                    database: "/apps/common/ide/projects.redb".into()
                },
                "apps/common/ide/projects.redb".into(),
                None
            ),
            Err(FS_ERR_ALREADY_EXISTS)
        );
        assert!(registry.discover(&[false; VM_LIMIT]).unwrap() == b"[]");
        assert!(
            registry.endpoints[0].is_some(),
            "pause must retain the writer lease"
        );
    }

    #[test]
    fn wrong_owner_and_repeated_replies_cannot_replace_a_result() {
        let mut registry = Registry::new();
        let owner = identity(0, 1);
        let client = identity(1, 1);
        let endpoint = register(&mut registry, owner);
        request(&mut registry, client, endpoint).unwrap();
        assert_eq!(
            request(&mut registry, client, endpoint),
            Err(FS_ERR_ALREADY_EXISTS)
        );
        let delivery = registry.poll(owner).unwrap();
        assert_eq!(
            registry.reply(identity(2, 1), reply(&delivery)),
            Err(FS_ERR_NOT_FOUND)
        );
        registry.reply(owner, reply(&delivery)).unwrap();
        assert_eq!(
            registry.reply(owner, reply(&delivery)),
            Err(FS_ERR_ALREADY_EXISTS)
        );
        assert!(!registry.take_reply(client).is_empty());
        assert!(registry.take_reply(client).is_empty());
    }

    #[test]
    fn cancel_then_reuse_request_id_rejects_the_old_reply() {
        let mut registry = Registry::new();
        let owner = identity(0, 1);
        let client = identity(1, 1);
        let endpoint = register(&mut registry, owner);
        request(&mut registry, client, endpoint).unwrap();
        let old = registry.poll(owner).unwrap();
        registry.cancel(client);
        request(&mut registry, client, endpoint).unwrap();
        let current = registry.poll(owner).unwrap();
        assert_ne!(reply(&old).client_token, reply(&current).client_token);
        assert_eq!(registry.reply(owner, reply(&old)), Err(FS_ERR_NOT_FOUND));
        registry.reply(owner, reply(&current)).unwrap();
    }

    #[test]
    fn owner_replacement_fails_pending_requests_without_following_new_endpoint() {
        let mut registry = Registry::new();
        let owner = identity(0, 1);
        let client = identity(1, 1);
        let endpoint = register(&mut registry, owner);
        request(&mut registry, client, endpoint).unwrap();
        let old = registry.poll(owner).unwrap();
        registry.release_run(0, 1);
        let replacement = register(&mut registry, identity(0, 2));
        assert_ne!(endpoint, replacement);
        let failure: Value = serde_json::from_slice(&registry.take_reply(client)).unwrap();
        assert_eq!(failure["reply"]["code"], "unavailable");
        assert_eq!(
            request(&mut registry, client, endpoint),
            Err(FS_ERR_NOT_FOUND)
        );
        assert_eq!(
            registry.reply(identity(0, 2), reply(&old)),
            Err(FS_ERR_NOT_FOUND)
        );
        registry.release_run(0, 1);
        assert!(
            registry.endpoints[0].is_some(),
            "old cleanup cannot remove a replacement"
        );
    }

    #[test]
    fn client_restart_discards_queued_request_and_does_not_inherit_result() {
        let mut registry = Registry::new();
        let owner = identity(0, 1);
        let client = identity(1, 1);
        let endpoint = register(&mut registry, owner);
        request(&mut registry, client, endpoint).unwrap();
        let mut identities = [None; VM_LIMIT];
        identities[0] = Some(owner);
        identities[1] = Some(identity(1, 2));
        registry.reap(&identities);
        assert!(registry.poll(owner).unwrap().is_empty());
        assert!(registry.take_reply(identity(1, 2)).is_empty());
        request(&mut registry, identity(1, 2), endpoint).unwrap();
    }

    #[test]
    fn oversize_delivery_does_not_claim_client_pending_slot() {
        let mut registry = Registry::new();
        let owner = identity(0, 1);
        let client = identity(1, 1);
        let endpoint = register(&mut registry, owner);
        assert_eq!(
            registry.request(
                client,
                Request {
                    version: 1,
                    endpoint,
                    request_id: 1,
                    command: Value::String("x".repeat(MAX_FRAME_BYTES))
                },
                &[true; VM_LIMIT]
            ),
            Err(FS_ERR_TOO_LARGE)
        );
        request(&mut registry, client, endpoint).unwrap();
    }

    #[test]
    fn queue_capacity_is_bounded_and_cancel_releases_admission() {
        let mut registry = Registry::new();
        let owner = identity(0, 1);
        let endpoint = register(&mut registry, owner);
        for vm in 1..=QUEUE_LIMIT {
            request(&mut registry, identity(vm as u8, 1), endpoint).unwrap();
        }
        let extra = identity((QUEUE_LIMIT + 1) as u8, 1);
        assert_eq!(
            request(&mut registry, extra, endpoint),
            Err(FS_ERR_NO_SPACE)
        );
        assert!(registry.pending[extra.vm as usize].is_none());
        registry.cancel(identity(1, 1));
        request(&mut registry, extra, endpoint).unwrap();
        for _ in 0..QUEUE_LIMIT {
            assert!(!registry.poll(owner).unwrap().is_empty());
        }
        assert!(registry.poll(owner).unwrap().is_empty());
    }
}
