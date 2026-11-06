#[cfg(test)]
mod regression {
    use super::*;
    use crate::{block_on, dev, host};
    use crate::usb3::heal_service;

    #[test]
    fn routed_live_observation_and_guarded_repair_preserve_ownership() {
        let mut host0 = host();
        let mut host1 = host();
        let pending_report = block_on(heal_service::register_pending_controller(0, dev(0), &mut host0, "initializing"));
        assert_eq!(pending_report.mode, heal_service::HealControllerMode::Initializing);
        assert!(pending_report.failure.is_none());
        let mut report0 = block_on(heal_service::register_live_controller(0, dev(0), &mut host0));
        let mut report1 = block_on(heal_service::register_live_controller(1, dev(1), &mut host1));
        let reports: serde_json::Value = serde_json::from_slice(&latest_report_json()).unwrap();
        assert_eq!(reports.as_array().unwrap().len(), 2);
        assert_ne!(report0.session_id, report1.session_id);

        let pending = submit_json(br#"{"op":"observe","api":1,"controller":1}"#).unwrap();
        assert!(!block_on(service_one(&mut host0, &mut report0)));
        assert!(block_on(service_one(&mut host1, &mut report1)));
        let response = block_on(pending.wait()).unwrap();
        assert_eq!(response.report.unwrap().seed.slot, 11);
        assert_eq!(host0.controller_resets + host1.controller_resets, 0);

        // A stale or live whole-controller transport request cannot reset any rings.
        let stale = block_on(execute(Some(&mut host1), &mut report1, HealRequest::Run {
            api: 1, controller: 1, expected_revision: 0, pattern: HealPattern::ControllerTransportV1 { max_slots: 8, noop_timeout_ms: 100 }
        }));
        assert_eq!(stale.error, Some(HealErrorCode::StaleRevision));
        let revision = report1.revision;
        let live = block_on(execute(Some(&mut host1), &mut report1, HealRequest::Run {
            api: 1, controller: 1, expected_revision: revision, pattern: HealPattern::ControllerTransportV1 { max_slots: 8, noop_timeout_ms: 100 }
        }));
        assert_eq!(live.error, Some(HealErrorCode::LiveControllerProtected));
        assert_eq!(host1.controller_resets, 0);

        // Pending enumeration/handoff or a stopped event pump rejects mutation before queueing.
        for flag in [&crate::usb3::runtime::BUSY, &crate::usb3::runtime::OPERATING] {
            flag.store(true, core::sync::atomic::Ordering::Relaxed);
            assert!(matches!(submit(HealRequest::Run { api: 1, controller: 1, expected_revision: report1.revision,
                pattern: HealPattern::Usb2PortResetV1 { port: HealPortSelector::Port { port_id: 1 }, timeout_ms: 100 }
            }), Err(HealSubmitError::OwnerBusy)));
            let cached: serde_json::Value = serde_json::from_slice(&latest_report_json()).unwrap();
            assert!(cached[1]["owner"]["probe_requested"] == true || cached[1]["owner"]["operation_requested"] == true);
            flag.store(false, core::sync::atomic::Ordering::Relaxed);
        }
        crate::usb3::runtime::READY.store(false, core::sync::atomic::Ordering::Relaxed);
        assert!(matches!(submit(HealRequest::Run { api: 1, controller: 1, expected_revision: report1.revision,
            pattern: HealPattern::Usb2PortResetV1 { port: HealPortSelector::Port { port_id: 1 }, timeout_ms: 100 }
        }), Err(HealSubmitError::OwnerBusy)));
        crate::usb3::runtime::READY.store(true, core::sync::atomic::Ordering::Relaxed);

        // Explicit repair of an unaddressed USB2 port completes and requests normal discovery.
        let revision = report1.revision;
        let repair = block_on(execute(Some(&mut host1), &mut report1, HealRequest::Run {
            api: 1, controller: 1, expected_revision: revision,
            pattern: HealPattern::Usb2PortResetV1 { port: HealPortSelector::Port { port_id: 1 }, timeout_ms: 100 }
        }));
        assert_eq!(repair.status, HealResponseStatus::Ok);
        assert_eq!(host1.port_resets, 1);
        let proof = report1.last_port_reset.as_ref().unwrap();
        assert!(proof.completed && proof.rescan_requested);
        assert!(heal_service::take_rescan_requested(1));
        assert!(!heal_service::take_rescan_requested(1));
        assert!(!heal_service::take_rescan_requested(0));

        // A disabled port is insufficient: ever-addressed ports require DMA teardown.
        host0.ever_probed = true;
        let revision = report0.revision;
        let protected = block_on(execute(Some(&mut host0), &mut report0, HealRequest::Run {
            api: 1, controller: 0, expected_revision: revision,
            pattern: HealPattern::Usb2PortResetV1 { port: HealPortSelector::Port { port_id: 1 }, timeout_ms: 100 }
        }));
        assert_eq!(protected.error, Some(HealErrorCode::PortResetFailure));
        assert_eq!(host0.port_resets, 0);
        assert!(!heal_service::take_rescan_requested(0));

        // An expired caller can never accidentally trigger a repair much later.
        host0.ever_probed = false;
        let pending = submit(HealRequest::Run { api: 1, controller: 0, expected_revision: report0.revision,
            pattern: HealPattern::Usb2PortResetV1 { port: HealPortSelector::Port { port_id: 1 }, timeout_ms: 100 }
        }).unwrap();
        assert!(matches!(block_on(pending.wait()), Err(HealWaitError::ExpiredBeforeExecution)));
        assert!(!block_on(service_one(&mut host0, &mut report0)));
        assert_eq!(host0.port_resets, 0);

        // Dropping a request before it executes also cancels it and frees capacity.
        for _ in 0..16 {
            drop(submit_json(br#"{"op":"observe","api":1}"#).unwrap());
        }
        assert!(!block_on(service_one(&mut host0, &mut report0)));
        let mut requests = Vec::new();
        for _ in 0..REQUEST_CAPACITY { requests.push(submit_json(br#"{"op":"observe","api":1}"#).unwrap()); }
        assert!(matches!(submit_json(br#"{"op":"observe","api":1}"#), Err(HealSubmitError::QueueFull)));
        drop(requests);

        assert!(matches!(submit_json(br#"{"op":"observe","api":1,"controller":2}"#), Err(HealSubmitError::ControllerUnavailable)));
        assert!(matches!(submit_json(br#"{"op":"observe","api":1,"register":32}"#), Err(HealSubmitError::InvalidJson)));
        assert!(matches!(submit_json(&vec![b' '; HEAL_MAX_REQUEST_BYTES + 1]), Err(HealSubmitError::TooLarge)));

        // A bounded reset failure records the attempt, keeps the owner live and schedules discovery.
        host0.reset_completes = false;
        let started = crate::Instant::now();
        let revision = report0.revision;
        let timed_out = block_on(execute(Some(&mut host0), &mut report0, HealRequest::Run {
            api: 1, controller: 0, expected_revision: revision,
            pattern: HealPattern::Usb2PortResetV1 { port: HealPortSelector::Port { port_id: 1 }, timeout_ms: 30 }
        }));
        assert_eq!(timed_out.error, Some(HealErrorCode::PortResetFailure));
        assert_eq!(crate::Instant::now().0 - started.0, 30);
        assert!(!report0.last_port_reset.as_ref().unwrap().completed);
        assert!(heal_service::take_rescan_requested(0));
        assert_eq!(host0.controller_resets, 0);

        let description: serde_json::Value = serde_json::from_slice(&api_description_json()).unwrap();
        assert!(description["patterns"].as_array().unwrap().iter().any(|pattern|
            pattern["name"] == "usb2_boot_hid_v1" && pattern["executable"] == false));
    }

    #[test]
    fn external_reset_guard_refuses_owned_enabled_disconnected_and_resetting_ports() {
        use crate::usb3::crabusb::explicit_reset_rejection as guard;
        assert!(guard(false, 1).is_none());
        assert!(guard(true, 1).unwrap().contains("DMA"));
        assert!(guard(false, 0).is_some());
        assert!(guard(false, 1 | (1 << 1)).is_some());
        assert!(guard(false, 1 | (1 << 4)).is_some());
        assert!(guard(false, 1 | (1 << 31)).is_some());
    }
}
