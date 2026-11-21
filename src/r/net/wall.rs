//! Bind static network rules to the host's record of a Blueprint launch.

use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use core::sync::atomic::{AtomicU16, Ordering};

use crate::r::fs::trueosfs::authorized::Actor;
use trueos_wall::Principal;
pub(crate) use trueos_wall::{Scope, Transport};

static NEXT_UDP_CLIENT_PORT: AtomicU16 = AtomicU16::new(49_152);

pub(crate) fn next_udp_client_port() -> u16 {
    NEXT_UDP_CLIENT_PORT
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |port| {
            Some(port.wrapping_add(1).max(49_152))
        })
        .unwrap()
}

pub(crate) fn current_vm() -> Option<u8> {
    crate::hv::current_hull_guest_context_vm_id()
        .or_else(crate::hv::current_guest_broker_vm_id)
        .or_else(crate::hv::current_guest_execution_context_vm_id)
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Caller {
    Kernel,
    Blueprint { vm: u8, generation: u64, epoch: u64 },
}

impl Caller {
    pub(crate) fn capture(vm: Option<u8>) -> Option<Self> {
        let Some(vm) = vm else {
            return Some(Self::Kernel);
        };
        let epoch = crate::access_key_db::principal_epoch(vm)?;
        let generation = crate::hv::vm_run_generation(vm)?;
        Some(Self::Blueprint {
            vm,
            generation,
            epoch,
        })
    }

    pub(crate) fn current() -> Option<Self> {
        Self::capture(current_vm())
    }

    pub(crate) fn actor(self) -> Actor {
        match self {
            Self::Kernel => Actor::Public,
            Self::Blueprint { vm, epoch, .. } => Actor::Vm(vm, epoch),
        }
    }

    pub(crate) fn from_actor(actor: Actor) -> Option<Self> {
        match actor {
            Actor::Host | Actor::Public => Some(Self::Kernel),
            Actor::Vm(vm, _) => Self::capture(Some(vm)).filter(|caller| caller.actor() == actor),
        }
    }

    fn permits(self, check: impl FnOnce(Principal<'_>) -> bool) -> bool {
        match self {
            Self::Kernel => check(Principal::Kernel),
            Self::Blueprint {
                vm,
                generation,
                epoch,
            } => {
                if crate::access_key_db::principal_epoch(vm) != Some(epoch)
                    || crate::hv::vm_run_generation(vm) != Some(generation)
                {
                    return false;
                }
                crate::hv::with_blueprint_network_identity(vm, generation, |archive, buildin| {
                    let name = archive.strip_suffix(".bp").unwrap_or(archive);
                    check(if buildin {
                        Principal::Buildin(name)
                    } else {
                        Principal::Blueprint(name)
                    })
                })
                .unwrap_or(false)
            }
        }
    }

    pub(crate) fn permits_socket(self, transport: Transport) -> bool {
        self.permits(|principal| trueos_wall::permits_socket(principal, transport))
    }

    pub(crate) fn permits_dns(self) -> bool {
        self.permits(trueos_wall::permits_dns)
    }

    pub(crate) fn permits_inbound(self, transport: Transport, scope: Scope, port: u16) -> bool {
        self.permits(|principal| trueos_wall::permits_inbound(principal, transport, scope, port))
    }

    pub(crate) fn permits_bind(self, transport: Transport, local: SocketAddr) -> bool {
        self.restrict_bind(transport, local).is_some()
    }

    pub(crate) fn restrict_bind(
        self,
        transport: Transport,
        mut local: SocketAddr,
    ) -> Option<SocketAddr> {
        if self.permits_inbound(transport, scope(local.ip()), local.port()) {
            return Some(local);
        }
        if local.ip().is_unspecified()
            && self.permits_inbound(transport, Scope::Local, local.port())
        {
            local.set_ip(match local {
                SocketAddr::V4(_) => Ipv4Addr::LOCALHOST.into(),
                SocketAddr::V6(_) => Ipv6Addr::LOCALHOST.into(),
            });
            return Some(local);
        }
        None
    }

    pub(crate) fn permits_outbound(self, transport: Transport, remote: SocketAddr) -> bool {
        self.permits_outbound_port(transport, scope(remote.ip()), remote.port())
    }

    pub(crate) fn permits_outbound_port(
        self,
        transport: Transport,
        scope: Scope,
        port: u16,
    ) -> bool {
        self.permits(|principal| trueos_wall::permits_outbound(principal, transport, scope, port))
    }
}

pub(crate) fn scope(address: IpAddr) -> Scope {
    if address.to_canonical().is_loopback() {
        Scope::Local
    } else {
        Scope::Network
    }
}
