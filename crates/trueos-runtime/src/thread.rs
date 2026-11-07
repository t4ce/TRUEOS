//! Logical threads and execution-local storage owned by the native process.
use crate::{abi, io, sync::Mutex};
use alloc::{boxed::Box, string::String, sync::Arc};
use core::{
    any::Any,
    cell::UnsafeCell,
    ffi::{c_char, c_void},
    fmt,
    marker::PhantomData,
    mem::ManuallyDrop,
    num::NonZeroUsize,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
unsafe extern "C" {
    fn trueos_cabi_thread_spawn(
        stack: usize,
        entry: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        arg: *mut c_void,
        out: *mut usize,
    ) -> i32;
    fn trueos_cabi_thread_join(handle: usize) -> i32;
    fn trueos_cabi_thread_detach(handle: usize) -> i32;
    fn trueos_cabi_thread_current_id() -> usize;
    fn trueos_cabi_thread_available_parallelism() -> usize;
    fn trueos_cabi_thread_set_name(name: *const c_char) -> i32;
    fn trueos_cabi_poll_once();
    fn trueos_cabi_sleep_ms(ms: u64);
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ThreadId(usize);
impl ThreadId {
    pub fn as_u64(self) -> core::num::NonZeroU64 {
        core::num::NonZeroU64::new(self.0 as u64).expect("native execution identity is zero")
    }
}
#[derive(Clone, Debug)]
pub struct Thread {
    id: ThreadId,
    name: Option<Arc<str>>,
    parker: Arc<Parker>,
}
impl Thread {
    pub fn id(&self) -> ThreadId {
        self.id
    }
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    pub fn unpark(&self) {
        *self.parker.notified.lock().unwrap() = true;
        self.parker.ready.notify_one();
    }
}
pub fn current() -> Thread {
    let context = context();
    Thread {
        id: context.id,
        name: context.name.clone(),
        parker: context.parker.clone(),
    }
}
/// Native builds use panic-abort and cannot observe a Rust unwinder in flight.
pub fn panicking() -> bool {
    false
}
pub fn yield_now() {
    unsafe {
        trueos_cabi_poll_once();
    }
}
pub fn sleep(duration: Duration) {
    let mut ms = duration.as_nanos().div_ceil(1_000_000);
    while ms != 0 {
        let chunk = ms.min(u64::MAX as u128) as u64;
        unsafe {
            trueos_cabi_sleep_ms(chunk);
        }
        ms -= chunk as u128;
    }
}
pub fn available_parallelism() -> io::Result<NonZeroUsize> {
    NonZeroUsize::new(unsafe { trueos_cabi_thread_available_parallelism() })
        .ok_or_else(|| io::other("native thread count is zero"))
}
pub type Result<T> = core::result::Result<T, Box<dyn Any + Send + 'static>>;
#[derive(Debug, Default)]
pub struct Builder {
    name: Option<String>,
    stack_size: Option<usize>,
}
impl Builder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn name(mut self, name: String) -> Self {
        assert!(!name.as_bytes().contains(&0), "thread name contains NUL");
        self.name = Some(name);
        self
    }
    pub fn stack_size(mut self, size: usize) -> Self {
        self.stack_size = Some(size);
        self
    }
    pub fn spawn<F, T>(self, f: F) -> io::Result<JoinHandle<T>>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let packet = Arc::new(Packet {
            value: UnsafeCell::new(None),
            finished: AtomicBool::new(false),
        });
        let output = packet.clone();
        let name = self.name.clone();
        let parker = Arc::new(Parker::new());
        let child_parker = parker.clone();
        let run: Box<dyn FnOnce() + Send> = Box::new(move || {
            install_context(name.as_deref().map(Arc::from), child_parker);
            if let Some(mut name) = name {
                name.push('\0');
                unsafe {
                    trueos_cabi_thread_set_name(name.as_ptr().cast());
                }
            }
            let value = f();
            unsafe {
                *output.value.get() = Some(value);
            }
            output.finished.store(true, Ordering::Release);
        });
        // Double box gives the C ABI a thin pointer; failure retains ownership.
        let arg = Box::into_raw(Box::new(run));
        let mut handle = 0;
        let status = unsafe {
            trueos_cabi_thread_spawn(
                self.stack_size.unwrap_or(2 * 1024 * 1024),
                start,
                arg.cast(),
                &mut handle,
            )
        };
        if status != 0 {
            unsafe {
                drop(Box::from_raw(arg));
            }
            return Err(io::errno_error(status));
        }
        assert_ne!(handle, 0, "native spawn returned zero handle");
        Ok(JoinHandle {
            handle,
            packet,
            thread: Thread {
                id: ThreadId(handle),
                name: self.name.map(Arc::from),
                parker,
            },
        })
    }
}
unsafe extern "C" fn start(arg: *mut c_void) -> *mut c_void {
    let run = unsafe { Box::from_raw(arg.cast::<Box<dyn FnOnce() + Send>>()) };
    run();
    ptr::null_mut()
}
struct Packet<T> {
    value: UnsafeCell<Option<T>>,
    finished: AtomicBool,
}
unsafe impl<T: Send> Send for Packet<T> {}
unsafe impl<T: Send> Sync for Packet<T> {}
pub struct JoinHandle<T> {
    handle: usize,
    packet: Arc<Packet<T>>,
    thread: Thread,
}
impl<T> fmt::Debug for JoinHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JoinHandle")
            .field("thread", &self.thread)
            .finish_non_exhaustive()
    }
}
impl<T> JoinHandle<T> {
    pub fn thread(&self) -> &Thread {
        &self.thread
    }
    pub fn is_finished(&self) -> bool {
        self.packet.finished.load(Ordering::Acquire)
    }
    pub fn join(self) -> Result<T> {
        let me = ManuallyDrop::new(self);
        let status = unsafe { trueos_cabi_thread_join(me.handle) };
        assert_eq!(status, 0, "native join failed");
        assert!(
            me.packet.finished.load(Ordering::Acquire),
            "native join returned before completion"
        );
        let value = unsafe {
            (*me.packet.value.get())
                .take()
                .expect("native thread returned no result")
        };
        unsafe {
            drop(ptr::read(&me.packet));
            drop(ptr::read(&me.thread));
        }
        Ok(value)
    }
}
impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        let status = unsafe { trueos_cabi_thread_detach(self.handle) };
        debug_assert_eq!(status, 0, "native detach failed");
    }
}
pub fn spawn<F, T>(f: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    Builder::new()
        .spawn(f)
        .expect("failed to spawn native thread")
}

#[derive(Debug, Clone, Copy)]
pub struct AccessError;
impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("execution-local value is initializing or destroyed")
    }
}
impl core::error::Error for AccessError {}
pub struct LocalKey<T: 'static> {
    init: fn() -> T,
    _value: PhantomData<fn() -> T>,
}
// The key is immutable; values are stored in each native logical execution.
unsafe impl<T> Sync for LocalKey<T> {}
struct LocalValue<T: 'static> {
    key: &'static LocalKey<T>,
    value: T,
}
impl<T: 'static> LocalKey<T> {
    pub const fn new(init: fn() -> T) -> Self {
        Self {
            init,
            _value: PhantomData,
        }
    }
    pub fn with<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        self.try_with(f)
            .expect("cannot access native execution-local value")
    }
    pub fn try_with<F, R>(&'static self, f: F) -> core::result::Result<R, AccessError>
    where
        F: FnOnce(&T) -> R,
    {
        let key = self as *const Self as usize;
        let mut value = unsafe { abi::trueos_platform_thread_local_get(key) };
        if value.addr() == 1 || value.addr() == 2 {
            return Err(AccessError);
        }
        if value.is_null() {
            assert_eq!(
                unsafe {
                    abi::trueos_platform_thread_local_set(key, ptr::without_provenance_mut(1), None)
                },
                0,
                "native TLS initialization failed"
            );
            value = Box::into_raw(Box::new(LocalValue {
                key: self,
                value: (self.init)(),
            }))
            .cast();
            let status =
                unsafe { abi::trueos_platform_thread_local_set(key, value, Some(destroy::<T>)) };
            if status != 0 {
                unsafe {
                    drop(Box::from_raw(value.cast::<LocalValue<T>>()));
                }
                panic!("native TLS publication failed");
            }
        }
        Ok(f(unsafe { &(*value.cast::<LocalValue<T>>()).value }))
    }
}
unsafe extern "C" fn destroy<T: 'static>(ptr: *mut u8) {
    let value = unsafe { Box::from_raw(ptr.cast::<LocalValue<T>>()) };
    let key = value.key as *const LocalKey<T> as usize;
    // Mark destroyed before running user drop; reentry must not resurrect T.
    let status =
        unsafe { abi::trueos_platform_thread_local_set(key, ptr::without_provenance_mut(2), None) };
    assert_eq!(status, 0, "native TLS destruction marker failed");
    drop(value);
}

// Per-execution park token: repeated unparks coalesce, and a wake preceding park
// is retained. The token is distinct from the kernel generation counter.
#[derive(Debug)]
struct Parker {
    notified: Mutex<bool>,
    ready: crate::sync::Condvar,
}
impl Parker {
    fn new() -> Self {
        Self {
            notified: Mutex::new(false),
            ready: crate::sync::Condvar::new(),
        }
    }
}

// Thread identity/name/park token outlive all ordinary execution-local values.
// Their cleanup is registered separately from LocalKey destructors, so an
// ordinary destructor can still inspect its real thread and consume a retained
// unpark token. The platform retires the key table after these final callbacks.
struct ThreadContext {
    id: ThreadId,
    name: Option<Arc<str>>,
    parker: Arc<Parker>,
}
static THREAD_CONTEXT_KEY: AtomicBool = AtomicBool::new(false);
fn context_key() -> usize {
    &THREAD_CONTEXT_KEY as *const AtomicBool as usize
}
fn install_context(name: Option<Arc<str>>, parker: Arc<Parker>) -> &'static ThreadContext {
    let key = context_key();
    assert!(
        unsafe { abi::trueos_platform_thread_local_get(key) }.is_null(),
        "native thread metadata initialized twice"
    );
    let id = unsafe { trueos_cabi_thread_current_id() };
    assert_ne!(id, 0, "native execution identity is zero");
    let value = Box::into_raw(Box::new(ThreadContext {
        id: ThreadId(id),
        name,
        parker,
    }));
    let published = unsafe { abi::trueos_platform_thread_local_set(key, value.cast(), None) };
    if published != 0 {
        unsafe {
            drop(Box::from_raw(value));
        }
        panic!("native thread metadata publication failed");
    }
    let registered = unsafe { abi::trueos_platform_thread_local_register_cleanup(destroy_context) };
    if registered != 0 {
        // No callback was admitted. Reclaim our allocation and remove the key;
        // shutdown must never retain an unowned pointer after this failure.
        let cleared = unsafe { abi::trueos_platform_thread_local_set(key, ptr::null_mut(), None) };
        assert_eq!(cleared, 0, "native thread metadata rollback failed");
        unsafe {
            drop(Box::from_raw(value));
        }
        panic!("native thread metadata cleanup registration failed");
    }
    // The final platform callback is the unique owner. This private reference
    // is used only while the current execution is live; public handles clone
    // the Arc fields and never expose a reference into this allocation.
    unsafe { &*value }
}
fn context() -> &'static ThreadContext {
    let value = unsafe { abi::trueos_platform_thread_local_get(context_key()) };
    if value.is_null() {
        return install_context(None, Arc::new(Parker::new()));
    }
    assert!(value.addr() > 2, "native thread metadata already retired");
    unsafe { &*value.cast::<ThreadContext>() }
}
unsafe extern "C" fn destroy_context() {
    let key = context_key();
    let value = unsafe { abi::trueos_platform_thread_local_get(key) };
    assert!(value.addr() > 2, "native thread metadata cleanup ran twice");
    let status =
        unsafe { abi::trueos_platform_thread_local_set(key, ptr::without_provenance_mut(2), None) };
    assert_eq!(status, 0, "native thread metadata retirement failed");
    unsafe {
        drop(Box::from_raw(value.cast::<ThreadContext>()));
    }
}
pub fn park() {
    let parker = context().parker.clone();
    let mut notified = parker.notified.lock().unwrap();
    while !*notified {
        notified = parker.ready.wait(notified).unwrap();
    }
    *notified = false;
}
pub fn park_timeout(duration: Duration) {
    let parker = context().parker.clone();
    let mut notified = parker.notified.lock().unwrap();
    if !*notified {
        let (guard, _) = parker.ready.wait_timeout(notified, duration).unwrap();
        notified = guard;
    }
    *notified = false;
}
