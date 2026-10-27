#[cfg_attr(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")), path = "listener_zkvm.rs")]
mod listener;
pub use self::listener::TcpListener;

#[cfg_attr(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")), path = "stream_zkvm.rs")]
mod stream;
pub use self::stream::TcpStream;
