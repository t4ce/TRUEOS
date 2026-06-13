//! Validation pass for the AOT-first OpenCL registry.
//!
//! This pass deliberately separates metadata validation from upload execution.
//! The status check only reads the existing GPGPU upload records; it does not
//! trigger a kernel upload by itself.

use super::{UploadedKernelRef, registry::KNOWN_AOT_KERNELS};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum KnownAotValidationIssueKind {
    EmptyRegistry,
    DuplicateKernelName,
    EmptyKernelName,
    EmptyTarget,
    ArtifactNameMismatch,
    ArtifactTargetMismatch,
    ContractNameMismatch,
    ContractTargetMismatch,
    EmptyContractSource,
    EmptyContractArgs,
    EmptyContractBindingSet,
    EmptyContractPayload,
    EmptyContractSimd,
    EmptyBinary,
    EmptySha256,
    UploadNameMismatch,
    UploadTargetMismatch,
    UploadByteMismatch,
    UploadMappingTooSmall,
    UploadMissingAddress,
    UploadNotReady,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct KnownAotValidationIssue {
    pub(crate) index: usize,
    pub(crate) name: &'static str,
    pub(crate) kind: KnownAotValidationIssueKind,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct KnownAotValidationReport {
    pub(crate) registry_kernels: usize,
    pub(crate) metadata_checked: usize,
    pub(crate) status_checked: usize,
    pub(crate) ready_uploads: usize,
    pub(crate) missing_uploads: usize,
    pub(crate) issues: usize,
    pub(crate) first_issue: Option<KnownAotValidationIssue>,
}

impl KnownAotValidationReport {
    pub(crate) const fn passed(self) -> bool {
        self.registry_kernels != 0 && self.issues == 0
    }

    pub(crate) const fn all_uploads_ready(self) -> bool {
        self.passed() && self.ready_uploads == self.registry_kernels
    }

    fn record_issue(
        &mut self,
        index: usize,
        name: &'static str,
        kind: KnownAotValidationIssueKind,
    ) {
        self.issues = self.issues.saturating_add(1);
        if self.first_issue.is_none() {
            self.first_issue = Some(KnownAotValidationIssue { index, name, kind });
        }
    }
}

pub(crate) fn validate_known_aot_registry() -> KnownAotValidationReport {
    let mut report = KnownAotValidationReport {
        registry_kernels: KNOWN_AOT_KERNELS.len(),
        ..KnownAotValidationReport::default()
    };

    if KNOWN_AOT_KERNELS.is_empty() {
        report.record_issue(0, "", KnownAotValidationIssueKind::EmptyRegistry);
        return report;
    }

    for (index, kernel) in KNOWN_AOT_KERNELS.iter().enumerate() {
        report.metadata_checked = report.metadata_checked.saturating_add(1);

        if kernel.name.is_empty() {
            report.record_issue(index, kernel.name, KnownAotValidationIssueKind::EmptyKernelName);
        }
        if kernel.artifact.target.is_empty() {
            report.record_issue(index, kernel.name, KnownAotValidationIssueKind::EmptyTarget);
        }
        if kernel.artifact.name != kernel.name {
            report.record_issue(
                index,
                kernel.name,
                KnownAotValidationIssueKind::ArtifactNameMismatch,
            );
        }
        if kernel.artifact.target != "adls" {
            report.record_issue(
                index,
                kernel.name,
                KnownAotValidationIssueKind::ArtifactTargetMismatch,
            );
        }
        if kernel.contract.name != kernel.name {
            report.record_issue(
                index,
                kernel.name,
                KnownAotValidationIssueKind::ContractNameMismatch,
            );
        }
        if kernel.contract.target != kernel.artifact.target {
            report.record_issue(
                index,
                kernel.name,
                KnownAotValidationIssueKind::ContractTargetMismatch,
            );
        }
        if kernel.contract.source_path.is_empty() {
            report.record_issue(
                index,
                kernel.name,
                KnownAotValidationIssueKind::EmptyContractSource,
            );
        }
        if kernel.contract.args.is_empty() {
            report.record_issue(index, kernel.name, KnownAotValidationIssueKind::EmptyContractArgs);
        }
        if kernel.contract.binding_count == 0 {
            report.record_issue(
                index,
                kernel.name,
                KnownAotValidationIssueKind::EmptyContractBindingSet,
            );
        }
        if kernel.contract.indirect_bytes() == 0 {
            report.record_issue(
                index,
                kernel.name,
                KnownAotValidationIssueKind::EmptyContractPayload,
            );
        }
        if kernel.contract.launch.simd_width == 0 {
            report.record_issue(index, kernel.name, KnownAotValidationIssueKind::EmptyContractSimd);
        }
        if kernel.artifact.bin.is_empty() {
            report.record_issue(index, kernel.name, KnownAotValidationIssueKind::EmptyBinary);
        }
        if kernel.artifact.bin_sha256 == [0; 32] {
            report.record_issue(index, kernel.name, KnownAotValidationIssueKind::EmptySha256);
        }

        for prior in KNOWN_AOT_KERNELS[..index].iter() {
            if prior.name == kernel.name {
                report.record_issue(
                    index,
                    kernel.name,
                    KnownAotValidationIssueKind::DuplicateKernelName,
                );
                break;
            }
        }
    }

    report
}

pub(crate) fn validate_known_aot_status() -> KnownAotValidationReport {
    let mut report = validate_known_aot_registry();

    for (index, kernel) in KNOWN_AOT_KERNELS.iter().enumerate() {
        let Some(upload) = kernel.status().map(UploadedKernelRef::from) else {
            report.missing_uploads = report.missing_uploads.saturating_add(1);
            continue;
        };

        report.status_checked = report.status_checked.saturating_add(1);
        if upload.is_ready() {
            report.ready_uploads = report.ready_uploads.saturating_add(1);
        } else {
            report.record_issue(index, kernel.name, KnownAotValidationIssueKind::UploadNotReady);
        }

        if upload.name != kernel.name {
            report.record_issue(
                index,
                kernel.name,
                KnownAotValidationIssueKind::UploadNameMismatch,
            );
        }
        if upload.target != kernel.artifact.target {
            report.record_issue(
                index,
                kernel.name,
                KnownAotValidationIssueKind::UploadTargetMismatch,
            );
        }
        if upload.bytes != kernel.artifact.bin.len() {
            report.record_issue(
                index,
                kernel.name,
                KnownAotValidationIssueKind::UploadByteMismatch,
            );
        }
        if upload.mapped_bytes < upload.bytes {
            report.record_issue(
                index,
                kernel.name,
                KnownAotValidationIssueKind::UploadMappingTooSmall,
            );
        }
        if upload.gpu == 0 || upload.phys == 0 {
            report.record_issue(
                index,
                kernel.name,
                KnownAotValidationIssueKind::UploadMissingAddress,
            );
        }
    }

    report
}
