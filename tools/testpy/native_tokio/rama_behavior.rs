mod boundary;
mod network_boundary;
#[path=@RAMA_NATIVE@]mod provider;
use rama_net::socket::{SocketOptions,opts::{Domain,Type,Protocol}};
use tokio::io::{AsyncReadExt,AsyncWriteExt};
#[unsafe(no_mangle)]pub extern "C" fn trueos_cabi_dns_resolve_ipv4(_: *const u8,_:usize,_:*mut u8)->i32 {panic!("numeric Rama transport unexpectedly resolved DNS")}
fn main(){tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async{
 let mut options=SocketOptions::default_tcp();options.address=Some("127.0.0.1:0".parse().unwrap());options.tcp_no_delay=Some(true);
 let listener=provider::listener(&options).unwrap();let address=listener.local_addr().unwrap();
 let stream=provider::connect(&options,address).await.unwrap();let (mut peer,source)=listener.accept().await.unwrap();assert_eq!(stream.local_addr().unwrap(),source);assert!(stream.nodelay().unwrap());assert!(peer.nodelay().unwrap());
 let client=tokio::spawn(async move{let mut stream=stream;stream.write_all(b"rama native").await.unwrap();stream.shutdown().await.unwrap();});let mut bytes=Vec::new();peer.read_to_end(&mut bytes).await.unwrap();assert_eq!(bytes,b"rama native");client.await.unwrap();drop((peer,listener));assert_eq!(network_boundary::live_sockets(),0);println!("ok native listener/connect localbind+NODELAY+fragmented IO");
 macro_rules! rejects {($field:ident,$value:expr)=>{{let mut o=SocketOptions::default_tcp();o.$field=Some($value);assert_eq!(provider::connect(&o,address).await.unwrap_err().kind(),std::io::ErrorKind::Unsupported);assert_eq!(provider::listener(&o).unwrap_err().kind(),std::io::ErrorKind::Unsupported);assert_eq!(network_boundary::live_sockets(),0);println!("ok reject {}",stringify!($field));}};}
 rejects!(read_timeout,std::time::Duration::from_millis(5));rejects!(write_timeout,std::time::Duration::from_millis(5));rejects!(keep_alive,false);rejects!(reuse_address,false);rejects!(send_buffer_size,1024);rejects!(recv_buffer_size,1024);rejects!(ttl,37);rejects!(only_v6,false);rejects!(broadcast,false);
 let mut o=SocketOptions::default_tcp();o.domain=Domain::Unix;assert_eq!(provider::listener(&o).unwrap_err().kind(),std::io::ErrorKind::Unsupported);o=SocketOptions::default_tcp();o.r#type=Type::Datagram;assert!(provider::listener(&o).is_err());o=SocketOptions::default_tcp();o.protocol=Some(Protocol::UDP);assert!(provider::listener(&o).is_err());assert_eq!(network_boundary::live_sockets(),0);println!("ok protocol/domain requests fail before allocation");
 let listener=rama_tcp::server::TcpListener::bind_address("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();let extensions=rama_core::extensions::Extensions::new();let address=rama_net::address::HostWithPort::from((address.ip(),address.port()));let (client,_)=rama_tcp::client::default_tcp_connect(&extensions,address).await.unwrap();let (peer,_)=listener.accept().await.unwrap();drop((client,peer,listener));assert_eq!(network_boundary::live_sockets(),0);println!("ok full Rama TCP source client/listener integration");

 {use rama_tcp::client::TcpStreamConnector;use rama_net::socket::Interface;use std::sync::Arc;
 let options=Arc::new(options);let listener=rama_tcp::server::TcpListener::build().bind(Interface::Socket(options.clone())).await.unwrap();let address=listener.local_addr().unwrap();let client=options.connect(address).await.unwrap();assert!(client.stream.nodelay().unwrap());let(peer,_)=listener.accept().await.unwrap();assert!(peer.stream.nodelay().unwrap());drop((client,peer,listener));assert_eq!(network_boundary::live_sockets(),0);println!("ok Rama SocketOptions client/listener uses native ownership");
 let mut unsupported=SocketOptions::default_tcp();unsupported.read_timeout=Some(std::time::Duration::from_millis(5));let error=Arc::new(unsupported).connect(address).await.unwrap_err();assert_eq!(error.downcast_ref::<std::io::Error>().unwrap().kind(),std::io::ErrorKind::Unsupported);assert_eq!(network_boundary::live_sockets(),0);println!("ok Rama client retains Unsupported error identity");
 }
});boundary::cleanup_tls();}
