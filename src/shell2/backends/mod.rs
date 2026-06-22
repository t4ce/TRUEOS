pub(crate) mod container;
pub(crate) mod crlf;
pub(crate) mod net_tcp;
pub(crate) mod uart;
pub(crate) mod uart1_com1;
pub(crate) mod ui3;

pub(crate) use container::{
    CONTAINER_SHELL_BACKEND, container_shell_drain_output, container_shell_read_output_byte,
    container_shell_submit_input,
};
pub(crate) use net_tcp::NET_TCP_SHELL_BACKEND;
pub(crate) use uart::UART1_COM1_BACKEND;
pub(crate) use ui3::{
    UI3_SHELL_BACKEND, Ui3ShellCell, Ui3ShellScreenSnapshot, queue_ui3_keyboard_event,
    ui3_shell_attach_window, ui3_shell_last_rendered_seq, ui3_shell_line_width,
    ui3_shell_mark_rendered, ui3_shell_rows, ui3_shell_set_line_width, ui3_shell_snapshot,
};
