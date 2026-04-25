use v::vnet as api;

const TAG_OPEN_UDP: u8 = 1;
const TAG_OPEN_TCP_LISTEN: u8 = 2;
const TAG_OPEN_TCP_CONNECT: u8 = 3;
const TAG_OPEN_TCP_CONNECT_V6: u8 = 4;
const TAG_SEND_UDP: u8 = 5;
const TAG_SEND_UDP_V6: u8 = 6;
const TAG_SEND_TCP: u8 = 7;
const TAG_CLOSE: u8 = 8;
const TAG_ICMP_ECHO: u8 = 9;
const TAG_ICMP_ECHO_V6: u8 = 10;

const TAG_OPENED: u8 = 1;
const TAG_CLOSED: u8 = 2;
const TAG_ERROR: u8 = 3;
const TAG_UDP_PACKET: u8 = 4;
const TAG_UDP_PACKET_V6: u8 = 5;
const TAG_TCP_ESTABLISHED: u8 = 6;
const TAG_TCP_DATA: u8 = 7;
const TAG_TCP_SENT: u8 = 8;
const TAG_ICMP_REPLY: u8 = 9;
const TAG_ICMP_REPLY_V6: u8 = 10;

const KIND_UDP: u8 = 1;
const KIND_TCP: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WireError {
    BufferTooSmall,
    InvalidTag,
    InvalidKind,
    Truncated,
}

struct Writer<'a> {
    out: &'a mut [u8],
    pos: usize,
}

impl<'a> Writer<'a> {
    fn new(out: &'a mut [u8]) -> Self {
        Self { out, pos: 0 }
    }

    fn finish(self) -> usize {
        self.pos
    }

    fn byte(&mut self, value: u8) -> Result<(), WireError> {
        if self.pos >= self.out.len() {
            return Err(WireError::BufferTooSmall);
        }
        self.out[self.pos] = value;
        self.pos += 1;
        Ok(())
    }

    fn bytes(&mut self, value: &[u8]) -> Result<(), WireError> {
        let end = self.pos.checked_add(value.len()).ok_or(WireError::BufferTooSmall)?;
        if end > self.out.len() {
            return Err(WireError::BufferTooSmall);
        }
        self.out[self.pos..end].copy_from_slice(value);
        self.pos = end;
        Ok(())
    }

    fn u16(&mut self, value: u16) -> Result<(), WireError> {
        self.bytes(&value.to_le_bytes())
    }

    fn u32(&mut self, value: u32) -> Result<(), WireError> {
        self.bytes(&value.to_le_bytes())
    }

    fn byte_buf<const N: usize>(&mut self, value: &api::ByteBuf<N>) -> Result<(), WireError> {
        let src = value.as_slice();
        let n = src.len().min(u16::MAX as usize);
        self.u16(n as u16)?;
        self.bytes(&src[..n])
    }
}

struct Reader<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    fn byte(&mut self) -> Result<u8, WireError> {
        if self.pos >= self.input.len() {
            return Err(WireError::Truncated);
        }
        let value = self.input[self.pos];
        self.pos += 1;
        Ok(value)
    }

    fn bytes(&mut self, len: usize) -> Result<&'a [u8], WireError> {
        let end = self.pos.checked_add(len).ok_or(WireError::Truncated)?;
        if end > self.input.len() {
            return Err(WireError::Truncated);
        }
        let out = &self.input[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], WireError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.bytes(N)?);
        Ok(out)
    }

    fn u16(&mut self) -> Result<u16, WireError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, WireError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn byte_buf<const N: usize>(&mut self) -> Result<api::ByteBuf<N>, WireError> {
        let len = self.u16()? as usize;
        Ok(api::ByteBuf::from_slice_trunc(self.bytes(len)?))
    }
}

fn kind_to_wire(kind: api::SocketKind) -> u8 {
    match kind {
        api::SocketKind::Udp => KIND_UDP,
        api::SocketKind::Tcp => KIND_TCP,
    }
}

fn kind_from_wire(kind: u8) -> Result<api::SocketKind, WireError> {
    match kind {
        KIND_UDP => Ok(api::SocketKind::Udp),
        KIND_TCP => Ok(api::SocketKind::Tcp),
        _ => Err(WireError::InvalidKind),
    }
}

pub(crate) fn encode_command(command: api::Command, out: &mut [u8]) -> Result<usize, WireError> {
    let mut w = Writer::new(out);
    match command {
        api::Command::OpenUdp { port } => {
            w.byte(TAG_OPEN_UDP)?;
            w.u16(port)?;
        }
        api::Command::OpenTcpListen { port } => {
            w.byte(TAG_OPEN_TCP_LISTEN)?;
            w.u16(port)?;
        }
        api::Command::OpenTcpConnect { remote } => {
            w.byte(TAG_OPEN_TCP_CONNECT)?;
            w.bytes(&remote.addr)?;
            w.u16(remote.port)?;
        }
        api::Command::OpenTcpConnectV6 { remote } => {
            w.byte(TAG_OPEN_TCP_CONNECT_V6)?;
            w.bytes(&remote.addr)?;
            w.u16(remote.port)?;
        }
        api::Command::SendUdp {
            handle,
            remote,
            data,
        } => {
            w.byte(TAG_SEND_UDP)?;
            w.u32(handle.0)?;
            w.bytes(&remote.addr)?;
            w.u16(remote.port)?;
            w.byte_buf(&data)?;
        }
        api::Command::SendUdpV6 {
            handle,
            remote,
            data,
        } => {
            w.byte(TAG_SEND_UDP_V6)?;
            w.u32(handle.0)?;
            w.bytes(&remote.addr)?;
            w.u16(remote.port)?;
            w.byte_buf(&data)?;
        }
        api::Command::SendTcp { handle, data } => {
            w.byte(TAG_SEND_TCP)?;
            w.u32(handle.0)?;
            w.byte_buf(&data)?;
        }
        api::Command::Close { handle } => {
            w.byte(TAG_CLOSE)?;
            w.u32(handle.0)?;
        }
        api::Command::IcmpEcho { target, seq, data } => {
            w.byte(TAG_ICMP_ECHO)?;
            w.bytes(&target)?;
            w.u16(seq)?;
            w.byte_buf(&data)?;
        }
        api::Command::IcmpEchoV6 { target, seq, data } => {
            w.byte(TAG_ICMP_ECHO_V6)?;
            w.bytes(&target)?;
            w.u16(seq)?;
            w.byte_buf(&data)?;
        }
    }
    Ok(w.finish())
}

pub(crate) fn decode_command(input: &[u8]) -> Result<api::Command, WireError> {
    let mut r = Reader::new(input);
    Ok(match r.byte()? {
        TAG_OPEN_UDP => api::Command::OpenUdp { port: r.u16()? },
        TAG_OPEN_TCP_LISTEN => api::Command::OpenTcpListen { port: r.u16()? },
        TAG_OPEN_TCP_CONNECT => api::Command::OpenTcpConnect {
            remote: api::EndpointV4 {
                addr: r.array()?,
                port: r.u16()?,
            },
        },
        TAG_OPEN_TCP_CONNECT_V6 => api::Command::OpenTcpConnectV6 {
            remote: api::EndpointV6 {
                addr: r.array()?,
                port: r.u16()?,
            },
        },
        TAG_SEND_UDP => api::Command::SendUdp {
            handle: api::NetHandle(r.u32()?),
            remote: api::EndpointV4 {
                addr: r.array()?,
                port: r.u16()?,
            },
            data: r.byte_buf()?,
        },
        TAG_SEND_UDP_V6 => api::Command::SendUdpV6 {
            handle: api::NetHandle(r.u32()?),
            remote: api::EndpointV6 {
                addr: r.array()?,
                port: r.u16()?,
            },
            data: r.byte_buf()?,
        },
        TAG_SEND_TCP => api::Command::SendTcp {
            handle: api::NetHandle(r.u32()?),
            data: r.byte_buf()?,
        },
        TAG_CLOSE => api::Command::Close {
            handle: api::NetHandle(r.u32()?),
        },
        TAG_ICMP_ECHO => api::Command::IcmpEcho {
            target: r.array()?,
            seq: r.u16()?,
            data: r.byte_buf()?,
        },
        TAG_ICMP_ECHO_V6 => api::Command::IcmpEchoV6 {
            target: r.array()?,
            seq: r.u16()?,
            data: r.byte_buf()?,
        },
        _ => return Err(WireError::InvalidTag),
    })
}

pub(crate) fn encode_event(event: api::Event, out: &mut [u8]) -> Result<usize, WireError> {
    let mut w = Writer::new(out);
    match event {
        api::Event::Opened { handle, kind } => {
            w.byte(TAG_OPENED)?;
            w.u32(handle.0)?;
            w.byte(kind_to_wire(kind))?;
        }
        api::Event::Closed { handle } => {
            w.byte(TAG_CLOSED)?;
            w.u32(handle.0)?;
        }
        api::Event::Error { .. } => {
            w.byte(TAG_ERROR)?;
        }
        api::Event::UdpPacket { handle, from, data } => {
            w.byte(TAG_UDP_PACKET)?;
            w.u32(handle.0)?;
            w.bytes(&from.addr)?;
            w.u16(from.port)?;
            w.byte_buf(&data)?;
        }
        api::Event::UdpPacketV6 { handle, from, data } => {
            w.byte(TAG_UDP_PACKET_V6)?;
            w.u32(handle.0)?;
            w.bytes(&from.addr)?;
            w.u16(from.port)?;
            w.byte_buf(&data)?;
        }
        api::Event::TcpEstablished { handle } => {
            w.byte(TAG_TCP_ESTABLISHED)?;
            w.u32(handle.0)?;
        }
        api::Event::TcpData { handle, data } => {
            w.byte(TAG_TCP_DATA)?;
            w.u32(handle.0)?;
            w.byte_buf(&data)?;
        }
        api::Event::TcpSent { handle, len } => {
            w.byte(TAG_TCP_SENT)?;
            w.u32(handle.0)?;
            w.u16(len)?;
        }
        api::Event::IcmpReply {
            from,
            seq,
            rtt_ms,
            data,
        } => {
            w.byte(TAG_ICMP_REPLY)?;
            w.bytes(&from)?;
            w.u16(seq)?;
            w.u32(rtt_ms)?;
            w.byte_buf(&data)?;
        }
        api::Event::IcmpReplyV6 {
            from,
            seq,
            rtt_ms,
            data,
        } => {
            w.byte(TAG_ICMP_REPLY_V6)?;
            w.bytes(&from)?;
            w.u16(seq)?;
            w.u32(rtt_ms)?;
            w.byte_buf(&data)?;
        }
    }
    Ok(w.finish())
}

pub(crate) fn decode_event(input: &[u8]) -> Result<api::Event, WireError> {
    let mut r = Reader::new(input);
    Ok(match r.byte()? {
        TAG_OPENED => api::Event::Opened {
            handle: api::NetHandle(r.u32()?),
            kind: kind_from_wire(r.byte()?)?,
        },
        TAG_CLOSED => api::Event::Closed {
            handle: api::NetHandle(r.u32()?),
        },
        TAG_ERROR => api::Event::Error { msg: "vmx-net" },
        TAG_UDP_PACKET => api::Event::UdpPacket {
            handle: api::NetHandle(r.u32()?),
            from: api::EndpointV4 {
                addr: r.array()?,
                port: r.u16()?,
            },
            data: r.byte_buf()?,
        },
        TAG_UDP_PACKET_V6 => api::Event::UdpPacketV6 {
            handle: api::NetHandle(r.u32()?),
            from: api::EndpointV6 {
                addr: r.array()?,
                port: r.u16()?,
            },
            data: r.byte_buf()?,
        },
        TAG_TCP_ESTABLISHED => api::Event::TcpEstablished {
            handle: api::NetHandle(r.u32()?),
        },
        TAG_TCP_DATA => api::Event::TcpData {
            handle: api::NetHandle(r.u32()?),
            data: r.byte_buf()?,
        },
        TAG_TCP_SENT => api::Event::TcpSent {
            handle: api::NetHandle(r.u32()?),
            len: r.u16()?,
        },
        TAG_ICMP_REPLY => api::Event::IcmpReply {
            from: r.array()?,
            seq: r.u16()?,
            rtt_ms: r.u32()?,
            data: r.byte_buf()?,
        },
        TAG_ICMP_REPLY_V6 => api::Event::IcmpReplyV6 {
            from: r.array()?,
            seq: r.u16()?,
            rtt_ms: r.u32()?,
            data: r.byte_buf()?,
        },
        _ => return Err(WireError::InvalidTag),
    })
}
