include!("selected.rs");

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("trueos-rama-unix-{}-{label}", std::process::id()))
}

#[cfg(target_os = "trueos")]
#[test]
fn native_existing_regular_file_untouched() {
    use std::os::unix::fs::MetadataExt;
    let path = temp_path("existing");
    std::fs::write(&path, b"sentinel\0must survive").unwrap();
    let before = std::fs::metadata(&path).unwrap();
    let error = runtime().block_on(UnixListenerBuilder.bind_path(&path)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert_eq!(std::fs::read(&path).unwrap(), b"sentinel\0must survive");
    let after = std::fs::metadata(&path).unwrap();
    assert_eq!((before.dev(), before.ino(), before.len(), before.mtime(), before.mtime_nsec()),
               (after.dev(), after.ino(), after.len(), after.mtime(), after.mtime_nsec()));
    std::fs::remove_file(path).unwrap();
}

#[cfg(target_os = "trueos")]
#[test]
fn native_missing_path_stays_missing() {
    let path = temp_path("missing");
    assert!(!path.exists());
    let error = runtime().block_on(UnixListenerBuilder.bind_path(&path)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert!(!path.exists());
}

#[cfg(target_os = "trueos")]
#[test]
fn native_directory_and_child_untouched() {
    let path = temp_path("directory");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("sentinel"), b"directory child").unwrap();
    let error = runtime().block_on(UnixListenerBuilder.bind_path(&path)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert_eq!(std::fs::read(path.join("sentinel")).unwrap(), b"directory child");
    std::fs::remove_dir_all(path).unwrap();
}

#[cfg(target_os = "trueos")]
#[test]
fn native_rejects_before_path_conversion() {
    struct EffectfulPath;
    impl AsRef<Path> for EffectfulPath {
        fn as_ref(&self) -> &Path { panic!("native factory consulted path before rejecting capability") }
    }
    let error = runtime().block_on(UnixListenerBuilder.bind_path(EffectfulPath)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
}

#[cfg(target_os = "trueos")]
#[test]
fn native_socket_factory_rejects_real_socket_without_listener_conversion() {
    let socket = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None).unwrap();
    let error = UnixListenerBuilder.bind_socket(socket).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
}

#[cfg(not(target_os = "trueos"))]
async fn exchange(listener: &UnixListener, path: &Path) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut client = tokio::net::UnixStream::connect(path).await.unwrap();
    let (mut server, _) = listener.inner.accept().await.unwrap();
    client.write_all(b"host unix io").await.unwrap();
    let mut bytes = [0; 12];
    server.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"host unix io");
    server.write_all(b"reply").await.unwrap();
    let mut reply = [0; 5];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"reply");
}

#[cfg(not(target_os = "trueos"))]
#[test]
fn host_path_factory_retains_stale_cleanup_real_io_and_drop_cleanup() {
    let path = temp_path("host-path");
    std::fs::write(&path, b"host upstream removes stale path").unwrap();
    runtime().block_on(async {
        let listener = UnixListenerBuilder.bind_path(&path).await.unwrap();
        exchange(&listener, &path).await;
        drop(listener);
    });
    assert!(!path.exists());
}

#[cfg(not(target_os = "trueos"))]
#[test]
fn host_socket_factory_owns_real_listener_without_path_cleanup() {
    let path = temp_path("host-socket");
    let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None).unwrap();
    socket.bind(&socket2::SockAddr::unix(&path).unwrap()).unwrap();
    socket.listen(8).unwrap();
    runtime().block_on(async {
        let listener = UnixListenerBuilder.bind_socket(socket).unwrap();
        exchange(&listener, &path).await;
        drop(listener);
    });
    assert!(path.exists());
    std::fs::remove_file(path).unwrap();
}
