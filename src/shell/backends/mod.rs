pub(crate) mod net_tcp;
pub(crate) mod uart;

pub(crate) use net_tcp::{NetTcpShellBackend, NET_TCP_SHELL_BACKEND};
pub(crate) use uart::{Uart1Com1Backend, UART1_COM1_BACKEND};
