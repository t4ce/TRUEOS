#![no_std]
#![no_main]
#[unsafe(no_mangle)]
pub extern "C" fn _start()->!{
 let addr=core::net::SocketAddr::from(([127,0,0,1],43210));
 let mut buf=[0;128];
 let _=mio_native_compile_probe::native_tcp_probe(addr,&mut buf);
 let _=mio_native_compile_probe::native_reactor_probe(addr,&mut buf);
 trueos::entry::finish()
}
