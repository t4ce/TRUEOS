use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{net::Ipv4Addr, path::Path, process::Command};

use crate::snapshot::FILES;

#[derive(Deserialize)]
struct Route {
    dev: Option<String>,
}

#[derive(Deserialize)]
struct Interface {
    #[serde(default)]
    addr_info: Vec<Address>,
}

#[derive(Deserialize)]
struct Address {
    family: String,
    scope: String,
    local: Ipv4Addr,
}

fn ip(args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("ip")
        .args(args)
        .output()
        .context("Cannot run ip (iproute2)")?;
    if !output.status.success() {
        bail!("ip: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(output.stdout)
}

pub fn interface_and_address(interface: Option<String>) -> Result<(String, Ipv4Addr)> {
    let interface = match interface {
        Some(interface) => interface,
        None => {
            serde_json::from_slice::<Vec<Route>>(&ip(&["-j", "-4", "route", "show", "default"])?)?
                .into_iter()
                .next()
                .and_then(|route| route.dev)
                .context("No default IPv4 interface; specify an interface explicitly")?
        }
    };
    let address = serde_json::from_slice::<Vec<Interface>>(&ip(&[
        "-j", "-4", "addr", "show", "dev", &interface,
    ])?)?
    .into_iter()
    .flat_map(|interface| interface.addr_info)
    .find(|address| address.family == "inet" && address.scope == "global")
    .with_context(|| format!("No global IPv4 address on {interface}"))?;
    Ok((interface, address.local))
}

pub fn dnsmasq_args(interface: &str, ip: Ipv4Addr, root: &Path) -> Vec<String> {
    vec![
        "--conf-file=/dev/null".into(),
        "--no-daemon".into(),
        "--port=0".into(),
        format!("--interface={interface}"),
        "--bind-interfaces".into(),
        format!("--listen-address={ip}"),
        format!("--no-dhcpv6-interface={interface}"),
        format!("--dhcp-range={ip},proxy"),
        "--dhcp-no-override".into(),
        "--dhcp-match=set:efi64,option:client-arch,7".into(),
        "--dhcp-match=set:efi64,option:client-arch,9".into(),
        "--dhcp-ignore=tag:!efi64".into(),
        format!("--dhcp-boot=tag:efi64,{},,{ip}", FILES[0]),
        format!(
            "--pxe-service=tag:efi64,7,TRUEOS native UEFI,{},{ip}",
            FILES[0]
        ),
        format!(
            "--pxe-service=tag:efi64,9,TRUEOS native UEFI,{},{ip}",
            FILES[0]
        ),
        format!("--enable-tftp={interface}"),
        format!("--tftp-root={}", root.display()),
        "--log-dhcp".into(),
    ]
}

pub fn validate_dnsmasq(args: &[String]) -> Result<()> {
    let check = Command::new("dnsmasq")
        .arg("--test")
        .args(args)
        .output()
        .context("Cannot run dnsmasq")?;
    if !check.status.success() {
        bail!(
            "dnsmasq configuration: {}",
            String::from_utf8_lossy(&check.stderr).trim()
        );
    }
    Ok(())
}
