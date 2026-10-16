use super::*;

fn opened(server: &mut HttpServer, id: u32) {
    assert!(matches!(server.on_event(api::Event::Opened { handle: api::NetHandle(id), kind: api::SocketKind::Tcp }), HttpServerEvent::None));
}
fn established(server: &mut HttpServer, id: u32) -> HttpServerEvent {
    server.on_event(api::Event::TcpEstablished { handle: api::NetHandle(id), peer: None, peer6: None })
}
fn closed(server: &mut HttpServer, id: u32) -> HttpServerEvent {
    server.on_event(api::Event::Closed { handle: api::NetHandle(id) })
}
fn request(server: &mut HttpServer, id: u32, bytes: &[u8]) -> HttpServerEvent {
    server.on_event(api::Event::TcpData { handle: api::NetHandle(id), data: api::ByteBuf::from_slice_trunc(bytes) })
}
fn assert_listen(event: HttpServerEvent) {
    assert!(matches!(event, HttpServerEvent::Submit(api::Command::OpenTcpListen { port: 80 })));
}
fn assert_close(event: HttpServerEvent, id: u32) {
    assert!(matches!(event, HttpServerEvent::Submit(api::Command::Close { handle }) if handle == api::NetHandle(id)));
}
fn assert_finish(event: HttpServerEvent, id: u32) {
    assert!(matches!(event, HttpServerEvent::Submit(api::Command::FinishTcp { handle }) if handle == api::NetHandle(id)));
}

#[test]
fn accepted_connection_rearms_before_response_or_eof() {
    let mut server = HttpServer::new(80, 1024);
    opened(&mut server, 1);
    assert_listen(established(&mut server, 1));
    opened(&mut server, 2);
    assert_listen(established(&mut server, 2));
    assert_eq!(server.sessions.len(), 2);
    assert!(matches!(request(&mut server, 1, b"GET / HTTP/1.1\r\nHost: fixture\r\n\r\n"), HttpServerEvent::RequestReady { .. }));
    assert!(matches!(request(&mut server, 2, b"GET / HTTP/1.1\r\nHost: fixture\r\n\r\n"), HttpServerEvent::RequestReady { .. }));
}

#[test]
fn old_session_eof_never_duplicates_pending_or_new_listener() {
    let mut server = HttpServer::new(80, 1024);
    opened(&mut server, 1);
    assert_listen(established(&mut server, 1));
    assert!(matches!(closed(&mut server, 1), HttpServerEvent::None));
    assert!(server.listen_pending);
    opened(&mut server, 2);
    assert!(matches!(closed(&mut server, 1), HttpServerEvent::None));
    assert_eq!(server.listener_handle, Some(api::NetHandle(2)));
    assert_listen(established(&mut server, 2));
    opened(&mut server, 3);
    assert!(matches!(closed(&mut server, 2), HttpServerEvent::None));
    assert_eq!(server.listener_handle, Some(api::NetHandle(3)));
}

#[test]
fn response_completion_finishes_only_accepted_session() {
    let mut server = HttpServer::new(80, 1024);
    opened(&mut server, 1);
    assert_listen(established(&mut server, 1));
    opened(&mut server, 2);
    assert!(matches!(request(&mut server, 1, b"GET / HTTP/1.1\r\nConnection: close\r\n\r\n"), HttpServerEvent::RequestReady { .. }));
    server.mark_response(api::NetHandle(1), 100, false);
    assert!(matches!(server.on_event(api::Event::TcpSent { handle: api::NetHandle(1), len: 40 }), HttpServerEvent::None));
    assert_finish(server.on_event(api::Event::TcpSent { handle: api::NetHandle(1), len: 60 }), 1);
    assert!(matches!(closed(&mut server, 1), HttpServerEvent::None));
    assert_eq!(server.listener_handle, Some(api::NetHandle(2)));
    assert_listen(established(&mut server, 2));
}

#[test]
fn capacity_blocks_growth_and_resumes_listen_when_session_retires() {
    let mut server = HttpServer::with_session_limit(80, 1024, 1);
    opened(&mut server, 1);
    assert!(matches!(established(&mut server, 1), HttpServerEvent::None));
    assert_eq!(server.sessions.len(), 1);
    assert!(!server.listen_pending);
    assert_close(established(&mut server, 99), 99);
    assert_close(request(&mut server, 99, b"GET / HTTP/1.1\r\n\r\n"), 99);
    server.mark_response(api::NetHandle(99), 100, false);
    assert_eq!(server.sessions.len(), 1);
    assert_listen(closed(&mut server, 1));
    opened(&mut server, 2);
    assert!(matches!(established(&mut server, 2), HttpServerEvent::None));
    assert_eq!(server.sessions.len(), 1);
    assert!(matches!(closed(&mut server, 1), HttpServerEvent::None));
}

#[test]
fn data_before_established_preserves_first_request_and_rearms_once() {
    let mut server = HttpServer::new(80, 1024);
    opened(&mut server, 1);
    let event = request(&mut server, 1, b"GET /early HTTP/1.1\r\nHost: fixture\r\n\r\n");
    assert!(matches!(event, HttpServerEvent::RequestReady { request, .. } if request.target() == "/early" && request.keep_alive()));
    assert_listen(established(&mut server, 1));
    assert!(matches!(established(&mut server, 1), HttpServerEvent::None));
    assert_eq!(server.sessions.len(), 1);
    assert!(server.listen_pending);
}

#[test]
fn listen_errors_are_reported_without_retry_loop_then_capacity_event_recovers() {
    let mut server = HttpServer::new(80, 1024);
    opened(&mut server, 1);
    assert_listen(established(&mut server, 1));
    assert!(matches!(server.on_event(api::Event::Error { msg: "no sockets available" }), HttpServerEvent::Error("no sockets available")));
    assert!(!server.listen_pending);
    assert_listen(closed(&mut server, 1));
    assert!(matches!(server.on_event(api::Event::Error { msg: "bad handle" }), HttpServerEvent::None));
    assert!(server.listen_pending, "unrelated bad-handle errors cannot release the pending listener token");
    opened(&mut server, 2);
    assert_close(server.on_event(api::Event::Opened { handle: api::NetHandle(3), kind: api::SocketKind::Tcp }), 3);
    assert_eq!(server.listener_handle, Some(api::NetHandle(2)));
}

#[test]
fn twenty_immediate_sequential_connections_keep_an_independent_listener() {
    let mut server = HttpServer::new(80, 1024);
    opened(&mut server, 1);
    for id in 1..=20 {
        assert_listen(established(&mut server, id));
        opened(&mut server, id + 1);
        assert!(matches!(request(&mut server, id, b"GET / HTTP/1.1\r\nConnection: close\r\n\r\n"), HttpServerEvent::RequestReady { .. }));
        server.mark_response(api::NetHandle(id), 10, false);
        assert_finish(server.on_event(api::Event::TcpSent { handle: api::NetHandle(id), len: 10 }), id);
        assert!(matches!(closed(&mut server, id), HttpServerEvent::None));
        assert_eq!(server.listener_handle, Some(api::NetHandle(id + 1)));
    }
    assert!(server.sessions.is_empty());
}

#[test]
fn request_size_and_malformed_header_checks_still_apply_to_admitted_session() {
    let mut server = HttpServer::new(80, 32);
    opened(&mut server, 1);
    assert_listen(established(&mut server, 1));
    assert!(matches!(request(&mut server, 1, &[b'x'; 33]), HttpServerEvent::RequestTooLarge { .. }));
    opened(&mut server, 2);
    assert_listen(established(&mut server, 2));
    assert!(matches!(request(&mut server, 2, b"BAD\r\n\r\n"), HttpServerEvent::BadRequest { .. }));
}

#[test]
fn keep_alive_completion_never_finishes_and_graceful_finish_waits_for_closed_cleanup() {
    let mut server = HttpServer::with_session_limit(80, 1024, 1);
    opened(&mut server, 1);
    assert!(matches!(established(&mut server, 1), HttpServerEvent::None));
    server.mark_response(api::NetHandle(1), 100, true);
    assert!(matches!(server.on_event(api::Event::TcpSent { handle: api::NetHandle(1), len: 100 }), HttpServerEvent::None));
    assert_eq!(server.sessions.len(), 1);
    // A subsequent response on the same session may request graceful closure.
    server.mark_response(api::NetHandle(1), 40, false);
    assert_finish(server.on_event(api::Event::TcpSent { handle: api::NetHandle(1), len: 40 }), 1);
    assert_eq!(server.sessions.len(), 1, "send admission is not disconnect");
    assert!(matches!(server.on_event(api::Event::TcpSent { handle: api::NetHandle(1), len: 40 }), HttpServerEvent::None), "finish is emitted once");
    assert_listen(closed(&mut server, 1));
    assert!(server.sessions.is_empty());
    assert!(matches!(closed(&mut server, 1), HttpServerEvent::None));
}
