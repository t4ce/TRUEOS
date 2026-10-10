//! One immutable SSH admission policy for every AP in this boot.
use zeroize::Zeroizing;

pub(super) struct Policy {
    pub allow_none: bool,
    host_seed: Option<Zeroizing<[u8; 32]>>,
}

static POLICY: spin::Once<Policy> = spin::Once::new();

pub(super) fn allow_none() -> bool {
    POLICY.get().is_some_and(|policy| policy.allow_none)
}

pub(super) fn host_seed() -> Result<Zeroizing<[u8; 32]>, crate::crypt::CryError> {
    let policy = POLICY.get().ok_or(crate::crypt::CryError::NotConfigured)?;
    if let Some(seed) = &policy.host_seed {
        return Ok(Zeroizing::new(**seed));
    }
    crate::crypt::ssh_host_seed()
}

/// No listener may be opened until this succeeds. Missing storage is unknown,
/// not a factory machine. Read every currently mounted account store in full.
pub(super) async fn initialize() -> Result<(), &'static str> {
    if POLICY.get().is_some() { return Ok(()); }
    crate::r::readiness::wait_for(
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED
            | crate::r::readiness::TRUEOSFS_INDEX_READY,
    ).await;
    if !crate::r::fs::trueosfs::mounts_settled() { return Err("account stores still mounting"); }
    let disk = crate::r::fs::trueosfs::primary_root_handle().ok_or("missing account store")?;
    let roots = crate::r::fs::trueosfs::list_roots();
    if roots.is_empty() { return Err("missing account store"); }
    let mut persisted = false;
    for root in &roots {
        let handle = crate::disc::block::device_handle(root.disk_id).ok_or("account store disappeared")?;
        persisted |= crate::r::fs::trueosfs::has_persisted_account_async(handle)
            .await.map_err(|_| "account store read failed")?;
    }
    if !crate::r::fs::trueosfs::mounts_settled()
        || crate::r::fs::trueosfs::primary_root_handle() != Some(disk)
        || crate::r::fs::trueosfs::list_roots() != roots {
        return Err("account stores changed during probe");
    }
    let host_seed = if persisted {
        None
    } else {
        let mut seed = Zeroizing::new([0; 32]);
        if !crate::tyche::fill_bytes(seed.as_mut()) { return Err("host-key entropy unavailable"); }
        Some(seed)
    };
    POLICY.call_once(|| Policy { allow_none: !persisted, host_seed });
    crate::log_info!(target: "service";
        "shell3-ssh: boot-account-probe=complete auth={} policy=boot-latched\n",
        if persisted { "required" } else { "none" });
    Ok(())
}
