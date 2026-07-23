// Build-generated Intel GPU kernel contract schema.
//
// The offline bakery emits values of these types.  Keep this file free of
// allocation and host-only dependencies: the same generated contract is part
// of the kernel image and is checked before any artifact reaches GGTT.

pub(crate) const GPGPU_KERNEL_ABI_SCHEMA_VERSION: u16 = 1;

pub(crate) const GPGPU_ADLS_4680_PCI_DEVICE_IDS: &[u16] = &[0x4680];

pub(crate) const GPGPU_ADLS_4680_TARGET: GpgpuKernelTarget = GpgpuKernelTarget {
    label: "adls",
    pci_device_ids: GPGPU_ADLS_4680_PCI_DEVICE_IDS,
    revision_min: 0,
    revision_max: u8::MAX,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuKernelTarget {
    pub(crate) label: &'static str,
    pub(crate) pci_device_ids: &'static [u16],
    pub(crate) revision_min: u8,
    pub(crate) revision_max: u8,
}

impl GpgpuKernelTarget {
    pub(crate) const fn supports_device_id(self, device_id: u16) -> bool {
        let mut index = 0;
        while index < self.pci_device_ids.len() {
            if self.pci_device_ids[index] == device_id {
                return true;
            }
            index += 1;
        }
        false
    }

    pub(crate) const fn supports_revision(self, revision_id: u8) -> bool {
        revision_id >= self.revision_min && revision_id <= self.revision_max
    }

    pub(crate) const fn supports(self, device_id: u16, revision_id: u8) -> bool {
        self.supports_device_id(device_id) && self.supports_revision(revision_id)
    }

    pub(crate) const fn validate(self) -> Result<(), GpgpuKernelTargetError> {
        if self.label.is_empty() {
            return Err(GpgpuKernelTargetError::EmptyLabel);
        }
        if self.pci_device_ids.is_empty() {
            return Err(GpgpuKernelTargetError::EmptyPciDeviceSet);
        }
        if self.revision_min > self.revision_max {
            return Err(GpgpuKernelTargetError::InvalidRevisionRange);
        }

        let mut index = 0;
        while index < self.pci_device_ids.len() {
            let mut prior = 0;
            while prior < index {
                if self.pci_device_ids[prior] == self.pci_device_ids[index] {
                    return Err(GpgpuKernelTargetError::DuplicatePciDevice);
                }
                prior += 1;
            }
            index += 1;
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuKernelTargetError {
    EmptyLabel,
    EmptyPciDeviceSet,
    InvalidRevisionRange,
    DuplicatePciDevice,
}

impl GpgpuKernelTargetError {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::EmptyLabel => "target-empty-label",
            Self::EmptyPciDeviceSet => "target-empty-pci-device-set",
            Self::InvalidRevisionRange => "target-invalid-revision-range",
            Self::DuplicatePciDevice => "target-duplicate-pci-device",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuArtifactArgKind {
    ByPointer,
    ByValue,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuArtifactArgAccess {
    None,
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuArtifactAddressMode {
    None,
    Stateful,
    Stateless,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuArtifactBinding {
    pub(crate) arg_index: u16,
    pub(crate) bti: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuArtifactPayloadArg {
    pub(crate) arg_index: u16,
    pub(crate) kind: GpgpuArtifactArgKind,
    pub(crate) access: GpgpuArtifactArgAccess,
    pub(crate) address_mode: GpgpuArtifactAddressMode,
    pub(crate) offset_bytes: u32,
    pub(crate) size_bytes: u32,
}

/// ABI facts extracted from Zebin ELF and `.ze_info` by the offline bakery.
///
/// The hashes bind these facts to immutable compiler outputs.  Runtime ELF
/// checks additionally prove that the declared text section still occupies the
/// generated offset and size; RCS encoders can therefore consume this contract
/// instead of repeating unverified numeric offsets.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuKernelAbiContract {
    pub(crate) schema_version: u16,
    pub(crate) kernel_name: &'static str,
    pub(crate) target: GpgpuKernelTarget,
    pub(crate) ze_info_major: u16,
    pub(crate) ze_info_minor: u16,
    pub(crate) zebin_sha256: [u8; 32],
    pub(crate) spv_sha256: [u8; 32],
    pub(crate) text_section_name: &'static str,
    pub(crate) text_section_offset: u64,
    pub(crate) text_section_size: u64,
    pub(crate) entry_offset: u64,
    pub(crate) entry_size: u64,
    pub(crate) simd_width: u8,
    pub(crate) grf_count: u16,
    pub(crate) scratch_bytes: u32,
    pub(crate) slm_bytes: u32,
    pub(crate) cross_thread_data_bytes: u32,
    pub(crate) per_thread_data_bytes: u32,
    pub(crate) bindings: &'static [GpgpuArtifactBinding],
    pub(crate) payload_args: &'static [GpgpuArtifactPayloadArg],
}

impl GpgpuKernelAbiContract {
    /// Validate both schema integrity and the subset supported by TRUEOS's
    /// current direct RCS/GuC encoder.
    pub(crate) const fn validate(self) -> Result<(), GpgpuKernelAbiContractError> {
        if self.schema_version != GPGPU_KERNEL_ABI_SCHEMA_VERSION {
            return Err(GpgpuKernelAbiContractError::UnsupportedSchemaVersion);
        }
        if self.kernel_name.is_empty() {
            return Err(GpgpuKernelAbiContractError::EmptyKernelName);
        }
        if let Err(error) = self.target.validate() {
            return Err(GpgpuKernelAbiContractError::InvalidTarget(error));
        }
        if digest_is_zero(&self.zebin_sha256) {
            return Err(GpgpuKernelAbiContractError::EmptyZebinHash);
        }
        if digest_is_zero(&self.spv_sha256) {
            return Err(GpgpuKernelAbiContractError::EmptySpirvHash);
        }
        // The parser/generator currently understands IGC's `.ze_info` 1.x
        // schema.  A major bump must be reviewed rather than silently admitted.
        if self.ze_info_major != 1 {
            return Err(GpgpuKernelAbiContractError::UnsupportedZeInfoVersion);
        }
        if !text_section_matches_kernel(self.text_section_name, self.kernel_name) {
            return Err(GpgpuKernelAbiContractError::InvalidTextSectionName);
        }
        let Some(text_section_end) = self.text_section_offset.checked_add(self.text_section_size)
        else {
            return Err(GpgpuKernelAbiContractError::InvalidTextRange);
        };
        let Some(entry_end) = self.entry_offset.checked_add(self.entry_size) else {
            return Err(GpgpuKernelAbiContractError::InvalidTextRange);
        };
        if self.text_section_offset < 64
            || self.text_section_offset & 63 != 0
            || self.text_section_size == 0
            || self.entry_offset < self.text_section_offset
            || self.entry_offset & 63 != 0
            || self.entry_size == 0
            || entry_end > text_section_end
        {
            return Err(GpgpuKernelAbiContractError::InvalidTextRange);
        }
        // Every checked-in direct-RCS kernel currently relies on the SIMD16
        // local-ID payload programmed by the encoder.  SIMD8/32 require an
        // explicit encoder capability addition before admission.
        if self.simd_width != 16 {
            return Err(GpgpuKernelAbiContractError::UnsupportedSimdWidth);
        }
        if self.grf_count == 0 || self.grf_count > 256 || self.grf_count % 32 != 0 {
            return Err(GpgpuKernelAbiContractError::InvalidGrfCount);
        }
        // Scratch and SLM programming are intentionally not inferred.  The
        // present RCS path does not allocate either resource.
        if self.scratch_bytes != 0 {
            return Err(GpgpuKernelAbiContractError::UnsupportedScratch);
        }
        if self.slm_bytes != 0 {
            return Err(GpgpuKernelAbiContractError::UnsupportedSlm);
        }
        if self.cross_thread_data_bytes == 0
            || self.cross_thread_data_bytes > 4096
            || self.cross_thread_data_bytes % 32 != 0
        {
            return Err(GpgpuKernelAbiContractError::InvalidCrossThreadData);
        }
        // SIMD16 local IDs occupy three 32-byte GRFs in current payload
        // assembly.  Any other value needs matching encoder work.
        if self.per_thread_data_bytes != 96 {
            return Err(GpgpuKernelAbiContractError::UnsupportedPerThreadData);
        }

        let mut index = 0;
        while index < self.payload_args.len() {
            let arg = self.payload_args[index];
            if arg.size_bytes == 0 {
                return Err(GpgpuKernelAbiContractError::InvalidPayloadArg);
            }
            if matches!(arg.kind, GpgpuArtifactArgKind::ByPointer) && arg.size_bytes != 8 {
                return Err(GpgpuKernelAbiContractError::InvalidPointerSize);
            }
            match arg.kind {
                GpgpuArtifactArgKind::ByPointer => {
                    if matches!(arg.access, GpgpuArtifactArgAccess::None)
                        || matches!(arg.address_mode, GpgpuArtifactAddressMode::None)
                    {
                        return Err(GpgpuKernelAbiContractError::MissingPointerQualifier);
                    }
                }
                GpgpuArtifactArgKind::ByValue => {
                    if !matches!(arg.access, GpgpuArtifactArgAccess::None)
                        || !matches!(arg.address_mode, GpgpuArtifactAddressMode::None)
                    {
                        return Err(GpgpuKernelAbiContractError::InvalidValueQualifier);
                    }
                }
            }
            let Some(end) = arg.offset_bytes.checked_add(arg.size_bytes) else {
                return Err(GpgpuKernelAbiContractError::InvalidPayloadArg);
            };
            if end > self.cross_thread_data_bytes {
                return Err(GpgpuKernelAbiContractError::PayloadArgOutOfBounds);
            }

            let mut prior = 0;
            while prior < index {
                let other = self.payload_args[prior];
                if other.arg_index == arg.arg_index {
                    return Err(GpgpuKernelAbiContractError::DuplicatePayloadArg);
                }
                let other_end = other.offset_bytes + other.size_bytes;
                if arg.offset_bytes < other_end && other.offset_bytes < end {
                    return Err(GpgpuKernelAbiContractError::OverlappingPayloadArgs);
                }
                prior += 1;
            }
            index += 1;
        }

        index = 0;
        while index < self.bindings.len() {
            let binding = self.bindings[index];
            let mut pointer_payload_found = false;
            let mut arg_index = 0;
            while arg_index < self.payload_args.len() {
                let arg = self.payload_args[arg_index];
                if arg.arg_index == binding.arg_index
                    && matches!(arg.kind, GpgpuArtifactArgKind::ByPointer)
                {
                    pointer_payload_found = true;
                    break;
                }
                arg_index += 1;
            }
            if !pointer_payload_found {
                return Err(GpgpuKernelAbiContractError::BindingWithoutPointerPayload);
            }

            let mut prior = 0;
            while prior < index {
                let other = self.bindings[prior];
                if other.arg_index == binding.arg_index {
                    return Err(GpgpuKernelAbiContractError::DuplicateBindingArg);
                }
                if other.bti == binding.bti {
                    return Err(GpgpuKernelAbiContractError::DuplicateBindingTableIndex);
                }
                prior += 1;
            }
            index += 1;
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuKernelAbiContractError {
    UnsupportedSchemaVersion,
    EmptyKernelName,
    InvalidTarget(GpgpuKernelTargetError),
    EmptyZebinHash,
    EmptySpirvHash,
    UnsupportedZeInfoVersion,
    InvalidTextSectionName,
    InvalidTextRange,
    UnsupportedSimdWidth,
    InvalidGrfCount,
    UnsupportedScratch,
    UnsupportedSlm,
    InvalidCrossThreadData,
    UnsupportedPerThreadData,
    InvalidPayloadArg,
    InvalidPointerSize,
    MissingPointerQualifier,
    InvalidValueQualifier,
    PayloadArgOutOfBounds,
    DuplicatePayloadArg,
    OverlappingPayloadArgs,
    BindingWithoutPointerPayload,
    DuplicateBindingArg,
    DuplicateBindingTableIndex,
}

impl GpgpuKernelAbiContractError {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::UnsupportedSchemaVersion => "contract-unsupported-schema-version",
            Self::EmptyKernelName => "contract-empty-kernel-name",
            Self::InvalidTarget(error) => error.label(),
            Self::EmptyZebinHash => "contract-empty-zebin-hash",
            Self::EmptySpirvHash => "contract-empty-spirv-hash",
            Self::UnsupportedZeInfoVersion => "contract-unsupported-ze-info-version",
            Self::InvalidTextSectionName => "contract-invalid-text-section-name",
            Self::InvalidTextRange => "contract-invalid-text-range",
            Self::UnsupportedSimdWidth => "contract-unsupported-simd-width",
            Self::InvalidGrfCount => "contract-invalid-grf-count",
            Self::UnsupportedScratch => "contract-unsupported-scratch",
            Self::UnsupportedSlm => "contract-unsupported-slm",
            Self::InvalidCrossThreadData => "contract-invalid-cross-thread-data",
            Self::UnsupportedPerThreadData => "contract-unsupported-per-thread-data",
            Self::InvalidPayloadArg => "contract-invalid-payload-arg",
            Self::InvalidPointerSize => "contract-invalid-pointer-size",
            Self::MissingPointerQualifier => "contract-missing-pointer-qualifier",
            Self::InvalidValueQualifier => "contract-invalid-value-qualifier",
            Self::PayloadArgOutOfBounds => "contract-payload-arg-out-of-bounds",
            Self::DuplicatePayloadArg => "contract-duplicate-payload-arg",
            Self::OverlappingPayloadArgs => "contract-overlapping-payload-args",
            Self::BindingWithoutPointerPayload => "contract-binding-without-pointer-payload",
            Self::DuplicateBindingArg => "contract-duplicate-binding-arg",
            Self::DuplicateBindingTableIndex => "contract-duplicate-bti",
        }
    }
}

/// Prove that generated text-section and kernel-entry facts describe the
/// admitted Zebin itself. Hash binding alone is not enough for RCS consumers:
/// the interface descriptor needs the exact, aligned `STT_FUNC` file offset.
pub(crate) fn validate_kernel_contract_elf(
    bytes: &[u8],
    contract: &GpgpuKernelAbiContract,
) -> Result<(), &'static str> {
    const ELF64_SECTION_HEADER_BYTES: usize = 64;
    const SHN_XINDEX: usize = 0xFFFF;
    const SHT_PROGBITS: u32 = 1;
    const SHT_SYMTAB: u32 = 2;
    const SHF_EXECINSTR: u64 = 1 << 2;
    const ELF64_SYMBOL_BYTES: usize = 24;
    const STT_FUNC: u8 = 2;

    let section_table_offset = usize::try_from(
        contract_elf_read_u64(bytes, 40).ok_or("contract-elf-missing-section-table")?,
    )
    .map_err(|_| "contract-elf-section-table-overflow")?;
    let section_header_bytes =
        contract_elf_read_u16(bytes, 58).ok_or("contract-elf-missing-section-table")? as usize;
    let section_count =
        contract_elf_read_u16(bytes, 60).ok_or("contract-elf-missing-section-table")? as usize;
    let section_names_index =
        contract_elf_read_u16(bytes, 62).ok_or("contract-elf-missing-section-table")? as usize;
    if section_header_bytes != ELF64_SECTION_HEADER_BYTES
        || section_count == 0
        || section_names_index == 0
        || section_names_index == SHN_XINDEX
        || section_names_index >= section_count
    {
        return Err("contract-elf-invalid-section-table");
    }
    let section_table_bytes = section_header_bytes
        .checked_mul(section_count)
        .ok_or("contract-elf-section-table-overflow")?;
    let section_table_end = section_table_offset
        .checked_add(section_table_bytes)
        .ok_or("contract-elf-section-table-overflow")?;
    if section_table_end > bytes.len() {
        return Err("contract-elf-section-table-out-of-bounds");
    }

    let names_header = contract_elf_section_header(
        bytes,
        section_table_offset,
        section_header_bytes,
        section_names_index,
    )
    .ok_or("contract-elf-string-table-out-of-bounds")?;
    let names = contract_elf_section_bytes(bytes, names_header)
        .ok_or("contract-elf-string-table-out-of-bounds")?;

    let mut matching_section = None;
    let mut index = 1;
    while index < section_count {
        let header =
            contract_elf_section_header(bytes, section_table_offset, section_header_bytes, index)
                .ok_or("contract-elf-section-header-out-of-bounds")?;
        let name_offset = contract_elf_read_u32(header, 0)
            .ok_or("contract-elf-section-header-out-of-bounds")? as usize;
        let Some(name) = contract_elf_string(names, name_offset) else {
            return Err("contract-elf-section-name-out-of-bounds");
        };
        if name == contract.text_section_name.as_bytes() {
            if matching_section.is_some() {
                return Err("contract-elf-duplicate-text-section");
            }
            matching_section = Some((index, header));
        }
        index += 1;
    }

    let (text_section_index, header) =
        matching_section.ok_or("contract-elf-text-section-missing")?;
    if contract_elf_read_u32(header, 4) != Some(SHT_PROGBITS) {
        return Err("contract-elf-text-section-wrong-type");
    }
    let flags = contract_elf_read_u64(header, 8).ok_or("contract-elf-invalid-text-section")?;
    if flags & SHF_EXECINSTR == 0 {
        return Err("contract-elf-text-section-not-executable");
    }
    let section_offset =
        contract_elf_read_u64(header, 24).ok_or("contract-elf-invalid-text-section")?;
    let section_size =
        contract_elf_read_u64(header, 32).ok_or("contract-elf-invalid-text-section")?;
    if section_offset != contract.text_section_offset || section_size != contract.text_section_size
    {
        return Err("contract-elf-text-section-range-mismatch");
    }
    let section_end = section_offset
        .checked_add(section_size)
        .ok_or("contract-elf-text-section-overflow")?;
    if usize::try_from(section_end).map_or(true, |end| end > bytes.len()) {
        return Err("contract-elf-text-range-out-of-bounds");
    }

    let mut matching_symbol = None;
    index = 1;
    while index < section_count {
        let symbol_header =
            contract_elf_section_header(bytes, section_table_offset, section_header_bytes, index)
                .ok_or("contract-elf-section-header-out-of-bounds")?;
        if contract_elf_read_u32(symbol_header, 4) != Some(SHT_SYMTAB) {
            index += 1;
            continue;
        }
        let entry_bytes = usize::try_from(
            contract_elf_read_u64(symbol_header, 56).ok_or("contract-elf-invalid-symtab")?,
        )
        .map_err(|_| "contract-elf-invalid-symtab")?;
        let symbols = contract_elf_section_bytes(bytes, symbol_header)
            .ok_or("contract-elf-symtab-out-of-bounds")?;
        if entry_bytes != ELF64_SYMBOL_BYTES || symbols.len() % entry_bytes != 0 {
            return Err("contract-elf-invalid-symtab");
        }
        let string_table_index =
            contract_elf_read_u32(symbol_header, 40).ok_or("contract-elf-invalid-symtab")? as usize;
        if string_table_index >= section_count {
            return Err("contract-elf-invalid-symbol-string-table");
        }
        let string_header = contract_elf_section_header(
            bytes,
            section_table_offset,
            section_header_bytes,
            string_table_index,
        )
        .ok_or("contract-elf-invalid-symbol-string-table")?;
        let symbol_names = contract_elf_section_bytes(bytes, string_header)
            .ok_or("contract-elf-symbol-string-table-out-of-bounds")?;

        let mut symbol_offset = 0;
        while symbol_offset < symbols.len() {
            let symbol = symbols
                .get(symbol_offset..symbol_offset + entry_bytes)
                .ok_or("contract-elf-invalid-symbol")?;
            let name_offset =
                contract_elf_read_u32(symbol, 0).ok_or("contract-elf-invalid-symbol")? as usize;
            let info = *symbol.get(4).ok_or("contract-elf-invalid-symbol")?;
            let symbol_section =
                contract_elf_read_u16(symbol, 6).ok_or("contract-elf-invalid-symbol")? as usize;
            let name = contract_elf_string(symbol_names, name_offset)
                .ok_or("contract-elf-symbol-name-out-of-bounds")?;
            if info & 0x0F == STT_FUNC
                && symbol_section == text_section_index
                && name == contract.kernel_name.as_bytes()
            {
                if matching_symbol.is_some() {
                    return Err("contract-elf-duplicate-kernel-symbol");
                }
                let value =
                    contract_elf_read_u64(symbol, 8).ok_or("contract-elf-invalid-symbol")?;
                let size =
                    contract_elf_read_u64(symbol, 16).ok_or("contract-elf-invalid-symbol")?;
                let entry_offset = section_offset
                    .checked_add(value)
                    .ok_or("contract-elf-entry-offset-overflow")?;
                matching_symbol = Some((entry_offset, size));
            }
            symbol_offset += entry_bytes;
        }
        index += 1;
    }

    let (entry_offset, entry_size) = matching_symbol.ok_or("contract-elf-kernel-symbol-missing")?;
    if entry_offset != contract.entry_offset || entry_size != contract.entry_size {
        return Err("contract-elf-entry-range-mismatch");
    }
    let entry_end = entry_offset
        .checked_add(entry_size)
        .ok_or("contract-elf-entry-range-overflow")?;
    if entry_offset < section_offset || entry_end > section_end {
        return Err("contract-elf-entry-out-of-bounds");
    }
    Ok(())
}

fn contract_elf_section_header(
    bytes: &[u8],
    table_offset: usize,
    entry_bytes: usize,
    index: usize,
) -> Option<&[u8]> {
    let offset = table_offset.checked_add(entry_bytes.checked_mul(index)?)?;
    bytes.get(offset..offset.checked_add(entry_bytes)?)
}

fn contract_elf_string(bytes: &[u8], offset: usize) -> Option<&[u8]> {
    let tail = bytes.get(offset..)?;
    let end = tail.iter().position(|byte| *byte == 0)?;
    tail.get(..end)
}

fn contract_elf_section_bytes<'a>(bytes: &'a [u8], header: &[u8]) -> Option<&'a [u8]> {
    let offset = usize::try_from(contract_elf_read_u64(header, 24)?).ok()?;
    let size = usize::try_from(contract_elf_read_u64(header, 32)?).ok()?;
    bytes.get(offset..offset.checked_add(size)?)
}

fn contract_elf_read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?))
}

fn contract_elf_read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?))
}

fn contract_elf_read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?))
}

const fn digest_is_zero(digest: &[u8; 32]) -> bool {
    let mut index = 0;
    while index < digest.len() {
        if digest[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

const fn text_section_matches_kernel(section_name: &str, kernel_name: &str) -> bool {
    const PREFIX: &[u8] = b".text.";
    let section_name = section_name.as_bytes();
    let kernel_name = kernel_name.as_bytes();
    if section_name.len() != PREFIX.len() + kernel_name.len() {
        return false;
    }
    let mut index = 0;
    while index < PREFIX.len() {
        if section_name[index] != PREFIX[index] {
            return false;
        }
        index += 1;
    }
    index = 0;
    while index < kernel_name.len() {
        if section_name[PREFIX.len() + index] != kernel_name[index] {
            return false;
        }
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    include!("../kernels/artifacts/adls/cpp/copy_rect_rgba8.contract.rs");

    const COPY_RECT_ZEBIN: &[u8] = include_bytes!("../kernels/artifacts/adls/copy_rect_rgba8.bin");
    const COPY_RECT_CPP_ZEBIN: &[u8] =
        include_bytes!("../kernels/artifacts/adls/cpp/copy_rect_rgba8.bin");
    const HASH: [u8; 32] = [0xA5; 32];
    const PAYLOAD_ARGS: &[GpgpuArtifactPayloadArg] = &[
        GpgpuArtifactPayloadArg {
            arg_index: 0,
            kind: GpgpuArtifactArgKind::ByPointer,
            access: GpgpuArtifactArgAccess::ReadOnly,
            address_mode: GpgpuArtifactAddressMode::Stateful,
            offset_bytes: 48,
            size_bytes: 8,
        },
        GpgpuArtifactPayloadArg {
            arg_index: 1,
            kind: GpgpuArtifactArgKind::ByValue,
            access: GpgpuArtifactArgAccess::None,
            address_mode: GpgpuArtifactAddressMode::None,
            offset_bytes: 56,
            size_bytes: 4,
        },
    ];
    const BINDINGS: &[GpgpuArtifactBinding] = &[GpgpuArtifactBinding {
        arg_index: 0,
        bti: 0,
    }];
    const CONTRACT: GpgpuKernelAbiContract = GpgpuKernelAbiContract {
        schema_version: GPGPU_KERNEL_ABI_SCHEMA_VERSION,
        kernel_name: "copy_rect_rgba8",
        target: GPGPU_ADLS_4680_TARGET,
        ze_info_major: 1,
        ze_info_minor: 64,
        zebin_sha256: HASH,
        spv_sha256: HASH,
        text_section_name: ".text.copy_rect_rgba8",
        text_section_offset: 64,
        text_section_size: 128,
        entry_offset: 64,
        entry_size: 128,
        simd_width: 16,
        grf_count: 128,
        scratch_bytes: 0,
        slm_bytes: 0,
        cross_thread_data_bytes: 96,
        per_thread_data_bytes: 96,
        bindings: BINDINGS,
        payload_args: PAYLOAD_ARGS,
    };
    const COPY_RECT_ELF_CONTRACT: GpgpuKernelAbiContract = GpgpuKernelAbiContract {
        text_section_size: 896,
        entry_size: 712,
        ..CONTRACT
    };

    #[test]
    fn adls_policy_does_not_admit_adln_or_rpl() {
        assert!(GPGPU_ADLS_4680_TARGET.supports(0x4680, 0));
        assert!(!GPGPU_ADLS_4680_TARGET.supports(0x46D1, 0));
        assert!(!GPGPU_ADLS_4680_TARGET.supports(0xA780, 0));
    }

    #[test]
    fn direct_rcs_contract_accepts_the_supported_shape() {
        assert_eq!(CONTRACT.validate(), Ok(()));
    }

    #[test]
    fn contract_rejects_unprogrammed_simd_width() {
        let invalid = GpgpuKernelAbiContract {
            simd_width: 32,
            ..CONTRACT
        };
        assert_eq!(invalid.validate(), Err(GpgpuKernelAbiContractError::UnsupportedSimdWidth));
    }

    #[test]
    fn contract_rejects_pointer_metadata_loss() {
        const INVALID_ARGS: &[GpgpuArtifactPayloadArg] = &[
            GpgpuArtifactPayloadArg {
                arg_index: 0,
                kind: GpgpuArtifactArgKind::ByPointer,
                access: GpgpuArtifactArgAccess::None,
                address_mode: GpgpuArtifactAddressMode::Stateful,
                offset_bytes: 48,
                size_bytes: 8,
            },
            PAYLOAD_ARGS[1],
        ];
        let invalid = GpgpuKernelAbiContract {
            payload_args: INVALID_ARGS,
            ..CONTRACT
        };
        assert_eq!(invalid.validate(), Err(GpgpuKernelAbiContractError::MissingPointerQualifier));
    }

    #[test]
    fn elf_contract_matches_exact_text_section_and_func_symbol() {
        assert_eq!(validate_kernel_contract_elf(COPY_RECT_ZEBIN, &COPY_RECT_ELF_CONTRACT), Ok(()));
        let wrong_entry = GpgpuKernelAbiContract {
            entry_size: COPY_RECT_ELF_CONTRACT.entry_size + 1,
            ..COPY_RECT_ELF_CONTRACT
        };
        assert_eq!(
            validate_kernel_contract_elf(COPY_RECT_ZEBIN, &wrong_entry),
            Err("contract-elf-entry-range-mismatch")
        );
    }

    #[test]
    fn generated_cpp_contract_is_runtime_admissible() {
        assert_eq!(COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(()));
        assert_eq!(
            validate_kernel_contract_elf(
                COPY_RECT_CPP_ZEBIN,
                &COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT,
            ),
            Ok(())
        );
        assert!(
            COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT
                .target
                .supports(0x4680, 0)
        );
        assert!(
            !COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT
                .target
                .supports(0x46D1, 0)
        );
        assert!(
            !COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT
                .target
                .supports(0xA780, 0)
        );
    }
}
