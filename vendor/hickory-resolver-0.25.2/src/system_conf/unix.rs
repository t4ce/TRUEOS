// Copyright 2015-2017 Benjamin Fry <benjaminfry@me.com>
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// https://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// https://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! System configuration loading
//!
//! This module is responsible for parsing and returning the configuration from
//!  the host system. It will read from the default location on each operating
//!  system, e.g. most Unixes have this written to `/etc/resolv.conf`

use std::fs::File;
use std::io;
use std::io::Read;
use std::net::SocketAddr;
use std::path::Path;
use core::str::FromStr;
use core::time::Duration;

use resolv_conf;

use crate::ResolveError;
use crate::config::{NameServerConfig, ResolverConfig, ResolverOpts};
use crate::proto::rr::Name;
use crate::proto::xfer::Protocol;

const DEFAULT_PORT: u16 = 53;

pub fn read_system_conf() -> Result<(ResolverConfig, ResolverOpts), ResolveError> {
    read_resolv_conf("/etc/resolv.conf")
}

fn read_resolv_conf<P: AsRef<Path>>(
    path: P,
) -> Result<(ResolverConfig, ResolverOpts), ResolveError> {
    let mut data = String::new();
    let mut file = File::open(path)?;
    file.read_to_string(&mut data)?;
    parse_resolv_conf(&data)
}

pub fn parse_resolv_conf<T: AsRef<[u8]>>(
    data: T,
) -> Result<(ResolverConfig, ResolverOpts), ResolveError> {
    let parsed_conf = resolv_conf::Config::parse(&data).map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("Error parsing resolv.conf: {e}"),
        )
    })?;
    into_resolver_config(parsed_conf)
}

// TODO: use a custom parsing error type maybe?
fn into_resolver_config(
    parsed_config: resolv_conf::Config,
) -> Result<(ResolverConfig, ResolverOpts), ResolveError> {
    let domain = if let Some(domain) = parsed_config.get_system_domain() {
        // The system domain name maybe appear to be valid to the resolv_conf
        // crate but actually be invalid. For example, if the hostname is "matt.schulte's computer"
        // In order to prevent a hostname which macOS or Windows would consider
        // valid from returning an error here we turn parse errors to options
        Name::from_str(domain.as_str()).ok()
    } else {
        None
    };

    // nameservers
    let mut nameservers = Vec::<NameServerConfig>::with_capacity(parsed_config.nameservers.len());
    for ip in &parsed_config.nameservers {
        nameservers.push(NameServerConfig {
            socket_addr: SocketAddr::new(ip.into(), DEFAULT_PORT),
            protocol: Protocol::Udp,
            tls_dns_name: None,
            http_endpoint: None,
            trust_negative_responses: false,
            bind_addr: None,
        });
        nameservers.push(NameServerConfig {
            socket_addr: SocketAddr::new(ip.into(), DEFAULT_PORT),
            protocol: Protocol::Tcp,
            tls_dns_name: None,
            http_endpoint: None,
            trust_negative_responses: false,
            bind_addr: None,
        });
    }
    if nameservers.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "no nameservers found in config",
        ))?;
    }

    // search
    let mut search = vec![];
    for search_domain in parsed_config.get_last_search_or_domain() {
        // Ignore invalid search domains
        if search_domain == "--" {
            continue;
        }

        search.push(Name::from_str_relaxed(search_domain).map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Error parsing resolv.conf: {e}"),
            )
        })?);
    }

    let config = ResolverConfig::from_parts(domain, search, nameservers);

    let options = ResolverOpts {
        ndots: parsed_config.ndots as usize,
        timeout: Duration::from_secs(u64::from(parsed_config.timeout)),
        attempts: parsed_config.attempts as usize,
        edns0: parsed_config.edns0,
        ..ResolverOpts::default()
    };

    Ok((config, options))
}
