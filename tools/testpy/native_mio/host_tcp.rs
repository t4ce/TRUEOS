use mio::net::TcpStream;
use std::io::{self,Read,Write};
use std::net::{Shutdown,TcpListener};
use std::time::{Duration,Instant};
fn pair()->(TcpStream,std::net::TcpStream){
 let listener=TcpListener::bind("127.0.0.1:0").unwrap();
 let client=std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
 let(peer,_)=listener.accept().unwrap();
 peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
 peer.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
 client.set_nonblocking(true).unwrap();(TcpStream::from_std(client),peer)
}
fn retry<T>(mut f:impl FnMut()->io::Result<T>)->T{
 let deadline=Instant::now()+Duration::from_secs(2);
 loop{match f(){Ok(v)=>return v,Err(e) if e.kind()==io::ErrorKind::WouldBlock&&Instant::now()<deadline=>std::thread::sleep(Duration::from_millis(1)),Err(e)=>panic!("{e}")}}
}
#[test]fn host_compat_write_half_close_preserves_reply(){
 let(mut client,mut peer)=pair();retry(||client.write(b"request"));
 client.shutdown(Shutdown::Write).unwrap();let mut request=Vec::new();peer.read_to_end(&mut request).unwrap();assert_eq!(request,b"request");
 peer.write_all(b"reply").unwrap();let mut buf=[0;5];assert_eq!(retry(||client.peek(&mut buf)),5);assert_eq!(&buf,b"reply");
 assert_eq!(retry(||client.read(&mut buf)),5);assert_eq!(&buf,b"reply");
}
#[test]fn host_compat_peek_does_not_consume(){
 let(mut client,mut peer)=pair();peer.write_all(b"abcdef").unwrap();let mut buf=[0x55;8];
 assert_eq!(retry(||client.peek(&mut buf[..3])),3);assert_eq!(&buf[..3],b"abc");assert_eq!(&buf[3..],&[0x55;5]);
 assert_eq!(retry(||client.peek(&mut buf[..3])),3);assert_eq!(retry(||client.read(&mut buf[..2])),2);assert_eq!(&buf[..2],b"ab");
 assert_eq!(retry(||client.peek(&mut buf)),4);assert_eq!(&buf[..4],b"cdef");
}
