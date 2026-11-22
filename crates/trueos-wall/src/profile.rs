use crate::{Scope, Transport};

pub(crate) struct AppProfile {
    pub name: &'static str,
    pub rules: &'static [Rule],
}

pub(crate) struct Rule {
    pub transport: Transport,
    pub scope: Scope,
    pub inbound: &'static [u16],
    pub outbound: &'static [u16],
}

const fn service(name: &'static str, rules: &'static [Rule]) -> AppProfile {
    AppProfile { name, rules }
}

const fn tcp(scope: Scope, inbound: &'static [u16], outbound: &'static [u16]) -> Rule {
    Rule {
        transport: Transport::Tcp,
        scope,
        inbound,
        outbound,
    }
}

const fn udp(scope: Scope, inbound: &'static [u16], outbound: &'static [u16]) -> Rule {
    Rule {
        transport: Transport::Udp,
        scope,
        inbound,
        outbound,
    }
}

use Scope::{Local, Network};

const WEB: Rule = tcp(Network, &[], &[80, 443]);
const HTTPS: Rule = tcp(Network, &[], &[443]);
const IDEDIT_HTML: u16 = 42_128;

pub(crate) const KERNEL_SERVICES: &[AppProfile] = &[
    service("logtotcp", &[tcp(Network, &[1], &[])]),
    service("ws-time", &[tcp(Network, &[2], &[])]),
    service("shell3-ssh", &[tcp(Network, &[22], &[])]),
    service("http-trueosfs", &[tcp(Network, &[80], &[80])]),
    service(
        "audio-http",
        &[tcp(Network, &[82], &[]), tcp(Local, &[82], &[])],
    ),
    service(
        "vm-store-replication",
        &[tcp(Network, &[32_123], &[32_123])],
    ),
    service(
        "http-fetch-and-update",
        &[WEB, tcp(Network, &[], &[8080]), tcp(Local, &[], &[80, 443])],
    ),
    service("secure-dns", &[tcp(Network, &[], &[443, 853])]),
    service("smtp", &[tcp(Network, &[], &[587])]),
    service("pop3", &[tcp(Network, &[], &[995])]),
    service("irc", &[tcp(Network, &[], &[6667, 6697])]),
    service("dhcpv4", &[udp(Network, &[68], &[67])]),
    service("dhcpv6", &[udp(Network, &[546], &[547])]),
    service("ntp-and-sntp", &[udp(Network, &[123], &[123])]),
    service(
        "printer-discovery-and-ipp",
        &[udp(Network, &[5353], &[5353]), tcp(Network, &[], &[631])],
    ),
    service(
        "lan-discovery",
        &[
            udp(Network, &[32_343], &[32_343]),
            udp(Local, &[], &[32_345]),
        ],
    ),
    service("h264-stream", &[udp(Network, &[], &[9650])]),
];

pub(crate) const BUILDIN_PROFILES: &[AppProfile] = &[
    service("apps", &[WEB]),
    service(
        "hull",
        &[
            WEB,
            tcp(
                Local,
                &[],
                &[
                    4,
                    10,
                    12,
                    18,
                    80,
                    82,
                    443,
                    1011,
                    1455,
                    1457,
                    4242,
                    4918,
                    8338,
                    8787,
                    9200,
                    9771,
                    9773,
                    11_011,
                    IDEDIT_HTML,
                    45_329,
                    48_080,
                    54_321,
                ],
            ),
        ],
    ),
    service("os", &[HTTPS]),
    service("share", &[tcp(Network, &[4918], &[])]),
    service("ssh", &[tcp(Network, &[], &[22]), tcp(Local, &[], &[22])]),
    service("trauma", &[WEB]),
];

pub(crate) const BLUEPRINT_PROFILES: &[AppProfile] = &[
    service("PlotTwist", &[tcp(Network, &[8338], &[])]),
    service("Frog", &[WEB]),
    service("bios", &[tcp(Network, &[8338], &[])]),
    service(
        "codex-cli",
        &[
            WEB,
            tcp(Local, &[1455, 1457], &[1455, 1457, 42_116, 42_127]),
        ],
    ),
    service(
        "cubes",
        &[tcp(Local, &[], &[18]), udp(Local, &[], &[30_018])],
    ),
    service(
        "cubesrv",
        &[tcp(Network, &[18], &[]), udp(Network, &[30_018], &[])],
    ),
    service("emomodel", &[HTTPS]),
    service("esp32_stt", &[udp(Network, &[5004], &[])]),
    service("hello_world_replicatable", &[tcp(Network, &[48_080], &[])]),
    service("idedit_htmlbuild_srv", &[tcp(Local, &[IDEDIT_HTML], &[])]),
    service("ipp-printer", &[tcp(Network, &[], &[631])]),
    service(
        "lico",
        &[tcp(Local, &[42_127], &[]), udp(Network, &[9_696], &[])],
    ),
    service("lily2vec", &[tcp(Local, &[9771], &[])]),
    service("lweb", &[tcp(Local, &[8787], &[])]),
    service("monaco", &[tcp(Network, &[1011], &[])]),
    service("osm", &[HTTPS]),
    service("pi_snake", &[tcp(Network, &[45_329], &[])]),
    service("prismq", &[tcp(Network, &[8338], &[])]),
    service(
        "ptts",
        &[tcp(Local, &[9773], &[82, 9771]), tcp(Network, &[], &[8787])],
    ),
    service("ptts-check", &[tcp(Local, &[], &[9771, 9773])]),
    service("rdp", &[udp(Network, &[], &[43_402])]),
    service(
        "rebsky",
        &[
            tcp(Network, &[37_202], &[37_201, 37_202]),
            udp(Network, &[37_202], &[37_202]),
        ],
    ),
    service(
        "reticulum",
        &[tcp(Network, &[4242], &[7822]), tcp(Local, &[], &[4242])],
    ),
    service("rustc-min", &[HTTPS]),
    service("sqlite", &[tcp(Local, &[40_894, 42_116], &[])]),
    service(
        "superseedr",
        &[
            WEB,
            tcp(Network, &[6681], &[6681]),
            udp(Network, &[6681], &[6681, 6881]),
        ],
    ),
    service(
        "swarm",
        &[
            tcp(Network, &[12], &[8080, 32_344]),
            udp(Local, &[32_345], &[]),
        ],
    ),
    service("webmail", &[tcp(Network, &[4], &[587])]),
    service("websys", &[tcp(Network, &[10, 11_011, 54_321], &[])]),
    service(
        "xerj-server",
        &[tcp(Local, &[8080, 8081, 9200, 9201, 9202], &[9200])],
    ),
];

// An application outside apps.json can be granted exact ports here by name.
pub(crate) const EXTRA_BLUEPRINTS: &[AppProfile] = &[];
