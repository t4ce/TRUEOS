//! Real encrypted protocol regressions for the existing incoming policy.
use crate::*;
use std::collections::VecDeque;

fn transfer<A: CliServ, B: CliServ>(
    source: &mut Runner<'_, A>,
    destination: &mut Runner<'_, B>,
    queued: &mut VecDeque<u8>,
) {
    // Exercise fragmentation and input backpressure: bytes not accepted by the
    // destination remain owned, and output is never consumed twice.
    // Like the native RX queue, this must hold a complete peer KEX packet:
    // Sunset drains its local KEX output before accepting peer packets.
    if queued.len() < 32768 {
        let mut bytes = [0u8; 7];
        let len = source.output(&mut bytes);
        queued.extend(&bytes[..len]);
    }
    let bytes = queued.make_contiguous();
    if !bytes.is_empty() {
        let len = destination.input(&bytes[..bytes.len().min(3)]).unwrap();
        queued.drain(..len);
    }
}

fn incoming_auth(interactive: bool, allow: bool) {
    let host = SignKey::generate(KeyType::Ed25519, None).unwrap();
    let identity = SignKey::generate(KeyType::Ed25519, None).unwrap();
    let mut client = Runner::new_client_owned();
    let mut server = Runner::new_server_owned();
    server.set_auth_methods(false, !interactive).unwrap();
    if interactive {
        server.enable_keyboard_interactive().unwrap();
    }
    let (mut to_server, mut to_client) = (VecDeque::new(), VecDeque::new());
    let (mut deferred, mut defer_ticks) = (false, 0);
    let (mut challenge, mut client_authed, mut server_authed) =
        (false, false, false);
    let (mut offered, mut rejected) = (false, false);
    let (mut key_probes, mut verified_keys) = (0, 0);
    assert!(!client.initial_kex_complete());
    for _ in 0..30000 {
        if deferred {
            defer_ticks += 1;
            assert!(!client_authed, "deferred policy must not authenticate early");
            if defer_ticks == 7 {
                server.finish_deferred_auth(allow).unwrap();
                deferred = false;
            }
        } else {
            match server.progress().unwrap() {
                Event::Serv(ServEvent::Hostkeys(event)) => {
                    event.hostkeys(&[&host]).unwrap()
                }
                Event::Serv(ServEvent::FirstAuth(mut event)) => {
                    event.set_auth_methods(false, !interactive).unwrap();
                    event.reject().unwrap();
                }
                Event::Serv(ServEvent::PubkeyAuth(event)) => {
                    assert_eq!(event.pubkey().unwrap(), identity.pubkey());
                    if event.signature_verified() { verified_keys += 1; } else { key_probes += 1; }
                    assert!(!server_authed, "a probe alone must not authenticate");
                    if allow {
                        event.allow().unwrap();
                    } else {
                        event.reject().unwrap();
                    }
                }
                Event::Serv(ServEvent::PasswordAuth(event)) => {
                    assert!(interactive);
                    assert_eq!(event.username().unwrap(), "incoming-user");
                    assert_eq!(event.password().unwrap(), "123456");
                    event.defer();
                    deferred = true;
                }
                Event::Serv(ServEvent::Authenticated) => server_authed = true,
                Event::None | Event::Progressed => {}
                other => panic!("Unexpected incoming event {other:?}"),
            }
        }
        match client.progress() {
            Ok(Event::Cli(event)) => match event {
                CliEvent::Hostkey(event) => {
                    assert_eq!(event.hostkey().unwrap(), host.pubkey());
                    event.accept().unwrap();
                }
                CliEvent::Username(event) => {
                    event.username("incoming-user").unwrap()
                }
                CliEvent::Pubkey(event) => {
                    if offered {
                        rejected = event.skip().is_err();
                    } else {
                        offered = true;
                        event.pubkey(identity.clone()).unwrap();
                    }
                }
                CliEvent::KeyboardInteractive(event) => {
                    let info = event.info().unwrap();
                    assert_eq!(info.prompts.len(), 1);
                    assert_eq!(info.name.to_str().unwrap(), "TrueOS");
                    assert_eq!(
                        info.instruction.to_str().unwrap(),
                        "Enter your cry authenticator code."
                    );
                    assert!(!info.prompts[0].echo);
                    drop(info);
                    challenge = true;
                    event.respond(&["123456"]).unwrap();
                }
                CliEvent::Authenticated => client_authed = true,
                other => panic!("Unexpected client event {other:?}"),
            },
            Ok(Event::None | Event::Progressed) => {}
            Err(Error::NoAuthMethods { .. }) if !allow => {
                rejected = true;
            }
            other => panic!("Unexpected client result {other:?}"),
        }
        transfer(&mut client, &mut server, &mut to_server);
        transfer(&mut server, &mut client, &mut to_client);
        if allow && client_authed && server_authed {
            assert!(client.initial_kex_complete());
            assert_eq!(challenge, interactive);
            if !interactive { assert_eq!(key_probes, 1); assert_eq!(verified_keys, 1); }
            return;
        }
        if !allow && rejected {
            assert!(!client_authed && !server_authed);
            assert_eq!(challenge, interactive);
            return;
        }
    }
    panic!("Fragmented incoming authentication did not finish");
}

#[test]
fn incoming_ed25519_still_authenticates() {
    incoming_auth(false, true);
}
#[test]
fn incoming_ed25519_rejects_unapproved_identity() {
    incoming_auth(false, false);
}
#[test]
fn incoming_cry_challenge_stays_pending_until_durable_policy_allows() {
    incoming_auth(true, true);
}
#[test]
fn incoming_cry_challenge_rejection_never_authenticates() {
    incoming_auth(true, false);
}
