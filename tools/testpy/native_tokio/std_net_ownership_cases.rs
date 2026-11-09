 test("prebound native TCP capabilities survive Tokio round trips and std clones", || {current().block_on(async {
  use native_std_net_fixture as native;
  let std_listener = native::TcpListener::bind("127.0.0.1:0").unwrap();
  let listener_id = std_listener.native_socket().registration_id();
  let retained_listener = std_listener.try_clone().unwrap();
  assert_eq!(listener_id, retained_listener.native_socket().registration_id());
  std_listener.set_nonblocking(true).unwrap();
  assert!(retained_listener.native_socket().nonblocking());
  let listener = TcpListener::from_std(std_listener).unwrap();
  // The reactor owns one registration per capability ID. A clone must not
  // replace it or independently close the listener when registration fails.
  let duplicate = retained_listener.try_clone().unwrap();
  assert_eq!(TcpListener::from_std(duplicate).unwrap_err().kind(),std::io::ErrorKind::AlreadyExists);
  assert_eq!(network_boundary::close_count(listener_id),0);
  let std_listener = listener.into_std().unwrap();
  assert_eq!(std_listener.native_socket().registration_id(), listener_id);
  let listener = TcpListener::from_std(std_listener).unwrap();
  let address = listener.local_addr().unwrap();
  let std_client = native::TcpStream::connect(address).unwrap();
  let client_id = std_client.native_socket().registration_id();
  let retained_client = std_client.try_clone().unwrap();
  std_client.set_nonblocking(true).unwrap();
  let client = TcpStream::from_std(std_client).unwrap();
  let std_client = client.into_std().unwrap();
  assert_eq!(std_client.native_socket().registration_id(), client_id);
  let mut client = TcpStream::from_std(std_client).unwrap();
  let (mut peer, _) = listener.accept().await.unwrap();
  client.write_all(b"prebound").await.unwrap();
  let mut request = [0;8]; peer.read_exact(&mut request).await.unwrap();
  assert_eq!(&request,b"prebound");
  peer.write_all(b"reply").await.unwrap();
  let mut reply = [0;5]; client.read_exact(&mut reply).await.unwrap(); assert_eq!(&reply,b"reply");
  drop(client); assert_eq!(network_boundary::close_count(client_id),0);
  drop(retained_client); assert_eq!(network_boundary::close_count(client_id),1);
  drop((peer,listener)); assert_eq!(network_boundary::close_count(listener_id),0);
  // Match XERJ's starting responder handoff: after the first Tokio listener
  // drops, its original std clone can register the same eagerly bound port.
  let handed_off = TcpListener::from_std(retained_listener.try_clone().unwrap()).unwrap();
  let extra = native::TcpStream::connect(address).unwrap();
  let (accepted,_) = handed_off.accept().await.unwrap(); drop((accepted,extra,handed_off));
  assert_eq!(network_boundary::close_count(listener_id),0);
  drop(retained_listener); assert_eq!(network_boundary::close_count(listener_id),1);
 });});
 test("prebound native UDP retains its std owner through async I/O and round trip", || {current().block_on(async {
  use native_std_net_fixture as native;
  let original = native::UdpSocket::bind("127.0.0.1:0").unwrap();
  let id = original.native_socket().registration_id();
  let retained = original.try_clone().unwrap(); original.set_nonblocking(true).unwrap();
  let socket = UdpSocket::from_std(original).unwrap();
  let original = socket.into_std().unwrap(); assert_eq!(original.native_socket().registration_id(),id);
  let socket = UdpSocket::from_std(original).unwrap();
  let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
  socket.send_to(b"native",peer.local_addr().unwrap()).await.unwrap();
  let mut bytes=[0;6]; let (n,from)=peer.recv_from(&mut bytes).await.unwrap();
  assert_eq!(n,6); assert_eq!(&bytes,b"native"); assert_eq!(from,socket.local_addr().unwrap());
  peer.send_to(b"reply",from).await.unwrap(); let (n,_)=socket.recv_from(&mut bytes).await.unwrap();
  assert_eq!(&bytes[..n],b"reply");
  drop(socket); assert_eq!(network_boundary::close_count(id),0);
  drop(retained); assert_eq!(network_boundary::close_count(id),1); drop(peer);
 });});
 test("native std adoption refuses blocking capabilities and closes them once", || {
  use native_std_net_fixture as native;
  let listener=native::TcpListener::bind("127.0.0.1:0").unwrap();
  let id=listener.native_socket().registration_id();
  assert_eq!(TcpListener::from_std(listener).unwrap_err().kind(),std::io::ErrorKind::InvalidInput);
  assert_eq!(network_boundary::close_count(id),1);
  let udp=native::UdpSocket::bind("127.0.0.1:0").unwrap();let id=udp.native_socket().registration_id();
  assert_eq!(UdpSocket::from_std(udp).unwrap_err().kind(),std::io::ErrorKind::InvalidInput);
  assert_eq!(network_boundary::close_count(id),1);
  let listener=native::TcpListener::bind("127.0.0.1:0").unwrap();
  let stream=native::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
  let id=stream.native_socket().registration_id();
  assert_eq!(TcpStream::from_std(stream).unwrap_err().kind(),std::io::ErrorKind::InvalidInput);
  assert_eq!(network_boundary::close_count(id),1);drop(listener);
 });
 test("Mio unregisters adopted std capability before its final close callback", || {
  use native_std_net_fixture as native;
  let original=native::UdpSocket::bind("127.0.0.1:0").unwrap();
  original.set_nonblocking(true).unwrap();let id=original.native_socket().registration_id();
  let mut socket=mio::net::UdpSocket::from_std(original);
  let poll=mio::Poll::new().unwrap();poll.registry().register(&mut socket,mio::Token(811),mio::Interest::WRITABLE).unwrap();
  let poll=std::sync::Arc::new(std::sync::Mutex::new(poll));
  let checked=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
  let observed=checked.clone();
  network_boundary::set_close_hook(Some(Box::new(move|closing| {if closing==id {
   let mut events=mio::Events::with_capacity(4);
   poll.lock().unwrap().poll(&mut events,Some(Duration::ZERO)).unwrap();
   // The service socket is still open+writable inside this pre-close callback.
   // A surviving selector registration would therefore emit its old token.
   assert!(!events.iter().any(|event|event.token()==mio::Token(811)));
   observed.store(true,Ordering::SeqCst);
  }})));
  drop(socket);network_boundary::set_close_hook(None);
  assert!(checked.load(Ordering::SeqCst));assert_eq!(network_boundary::close_count(id),1);
 });
 test("Mio returns adopted std owner only after removing selector registration", || {
  use native_std_net_fixture as native;
  let original=native::UdpSocket::bind("127.0.0.1:0").unwrap();original.set_nonblocking(true).unwrap();
  let id=original.native_socket().registration_id();let mut socket=mio::net::UdpSocket::from_std(original);
  let mut poll=mio::Poll::new().unwrap();poll.registry().register(&mut socket,mio::Token(812),mio::Interest::WRITABLE).unwrap();
  let original=socket.into_std().unwrap();assert_eq!(original.native_socket().registration_id(),id);
  let mut events=mio::Events::with_capacity(4);poll.poll(&mut events,Some(Duration::ZERO)).unwrap();
  assert!(!events.iter().any(|event|event.token()==mio::Token(812)));assert_eq!(network_boundary::close_count(id),0);
  let peer=native::UdpSocket::bind("127.0.0.1:0").unwrap();
  original.send_to(b"owned",peer.local_addr().unwrap()).unwrap();let mut bytes=[0;5];assert_eq!(peer.recv_from(&mut bytes).unwrap().0,5);assert_eq!(&bytes,b"owned");
  drop((original,peer));assert_eq!(network_boundary::close_count(id),1);
 });
