pub(crate) mod crlf;
pub(crate) mod net_tcp;
pub(crate) mod uart;
pub(crate) mod uart1_com1;

pub(crate) use net_tcp::NET_TCP_SHELL_BACKEND;
pub(crate) use uart::UART1_COM1_BACKEND;
