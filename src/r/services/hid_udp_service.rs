//! Legacy unauthenticated UDP input ingress is retired.
//! Remote devices are owned by the account-bound `trueos-rdp` SSH subsystem.
use trueos_executor::task;

#[task]
pub async fn hid_udp_srv_task() {
    // Remote HID now enters only through the account-bound SSH subsystem.
    // A LAN datagram cannot establish a user or obtain desktop input authority.
    crate::log_important!("hid-udp: disabled ingress=ssh-subsystem name=trueos-rdp\n");
}
