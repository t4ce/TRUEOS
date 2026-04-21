pub(crate) mod crlf;
pub(crate) mod net_tcp;
pub(crate) mod uart;
pub(crate) mod uart1_com1;
pub(crate) mod ui2;

pub(crate) use net_tcp::NET_TCP_SHELL_BACKEND;
pub(crate) use uart::UART1_COM1_BACKEND;
pub(crate) use ui2::{
    UI2_SHELL_BACKEND, Ui2ShellCell, Ui2ShellScreenSnapshot, queue_ui2_keyboard_event,
    ui2_shell_attach_window, ui2_shell_last_rendered_seq, ui2_shell_mark_rendered,
    ui2_shell_snapshot,
};
