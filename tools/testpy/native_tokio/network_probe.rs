#![no_std]
extern crate alloc;
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{TcpSocket, TcpStream, UdpSocket}};

pub fn exercise_network() {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let socket = TcpSocket::new_v4().unwrap();
        socket.bind("0.0.0.0:0".parse().unwrap()).unwrap();
        socket.set_nodelay(true).unwrap();
        let listener = socket.listen(7).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0; 4];
            stream.read_exact(&mut buf).await.unwrap();
            stream.write_all(&buf).await.unwrap();
            stream.shutdown().await.unwrap();
        });
        let client = TcpSocket::new_v4().unwrap();
        client.bind("0.0.0.0:0".parse().unwrap()).unwrap();
        client.set_nodelay(true).unwrap();
        let mut stream = client.connect(endpoint).await.unwrap();
        stream.write_all(b"echo").await.unwrap();
        let mut bytes = alloc::vec::Vec::new();
        stream.read_to_end(&mut bytes).await.unwrap();
        server.await.unwrap();
        assert_eq!(bytes, b"echo");
        let udp = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        udp.set_ttl(37).unwrap();
        let mut buf = [0; 4];
        let _ = udp.peek_from(&mut buf).await;
        let _ = TcpStream::connect("localhost:9").await;
    });
}
