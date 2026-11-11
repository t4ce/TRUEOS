//! Bounded, reset-free initialization of the primary i8042 keyboard port.
//! The caller owns the controller; AUX bytes never become keyboard replies.

const OUTPUT_FULL: u8 = 1 << 0;
const INPUT_FULL: u8 = 1 << 1;
const AUX_OUTPUT: u8 = 1 << 5;
const RECEIVE_ERROR: u8 = (1 << 6) | (1 << 7);
const KEYBOARD_IRQ: u8 = 1 << 0;
const KEYBOARD_DISABLED: u8 = 1 << 4;
const TRANSLATION: u8 = 1 << 6;
const WAIT_MS: u64 = 200;
const POLL_LIMIT: usize = 256;
const FLUSH_LIMIT: usize = 64;
const SEND_ATTEMPTS: usize = 3;

pub(crate) trait Bus {
    fn status(&mut self) -> u8;
    fn read_data(&mut self) -> u8;
    fn write_command(&mut self, value: u8);
    fn write_data(&mut self, value: u8);
    fn now_ms(&self) -> u64;
    async fn pause(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Error {
    pub stage: &'static str,
    pub reason: &'static str,
}

fn error(stage: &'static str, reason: &'static str) -> Error {
    Error { stage, reason }
}

async fn wait_input(bus: &mut impl Bus, stage: &'static str) -> Result<(), Error> {
    let deadline = bus.now_ms().saturating_add(WAIT_MS);
    for _ in 0..POLL_LIMIT {
        let status = bus.status();
        if status == 0xff {
            return Err(error(stage, "controller absent"));
        }
        if status & INPUT_FULL == 0 {
            return Ok(());
        }
        if bus.now_ms() >= deadline {
            break;
        }
        bus.pause().await;
    }
    Err(error(stage, "input buffer timeout"))
}

async fn command(bus: &mut impl Bus, value: u8, stage: &'static str) -> Result<(), Error> {
    wait_input(bus, stage).await?;
    bus.write_command(value);
    Ok(())
}

async fn data(bus: &mut impl Bus, value: u8, stage: &'static str) -> Result<(), Error> {
    wait_input(bus, stage).await?;
    bus.write_data(value);
    Ok(())
}

async fn primary_reply(
    bus: &mut impl Bus,
    deadline: u64,
    stage: &'static str,
) -> Result<u8, Error> {
    for _ in 0..POLL_LIMIT {
        let status = bus.status();
        if status == 0xff {
            return Err(error(stage, "controller absent"));
        }
        if status & OUTPUT_FULL != 0 {
            let byte = bus.read_data();
            if status & (AUX_OUTPUT | RECEIVE_ERROR) == 0 {
                return Ok(byte);
            }
        }
        if bus.now_ms() >= deadline {
            break;
        }
        bus.pause().await;
    }
    Err(error(stage, "keyboard reply timeout"))
}

async fn device_command(bus: &mut impl Bus, value: u8, stage: &'static str) -> Result<(), Error> {
    for _ in 0..SEND_ATTEMPTS {
        data(bus, value, stage).await?;
        let deadline = bus.now_ms().saturating_add(WAIT_MS);
        // A held key can have queued scan bytes before F5 takes effect. Neither
        // those bytes nor AUX/error bytes count as acknowledgements.
        for _ in 0..POLL_LIMIT {
            match primary_reply(bus, deadline, stage).await? {
                0xfa => return Ok(()),
                0xfe => break,
                0xfc | 0xfd => return Err(error(stage, "keyboard rejected command")),
                _ if bus.now_ms() < deadline => {}
                _ => return Err(error(stage, "keyboard reply timeout")),
            }
        }
    }
    Err(error(stage, "keyboard resend limit"))
}

async fn read_config(bus: &mut impl Bus, stage: &'static str) -> Result<u8, Error> {
    command(bus, 0x20, stage).await?;
    primary_reply(bus, bus.now_ms().saturating_add(WAIT_MS), stage).await
}

async fn write_config(bus: &mut impl Bus, value: u8, stage: &'static str) -> Result<(), Error> {
    command(bus, 0x60, stage).await?;
    data(bus, value, stage).await
}

async fn configure(
    bus: &mut impl Bus,
    saved_config: &mut Option<u8>,
    touched: &mut bool,
) -> Result<(), Error> {
    command(bus, 0xad, "inhibit").await?;
    *touched = true;
    // AD is asynchronous on real controllers. Do not mistake an empty output
    // buffer before AD has executed for a completed keyboard inhibit/drain.
    wait_input(bus, "inhibit-complete").await?;
    for _ in 0..FLUSH_LIMIT {
        let status = bus.status();
        if status == 0xff {
            return Err(error("flush", "controller absent"));
        }
        if status & OUTPUT_FULL == 0 {
            break;
        }
        let _ = bus.read_data();
    }
    if bus.status() & OUTPUT_FULL != 0 {
        return Err(error("flush", "output buffer did not drain"));
    }

    let original = read_config(bus, "read-config").await?;
    *saved_config = Some(original);
    // Poll only the keyboard. Preserve AUX IRQ/inhibit policy, the system flag
    // and the remaining configuration bits; do not reset or probe the mouse.
    let config = original & !(KEYBOARD_IRQ | TRANSLATION | KEYBOARD_DISABLED);
    write_config(bus, config, "write-config").await?;
    command(bus, 0xae, "enable-port").await?;
    device_command(bus, 0xf5, "disable-scanning").await?;
    device_command(bus, 0xf0, "select-set").await?;
    device_command(bus, 0x02, "select-set2").await?;

    let observed = read_config(bus, "verify-config").await?;
    if observed & (KEYBOARD_IRQ | TRANSLATION | KEYBOARD_DISABLED) != 0 {
        return Err(error(
            "verify-config",
            "keyboard polling configuration rejected",
        ));
    }
    device_command(bus, 0xf0, "query-set").await?;
    device_command(bus, 0x00, "query-set-value").await?;
    let scan_set = primary_reply(bus, bus.now_ms().saturating_add(WAIT_MS), "verify-set2").await?;
    if scan_set != 2 {
        return Err(error("verify-set2", "keyboard did not select raw set2"));
    }
    device_command(bus, 0xf4, "enable-scanning").await
}

/// Select raw scan-code set 2 and polling without a keyboard/controller reset.
/// Any failure restores cached configuration and reenables scanning best effort.
/// AD already inhibited the primary port before its configuration was saved,
/// so cleanup explicitly reenables that port rather than preserving that bit.
pub(crate) async fn initialize(bus: &mut impl Bus) -> Result<(), Error> {
    let mut saved_config = None;
    let mut touched = false;
    let result = configure(bus, &mut saved_config, &mut touched).await;
    if result.is_err() && touched {
        if let Some(config) = saved_config {
            let _ = write_config(bus, config & !KEYBOARD_DISABLED, "restore-config").await;
        }
        let _ = command(bus, 0xae, "restore-port").await;
        let _ = device_command(bus, 0xf4, "restore-scanning").await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        future::Future,
        pin::pin,
        sync::Arc,
        task::{Context, Poll, Wake, Waker},
    };

    struct NoopWake;
    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    fn run<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        let mut future = pin!(future);
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
                return value;
            }
        }
    }

    struct FakeBus {
        time: u64,
        config: u8,
        output: VecDeque<(u8, u8)>,
        commands: Vec<u8>,
        data: Vec<u8>,
        config_write: bool,
        scan_parameter: bool,
        missing_ack: Option<u8>,
        resend: Option<u8>,
        resend_always: bool,
        stuck_input: bool,
        aux_only: bool,
        reject_config: bool,
        absent: bool,
        scan_set: u8,
        inhibit_delay: usize,
        inhibit_pending: bool,
    }

    impl FakeBus {
        fn new(config: u8) -> Self {
            Self {
                time: 0,
                config,
                output: VecDeque::new(),
                commands: Vec::new(),
                data: Vec::new(),
                config_write: false,
                scan_parameter: false,
                missing_ack: None,
                resend: None,
                resend_always: false,
                stuck_input: false,
                aux_only: false,
                reject_config: false,
                absent: false,
                scan_set: 2,
                inhibit_delay: 0,
                inhibit_pending: false,
            }
        }
        fn reply(&mut self, value: u8) {
            self.output.push_back((OUTPUT_FULL, value));
        }
    }

    impl Bus for FakeBus {
        fn status(&mut self) -> u8 {
            if self.absent {
                return 0xff;
            }
            if self.stuck_input {
                return INPUT_FULL;
            }
            if self.inhibit_pending {
                if self.inhibit_delay != 0 {
                    self.inhibit_delay -= 1;
                    return INPUT_FULL;
                }
                self.inhibit_pending = false;
                self.reply(0x1c); // Last scan byte while AD was being processed.
            }
            self.output.front().map(|item| item.0).unwrap_or(0)
        }
        fn read_data(&mut self) -> u8 {
            self.output.pop_front().unwrap().1
        }
        fn write_command(&mut self, value: u8) {
            self.commands.push(value);
            match value {
                0xad => {
                    self.config |= KEYBOARD_DISABLED;
                    self.inhibit_pending = self.inhibit_delay != 0;
                }
                0xae => self.config &= !KEYBOARD_DISABLED,
                0x20 => self.reply(self.config),
                0x60 => self.config_write = true,
                _ => panic!("unexpected controller command {value:x}"),
            }
        }
        fn write_data(&mut self, value: u8) {
            self.data.push(value);
            if self.config_write {
                self.config_write = false;
                if !self.reject_config {
                    self.config = value;
                }
                return;
            }
            if self.missing_ack == Some(value) {
                return;
            }
            if self.aux_only {
                self.output.push_back((OUTPUT_FULL | AUX_OUTPUT, 0xfa));
                return;
            }
            if self.resend == Some(value) {
                if !self.resend_always {
                    self.resend = None;
                }
                self.reply(0xfe);
                return;
            }
            self.reply(0xfa);
            if self.scan_parameter {
                self.scan_parameter = false;
                if value == 0 {
                    self.reply(self.scan_set);
                }
            } else if value == 0xf0 {
                self.scan_parameter = true;
            }
        }
        fn now_ms(&self) -> u64 {
            self.time
        }
        async fn pause(&mut self) {
            self.time += 1;
        }
    }

    #[test]
    fn initializes_without_reset_and_preserves_aux_policy() {
        let original = 0xef; // Translation, AUX IRQ/inhibit, system and reserved bits.
        let mut bus = FakeBus::new(original);
        bus.output
            .extend([(OUTPUT_FULL, 0x1c), (OUTPUT_FULL | AUX_OUTPUT, 0xfa)]);
        assert_eq!(run(initialize(&mut bus)), Ok(()));
        assert_eq!(
            bus.config,
            original & !(KEYBOARD_IRQ | TRANSLATION | KEYBOARD_DISABLED)
        );
        assert_eq!(bus.commands, [0xad, 0x20, 0x60, 0xae, 0x20]);
        assert_eq!(&bus.data[1..], [0xf5, 0xf0, 0x02, 0xf0, 0x00, 0xf4]);
        assert!(!bus.commands.contains(&0xaa));
        assert!(!bus.data.contains(&0xff));
        assert_eq!(bus.time, 0);
    }

    #[test]
    fn resend_retries_only_the_current_command() {
        let mut bus = FakeBus::new(0x47);
        bus.resend = Some(0x02);
        assert_eq!(run(initialize(&mut bus)), Ok(()));
        assert_eq!(bus.data.iter().filter(|&&value| value == 0x02).count(), 2);
    }

    #[test]
    fn missing_ack_times_out_and_restores_configuration() {
        let original = 0x67;
        let mut bus = FakeBus::new(original);
        bus.missing_ack = Some(0xf0);
        assert_eq!(
            run(initialize(&mut bus)),
            Err(error("select-set", "keyboard reply timeout"))
        );
        assert_eq!(bus.config, original & !KEYBOARD_DISABLED);
        assert!(bus.time >= WAIT_MS && bus.time <= WAIT_MS * 2);
        assert_eq!(bus.data.last(), Some(&0xf4));
    }

    #[test]
    fn input_buffer_stall_is_bounded_before_writes() {
        let mut bus = FakeBus::new(0);
        bus.stuck_input = true;
        assert_eq!(
            run(initialize(&mut bus)),
            Err(error("inhibit", "input buffer timeout"))
        );
        assert!(bus.commands.is_empty());
        assert_eq!(bus.time, WAIT_MS);
    }

    #[test]
    fn aux_ack_is_not_a_keyboard_ack() {
        let mut bus = FakeBus::new(0x04);
        bus.aux_only = true;
        assert_eq!(
            run(initialize(&mut bus)),
            Err(error("disable-scanning", "keyboard reply timeout"))
        );
        assert!(bus.time <= WAIT_MS * 2);
    }

    #[test]
    fn repeated_resend_is_bounded() {
        let mut bus = FakeBus::new(0x04);
        bus.resend = Some(0xf5);
        bus.resend_always = true;
        assert_eq!(
            run(initialize(&mut bus)),
            Err(error("disable-scanning", "keyboard resend limit"))
        );
        assert_eq!(
            bus.data.iter().filter(|&&value| value == 0xf5).count(),
            SEND_ATTEMPTS
        );
    }

    #[test]
    fn ignored_configuration_write_does_not_admit_translated_bytes() {
        let mut bus = FakeBus::new(0x47);
        bus.reject_config = true;
        assert_eq!(
            run(initialize(&mut bus)),
            Err(error(
                "verify-config",
                "keyboard polling configuration rejected"
            ))
        );
    }

    #[test]
    fn absent_controller_is_not_written() {
        let mut bus = FakeBus::new(0);
        bus.absent = true;
        assert_eq!(
            run(initialize(&mut bus)),
            Err(error("inhibit", "controller absent"))
        );
        assert!(bus.commands.is_empty() && bus.data.is_empty());
    }

    #[test]
    fn corrupt_and_aux_bytes_are_not_primary_replies() {
        let mut bus = FakeBus::new(0);
        bus.output.extend([
            (OUTPUT_FULL | AUX_OUTPUT, 0xfa),
            (OUTPUT_FULL | RECEIVE_ERROR, 0xfa),
            (OUTPUT_FULL, 0x02),
        ]);
        assert_eq!(run(primary_reply(&mut bus, WAIT_MS, "test")), Ok(0x02));
        assert_eq!(bus.time, 2);
    }

    #[test]
    fn keyboard_that_did_not_select_set2_is_rejected() {
        let mut bus = FakeBus::new(0x47);
        bus.scan_set = 1;
        assert_eq!(
            run(initialize(&mut bus)),
            Err(error("verify-set2", "keyboard did not select raw set2"))
        );
        assert_eq!(bus.config, 0x47);
    }

    #[test]
    fn initial_output_flood_is_bounded() {
        let mut bus = FakeBus::new(0x47);
        bus.output
            .extend((0..FLUSH_LIMIT + 1).map(|_| (OUTPUT_FULL, 0x1c)));
        assert_eq!(
            run(initialize(&mut bus)),
            Err(error("flush", "output buffer did not drain"))
        );
        assert!(!bus.commands.contains(&0x20));
    }

    #[test]
    fn drain_waits_for_inhibit_to_finish() {
        let mut bus = FakeBus::new(0xef);
        bus.inhibit_delay = 2;
        assert_eq!(run(initialize(&mut bus)), Ok(()));
        assert_eq!(bus.config, 0xae);
        assert_eq!(bus.time, 2);
    }
}
