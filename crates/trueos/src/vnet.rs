/// Virtual networking surface API.
///
/// This is meant to be stable and portable for higher layers (e.g. containers).
/// The kernel adapts these types to its internal network stack and device drivers.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NetHandle(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MacAddr(pub [u8; 6]);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EndpointV4 {
    pub addr: [u8; 4],
    pub port: u16,
}

impl EndpointV4 {
    pub const fn new(addr: [u8; 4], port: u16) -> Self {
        Self { addr, port }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EndpointV6 {
    pub addr: [u8; 16],
    pub port: u16,
}

impl EndpointV6 {
    pub const fn new(addr: [u8; 16], port: u16) -> Self {
        Self { addr, port }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SocketKind {
    Udp,
    Tcp,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ByteBuf<const N: usize> {
    len: u16,
    data: [u8; N],
}

impl<const N: usize> ByteBuf<N> {
    pub const fn new() -> Self {
        Self {
            len: 0,
            data: [0u8; N],
        }
    }

    pub fn from_slice_trunc(src: &[u8]) -> Self {
        let mut out = Self::new();
        let n = core::cmp::min(N, src.len());
        out.data[..n].copy_from_slice(&src[..n]);
        out.len = n as u16;
        out
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.len()]
    }
}

pub const MAX_MSG: usize = 8192;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Command {
    OpenUdp {
        port: u16,
    },
    OpenTcpListen {
        port: u16,
    },
    OpenTcpConnect {
        remote: EndpointV4,
    },
    OpenTcpConnectV6 {
        remote: EndpointV6,
    },
    SendUdp {
        handle: NetHandle,
        remote: EndpointV4,
        data: ByteBuf<MAX_MSG>,
    },
    SendUdpV6 {
        handle: NetHandle,
        remote: EndpointV6,
        data: ByteBuf<MAX_MSG>,
    },
    SendTcp {
        handle: NetHandle,
        data: ByteBuf<MAX_MSG>,
    },
    Close {
        handle: NetHandle,
    },
    IcmpEcho {
        target: [u8; 4],
        seq: u16,
        data: ByteBuf<MAX_MSG>,
    },
    IcmpEchoV6 {
        target: [u8; 16],
        seq: u16,
        data: ByteBuf<MAX_MSG>,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Opened {
        handle: NetHandle,
        kind: SocketKind,
    },
    Closed {
        handle: NetHandle,
    },
    Error {
        msg: &'static str,
    },
    UdpPacket {
        handle: NetHandle,
        from: EndpointV4,
        data: ByteBuf<MAX_MSG>,
    },
    UdpPacketV6 {
        handle: NetHandle,
        from: EndpointV6,
        data: ByteBuf<MAX_MSG>,
    },
    TcpEstablished {
        handle: NetHandle,
    },
    TcpData {
        handle: NetHandle,
        data: ByteBuf<MAX_MSG>,
    },
    TcpSent {
        handle: NetHandle,
        len: u16,
    },
    IcmpReply {
        from: [u8; 4],
        seq: u16,
        rtt_ms: u32,
        data: ByteBuf<MAX_MSG>,
    },
    IcmpReplyV6 {
        from: [u8; 16],
        seq: u16,
        rtt_ms: u32,
        data: ByteBuf<MAX_MSG>,
    },
}
