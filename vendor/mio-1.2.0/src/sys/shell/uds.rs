pub(crate) mod datagram {
    use crate::io;
    use std::os::unix::net::{self, SocketAddr};

    pub(crate) fn bind_addr(_: &SocketAddr) -> io::Result<net::UnixDatagram> {
        unsupported_io!("native socket backend does not support Unix-domain transport")
    }

    pub(crate) fn unbound() -> io::Result<net::UnixDatagram> {
        unsupported_io!("native socket backend does not support Unix-domain transport")
    }

    pub(crate) fn pair() -> io::Result<(net::UnixDatagram, net::UnixDatagram)> {
        unsupported_io!("native socket backend does not support Unix-domain transport")
    }
}

pub(crate) mod listener {
    use crate::io;
    use std::os::unix::net::{self, SocketAddr};

    use crate::net::UnixStream;

    pub(crate) fn bind_addr(_: &SocketAddr) -> io::Result<net::UnixListener> {
        unsupported_io!("native socket backend does not support Unix-domain transport")
    }

    pub(crate) fn accept(_: &net::UnixListener) -> io::Result<(UnixStream, SocketAddr)> {
        unsupported_io!("native socket backend does not support Unix-domain transport")
    }
}

pub(crate) mod stream {
    use crate::io;
    use std::os::unix::net::{self, SocketAddr};

    pub(crate) fn connect_addr(_: &SocketAddr) -> io::Result<net::UnixStream> {
        unsupported_io!("native socket backend does not support Unix-domain transport")
    }

    pub(crate) fn pair() -> io::Result<(net::UnixStream, net::UnixStream)> {
        unsupported_io!("native socket backend does not support Unix-domain transport")
    }
}
