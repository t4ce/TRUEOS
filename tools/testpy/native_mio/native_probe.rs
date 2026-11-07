#![no_std]
use core::net::SocketAddr;
use mio::{Events, Interest, Poll, Token, Waker};
use mio::net::{TcpListener, TcpStream, UdpSocket, Shutdown};
use trueos_io::{self as io, Read, Write};
#[unsafe(no_mangle)]
pub fn native_tcp_probe(addr:SocketAddr, buf:&mut [u8]) -> io::Result<usize> {
 let mut stream=TcpStream::connect(addr)?;
 let peeked=stream.peek(buf)?;
 stream.shutdown(Shutdown::Write)?;
 let read=stream.read(buf)?;
 stream.shutdown(Shutdown::Read)?;
 stream.shutdown(Shutdown::Both)?;
 Ok(peeked+read)
}
#[unsafe(no_mangle)]
pub fn native_reactor_probe(addr:SocketAddr,buf:&mut[u8])->io::Result<usize>{
 let mut poll=Poll::new()?;
 let mut stream=TcpStream::connect(addr)?;
 let mut listener=TcpListener::bind(addr)?;
 let mut udp=UdpSocket::bind(addr)?;
 poll.registry().register(&mut stream,Token(7),Interest::READABLE|Interest::WRITABLE)?;
 poll.registry().register(&mut listener,Token(8),Interest::READABLE)?;
 poll.registry().register(&mut udp,Token(9),Interest::READABLE)?;
 let wake=Waker::new(poll.registry(),Token(10))?;wake.wake()?;
 let mut events=Events::with_capacity(16);
 poll.poll(&mut events,Some(core::time::Duration::ZERO))?;
 let wrote=stream.write(buf)?;
 poll.registry().deregister(&mut stream)?;
 Ok(events.iter().count()+wrote)
}
