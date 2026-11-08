#![allow(dead_code)]
use core::future::{Future, poll_fn};
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::task::{Context, Poll, Waker};
use std::collections::VecDeque;
use std::sync::Arc;
// These adapters isolate lifecycle behavior from secret storage and OS locks.
type Zeroizing<T> = T;
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
trait Zeroize { fn zeroize(&mut self); }
impl Zeroize for Vec<u8> { fn zeroize(&mut self) { self.fill(0); } }
#[derive(Clone)] struct MatrixTarget(u64);
#[derive(Clone, Copy)] struct Frontend { id: u64 }
#[derive(Clone, Copy)] enum Kind { Cry, Disc }
#[derive(Clone, Copy)] struct Duration;
impl Duration { fn from_millis(_: u64) -> Self { Self } }
struct Timer;
impl Timer {
    async fn after(_: Duration) {
        let mut yielded = false;
        poll_fn(|_| if std::mem::replace(&mut yielded, true) { Poll::Ready(()) } else { Poll::Pending }).await;
    }
}
mod chronos { pub fn monotonic_nanos() -> u64 { 0 } }
mod crypt {
    pub struct Status { pub username: Option<String> }
    pub fn status() -> Status { Status { username: Some("t4ce".into()) } }
}
mod cry {
    use super::*;
    pub struct AccountEnrollment;
    pub static READY: AtomicBool = AtomicBool::new(true);
    pub static FAIL: AtomicBool = AtomicBool::new(false);
    pub static CALLS: AtomicUsize = AtomicUsize::new(0);
    pub async fn login_for_scope(_: u8, code: &str) -> Result<(String, u64), String> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        poll_fn(|_| if READY.load(Ordering::SeqCst) { Poll::Ready(()) } else { Poll::Pending }).await;
        if FAIL.load(Ordering::SeqCst) { return Err("Invalid authenticator code.".into()); }
        Ok(("Signed in; account saved.".into(), if code == "654321" { 814 } else { 712 }))
    }
}
struct Route {
    id: u64, name: String, exists: bool, visible: bool, owner: u64,
    account: Option<u64>, returned: usize, input: VecDeque<Vec<u8>>,
    painted: Vec<(&'static str, String)>,
}
static ROUTES: Mutex<Vec<Route>> = Mutex::new(Vec::new());
fn route<T>(id: u64, f: impl FnOnce(&mut Route) -> T) -> T {
    f(ROUTES.lock().iter_mut().find(|route| route.id == id).unwrap())
}
struct Token(MatrixTarget);
struct Spawner;
static SPAWNED: Mutex<Vec<MatrixTarget>> = Mutex::new(Vec::new());
impl Spawner { fn spawn(&self, token: Token) { SPAWNED.lock().push(token.0); } }
fn login_task(target: MatrixTarget) -> Result<Token, ()> { Ok(Token(target)) }
mod helper {
    use super::*;
    pub fn admit(name: &str, frontend: Frontend) -> Result<Option<(MatrixTarget, Spawner)>, String> {
        let mut routes = ROUTES.lock();
        if let Some(route) = routes.iter_mut().find(|route| route.name == name) {
            route.visible = true;
            return Ok(None);
        }
        routes.push(Route { id: frontend.id, name: name.into(), exists: true, visible: true,
            owner: frontend.id, account: None, returned: 0, input: VecDeque::new(), painted: Vec::new() });
        Ok(Some((MatrixTarget(frontend.id), Spawner)))
    }
}
mod tui {
    use super::*;
    pub struct Surface { pub rows: u16, pub cols: u16 }
    pub fn native_visible(target: &MatrixTarget) -> bool { route(target.0, |r| r.exists && r.visible) }
    pub fn native_frontend_id(target: &MatrixTarget) -> Option<u64> {
        route(target.0, |r| r.exists.then_some(r.owner))
    }
    pub fn bind_authenticated_frontend(id: u64, account: Option<u64>) -> bool {
        route(id, |r| { if !r.exists { return false; } r.account = account; true })
    }
    pub fn native_return(target: &MatrixTarget) {
        route(target.0, |r| { r.returned += 1; r.visible = false; r.input.clear(); });
    }
    pub fn cancel_native_attach(target: &MatrixTarget) { route(target.0, |r| r.exists = false); }
    pub fn native_read(target: &MatrixTarget) -> Option<(Vec<u8>, Vec<String>)> {
        route(target.0, |r| r.exists.then(|| (r.input.pop_front().unwrap_or_default(), Vec::new())))
    }
    pub fn surface(target: &MatrixTarget) -> Option<Surface> {
        route(target.0, |r| r.exists.then_some(Surface { rows: 24, cols: 80 }))
    }
}
struct Screen;
impl Screen { fn paint(&mut self, _: &MatrixTarget, _: &[String], _: usize, _: usize) {} }
enum Field { LoginCode }
enum Page {
    Home, Field(Field), Enrollment(Zeroizing<Vec<String>>),
    SavedRecovery { username: String, key: Zeroizing<String> },
}
impl Page {
    fn sensitive(&self) -> bool { !matches!(self, Self::Home) }
    fn label(&self) -> &'static str { if matches!(self, Self::Field(Field::LoginCode)) { "code" } else { "home" } }
}
fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}
mod admin {
use super::*;
@START_LOGIN@
@LOGIN_ATTEMPT@
@LOGIN_ATTEMPT_IMPL@
@OUTCOME@
@OUTCOME_IMPL@
@JOB@
type Reply = Arc<Mutex<Job>>;
enum Request { @LOGIN_VARIANT@ }
impl Request {
    async fn run(self, _: &Reply) -> Result<Outcome, String> { match self { @LOGIN_ARM@ } }
}
static REQUESTS: Mutex<Vec<(u64, Request, Reply)>> = Mutex::new(Vec::new());
struct View {
    login_only: bool, login_attempt: Option<Arc<AtomicBool>>, page: Page,
    pending: Option<Reply>, draft: Option<Arc<cry::AccountEnrollment>>,
    result: String, screen: Screen, geometry: (usize, usize),
}
impl View {
    fn new(_: Kind) -> Self {
        Self { login_only: false, login_attempt: None, page: Page::Home, pending: None,
            draft: None, result: String::new(), screen: Screen, geometry: (80, 24) }
    }
    fn set_page(&mut self, page: Page) { self.page = page; }
    fn problem(&mut self, error: &str) { self.result = format!("ERROR: {error}"); }
    fn submit(&mut self, target: &MatrixTarget, request: Request) {
        let reply = Arc::new(Mutex::new(Job { progress: "Working".into(), result: None }));
        self.pending = Some(reply.clone()); self.set_page(Page::Home);
        REQUESTS.lock().push((target.0, request, reply));
    }
    fn input(&mut self, target: &MatrixTarget, bytes: &[u8], _: u64, _: usize) {
        if bytes == b"cancel" { self.leave(target); }
        else if !bytes.is_empty() { self.submit_login(target, 0, String::from_utf8(bytes.to_vec()).unwrap()); }
    }
    fn paint(&mut self, target: &MatrixTarget, _: usize, _: usize, _: u64) {
        route(target.0, |r| r.painted.push((self.page.label(), self.result.clone())));
    }
    @SUBMIT_LOGIN@
    @CANCEL_LOGIN@
    @COMPLETE@
    @LEAVE@
}
@DROP@
@RUN_MENU@
#[cfg(test)] mod tests {
use super::*;
fn reset() {
    ROUTES.lock().clear(); SPAWNED.lock().clear(); REQUESTS.lock().clear();
    cry::READY.store(true, Ordering::SeqCst); cry::FAIL.store(false, Ordering::SeqCst);
    cry::CALLS.store(0, Ordering::SeqCst);
}
fn admit(id: u64) -> MatrixTarget {
    assert_eq!(start_login(Frontend { id }).unwrap(), format!("cry-login-{id}"));
    MatrixTarget(id)
}
fn take_request(id: u64) -> (Request, Reply) {
    let mut requests = REQUESTS.lock();
    let index = requests.iter().position(|entry| entry.0 == id).unwrap();
    let (_, request, reply) = requests.remove(index); (request, reply)
}
fn input(id: u64, value: &[u8]) { route(id, |r| r.input.push_back(value.to_vec())); }
fn ready_result(request: Request, reply: &Reply) -> Result<Outcome, String> {
    let mut future = std::pin::pin!(request.run(reply));
    match poll(future.as_mut()) { Poll::Ready(result) => result, Poll::Pending => panic!("unexpected pending login") }
}
#[test] fn starts_on_account_code_form_and_success_returns_to_shell() {
    reset(); let target = admit(10);
    let mut menu = std::pin::pin!(run_menu(Kind::Cry, target.clone(), true));
    assert!(poll(menu.as_mut()).is_pending());
    route(10, |r| assert_eq!(r.painted.last(), Some(&("code", "Sign in as t4ce.".into()))));
    input(10, b"123456"); assert!(poll(menu.as_mut()).is_pending());
    let (request, reply) = take_request(10); reply.lock().result = Some(ready_result(request, &reply));
    assert!(poll(menu.as_mut()).is_pending());
    route(10, |r| { assert_eq!(r.account, Some(712)); assert_eq!(r.returned, 1); assert!(!r.visible); });
}
#[test] fn invalid_code_keeps_the_form_and_can_retry() {
    reset(); let target = admit(10); cry::FAIL.store(true, Ordering::SeqCst);
    let mut menu = std::pin::pin!(run_menu(Kind::Cry, target, true));
    input(10, b"123456"); assert!(poll(menu.as_mut()).is_pending());
    let (request, reply) = take_request(10); reply.lock().result = Some(ready_result(request, &reply));
    assert!(poll(menu.as_mut()).is_pending());
    route(10, |r| { assert!(r.visible); assert_eq!(r.account, None); assert_eq!(r.returned, 0);
        let (page, message) = r.painted.last().unwrap(); assert_eq!(*page, "code"); assert!(message.contains("Invalid authenticator code")); });
    cry::FAIL.store(false, Ordering::SeqCst); input(10, b"123456");
    assert!(poll(menu.as_mut()).is_pending());
    let (request, reply) = take_request(10); reply.lock().result = Some(ready_result(request, &reply));
    assert!(poll(menu.as_mut()).is_pending()); route(10, |r| assert_eq!(r.account, Some(712)));
}
#[test] fn cancelled_before_work_starts_never_calls_cry_or_binds() {
    reset(); let target = admit(10);
    let mut menu = std::pin::pin!(run_menu(Kind::Cry, target, true));
    input(10, b"123456"); assert!(poll(menu.as_mut()).is_pending());
    let (request, reply) = take_request(10); input(10, b"cancel"); assert!(poll(menu.as_mut()).is_pending());
    assert!(ready_result(request, &reply).is_err()); assert_eq!(cry::CALLS.load(Ordering::SeqCst), 0);
    route(10, |r| assert_eq!(r.account, None));
}
#[test] fn cancel_during_persistence_then_reopen_rejects_old_completion() {
    reset(); let target = admit(10); cry::READY.store(false, Ordering::SeqCst);
    let mut menu = std::pin::pin!(run_menu(Kind::Cry, target, true));
    input(10, b"123456"); assert!(poll(menu.as_mut()).is_pending());
    let (request, reply) = take_request(10); let mut auth = std::pin::pin!(request.run(&reply));
    assert!(poll(auth.as_mut()).is_pending()); input(10, b"cancel"); assert!(poll(menu.as_mut()).is_pending());
    admit(10); assert!(poll(menu.as_mut()).is_pending());
    cry::READY.store(true, Ordering::SeqCst);
    assert!(matches!(poll(auth.as_mut()), Poll::Ready(Err(_)))); route(10, |r| assert_eq!(r.account, None));
}
#[test] fn hidden_and_closed_frontends_reject_in_flight_completion() {
    for close in [false, true] {
        reset(); let target = admit(10); cry::READY.store(false, Ordering::SeqCst);
        let mut view = View::new(Kind::Cry); view.login_only = true;
        view.submit_login(&target, 0, "123456".into()); let (request, reply) = take_request(10);
        let mut auth = std::pin::pin!(request.run(&reply)); assert!(poll(auth.as_mut()).is_pending());
        route(10, |r| if close { r.exists = false; } else { r.visible = false; });
        cry::READY.store(true, Ordering::SeqCst);
        assert!(matches!(poll(auth.as_mut()), Poll::Ready(Err(_)))); route(10, |r| assert_eq!(r.account, None));
    }
}
#[test] fn parking_and_resuming_the_menu_cancels_its_previous_attempt() {
    reset(); let target = admit(10); cry::READY.store(false, Ordering::SeqCst);
    let mut menu = std::pin::pin!(run_menu(Kind::Cry, target, true));
    input(10, b"123456"); assert!(poll(menu.as_mut()).is_pending());
    let (request, reply) = take_request(10); let mut auth = std::pin::pin!(request.run(&reply));
    assert!(poll(auth.as_mut()).is_pending());
    route(10, |r| r.visible = false); assert!(poll(menu.as_mut()).is_pending());
    route(10, |r| r.visible = true); assert!(poll(menu.as_mut()).is_pending());
    route(10, |r| assert_eq!(r.painted.last().unwrap().0, "code"));
    cry::READY.store(true, Ordering::SeqCst);
    assert!(matches!(poll(auth.as_mut()), Poll::Ready(Err(_)))); route(10, |r| assert_eq!(r.account, None));
}
#[test] fn dropped_menu_invalidates_a_pending_login_even_when_surface_survives() {
    reset(); let target = admit(10);
    let mut view = View::new(Kind::Cry); view.login_only = true;
    view.submit_login(&target, 0, "123456".into()); let (request, reply) = take_request(10);
    drop(view); assert!(ready_result(request, &reply).is_err()); route(10, |r| assert_eq!(r.account, None));
}
#[test] fn wrong_frontend_cannot_finish_another_windows_login() {
    reset(); let target = admit(10); let mut view = View::new(Kind::Cry); view.login_only = true;
    view.submit_login(&target, 0, "123456".into()); let (request, reply) = take_request(10);
    route(10, |r| r.owner = 20); assert!(ready_result(request, &reply).is_err());
    assert_eq!(cry::CALLS.load(Ordering::SeqCst), 0); route(10, |r| assert_eq!(r.account, None));
}
#[test] fn two_windows_have_distinct_helpers_and_only_bind_their_own_frontends() {
    reset(); let a = admit(10); let b = admit(20); assert_eq!(SPAWNED.lock().len(), 2);
    admit(10); assert_eq!(SPAWNED.lock().len(), 2);
    let mut first = View::new(Kind::Cry); first.login_only = true;
    let mut second = View::new(Kind::Cry); second.login_only = true;
    first.submit_login(&a, 0, "123456".into()); second.submit_login(&b, 0, "654321".into());
    let (request, reply) = take_request(10); assert!(ready_result(request, &reply).is_ok());
    route(10, |r| assert_eq!(r.account, Some(712))); route(20, |r| assert_eq!(r.account, None));
    let (request, reply) = take_request(20); assert!(ready_result(request, &reply).is_ok());
    route(10, |r| assert_eq!(r.account, Some(712))); route(20, |r| assert_eq!(r.account, Some(814)));
}
#[test] fn normal_admin_menu_keeps_home_and_explicit_return_behavior() {
    reset(); let target = admit(10);
    let mut menu = std::pin::pin!(run_menu(Kind::Cry, target, false));
    assert!(poll(menu.as_mut()).is_pending()); route(10, |r| assert_eq!(r.painted.last().unwrap().0, "home"));
    input(10, b"123456"); assert!(poll(menu.as_mut()).is_pending());
    let (request, reply) = take_request(10); reply.lock().result = Some(ready_result(request, &reply));
    assert!(poll(menu.as_mut()).is_pending()); route(10, |r| { assert!(r.visible); assert_eq!(r.returned, 0); });
}
}
}
