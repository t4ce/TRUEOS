#[cfg_attr(all(any(target_os = "trueos", target_os = "zkvm"), any(not(feature = "std-compat"), feature = "native-net")), path = "listener_zkvm.rs")]
mod listener;
pub use self::listener::TcpListener;

#[cfg_attr(all(any(target_os = "trueos", target_os = "zkvm"), any(not(feature = "std-compat"), feature = "native-net")), path = "stream_zkvm.rs")]
mod stream;
pub use self::stream::TcpStream;

#[cfg(all(any(target_os = "trueos", target_os = "zkvm"), any(not(feature = "std-compat"), feature = "native-net")))]
#[path = "socket_zkvm.rs"]
mod socket;
#[cfg(all(any(target_os = "trueos", target_os = "zkvm"), any(not(feature = "std-compat"), feature = "native-net")))]
pub use self::socket::TcpSocket;
