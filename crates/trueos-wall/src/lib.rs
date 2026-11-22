#![no_std]

extern crate alloc;

pub mod packet;
mod profile;

use profile::{AppProfile, Rule};

include!(concat!(env!("OUT_DIR"), "/catalogs.rs"));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Tcp,
    Udp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Local,
    Network,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Principal<'a> {
    Kernel,
    Buildin(&'a str),
    Blueprint(&'a str),
}

fn find_app<'a>(name: &str, profiles: &'a [AppProfile]) -> Option<&'a AppProfile> {
    profiles.iter().find(|profile| profile.name == name)
}

fn app_rules(principal: Principal<'_>) -> &'static [Rule] {
    match principal {
        Principal::Buildin(name) if BUILDINS.binary_search(&name).is_ok() => {
            find_app(name, profile::BUILDIN_PROFILES)
        }
        Principal::Blueprint(name) => {
            let name = match name {
                "codex" => "codex-cli",
                "xerj" => "xerj-server",
                _ => name,
            };
            if BLUEPRINTS.binary_search(&name).is_ok() {
                find_app(name, profile::BLUEPRINT_PROFILES)
            } else {
                find_app(name, profile::EXTRA_BLUEPRINTS)
            }
        }
        _ => None,
    }
    .map_or(&[], |profile| profile.rules)
}

fn principal_has_rule(principal: Principal<'_>, matches: impl Fn(&Rule) -> bool) -> bool {
    if principal == Principal::Kernel {
        profile::KERNEL_SERVICES
            .iter()
            .flat_map(|service| service.rules)
            .any(matches)
    } else {
        app_rules(principal).iter().any(matches)
    }
}

/// Admit a socket of this transport before an endpoint has been selected.
pub fn permits_socket(principal: Principal<'_>, transport: Transport) -> bool {
    principal_has_rule(principal, |rule| rule.transport == transport)
}

/// Admit queries through the kernel resolver, which owns the DoH and DoT transports.
pub fn permits_dns(principal: Principal<'_>) -> bool {
    principal_has_rule(principal, |rule| {
        rule.scope == Scope::Network && !rule.outbound.is_empty()
    })
}

pub fn permits_inbound(
    principal: Principal<'_>,
    transport: Transport,
    scope: Scope,
    port: u16,
) -> bool {
    port != 0
        && principal_has_rule(principal, |rule| {
            rule.transport == transport
                && (rule.scope == scope || scope == Scope::Local && rule.scope == Scope::Network)
                && rule.inbound.contains(&port)
        })
}

pub fn permits_outbound(
    principal: Principal<'_>,
    transport: Transport,
    scope: Scope,
    port: u16,
) -> bool {
    port != 0
        && principal_has_rule(principal, |rule| {
            rule.transport == transport && rule.scope == scope && rule.outbound.contains(&port)
        })
}

fn any_principal(mut permits: impl FnMut(Principal<'_>) -> bool) -> bool {
    permits(Principal::Kernel)
        || BUILDINS
            .iter()
            .any(|name| permits(Principal::Buildin(name)))
        || profile::BLUEPRINT_PROFILES
            .iter()
            .chain(profile::EXTRA_BLUEPRINTS)
            .any(|app| permits(Principal::Blueprint(app.name)))
}

/// Check whether any service may receive new traffic to this port.
pub fn allows_inbound(transport: Transport, scope: Scope, port: u16) -> bool {
    any_principal(|principal| permits_inbound(principal, transport, scope, port))
}

/// Check whether any service may initiate traffic to this port.
pub fn allows_outbound(transport: Transport, scope: Scope, port: u16) -> bool {
    any_principal(|principal| permits_outbound(principal, transport, scope, port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use Scope::{Local, Network};
    use Transport::{Tcp, Udp};

    #[test]
    fn catalog_membership_does_not_grant_a_socket() {
        assert!(BLUEPRINTS.contains(&"hello_world"));
        assert!(!permits_socket(Principal::Blueprint("hello_world"), Tcp));
        assert!(!permits_socket(Principal::Blueprint("unlisted"), Udp));
        assert!(!permits_dns(Principal::Blueprint("hello_world")));
        assert!(permits_dns(Principal::Blueprint("ipp-printer")));
        assert!(!allows_outbound(Udp, Network, 53));
    }

    #[test]
    fn separate_direction_transport_and_app_name() {
        let printer = Principal::Blueprint("ipp-printer");
        assert!(permits_outbound(printer, Tcp, Network, 631));
        assert!(!permits_outbound(printer, Tcp, Network, 632));
        assert!(!permits_outbound(printer, Udp, Network, 631));
        assert!(!permits_inbound(printer, Tcp, Network, 631));
        assert!(!permits_outbound(
            Principal::Blueprint("cubesrv"),
            Tcp,
            Network,
            631
        ));
    }

    #[test]
    fn loopback_services_do_not_open_physical_ports() {
        assert!(permits_inbound(
            Principal::Blueprint("sqlite"),
            Tcp,
            Local,
            42116
        ));
        assert!(permits_outbound(
            Principal::Blueprint("codex"),
            Tcp,
            Local,
            42116
        ));
        assert!(allows_inbound(Udp, Local, 32345));
        assert!(!allows_inbound(Tcp, Network, 42116));
        assert!(!allows_outbound(Tcp, Network, 42116));
        assert!(!allows_inbound(Udp, Network, 32345));
    }

    #[test]
    fn lico_accepts_piano_datagrams_and_keeps_mcp_local() {
        let lico = Principal::Blueprint("lico");
        assert!(permits_inbound(lico, Udp, Network, 9_696));
        assert!(allows_inbound(Udp, Network, 9_696));
        assert!(!permits_inbound(lico, Udp, Network, 9_697));
        assert!(!permits_inbound(lico, Tcp, Network, 9_696));
        assert!(!permits_outbound(lico, Udp, Network, 9_696));
        assert!(!permits_outbound(lico, Udp, Local, 9_696));
        assert!(!permits_inbound(Principal::Kernel, Udp, Network, 9_696));
        assert!(!permits_inbound(
            Principal::Blueprint("cubesrv"),
            Udp,
            Network,
            9_696
        ));
        assert!(permits_inbound(lico, Tcp, Local, 42_127));
        assert!(!permits_inbound(lico, Tcp, Network, 42_127));
    }

    #[test]
    fn buildin_and_blueprint_identities_have_distinct_profiles() {
        if BUILDINS.contains(&"share") {
            assert!(permits_inbound(
                Principal::Buildin("share"),
                Tcp,
                Network,
                4918
            ));
            assert!(!permits_inbound(
                Principal::Blueprint("share"),
                Tcp,
                Network,
                4918
            ));
        }
        assert!(!permits_socket(Principal::Buildin("codex"), Tcp));
    }
}
