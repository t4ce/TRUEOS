#[cfg_attr(target_os = "zkvm", path = "listener_zkvm.rs")]
mod listener;
pub use self::listener::TcpListener;

#[cfg_attr(target_os = "zkvm", path = "stream_zkvm.rs")]
mod stream;
pub use self::stream::TcpStream;
