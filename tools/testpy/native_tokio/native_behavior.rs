//! Host ABI simulation, not a TRUEOS kernel/carrier execution proof.
mod boundary;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
tokio::task_local! {static TASK_VALUE:usize;}
fn current() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
}
fn test(name: &str, f: impl FnOnce()) {
    f();
    println!("ok {name}");
}
struct CountDrop(Arc<AtomicUsize>);
impl Drop for CountDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
static EXEC_VALUE: trueos_runtime::thread::LocalKey<std::cell::Cell<usize>> =
    trueos_runtime::thread::LocalKey::new(|| std::cell::Cell::new(0));
static EXEC_DROP: trueos_runtime::thread::LocalKey<CountDrop> =
    trueos_runtime::thread::LocalKey::new(|| CountDrop(DROP_COUNT.get().unwrap().clone()));
static DROP_COUNT: std::sync::OnceLock<Arc<AtomicUsize>> = std::sync::OnceLock::new();
static FINAL_METADATA_CHECKS: AtomicUsize = AtomicUsize::new(0);
static OTHER_CLEANUP_CHECKS: AtomicUsize = AtomicUsize::new(0);
struct MetadataOnDrop {
    id: trueos_runtime::thread::ThreadId,
    name: String,
}
impl Drop for MetadataOnDrop {
    fn drop(&mut self) {
        let current = trueos_runtime::thread::current();
        assert_eq!(current.id(), self.id);
        assert_eq!(current.name(), Some(self.name.as_str()));
        current.unpark();
        trueos_runtime::thread::park();
        FINAL_METADATA_CHECKS.fetch_add(1, Ordering::SeqCst);
    }
}
unsafe extern "C" fn other_cleanup() {
    OTHER_CLEANUP_CHECKS.fetch_add(1, Ordering::SeqCst);
}
fn main() {
    test("current-thread spawn channels timer", || {
        assert_eq!(
            current().block_on(async {
                let (tx, mut rx) = tokio::sync::mpsc::channel(1);
                let j = tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                    tx.send(42).await.unwrap();
                });
                let v = rx.recv().await.unwrap();
                j.await.unwrap();
                v
            }),
            42
        );
    });
    test("notify wake-before-await and coalescing", || {
        current().block_on(async {
            let n = tokio::sync::Notify::new();
            n.notify_one();
            n.notify_one();
            n.notified().await;
            assert!(
                tokio::time::timeout(Duration::from_millis(2), n.notified())
                    .await
                    .is_err()
            );
        });
    });
    test("abort drops polled task and reports cancellation", || {
        let count = Arc::new(AtomicUsize::new(0));
        current().block_on(async {
            let c = count.clone();
            let j = tokio::spawn(async move {
                let _guard = CountDrop(c);
                std::future::pending::<()>().await;
            });
            tokio::task::yield_now().await;
            j.abort();
            assert!(j.await.unwrap_err().is_cancelled());
        });
        assert_eq!(count.load(Ordering::SeqCst), 1);
    });
    test("joinset queue and cooperative yielding", || {
        current().block_on(async {
            let mut set = tokio::task::JoinSet::new();
            for i in 0..128 {
                set.spawn(async move {
                    tokio::task::yield_now().await;
                    i
                });
            }
            let mut sum = 0;
            while let Some(v) = set.join_next().await {
                sum += v.unwrap();
            }
            assert_eq!(sum, (0..128).sum::<usize>());
        });
    });
    test("task local nested scope restores values", || {
        current().block_on(TASK_VALUE.scope(10, async {
            assert_eq!(TASK_VALUE.get(), 10);
            TASK_VALUE
                .scope(20, async {
                    assert_eq!(TASK_VALUE.get(), 20);
                    tokio::task::yield_now().await;
                    assert_eq!(TASK_VALUE.get(), 20);
                })
                .await;
            assert_eq!(TASK_VALUE.get(), 10);
        }));
        assert!(TASK_VALUE.try_with(|_| ()).is_err());
    });
    test("localset retains non-Send state", || {
        let local = tokio::task::LocalSet::new();
        let value = std::rc::Rc::new(std::cell::Cell::new(0));
        let output = value.clone();
        local.block_on(&current(), async move {
            let j = tokio::task::spawn_local(async move {
                tokio::task::yield_now().await;
                output.set(91);
            });
            j.await.unwrap();
        });
        assert_eq!(value.get(), 91);
    });
    test("duplex backpressure write read shutdown", || {
        current().block_on(async {
            let (mut a, mut b) = tokio::io::duplex(3);
            let writer = tokio::spawn(async move {
                a.write_all(b"abcdefgh").await.unwrap();
                a.shutdown().await.unwrap();
            });
            let mut data = Vec::new();
            assert_eq!(b.read_to_end(&mut data).await.unwrap(), 8);
            writer.await.unwrap();
            assert_eq!(data, b"abcdefgh");
        });
    });
    test("cursor growing zero fill and vectored write", || {
        current().block_on(async {
            let mut c = trueos_runtime::io::Cursor::new(Vec::new());
            c.set_position(3);
            c.write_all(b"ab").await.unwrap();
            assert_eq!(c.into_inner(), b"\0\0\0ab");
            let mut c = trueos_runtime::io::Cursor::new(vec![0; 3].into_boxed_slice());
            let slices = [
                trueos_runtime::io::IoSlice::new(b"ab"),
                trueos_runtime::io::IoSlice::new(b"cd"),
            ];
            assert_eq!(c.write_vectored(&slices).await.unwrap(), 3);
            assert_eq!(&*c.into_inner(), b"abc");
        });
    });
    test("invalid UTF8 read_line retains original string", || {
        current().block_on(async {
            let data = &b"abc\xff\n"[..];
            let mut reader = tokio::io::BufReader::new(data);
            let mut out = String::from("before");
            let err = reader.read_line(&mut out).await.unwrap_err();
            assert_eq!(err.kind(), trueos_runtime::io::ErrorKind::InvalidData);
            assert_eq!(out, "before");
        });
    });
    test("semaphore cancelled waiter does not consume permit", || {
        current().block_on(async {
            let s = Arc::new(tokio::sync::Semaphore::new(1));
            let permit = s.clone().acquire_owned().await.unwrap();
            let s2 = s.clone();
            let j = tokio::spawn(async move { s2.acquire_owned().await.unwrap() });
            tokio::task::yield_now().await;
            j.abort();
            assert!(j.await.unwrap_err().is_cancelled());
            drop(permit);
            assert_eq!(s.available_permits(), 1);
            drop(s.try_acquire().unwrap());
        });
    });
    test("watch and rwlock cross task publication", || {
        current().block_on(async {
            let (tx, mut rx) = tokio::sync::watch::channel(0);
            let data = Arc::new(tokio::sync::RwLock::new(0));
            let d = data.clone();
            let j = tokio::spawn(async move {
                *d.write().await = 47;
                tx.send(1).unwrap();
            });
            rx.changed().await.unwrap();
            assert_eq!(*data.read().await, 47);
            assert_eq!(*rx.borrow(), 1);
            j.await.unwrap();
        });
    });
    test("blocking pool executes and rejoins native threads", || {
        let rt = current();
        let id = trueos_runtime::thread::current().id();
        assert_eq!(
            rt.block_on(async move {
                tokio::task::spawn_blocking(move || {
                    assert_ne!(trueos_runtime::thread::current().id(), id);
                    6 * 7
                })
                .await
                .unwrap()
            }),
            42
        );
        drop(rt);
    });
    test(
        "native TLS separates identities and joins after destructors",
        || {
            let drops = Arc::new(AtomicUsize::new(0));
            DROP_COUNT
                .set(drops.clone())
                .unwrap_or_else(|_| panic!("duplicate"));
            EXEC_VALUE.with(|v| v.set(9));
            let j = trueos_runtime::thread::Builder::new()
                .name("native-test".into())
                .spawn(|| {
                    let id = trueos_runtime::thread::current().id();
                    assert_eq!(
                        trueos_runtime::thread::current().name(),
                        Some("native-test")
                    );
                    EXEC_VALUE.with(|v| {
                        assert_eq!(v.get(), 0);
                        v.set(22)
                    });
                    EXEC_DROP.with(|_| ());
                    id
                })
                .unwrap();
            let expected = j.thread().id();
            assert_eq!(j.join().unwrap(), expected);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            EXEC_VALUE.with(|v| assert_eq!(v.get(), 9));
        },
    );
    test("native unpark token retained before park", || {
        let (tx, rx) = std::sync::mpsc::channel();
        let j = trueos_runtime::thread::spawn(move || {
            let me = trueos_runtime::thread::current();
            me.unpark();
            trueos_runtime::thread::park();
            tx.send(()).unwrap();
        });
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        j.join().unwrap();
    });
    test(
        "multithread scheduler executes remote wakes and timers",
        || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_time()
                .build()
                .unwrap();
            rt.block_on(async {
                let shared = Arc::new(tokio::sync::Mutex::new(0));
                let mut jobs = Vec::new();
                for _ in 0..64 {
                    let s = shared.clone();
                    jobs.push(tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                        *s.lock().await += 1;
                    }));
                }
                for j in jobs {
                    j.await.unwrap();
                }
                assert_eq!(*shared.lock().await, 64);
            });
            drop(rt);
        },
    );
    test("generation wait releases mutex and observes notify", || {
        let pair = Arc::new((
            trueos_runtime::sync::Mutex::new(false),
            trueos_runtime::sync::Condvar::new(),
        ));
        let p = pair.clone();
        let j = trueos_runtime::thread::spawn(move || {
            let mut value = p.0.lock().unwrap();
            while !*value {
                value = p.1.wait(value).unwrap();
            }
        });
        *pair.0.lock().unwrap() = true;
        pair.1.notify_one();
        j.join().unwrap();
    });
    test(
        "once contention publishes exactly once and fast path",
        || {
            let once = Arc::new(trueos_runtime::sync::Once::new());
            let n = Arc::new(AtomicUsize::new(0));
            let mut jobs = Vec::new();
            for _ in 0..8 {
                let o = once.clone();
                let n = n.clone();
                jobs.push(trueos_runtime::thread::spawn(move || {
                    o.call_once(|| {
                        n.fetch_add(1, Ordering::SeqCst);
                    })
                }));
            }
            for j in jobs {
                j.join().unwrap();
            }
            assert!(once.is_completed());
            assert_eq!(n.load(Ordering::SeqCst), 1);
        },
    );
    test(
        "thread metadata survives ordered user TLS destructors and coexistence with std cleanup",
        || {
            let j = trueos_runtime::thread::Builder::new()
                .name("metadata-final".into())
                .spawn(|| {
                    let _ = trueos_runtime::thread::current();
                    boundary::trueos_platform_thread_local_register_cleanup(other_cleanup);
                    boundary::trueos_platform_thread_local_register_cleanup(other_cleanup);
                    // This heap key sorts after runtime static keys in the native BTreeMap.
                    // Retain it through execution teardown, just like an ordinary static key.
                    let key = Box::leak(Box::new(trueos_runtime::thread::LocalKey::new(|| {
                        let current = trueos_runtime::thread::current();
                        MetadataOnDrop {
                            id: current.id(),
                            name: current.name().unwrap().to_owned(),
                        }
                    })));
                    key.with(|_| ());
                })
                .unwrap();
            j.join().unwrap();
            assert_eq!(FINAL_METADATA_CHECKS.load(Ordering::SeqCst), 1);
            assert_eq!(OTHER_CLEANUP_CHECKS.load(Ordering::SeqCst), 1);
        },
    );
    println!("18 native-runtime behavior cases passed (host ABI simulation)");
    boundary::cleanup_tls();
}
