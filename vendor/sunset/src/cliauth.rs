use self::{conn::DispatchEvent, event::CliEventId};

#[allow(unused_imports)]
use {
    crate::error::{Error, Result, TrapBug},
    log::{debug, error, info, log, trace, warn},
};

use heapless::String;

use crate::{packets::UserauthPkOk, *};
use auth::AuthType;
use kex::SessId;
use packets::{AuthMethod, MethodPubKey, ParseContext};
use packets::{Packet, Userauth60};
use sign::{OwnedSig, SignKey};
use sshnames::*;
use traffic::TrafSend;

#[derive(Debug)]
enum AuthState {
    Unstarted,
    MethodQuery,
    // other request types
    Request,
    RequestKey { key: SignKey },
    Interactive { pending: Option<usize> },
    Idle,
}

#[derive(Debug)]
pub(crate) struct CliAuth {
    state: AuthState,

    username: String<{ config::MAX_USERNAME }>,

    /// Starts as true, set to false if hook.auth_password() returns None.
    /// Not set false if the server rejects auth.
    try_password: bool,

    /// Set to false if hook.next_authkey() returns None.
    try_pubkey: bool,
    try_interactive: bool,
    allow_pubkey: bool,
    allow_password: bool,
    allow_interactive: bool,
    last_method: Option<AuthType>,

    /// Set once we are OKed from MSG_EXT_INFO
    allow_rsa_sha2: bool,
}

impl Default for CliAuth {
    fn default() -> Self {
        CliAuth {
            state: AuthState::Unstarted,
            username: String::new(),
            try_password: true,
            try_pubkey: true,
            try_interactive: true,
            allow_pubkey: false,
            allow_password: false,
            allow_interactive: false,
            last_method: None,
            allow_rsa_sha2: false,
        }
    }
}

impl CliAuth {
    // May be called multiple times
    pub fn progress(&mut self) -> DispatchEvent {
        if let AuthState::Unstarted = self.state {
            self.state = AuthState::MethodQuery;
            DispatchEvent::CliEvent(event::CliEventId::Username)
            // continued in resume_username()
        } else {
            Default::default()
        }
    }

    pub fn resume_username(
        &mut self,
        s: &mut TrafSend,
        username: &str,
    ) -> Result<()> {
        // TODO; error handling
        self.username =
            username.try_into().map_err(|_| Error::msg("Username too long"))?;
        s.send(packets::ServiceRequest { name: SSH_SERVICE_USERAUTH })?;

        s.send(packets::UserauthRequest {
            username: self.username.as_str().into(),
            service: SSH_SERVICE_CONNECTION,
            method: packets::AuthMethod::None,
        })?;
        Ok(())
    }

    pub fn auth60(
        &mut self,
        auth60: &packets::Userauth60,
        sess_id: &SessId,
        parse_ctx: &mut ParseContext,
        s: &mut TrafSend,
    ) -> Result<DispatchEvent> {
        match auth60 {
            Userauth60::InfoRequest(request) => {
                let AuthState::Interactive { pending } = &mut self.state else {
                    return error::SSHProto.fail();
                };
                if pending.is_some() { return error::SSHProto.fail(); }
                *pending = Some(request.prompts.len());
                Ok(DispatchEvent::CliEvent(CliEventId::KeyboardInteractive))
            }
            Userauth60::PkOk(pkok) => self.auth_pkok(pkok, sess_id, parse_ctx, s),
            Userauth60::PwChangeReq(_req) => {
                self.change_password()?;
                parse_ctx.cli_auth_type = None;
                Ok(DispatchEvent::None)
            }
        }
    }

    fn auth_sig_msg<'b>(
        &'b self,
        key: &'b SignKey,
        sess_id: &'b SessId,
    ) -> Result<AuthSigMsg<'b>> {
        let p = req_packet_pubkey(&self.username, key, None, true)?;
        Ok(auth::AuthSigMsg::new(p, sess_id))
    }

    fn auth_pkok(
        &mut self,
        pkok: &UserauthPkOk,
        sess_id: &SessId,
        parse_ctx: &mut ParseContext,
        s: &mut TrafSend,
    ) -> Result<DispatchEvent> {
        let AuthState::RequestKey { key } = &self.state else {
            trace!("Unexpected userauth60");
            return error::SSHProto.fail();
        };

        if key.pubkey() != pkok.key.0 {
            trace!("Received pkok for a different key");
            return error::SSHProto.fail();
        }

        if key.is_agent() {
            return Ok(DispatchEvent::CliEvent(CliEventId::AgentSign));
        }

        // Sign the packet without the signature
        let msg = self.auth_sig_msg(key, sess_id)?;
        let sig = key.sign(&msg)?;
        let p = req_packet_pubkey(&self.username, key, Some(&sig), true)?;

        s.send(p)?;
        self.last_method = Some(AuthType::PubKey);
        parse_ctx.cli_auth_type = None;
        Ok(DispatchEvent::None)
    }

    pub fn resume_agentsign(
        &mut self,
        sig: Option<&OwnedSig>,
        parse_ctx: &mut ParseContext,
        s: &mut TrafSend,
    ) -> Result<DispatchEvent> {
        let AuthState::RequestKey { key } = &self.state else {
            return Err(Error::bug());
        };

        parse_ctx.cli_auth_type = None;

        let Some(sig) = sig else {
            // Request the next key
            return Ok(DispatchEvent::CliEvent(CliEventId::Pubkey));
        };

        let p = req_packet_pubkey(&self.username, key, Some(sig), true)?;
        s.send(p)?;
        self.last_method = Some(AuthType::PubKey);
        Ok(DispatchEvent::None)
    }

    fn change_password(&self) -> Result<()> {
        // Doesn't seem to be widely implemented, we'll just fail.
        Err(Error::msg("Password has expired"))
    }

    /// handles auth failure packet from the server (which also provides
    /// the initial list of auth methods)
    pub fn failure(
        &mut self,
        failure: &packets::UserauthFailure,
        parse_ctx: &mut ParseContext,
        s: &mut TrafSend,
    ) -> Result<DispatchEvent> {
        parse_ctx.cli_auth_type = None;
        if failure.partial {
            // A completed factor advances to the server's remaining methods;
            // it never produces Authenticated or reoffers that factor.
            match self.last_method {
                Some(AuthType::PubKey) => self.try_pubkey = false,
                Some(AuthType::Password) => self.try_password = false,
                Some(AuthType::KeyboardInteractive) => self.try_interactive = false,
                None => return error::SSHProto.fail(),
            }
        }
        self.state = AuthState::Idle;
        self.allow_pubkey = failure.methods.has_algo(SSH_AUTHMETHOD_PUBLICKEY)?;
        self.allow_password = failure.methods.has_algo(SSH_AUTHMETHOD_PASSWORD)?;
        self.allow_interactive = failure.methods.has_algo("keyboard-interactive")?;
        self.next_method(s, parse_ctx)
    }

    fn next_method(&mut self, s: &mut TrafSend, ctx: &mut ParseContext) -> Result<DispatchEvent> {
        if self.try_pubkey && self.allow_pubkey {
            return Ok(DispatchEvent::CliEvent(event::CliEventId::Pubkey));
        }
        if self.try_interactive && self.allow_interactive {
            self.try_interactive = false;
            self.last_method = Some(AuthType::KeyboardInteractive);
            self.state = AuthState::Interactive { pending: None };
            ctx.cli_auth_type = Some(AuthType::KeyboardInteractive);
            s.send(packets::UserauthRequest {
                username: self.username.as_str().into(),
                service: SSH_SERVICE_CONNECTION,
                method: AuthMethod::KeyboardInteractive(packets::MethodKeyboardInteractive {
                    language: "", submethods: "",
                }),
            })?;
            return Ok(DispatchEvent::None);
        }
        if self.try_password && self.allow_password {
            return Ok(DispatchEvent::CliEvent(event::CliEventId::Password));
        }

        error::NoAuthMethods.fail()
    }

    pub fn resume_password(
        &mut self,
        s: &mut TrafSend,
        password: Option<&str>,
        parse_ctx: &mut ParseContext,
    ) -> Result<DispatchEvent> {
        let Some(password) = password else {
            self.try_password = false;
            return self.next_method(s, parse_ctx);
        };

        let p = req_packet_password(&self.username, password);
        s.send(p)?;
        parse_ctx.cli_auth_type = Some(AuthType::Password);
        self.state = AuthState::Request;
        self.last_method = Some(AuthType::Password);
        Ok(DispatchEvent::None)
    }

    // May return another event to try, such as password auth
    pub fn resume_pubkey(
        &mut self,
        s: &mut TrafSend,
        key: Option<SignKey>,
        parse_ctx: &mut ParseContext,
    ) -> Result<DispatchEvent> {
        let Some(key) = key else {
            self.try_pubkey = false;
            return self.next_method(s, parse_ctx);
        };

        #[cfg(feature = "rsa")]
        if (matches!(key, SignKey::RSA(_)) || matches!(key, SignKey::AgentRSA(_)))
            && !self.allow_rsa_sha2
        {
            // RSA keys are only used when the server has confirmed that rsa-sha2
            // signatures are OK by sending ext-info.
            trace!("Skipping rsa key, no ext-info");
            // Ask for another public key
            return Ok(DispatchEvent::CliEvent(CliEventId::Pubkey));
        }

        let p = req_packet_pubkey(&self.username, &key, None, false)?;
        s.send(p)?;
        parse_ctx.cli_auth_type = Some(AuthType::PubKey);
        trace!("authtype {:?}", parse_ctx.cli_auth_type);
        self.state = AuthState::RequestKey { key };
        self.last_method = None; // An unsigned probe cannot complete a factor.
        Ok(DispatchEvent::None)
    }

    pub fn resume_interactive(&mut self, s: &mut TrafSend, answers: &[&str]) -> Result<()> {
        let AuthState::Interactive { pending } = &mut self.state else {
            return error::BadUsage.fail();
        };
        if *pending != Some(answers.len()) { return error::BadUsage.fail(); }
        let mut responses = packets::KbiList::new();
        for answer in answers {
            responses.push((*answer).into()).map_err(|_| error::BadUsage.build())?;
        }
        s.send(packets::UserauthInfoResponse { responses })?;
        *pending = None;
        Ok(())
    }

    pub fn fetch_agentsign_key(&self) -> Result<&SignKey> {
        let AuthState::RequestKey { key } = &self.state else {
            return Err(Error::bug());
        };
        debug_assert!(key.is_agent());
        Ok(key)
    }

    pub fn fetch_agentsign_msg<'b>(
        &'b self,
        sess_id: &'b SessId,
    ) -> Result<AuthSigMsg<'b>> {
        let AuthState::RequestKey { key } = &self.state else {
            return Err(Error::bug());
        };

        // Sign the packet without the signature
        self.auth_sig_msg(key, sess_id)
    }

    pub fn success(&mut self) -> DispatchEvent {
        // TODO: check current state? Probably just informational
        self.state = AuthState::Idle;
        DispatchEvent::CliEvent(CliEventId::Authenticated)
    }

    pub fn handle_ext_info(&mut self, p: &packets::ExtInfo) {
        if let Some(ref algs) = p.server_sig_algs {
            // we only worry about rsa-sha256, assuming other older key types are fine

            // OK unwrap: is a remote namelist
            self.allow_rsa_sha2 = algs.has_algo(SSH_NAME_RSA_SHA256).unwrap();
            trace!("setting allow_rsa_sha2 = {}", self.allow_rsa_sha2);
        }
    }
}

fn req_packet_password<'b>(username: &'b str, password: &'b str) -> Packet<'b> {
    packets::UserauthRequest {
        username: username.into(),
        service: SSH_SERVICE_CONNECTION,
        method: packets::AuthMethod::Password(packets::MethodPassword {
            change: false,
            password: password.into(),
        }),
    }
    .into()
}

fn req_packet_pubkey<'b>(
    username: &'b str,
    key: &'b SignKey,
    sig: Option<&'b OwnedSig>,
    force_sig: bool,
) -> Result<packets::UserauthRequest<'b>> {
    let mut mp = MethodPubKey::new(key.pubkey(), sig)?;
    mp.force_sig = force_sig;
    let method = AuthMethod::PubKey(mp);
    Ok(packets::UserauthRequest {
        username: username.into(),
        service: SSH_SERVICE_CONNECTION,
        method,
    })
}

#[cfg(test)]
mod native_client_tests {
    use super::*;
    use crate::encrypt::KeyState;
    use crate::packets::{KbiList, KeyboardPrompt, UserauthInfoRequest};
    use crate::traffic::TrafOut;

    fn encrypted_keys() -> KeyState {
        use crate::encrypt::{Cipher, Integ, KeysSend};
        let session = SessId::from_slice(&[7; 32]).unwrap();
        let output = kex::KexOutput::new_test(b"unit-test-only shared secret", &session);
        let algorithms = kex::Algos::<Client> {
            kex: kex::SharedSecret::from_name(SSH_NAME_CURVE25519).unwrap(),
            hostsig: sign::SigType::Ed25519,
            cipher_enc: Cipher::ChaPoly, cipher_dec: Cipher::ChaPoly,
            integ_enc: Integ::ChaPoly, integ_dec: Integ::ChaPoly,
            discard_next: false, send_ext_info: false, strict_kex: false,
            _cs: core::marker::PhantomData,
        };
        let mut keys = KeyState::new_cleartext();
        keys.rekey_send(KeysSend::new(&output, &session, &algorithms));
        keys
    }

    fn challenge(count: usize) -> UserauthInfoRequest<'static> {
        let mut prompts = KbiList::new();
        for i in 0..count {
            prompts.push(KeyboardPrompt { prompt: "Prompt: ".into(), echo: i % 2 == 0 }).unwrap();
        }
        UserauthInfoRequest { name: "MFA".into(), instruction: "Two rounds".into(), language: "".into(), prompts }
    }

    #[test]
    fn interactive_zero_multiple_and_repeated_rounds_require_exact_answers() {
        let mut auth = CliAuth::default();
        let mut bytes = [0u8; 8192];
        let mut output = TrafOut::new(&mut bytes);
        let mut keys = encrypted_keys();
        let mut send = output.sender(&mut keys);
        let mut ctx = ParseContext::new();
        let session = SessId::from_slice(&[3; 32]).unwrap();
        let failure = packets::UserauthFailure { methods: "keyboard-interactive".try_into().unwrap(), partial: false };
        assert!(matches!(auth.failure(&failure, &mut ctx, &mut send).unwrap(), DispatchEvent::None));
        assert_eq!(ctx.cli_auth_type, Some(AuthType::KeyboardInteractive));
        for count in [0, 2, 1] {
            let request = Userauth60::InfoRequest(challenge(count));
            assert!(matches!(auth.auth60(&request, &session, &mut ctx, &mut send).unwrap(), DispatchEvent::CliEvent(CliEventId::KeyboardInteractive)));
            let answers = vec!["answer"; count];
            assert!(auth.resume_interactive(&mut send, &["wrong count"; 3]).is_err());
            auth.resume_interactive(&mut send, &answers).unwrap();
            assert!(auth.resume_interactive(&mut send, &answers).is_err());
        }
        // Answering a challenge is still awaiting a server decision.
        assert!(matches!(auth.state, AuthState::Interactive { pending: None }));
        assert!(matches!(auth.success(), DispatchEvent::CliEvent(CliEventId::Authenticated)));
    }

    #[test]
    fn signed_key_partial_success_advances_without_reoffering_key() {
        let mut auth = CliAuth::default();
        let mut bytes = [0u8; 8192];
        let mut output = TrafOut::new(&mut bytes);
        let mut keys = encrypted_keys();
        let mut send = output.sender(&mut keys);
        let mut ctx = ParseContext::new();
        let session = SessId::from_slice(&[3; 32]).unwrap();
        let key = SignKey::generate(KeyType::Ed25519, None).unwrap();
        let public = key.pubkey();
        let failure = packets::UserauthFailure { methods: "publickey,keyboard-interactive,password".try_into().unwrap(), partial: false };
        assert!(matches!(auth.failure(&failure, &mut ctx, &mut send).unwrap(), DispatchEvent::CliEvent(CliEventId::Pubkey)));
        let _ = auth.resume_pubkey(&mut send, Some(key.clone()), &mut ctx).unwrap();
        let ok = Userauth60::PkOk(UserauthPkOk { algo: SSH_NAME_ED25519, key: sshwire::Blob(public) });
        let _ = auth.auth60(&ok, &session, &mut ctx, &mut send).unwrap();
        let partial = packets::UserauthFailure { methods: "publickey,keyboard-interactive".try_into().unwrap(), partial: true };
        assert!(matches!(auth.failure(&partial, &mut ctx, &mut send).unwrap(), DispatchEvent::None));
        assert!(!auth.try_pubkey);
        assert!(matches!(auth.state, AuthState::Interactive { .. }));
        // A failed second factor cannot fall back to an unadvertised password.
        let failed = packets::UserauthFailure { methods: "keyboard-interactive".try_into().unwrap(), partial: false };
        assert!(auth.failure(&failed, &mut ctx, &mut send).is_err());
    }

    #[test]
    fn unsigned_key_cannot_be_a_completed_factor() {
        let mut auth = CliAuth::default();
        let mut bytes = [0u8; 8192];
        let mut output = TrafOut::new(&mut bytes);
        let mut keys = encrypted_keys();
        let mut send = output.sender(&mut keys);
        let mut ctx = ParseContext::new();
        let _ = auth.resume_pubkey(&mut send, Some(SignKey::generate(KeyType::Ed25519, None).unwrap()), &mut ctx).unwrap();
        let partial = packets::UserauthFailure { methods: "password".try_into().unwrap(), partial: true };
        assert!(auth.failure(&partial, &mut ctx, &mut send).is_err());
    }

    #[test]
    fn exhausted_keys_follow_advertised_methods_only() {
        let mut auth = CliAuth::default();
        let mut bytes = [0u8; 8192];
        let mut output = TrafOut::new(&mut bytes);
        let mut keys = encrypted_keys();
        let mut send = output.sender(&mut keys);
        let mut ctx = ParseContext::new();
        let failure = packets::UserauthFailure { methods: "publickey".try_into().unwrap(), partial: false };
        let _ = auth.failure(&failure, &mut ctx, &mut send).unwrap();
        assert!(auth.resume_pubkey(&mut send, None, &mut ctx).is_err());
    }

    #[test]
    fn rejected_interactive_can_fall_back_to_advertised_password() {
        let mut auth = CliAuth::default();
        let mut bytes = [0u8; 8192];
        let mut output = TrafOut::new(&mut bytes);
        let mut keys = encrypted_keys();
        let mut send = output.sender(&mut keys);
        let mut ctx = ParseContext::new();
        let failure = packets::UserauthFailure { methods: "keyboard-interactive,password".try_into().unwrap(), partial: false };
        let _ = auth.failure(&failure, &mut ctx, &mut send).unwrap();
        assert!(matches!(auth.failure(&failure, &mut ctx, &mut send).unwrap(), DispatchEvent::CliEvent(CliEventId::Password)));
        let _ = auth.resume_password(&mut send, Some("secret"), &mut ctx).unwrap();
        assert_eq!(ctx.cli_auth_type, Some(AuthType::Password));
    }

    #[test]
    fn interactive_wire_roundtrip_preserves_metadata_and_hides_answers() {
        for count in [0, 2, 32, 64] {
            let request = packets::Packet::Userauth60(Userauth60::InfoRequest(challenge(count)));
            let mut bytes = [0u8; 8192];
            let len = sshwire::write_ssh(&mut bytes, &request).unwrap();
            let ctx = ParseContext { cli_auth_type: Some(AuthType::KeyboardInteractive), ..ParseContext::new() };
            let packets::Packet::Userauth60(Userauth60::InfoRequest(decoded)) = sshwire::packet_from_bytes(&bytes[..len], &ctx).unwrap() else { panic!() };
            assert_eq!(decoded.prompts.len(), count);
            assert_eq!(decoded.instruction.to_str().unwrap(), "Two rounds");
            for (i, prompt) in decoded.prompts.iter().enumerate() { assert_eq!(prompt.echo, i % 2 == 0); }
        }
        let mut responses = KbiList::new();
        responses.push("never print this secret".into()).unwrap();
        let response = packets::UserauthInfoResponse { responses };
        assert!(!format!("{response:?}").contains("never print"));
    }

    #[test]
    fn interactive_wire_rejects_forged_counts_and_trailing_bytes() {
        let mut bytes = vec![61, 0xff, 0xff, 0xff, 0xff];
        assert!(sshwire::packet_from_bytes(&bytes, &ParseContext::new()).is_err());
        bytes = vec![61, 0, 0, 0, 0, 99];
        assert!(sshwire::packet_from_bytes(&bytes, &ParseContext::new()).is_err());
    }
}
