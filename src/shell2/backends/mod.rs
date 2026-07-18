pub(crate) mod container;
pub(crate) mod crlf;
pub(crate) mod net_tcp;
pub(crate) mod net_tcp_shell;

pub(crate) use container::{
    CONTAINER_SHELL_BACKEND, container_shell_drain_output, container_shell_read_output_byte,
    container_shell_submit_input,
};
pub(crate) use net_tcp::NET_TCP_SHELL_BACKEND;
