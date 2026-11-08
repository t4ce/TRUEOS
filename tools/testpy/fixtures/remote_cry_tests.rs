#[cfg(test)]
mod remote_login_regressions {
    use super::*;
    use crate::shell3::{ssh,system::cry as command};
    fn reset() {
        *CRY_STATE.lock()=CryState::new();
        crate::reset_clock(); command::reset_store(); ssh::clear_queue();
    }
    fn fresh() -> AccountId {
        reset(); setup_root_key("t4ce").unwrap(); begin_totp_enrollment().unwrap();
        let account=AccountId::new(712);
        CRY_STATE.lock().credential.as_mut().unwrap().account=account;
        let secret=CRY_STATE.lock().totp.as_ref().unwrap().secret;
        let value=trueos_crypto::generate_totp_sha1(&secret,crate::unix_seconds()/TOTP_PERIOD_SECONDS).unwrap();
        let proof=prepare_login(&alloc::format!("{value:06}"),1).unwrap();
        let plan=prepare_persistence(proof.challenge_sequence).unwrap();
        let key=*plan.recovery_key_bytes(); let envelope=plan.envelope.to_vec();
        complete_persisted_login(plan).unwrap();
        *CRY_STATE.lock()=CryState::new(); unlock_persisted("t4ce",&key,&envelope).unwrap();
        crate::advance(30); account
    }
    fn code() -> String {
        let secret=CRY_STATE.lock().totp.as_ref().unwrap().secret;
        let value=trueos_crypto::generate_totp_sha1(&secret,crate::unix_seconds()/TOTP_PERIOD_SECONDS).unwrap();
        alloc::format!("{value:06}")
    }
    fn wrong_code() -> String {
        let secret=CRY_STATE.lock().totp.as_ref().unwrap().secret;
        for value in 0..1_000_000 {
            if trueos_crypto::verify_totp_sha1(&secret,crate::unix_seconds(),value,TOTP_SKEW_STEPS).unwrap().is_none() {
                return alloc::format!("{value:06}");
            }
        }
        panic!("no invalid code")
    }
    fn no_global_session() { for scope in [0,1,2,3] { assert_eq!(authenticated_access_account(scope),None); } }
    #[test]
    fn actual_remote_command_commits_exact_account_with_no_enrolled_ssh_keys() {
        let account=fresh(); assert!(ssh_authorized_keys().is_empty()); assert!(ssh_requires_account_proof("t4ce"));
        let code=code(); assert_eq!(crate::run(command::remote_login("t4ce",&code)),Ok(account));
        assert_eq!(command::calls(),1); no_global_session();
        let (key,envelope)=command::saved_image();
        *CRY_STATE.lock()=CryState::new(); unlock_persisted("t4ce",&key,&envelope).unwrap();
        assert!(crate::run(command::remote_login("t4ce",&code)).unwrap_err().contains("already consumed"));
        assert_eq!(command::calls(),1); no_global_session(); reset();
    }
    #[test]
    fn actual_remote_command_rejects_wrong_code_username_and_replay_before_storage() {
        fresh(); let right=code(); let bad=wrong_code();
        assert!(crate::run(command::remote_login("t4ce",&bad)).is_err());
        assert!(crate::run(command::remote_login("someone-else",&right)).is_err());
        assert_eq!(command::calls(),0); no_global_session();
        assert!(crate::run(command::remote_login("t4ce",&right)).is_ok());
        assert!(crate::run(command::remote_login("t4ce",&right)).is_err());
        assert_eq!(command::calls(),1); no_global_session(); reset();
    }
    #[test]
    fn actual_remote_command_storage_failure_aborts_pending_proof_and_cannot_authenticate() {
        fresh(); let old_generation=CRY_STATE.lock().durable.as_ref().unwrap().generation;
        command::fail_store(); assert!(crate::run(command::remote_login("t4ce",&code())).is_err());
        assert!(CRY_STATE.lock().pending_login.is_none());
        assert_eq!(CRY_STATE.lock().durable.as_ref().unwrap().generation,old_generation);
        assert!(!command::has_saved_image()); no_global_session();
        command::ready_store(); crate::advance(30);
        assert_eq!(crate::run(command::remote_login("t4ce",&code())),Ok(AccountId::new(712))); reset();
    }
    #[test]
    fn actual_remote_command_preserves_an_existing_local_scope_session() {
        fresh(); let local=prepare_login(&code(),3).unwrap(); let plan=prepare_persistence(local.challenge_sequence).unwrap();
        complete_persisted_login(plan).unwrap(); let before=CRY_STATE.lock().session.unwrap();
        crate::advance(30); assert_eq!(crate::run(command::remote_login("t4ce",&code())),Ok(AccountId::new(712)));
        let after=CRY_STATE.lock().session.unwrap(); assert_eq!((after.account,after.scope_id,after.challenge_sequence),(before.account,before.scope_id,before.challenge_sequence));
        assert_eq!(authenticated_access_account(3),Some(AccountId::new(712))); assert_eq!(authenticated_access_account(0),None); reset();
    }
    #[test]
    fn actual_service_auth_queue_verifies_one_request_and_denies_bad_proofs() {
        let account=fresh(); let good=ssh::enqueue("t4ce",&code(),60);
        assert_eq!(good.outcome(),None); crate::run(ssh::service_for_test()); assert_eq!(good.outcome(),Some(Ok(account))); no_global_session();
        crate::advance(30); let bad=ssh::enqueue("t4ce",&wrong_code(),60); let wrong_user=ssh::enqueue("other",&code(),60);
        crate::run(ssh::service_for_test()); assert_eq!(bad.outcome(),Some(Err(()))); assert_eq!(wrong_user.outcome(),None);
        crate::run(ssh::service_for_test()); assert_eq!(wrong_user.outcome(),Some(Err(()))); no_global_session(); reset();
    }
    #[test]
    fn actual_service_auth_queue_cancellation_deadline_and_storage_error_never_bind() {
        fresh(); let cancelled=ssh::enqueue("t4ce",&code(),60); cancelled.cancel(); crate::run(ssh::service_for_test());
        assert_eq!(cancelled.outcome(),Some(Err(()))); assert_eq!(command::calls(),0);
        let expired=ssh::enqueue("t4ce",&code(),0); crate::run(ssh::service_for_test()); assert_eq!(expired.outcome(),Some(Err(()))); assert_eq!(command::calls(),0);
        command::fail_store(); let failed=ssh::enqueue("t4ce",&code(),60); crate::run(ssh::service_for_test()); assert_eq!(failed.outcome(),Some(Err(()))); assert!(CRY_STATE.lock().pending_login.is_none()); no_global_session(); reset();
    }
    #[test]
    fn actual_service_auth_queue_rechecks_cancel_and_deadline_after_pending_persistence() {
        for cancel in [true,false] {
            fresh(); command::pause_store(); let ticket=ssh::enqueue("t4ce",&code(),60);
            let mut worker=std::pin::pin!(ssh::service_for_test()); assert!(crate::poll(worker.as_mut()).is_pending());
            assert_eq!(ticket.outcome(),None); assert!(CRY_STATE.lock().pending_login.is_some()); no_global_session();
            if cancel { ticket.cancel(); } else { crate::advance(61); }
            command::ready_store(); assert!(crate::poll(worker.as_mut()).is_ready());
            assert_eq!(ticket.outcome(),Some(Err(()))); no_global_session();
        }
        reset();
    }
}
