extern crate std;

use mio::{Events, Poll, Token, Waker};
use std::io;
use std::net::SocketAddr;

fn log_io_failure(stage: &str, err: &io::Error) {
    crate::log!(
        "mio_probe: failure {} kind={:?} err={}\n",
        stage,
        err.kind(),
        err
    );
}

pub(crate) fn log_boot_probe() {
    crate::log!(
        "mio_probe: direct mio 1.2.0 boot probe after tokio_probe (poll+waker+net surface)\n"
    );

    let mut poll = match Poll::new() {
        Ok(poll) => {
            crate::log!("mio_probe: success poll.new\n");
            poll
        }
        Err(err) => {
            log_io_failure("poll.new", &err);
            crate::log!(
                "mio_probe: note selector construction is the current root blocker for zkvm mio\n"
            );
            return;
        }
    };

    let mut events = Events::with_capacity(4);
    crate::log!("mio_probe: success events.with_capacity\n");

    match Waker::new(poll.registry(), Token(0x4D10)) {
        Ok(waker) => {
            crate::log!("mio_probe: success waker.new\n");
            match waker.wake() {
                Ok(()) => crate::log!("mio_probe: success waker.wake\n"),
                Err(err) => log_io_failure("waker.wake", &err),
            }
        }
        Err(err) => log_io_failure("waker.new", &err),
    }

    match poll.poll(&mut events, Some(core::time::Duration::ZERO)) {
        Ok(()) => crate::log!(
            "mio_probe: success poll.poll timeout0 events={}\n",
            events.iter().count()
        ),
        Err(err) => log_io_failure("poll.poll", &err),
    }

    let loopback_probe = SocketAddr::from(([127, 0, 0, 1], 1));

    match mio::net::TcpListener::bind(loopback_probe) {
        Ok(_) => crate::log!("mio_probe: success net.tcp_listener.bind\n"),
        Err(err) => log_io_failure("net.tcp_listener.bind", &err),
    }

    match mio::net::TcpStream::connect(loopback_probe) {
        Ok(_) => crate::log!("mio_probe: success net.tcp_stream.connect\n"),
        Err(err) => log_io_failure("net.tcp_stream.connect", &err),
    }

    match mio::net::UdpSocket::bind(SocketAddr::from(([127, 0, 0, 1], 0))) {
        Ok(_) => crate::log!("mio_probe: success net.udp_socket.bind\n"),
        Err(err) => log_io_failure("net.udp_socket.bind", &err),
    }
}