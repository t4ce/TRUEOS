extern crate std;

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Timer};
use mio::{Events, Interest, Poll, Token, Waker};
use std::io;
use std::net::SocketAddr;

const MIO_NET_PROBE_PORT: u16 = crate::allports::probes::MIO_NET_PROBE_PORT;

static MIO_NET_PROBE_TASK_SPAWNED: AtomicBool = AtomicBool::new(false);

fn log_io_failure(stage: &str, err: &io::Error) {
    crate::log_trace!("mio_probe: failure {} kind={:?} err={}\n", stage, err.kind(), err);
}

fn primary_ipv4_probe_addr(port: u16) -> Option<SocketAddr> {
    let dev_idx = crate::net::primary_device_index();
    let ip = crate::net::adapter::ipv4_at(dev_idx)?;
    Some(SocketAddr::from((ip, port)))
}

async fn log_net_surface_probe() {
    let Some(tcp_probe) = primary_ipv4_probe_addr(MIO_NET_PROBE_PORT) else {
        crate::log_trace!("mio_probe: note net surface skipped (no primary ipv4 yet)\n");
        return;
    };

    let Some(udp_probe) = primary_ipv4_probe_addr(0) else {
        crate::log_trace!("mio_probe: note net surface skipped (no primary ipv4 yet)\n");
        return;
    };

    match mio::net::TcpListener::bind(tcp_probe) {
        Ok(_) => crate::log_trace!("mio_probe: success net.tcp_listener.bind\n"),
        Err(err) => log_io_failure("net.tcp_listener.bind", &err),
    }

    match mio::net::TcpStream::connect(tcp_probe) {
        Ok(_) => crate::log_trace!("mio_probe: success net.tcp_stream.connect\n"),
        Err(err) => log_io_failure("net.tcp_stream.connect", &err),
    }

    match mio::net::UdpSocket::bind(udp_probe) {
        Ok(mut udp) => {
            crate::log_trace!("mio_probe: success net.udp_socket.bind\n");

            let mut poll = match Poll::new() {
                Ok(poll) => poll,
                Err(err) => {
                    log_io_failure("net.udp_socket.poll.new", &err);
                    return;
                }
            };
            let mut events = Events::with_capacity(4);
            match poll
                .registry()
                .register(&mut udp, Token(0x4D11), Interest::WRITABLE)
            {
                Ok(()) => crate::log_trace!("mio_probe: success net.udp_socket.register_writable\n"),
                Err(err) => {
                    log_io_failure("net.udp_socket.register_writable", &err);
                    return;
                }
            }
            let max_spins = crate::allcaps::probes::TOKIO_NET_WRITABLE_TIMEOUT_MS
                .saturating_add(1)
                .max(1);
            for spin in 0..max_spins {
                match poll.poll(&mut events, Some(core::time::Duration::ZERO)) {
                    Ok(()) if events.iter().any(|event| event.is_writable()) => {
                        crate::log_trace!("mio_probe: success net.udp_socket.writable spins={}\n", spin);
                        crate::r::readiness::set(crate::r::readiness::NET_SOCKET_READY);
                        return;
                    }
                    Ok(()) => {}
                    Err(err) => {
                        log_io_failure("net.udp_socket.poll_writable", &err);
                        return;
                    }
                }
                Timer::after(EmbassyDuration::from_micros(0)).await;
            }
            crate::log_trace!("mio_probe: failure net.udp_socket.writable_timeout events=0\n");
        }
        Err(err) => log_io_failure("net.udp_socket.bind", &err),
    }
}

#[task]
async fn mio_net_probe_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;
    crate::log_trace!("mio_probe: resume net surface after NET_ANY_CONFIGURED\n");
    log_net_surface_probe().await;
}

pub(crate) fn log_boot_probe() {
    crate::log_trace!(
        "mio_probe: direct mio 1.2.0 boot probe after tokio_probe (poll+waker+net surface)\n"
    );

    let mut poll = match Poll::new() {
        Ok(poll) => {
            crate::log_trace!("mio_probe: success poll.new\n");
            poll
        }
        Err(err) => {
            log_io_failure("poll.new", &err);
            crate::log_trace!(
                "mio_probe: note selector construction is the current root blocker for TRUEOS mio\n"
            );
            return;
        }
    };

    let mut events = Events::with_capacity(4);
    crate::log_trace!("mio_probe: success events.with_capacity\n");

    match Waker::new(poll.registry(), Token(0x4D10)) {
        Ok(waker) => {
            crate::log_trace!("mio_probe: success waker.new\n");
            match waker.wake() {
                Ok(()) => crate::log_trace!("mio_probe: success waker.wake\n"),
                Err(err) => log_io_failure("waker.wake", &err),
            }
        }
        Err(err) => log_io_failure("waker.new", &err),
    }

    match poll.poll(&mut events, Some(core::time::Duration::ZERO)) {
        Ok(()) => {
            crate::log_trace!("mio_probe: success poll.poll timeout0 events={}\n", events.iter().count())
        }
        Err(err) => log_io_failure("poll.poll", &err),
    }

    if crate::r::readiness::is_set(crate::r::readiness::NET_ANY_CONFIGURED) {
        spawn_deferred_net_readiness_probe();
        return;
    }

    crate::log_trace!("mio_probe: note net surface deferred until NET_ANY_CONFIGURED\n");

    spawn_deferred_net_readiness_probe();
}

pub(crate) fn spawn_deferred_net_readiness_probe() {
    if MIO_NET_PROBE_TASK_SPAWNED.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(spawner) = crate::workers::spawner_for_slot(0) else {
        crate::log_trace!("mio_probe: note net surface task not spawned (no slot0 spawner)\n");
        return;
    };

    match mio_net_probe_task() {
        Ok(token) => spawner.spawn(token),
        Err(err) => {
            crate::log_trace!("mio_probe: note net surface task spawn failed: {:?}\n", err)
        }
    }
}
