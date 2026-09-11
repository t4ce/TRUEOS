//! Admission shared by the recorder and the UDP view subscriber. Input-only
//! RDP never enters this gate. Keep the short inter-session subscription gap
//! reserved for an existing viewer as well.

const RDP_RECONNECT_GRACE_NS: u64 = 2_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Idle,
    Rdp,
    Film,
    Quarantined,
}

pub(super) struct CaptureSessionGate {
    owner: Owner,
    last_view_end_ns: Option<u64>,
}

impl CaptureSessionGate {
    pub(super) const fn new() -> Self {
        Self {
            owner: Owner::Idle,
            last_view_end_ns: None,
        }
    }

    pub(super) fn claim_film(&mut self, now_ns: u64) -> Result<(), &'static str> {
        match self.owner {
            Owner::Film => return Err("a recording is already running"),
            Owner::Quarantined => return Err("capture hardware requires recovery"),
            Owner::Rdp => {
                return Err("RDP View-Mode is active; disconnect viewing or use --no-view");
            }
            Owner::Idle => {}
        }
        if self
            .last_view_end_ns
            .is_some_and(|end| now_ns.saturating_sub(end) < RDP_RECONNECT_GRACE_NS)
        {
            return Err("RDP View-Mode just ended; retry in two seconds with viewing disconnected");
        }
        self.owner = Owner::Film;
        Ok(())
    }

    pub(super) fn finish_film(&mut self, retired: bool) {
        if self.owner == Owner::Film {
            self.owner = if retired {
                Owner::Idle
            } else {
                Owner::Quarantined
            };
        }
    }

    pub(super) fn claim_view(&mut self) -> bool {
        if self.owner != Owner::Idle {
            return false;
        }
        self.owner = Owner::Rdp;
        true
    }

    pub(super) fn finish_view(&mut self, now_ns: u64) {
        if self.owner == Owner::Rdp {
            self.last_view_end_ns = Some(now_ns);
            self.owner = Owner::Idle;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_and_film_are_mutually_exclusive_in_both_orders() {
        let mut gate = CaptureSessionGate::new();
        assert!(gate.claim_view());
        assert!(gate.claim_film(0).is_err());
        gate.finish_view(10);
        assert!(gate.claim_film(11).is_err());
        assert!(gate.claim_film(10 + RDP_RECONNECT_GRACE_NS).is_ok());
        assert!(!gate.claim_view());
        assert!(gate.claim_film(u64::MAX).is_err());
        gate.finish_film(true);
        assert!(gate.claim_view());
    }

    #[test]
    fn failed_retirement_keeps_both_consumers_out() {
        let mut gate = CaptureSessionGate::new();
        assert!(gate.claim_film(0).is_ok());
        gate.finish_film(false);
        assert!(!gate.claim_view());
        assert!(gate.claim_film(u64::MAX).is_err());
    }

    #[test]
    fn cancelled_admission_can_retry_without_a_view_cooldown() {
        let mut gate = CaptureSessionGate::new();
        assert!(gate.claim_film(0).is_ok());
        gate.finish_film(true);
        assert!(gate.claim_film(1).is_ok());
    }
}
