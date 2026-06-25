BUILD_MODE ?= debug
KERNEL_TARGET_DIR = x86_64-unknown-trueos
KERNEL_BIN = tgt/$(KERNEL_TARGET_DIR)/$(BUILD_MODE)/TRUEOS
KERNEL_EMPTY_LIB_DIR = bld/empty-libs
ARTIFACT_BUILD_ID ?= $(shell git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)
ARTIFACT_DIR = bld/artifacts/$(BUILD_MODE)-$(ARTIFACT_BUILD_ID)
ARTIFACT_RUNTIME_ELF = $(ARTIFACT_DIR)/TRUEOS.elf
ARTIFACT_DEBUG_ELF = $(ARTIFACT_DIR)/TRUEOS.full.elf
ARTIFACT_BUILD_INFO = $(ARTIFACT_DIR)/BUILD_INFO
PROVENANCE_DIR := bld/provenance
PROVENANCE_LATEST := $(PROVENANCE_DIR)/latest.json
PROVENANCE_LATEST_SOURCE_MANIFEST := $(PROVENANCE_DIR)/latest.source-files.sha256
PROVENANCE_SCRIPT := tools/provenance_chain.py
PROVENANCE_CLEAN_FLAG ?= --require-clean
PROVENANCE_SOURCE_MANIFEST ?= git-commit
START_BAREMETAL_LOG ?= 1
ISO_DIR := bld
ISO_PATH := bld/trueos.iso
ISO_BOOT_DIR := bld/iso-bootroot
ISO_EFI_IMG := efi.img
UPDATE_7Z_FLAGS ?= -mx=9 -m0=LZMA2 -ms=off
RELEASE_BUNDLE_DIR := $(ISO_DIR)/trueos-release
RELEASE_ARCHIVE := $(ISO_DIR)/TrueOS.7z
PUBLISH_RELEASE_SMB ?= 1
BUNDLED_OVMF_NAME := ovmf-code-x86_64.fd
OVMF_BUNDLE_PATH ?= $(firstword $(wildcard /usr/share/ovmf/OVMF.fd /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd /opt/homebrew/share/qemu/edk2-x86_64-code.fd /usr/local/share/qemu/edk2-x86_64-code.fd))
OVMF_LICENSE_PATH ?= $(firstword $(wildcard /usr/share/doc/ovmf/copyright /opt/homebrew/share/doc/qemu/LICENSE /usr/local/share/doc/qemu/LICENSE))
# Extra slack added on top of the EFI bootloader when sizing the embedded EFI
# System Partition image. Runtime payloads live once in the ISO filesystem.
EFI_IMG_OVERHEAD_KIB ?= 1024
EFI_IMG_MIN_SIZE_KIB ?= 0
LIMINE_CFG := limine.conf
LIMINE_CFG_GENERATED := $(ISO_DIR)/limine.generated.conf
LIMINE_SUBMODULE := vendor/limine
LIMINE_DIST ?= .limine
LIMINE_SRC := $(LIMINE_DIST)/src
LIMINE_BUILD_DIR := $(LIMINE_DIST)/build-x86_64
LIMINE_PREFIX := $(LIMINE_DIST)/prefix-x86_64
LIMINE_SHARE := $(LIMINE_PREFIX)/share/limine
LIMINE_BOOTX64 := $(LIMINE_SHARE)/BOOTX64.EFI
LIMINE_UEFI_CD := $(LIMINE_SHARE)/limine-uefi-cd.bin
LIMINE_CONFIG_ARGS ?= --prefix=$(abspath $(LIMINE_PREFIX)) --enable-uefi-x86-64 --enable-uefi-cd
LIMINE_SOURCE_STAMP := $(LIMINE_DIST)/.source_stamp
LIMINE_CONFIG_STAMP := $(LIMINE_BUILD_DIR)/.config_args
LIMINE_TOOLCHAIN_STAMP := $(LIMINE_BUILD_DIR)/.toolchain_args
LIMINE_INSTALL_STAMP := $(LIMINE_BUILD_DIR)/.installed
# Upstream i915 maps ADL-S/RKL GuC and HuC firmware selection to TGL blobs.
GUC_FW_HOST_PATH ?= /lib/firmware/i915/tgl_guc_70.bin.zst
GUC_FW_ISO_REL_PATH ?= EFI/BOOT/tgl_guc_70.bin
DMC_FW_HOST_PATH ?= /lib/firmware/i915/adls_dmc_ver2_01.bin.zst
DMC_FW_ISO_REL_PATH ?= EFI/BOOT/adls_dmc_ver2_01.bin
HUC_FW_HOST_PATH ?= /lib/firmware/i915/tgl_huc.bin.zst
HUC_FW_ISO_REL_PATH ?= EFI/BOOT/tgl_huc.bin
HORIZON_BP_HOST_PATH ?= ../TRUEOS-Blueprints/dist/horizon.bp
HORIZON_BP_ISO_REL_PATH ?= EFI/BOOT/apps/horizon.bp
ENABLE_BLUEPRINTS ?= 0
QEMU_RUNNER := tools/qemu/run.sh
QEMU_BIN ?= qemu-system-x86_64
QEMU_MEMORY ?= 12000M
QEMU_UEFI_FIRMWARE = $(OVMF_BUNDLE_PATH)
NVME_IMG := tools/nvme.img
CNT_FILE := bld/cnt
QEMU_BRIDGE ?= br0
QEMU_BRIDGE_HELPER ?= $(firstword $(wildcard /usr/lib/qemu/qemu-bridge-helper /usr/libexec/qemu-bridge-helper /usr/lib/qemu-bridge-helper))
QEMU_HDA_AUDIODEV ?= none,id=snd0
QEMU_HOST_TCP_PORT_3 ?= 10003
QEMU_HOST_TCP_PORT_4 ?= 10004
QEMU_HOST_TCP_PORT_100 ?= 10100
QEMU_HOST_TCP_PORT_80 ?= 8080
QEMU_HOST_TCP_PORT_54321 ?= 15432
QEMU_HOST_TCP_PORT_32123 ?= 32123
QEMU_HOST_UDP_PORT_32343 ?= 32343
QEMU_RUN_ENV = ISO_PATH="$(ISO_PATH)" QEMU_BIN="$(QEMU_BIN)" QEMU_MEMORY="$(QEMU_MEMORY)" QEMU_UEFI_FIRMWARE="$(QEMU_UEFI_FIRMWARE)" QEMU_NVME_IMG="$(NVME_IMG)" QEMU_BRIDGE="$(QEMU_BRIDGE)" QEMU_BRIDGE_HELPER="$(QEMU_BRIDGE_HELPER)" QEMU_HDA_AUDIODEV="$(QEMU_HDA_AUDIODEV)" QEMU_HOST_TCP_PORT_3="$(QEMU_HOST_TCP_PORT_3)" QEMU_HOST_TCP_PORT_4="$(QEMU_HOST_TCP_PORT_4)" QEMU_HOST_TCP_PORT_100="$(QEMU_HOST_TCP_PORT_100)" QEMU_HOST_TCP_PORT_80="$(QEMU_HOST_TCP_PORT_80)" QEMU_HOST_TCP_PORT_54321="$(QEMU_HOST_TCP_PORT_54321)" QEMU_HOST_TCP_PORT_32123="$(QEMU_HOST_TCP_PORT_32123)" QEMU_HOST_UDP_PORT_32343="$(QEMU_HOST_UDP_PORT_32343)"
BAREMETAL_LOG_DRAIN := tools/baremetal-log-drain.sh
BAREMETAL_LOG_HOST ?= 192.168.178.94
BAREMETAL_LOG_PORT ?= 1
BAREMETAL_LOG_DELAY ?= 15
BAREMETAL_LOG_DIR ?= bld/baremetal-logs

CARGO_BUILD_FLAGS ?=

CARGO_GFX_FLAGS =

IMG_SIZE ?= 25G

.PHONY: images empty-libs kernel artifacts limine baremetal-reboot-log iso provenance-git-clean provenance verify-provenance release-git-clean release dbg run

images: $(NVME_IMG)

$(NVME_IMG):
	mkdir -p $(@D)
	truncate -s $(IMG_SIZE) $@

empty-libs:
	mkdir -p $(KERNEL_EMPTY_LIB_DIR)
	rm -f $(KERNEL_EMPTY_LIB_DIR)/empty.o
	ar crs $(KERNEL_EMPTY_LIB_DIR)/libc.a
	ar crs $(KERNEL_EMPTY_LIB_DIR)/libgcc_s.a

kernel: empty-libs
	@mkdir -p "$$(dirname "$(CNT_FILE)")"; count=$$(cat "$(CNT_FILE)" 2>/dev/null || echo 0); count=$${count:-0}; printf '%s\n' $$((count + 1)) | tee "$(CNT_FILE)"
	cargo +nightly build $(CARGO_GFX_FLAGS) $(CARGO_BUILD_FLAGS) -Z build-std=core,compiler_builtins,alloc,panic_abort -Z json-target-spec --target .cargo/x86_64-unknown-trueos.json

artifacts: kernel
	mkdir -p $(ARTIFACT_DIR)
	cp $(KERNEL_BIN) $(ARTIFACT_RUNTIME_ELF)
	cp $(KERNEL_BIN) $(ARTIFACT_DEBUG_ELF)
	strip -s $(ARTIFACT_RUNTIME_ELF) || true
	@{ \
		commit=$$(git rev-parse HEAD 2>/dev/null || echo unknown); \
		ts=$$(date -u +"%Y-%m-%dT%H:%M:%SZ"); \
		printf "build_mode=%s\n" "$(BUILD_MODE)"; \
		printf "build_id=%s\n" "$(ARTIFACT_BUILD_ID)"; \
		printf "commit=%s\n" "$$commit"; \
		printf "timestamp_utc=%s\n" "$$ts"; \
		printf "runtime_elf=%s\n" "$(ARTIFACT_RUNTIME_ELF)"; \
		printf "debug_elf=%s\n" "$(ARTIFACT_DEBUG_ELF)"; \
	} > $(ARTIFACT_BUILD_INFO)

limine:
	@set -e; \
	if [ ! -f "$(LIMINE_SUBMODULE)/bootstrap" ]; then \
		git submodule update --init "$(LIMINE_SUBMODULE)"; \
	fi; \
	if [ ! -f "$(LIMINE_SUBMODULE)/bootstrap" ]; then \
		echo "error: missing Limine submodule at $(LIMINE_SUBMODULE)"; \
		exit 1; \
	fi; \
	cc=$$(command -v gcc || command -v clang || command -v cc || true); \
	ld=$$(command -v ld.lld || command -v gld || command -v ld || true); \
	objcopy=$$(command -v llvm-objcopy || command -v gobjcopy || command -v objcopy || true); \
	objdump=$$(command -v llvm-objdump || command -v gobjdump || command -v objdump || true); \
	readelf=$$(command -v llvm-readelf || command -v greadelf || command -v readelf || true); \
	for tool in cc ld objcopy objdump readelf; do \
		eval value=\$$$$tool; \
		if [ -z "$$value" ]; then \
			echo "error: missing required Limine build tool: $$tool"; \
			exit 1; \
		fi; \
	done; \
	source_stamp="submodule:$$(git -C "$(LIMINE_SUBMODULE)" rev-parse HEAD)"; \
	source_changed=0; \
	if [ "$$(cat "$(LIMINE_SOURCE_STAMP)" 2>/dev/null || true)" != "$$source_stamp" ] || [ ! -f "$(LIMINE_SRC)/bootstrap" ]; then \
		rm -rf "$(LIMINE_SRC)"; \
		mkdir -p "$(LIMINE_SRC)"; \
		(cd "$(LIMINE_SUBMODULE)" && tar --exclude .git --exclude trueos_dist -cf - .) | (cd "$(LIMINE_SRC)" && tar -xf -); \
		mkdir -p "$(LIMINE_DIST)"; \
		printf '%s\n' "$$source_stamp" > "$(LIMINE_SOURCE_STAMP)"; \
		source_changed=1; \
	fi; \
	toolchain_stamp=$$(printf 'CC_FOR_TARGET=%s\nLD_FOR_TARGET=%s\nOBJCOPY_FOR_TARGET=%s\nOBJDUMP_FOR_TARGET=%s\nREADELF_FOR_TARGET=%s\n' "$$cc" "$$ld" "$$objcopy" "$$objdump" "$$readelf"); \
	if [ "$$source_changed" = "0" ] && [ -f "$(LIMINE_BOOTX64)" ] && [ -f "$(LIMINE_UEFI_CD)" ] && [ -f "$(LIMINE_INSTALL_STAMP)" ] && [ "$$(cat "$(LIMINE_CONFIG_STAMP)" 2>/dev/null || true)" = "$(LIMINE_CONFIG_ARGS)" ] && [ "$$(cat "$(LIMINE_TOOLCHAIN_STAMP)" 2>/dev/null || true)" = "$$toolchain_stamp" ]; then \
		exit 0; \
	fi; \
	if [ "$$source_changed" = "1" ] || [ "$$(cat "$(LIMINE_CONFIG_STAMP)" 2>/dev/null || true)" != "$(LIMINE_CONFIG_ARGS)" ] || [ "$$(cat "$(LIMINE_TOOLCHAIN_STAMP)" 2>/dev/null || true)" != "$$toolchain_stamp" ]; then \
		rm -rf "$(LIMINE_BUILD_DIR)" "$(LIMINE_PREFIX)"; \
	fi; \
	mkdir -p "$(LIMINE_BUILD_DIR)" "$(LIMINE_PREFIX)"; \
	printf '%s\n' "$(LIMINE_CONFIG_ARGS)" > "$(LIMINE_CONFIG_STAMP)"; \
	printf '%s\n' "$$toolchain_stamp" > "$(LIMINE_TOOLCHAIN_STAMP)"; \
	if [ ! -f "$(LIMINE_SRC)/configure" ]; then \
		command -v autoreconf >/dev/null 2>&1 || { echo "error: missing autoreconf; install autoconf + automake"; exit 1; }; \
		(cd "$(LIMINE_SRC)" && ./bootstrap); \
	fi; \
	(cd "$(LIMINE_BUILD_DIR)" && \
		CC_FOR_TARGET="$$cc" \
		LD_FOR_TARGET="$$ld" \
		OBJCOPY_FOR_TARGET="$$objcopy" \
		OBJDUMP_FOR_TARGET="$$objdump" \
		READELF_FOR_TARGET="$$readelf" \
		$(abspath $(LIMINE_SRC))/configure $(LIMINE_CONFIG_ARGS)); \
	make -C "$(LIMINE_BUILD_DIR)"; \
	make -C "$(LIMINE_BUILD_DIR)" install; \
	printf 'ok\n' > "$(LIMINE_INSTALL_STAMP)"

baremetal-reboot-log:
	-fuser -k 7777/udp || true
	python3 -c "import socket; s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); s.bind(('',7777)); exec(\"while True:\n d,a=s.recvfrom(2048)\n if d==b'probe': s.sendto(b'ack',(a[0],7777)); break\")" &
	@TRUEOS_BAREMETAL_LOG_HOST="$(BAREMETAL_LOG_HOST)" TRUEOS_BAREMETAL_LOG_PORT="$(BAREMETAL_LOG_PORT)" TRUEOS_BAREMETAL_LOG_DELAY="$(BAREMETAL_LOG_DELAY)" TRUEOS_BAREMETAL_LOG_DIR="$(BAREMETAL_LOG_DIR)" "$(BAREMETAL_LOG_DRAIN)" start

FORCE:

iso: artifacts images limine
	rm -rf $(ISO_BOOT_DIR)
	rm -f $(ISO_PATH)
	mkdir -p $(ISO_BOOT_DIR)
	cp $(ARTIFACT_RUNTIME_ELF) $(ISO_BOOT_DIR)/TRUEOS.elf
	mkdir -p $(ISO_DIR)/EFI/BOOT
	rm -f $(ISO_DIR)/EFI/BOOT/adlp_guc_70.bin $(ISO_DIR)/EFI/BOOT/tgl_guc_70.bin
	cp $(LIMINE_BOOTX64) $(ISO_DIR)/EFI/BOOT/BOOTX64.EFI
	@if [ ! -f "$(GUC_FW_HOST_PATH)" ]; then \
		echo "error: GUC firmware not found at $(GUC_FW_HOST_PATH)"; \
		exit 1; \
	fi
	@case "$(GUC_FW_HOST_PATH)" in \
		*.zst) \
			command -v zstd >/dev/null 2>&1 || { echo "error: zstd command not found; cannot decompress $(GUC_FW_HOST_PATH)"; exit 1; }; \
			zstd -d -c "$(GUC_FW_HOST_PATH)" > "$(ISO_DIR)/EFI/BOOT/$$(basename "$(GUC_FW_ISO_REL_PATH)")"; \
			;; \
		*) \
			cp "$(GUC_FW_HOST_PATH)" "$(ISO_DIR)/EFI/BOOT/$$(basename "$(GUC_FW_ISO_REL_PATH)")"; \
			;; \
	esac
	@if [ -f "$(DMC_FW_HOST_PATH)" ]; then \
		case "$(DMC_FW_HOST_PATH)" in \
			*.zst) zstd -d -c "$(DMC_FW_HOST_PATH)" > "$(ISO_DIR)/EFI/BOOT/$$(basename "$(DMC_FW_ISO_REL_PATH)")" ;; \
			*) cp "$(DMC_FW_HOST_PATH)" "$(ISO_DIR)/EFI/BOOT/$$(basename "$(DMC_FW_ISO_REL_PATH)")" ;; \
		esac; \
	else \
		echo "iso: skipping DMC firmware probe artifact, missing $(DMC_FW_HOST_PATH)"; \
	fi
	@if [ -f "$(HUC_FW_HOST_PATH)" ]; then \
		case "$(HUC_FW_HOST_PATH)" in \
			*.zst) zstd -d -c "$(HUC_FW_HOST_PATH)" > "$(ISO_DIR)/EFI/BOOT/$$(basename "$(HUC_FW_ISO_REL_PATH)")" ;; \
			*) cp "$(HUC_FW_HOST_PATH)" "$(ISO_DIR)/EFI/BOOT/$$(basename "$(HUC_FW_ISO_REL_PATH)")" ;; \
		esac; \
	else \
		echo "iso: skipping HuC firmware probe artifact, missing $(HUC_FW_HOST_PATH)"; \
	fi
	mkdir -p $(ISO_BOOT_DIR)/$(dir $(GUC_FW_ISO_REL_PATH))
	cp "$(ISO_DIR)/EFI/BOOT/$$(basename "$(GUC_FW_ISO_REL_PATH)")" "$(ISO_BOOT_DIR)/$(GUC_FW_ISO_REL_PATH)"
	@if [ -f "$(ISO_DIR)/EFI/BOOT/$$(basename "$(DMC_FW_ISO_REL_PATH)")" ]; then \
		mkdir -p $(ISO_BOOT_DIR)/$(dir $(DMC_FW_ISO_REL_PATH)); \
		cp "$(ISO_DIR)/EFI/BOOT/$$(basename "$(DMC_FW_ISO_REL_PATH)")" "$(ISO_BOOT_DIR)/$(DMC_FW_ISO_REL_PATH)"; \
	fi
	@if [ -f "$(ISO_DIR)/EFI/BOOT/$$(basename "$(HUC_FW_ISO_REL_PATH)")" ]; then \
		mkdir -p $(ISO_BOOT_DIR)/$(dir $(HUC_FW_ISO_REL_PATH)); \
		cp "$(ISO_DIR)/EFI/BOOT/$$(basename "$(HUC_FW_ISO_REL_PATH)")" "$(ISO_BOOT_DIR)/$(HUC_FW_ISO_REL_PATH)"; \
	fi
	@if [ "$(ENABLE_BLUEPRINTS)" = "1" ]; then \
		if [ ! -f "$(HORIZON_BP_HOST_PATH)" ]; then \
			echo "error: Horizon blueprint not found at $(HORIZON_BP_HOST_PATH)"; \
			echo "       run: cd ../TRUEOS-Blueprints && cargo bp horizon"; \
			exit 1; \
		fi; \
		mkdir -p "$(ISO_BOOT_DIR)/$(dir $(HORIZON_BP_ISO_REL_PATH))"; \
		cp "$(HORIZON_BP_HOST_PATH)" "$(ISO_BOOT_DIR)/$(HORIZON_BP_ISO_REL_PATH)"; \
		mkdir -p "$(ISO_DIR)/$(dir $(HORIZON_BP_ISO_REL_PATH))"; \
		cp "$(HORIZON_BP_HOST_PATH)" "$(ISO_DIR)/$(HORIZON_BP_ISO_REL_PATH)"; \
	else \
		echo "iso: skipping Blueprint modules (ENABLE_BLUEPRINTS=0)"; \
	fi
	cp "$(LIMINE_CFG)" "$(LIMINE_CFG_GENERATED)"
	@if [ -f "$(ISO_BOOT_DIR)/$(DMC_FW_ISO_REL_PATH)" ]; then \
		printf '%s\n%s\n' \
			"module_path: boot():/$(DMC_FW_ISO_REL_PATH)" \
			"module_string: trueos.fw.dmc" \
			>> "$(LIMINE_CFG_GENERATED)"; \
	fi
	@if [ -f "$(ISO_BOOT_DIR)/$(HUC_FW_ISO_REL_PATH)" ]; then \
		printf '%s\n%s\n' \
			"module_path: boot():/$(HUC_FW_ISO_REL_PATH)" \
			"module_string: trueos.fw.huc.tgl" \
			>> "$(LIMINE_CFG_GENERATED)"; \
	fi
	printf '%s\n%s\n' \
		"module_path: boot():/$(ISO_EFI_IMG)" \
		"module_string: trueos.install.efi_img" \
		>> "$(LIMINE_CFG_GENERATED)"
	@if [ "$(ENABLE_BLUEPRINTS)" = "1" ]; then \
		printf '%s\n%s\n' \
			"module_path: boot():/$(HORIZON_BP_ISO_REL_PATH)" \
			"module_string: trueos.app.horizon" \
			>> "$(LIMINE_CFG_GENERATED)"; \
	fi
	@if [ "$(GUC_FW_ISO_REL_PATH)" != "EFI/BOOT/tgl_guc_70.bin" ]; then \
		mkdir -p $(ISO_BOOT_DIR)/EFI/BOOT; \
		cp "$(ISO_DIR)/EFI/BOOT/$$(basename "$(GUC_FW_ISO_REL_PATH)")" "$(ISO_BOOT_DIR)/EFI/BOOT/tgl_guc_70.bin"; \
	fi
	cp $(LIMINE_CFG_GENERATED) $(ISO_BOOT_DIR)/limine.conf
	cp $(LIMINE_CFG_GENERATED) $(ISO_DIR)/EFI/BOOT/limine.conf
	cp $(ISO_BOOT_DIR)/TRUEOS.elf $(ISO_DIR)/TRUEOS.elf
	rm -f $(ISO_BOOT_DIR)/$(ISO_EFI_IMG)
	@efi_payload_kib=$$(du -sk "$(LIMINE_BOOTX64)" | cut -f1); \
		efi_img_size_kib=$$((efi_payload_kib + $(EFI_IMG_OVERHEAD_KIB))); \
		if [ "$$efi_img_size_kib" -lt "$(EFI_IMG_MIN_SIZE_KIB)" ]; then \
			efi_img_size_kib="$(EFI_IMG_MIN_SIZE_KIB)"; \
		fi; \
		echo "iso: sizing $(ISO_EFI_IMG) to $${efi_img_size_kib} KiB (payload=$${efi_payload_kib} KiB, overhead=$(EFI_IMG_OVERHEAD_KIB) KiB)"; \
		dd if=/dev/zero of=$(ISO_BOOT_DIR)/$(ISO_EFI_IMG) bs=1k count=$$efi_img_size_kib
	mkfs.vfat -n TRUEOS_EFI $(ISO_BOOT_DIR)/$(ISO_EFI_IMG)
	mmd -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) ::/EFI ::/EFI/BOOT
	mcopy -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) $(LIMINE_BOOTX64) ::/EFI/BOOT/BOOTX64.EFI
	cp $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) $(ISO_DIR)/$(ISO_EFI_IMG)
	xorriso -as mkisofs \
		-iso-level 3 -full-iso9660-filenames \
		-R \
		-r \
		-J -joliet-long \
		-e $(ISO_EFI_IMG) -no-emul-boot \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		-o $(ISO_PATH) $(ISO_BOOT_DIR)
	@if [ "$(START_BAREMETAL_LOG)" = "1" ]; then \
		$(MAKE) --no-print-directory baremetal-reboot-log; \
	else \
		echo "iso: skipping baremetal log drain (START_BAREMETAL_LOG=$(START_BAREMETAL_LOG))"; \
	fi

provenance-git-clean:
	@if [ "$(PROVENANCE_CLEAN_FLAG)" = "--require-clean" ]; then \
		set -e; \
		if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then \
			echo "error: provenance requires a Git checkout"; \
			exit 1; \
		fi; \
		status=$$(git status --porcelain=v1 --untracked-files=all --ignore-submodules=none); \
		if [ -n "$$status" ]; then \
			echo "error: provenance requires a clean TRUEOS checkout"; \
			echo "$$status"; \
			echo "commit, stash, or remove these changes before creating release provenance"; \
			exit 1; \
		fi; \
	fi

provenance: provenance-git-clean iso
	python3 $(PROVENANCE_SCRIPT) attest \
		--source-root . \
		--out-dir $(PROVENANCE_DIR) \
		--elf $(ARTIFACT_RUNTIME_ELF) \
		--debug-elf $(ARTIFACT_DEBUG_ELF) \
		--iso $(ISO_PATH) \
		--build-info $(ARTIFACT_BUILD_INFO) \
		--source-manifest $(PROVENANCE_SOURCE_MANIFEST) \
		$(PROVENANCE_CLEAN_FLAG)

verify-provenance:
	python3 $(PROVENANCE_SCRIPT) verify \
		--source-root . \
		--record $(PROVENANCE_LATEST)

release-git-clean:
	@set -e; \
	if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then \
		echo "error: release requires a Git checkout"; \
		exit 1; \
	fi; \
	status=$$(git status --porcelain=v1 --untracked-files=all --ignore-submodules=none); \
	if [ -n "$$status" ]; then \
		echo "error: release requires a clean TRUEOS checkout"; \
		echo "$$status"; \
		echo "commit, stash, or remove these changes before building an official ISO"; \
		exit 1; \
	fi; \
	printf 'release source commit: %s\n' "$$(git rev-parse HEAD)"; \
	printf 'release source tree:   %s\n' "$$(git rev-parse 'HEAD^{tree}')"

release: BUILD_MODE := release
release: CARGO_BUILD_FLAGS += --release
release: PROVENANCE_CLEAN_FLAG := --require-clean
release: release-git-clean provenance
	$(MAKE) --no-print-directory verify-provenance
	@if [ -z "$(OVMF_BUNDLE_PATH)" ] || [ ! -f "$(OVMF_BUNDLE_PATH)" ]; then \
		echo "error: no OVMF firmware found to bundle"; \
		echo "       install OVMF/edk2-ovmf or run: make release OVMF_BUNDLE_PATH=/path/to/ovmf-code-x86_64.fd"; \
		exit 1; \
	fi
	rm -rf $(RELEASE_BUNDLE_DIR)
	rm -f $(RELEASE_ARCHIVE)
	mkdir -p $(RELEASE_BUNDLE_DIR)
	cp $(ISO_PATH) $(RELEASE_BUNDLE_DIR)/trueos.iso
	cp $(PROVENANCE_LATEST) $(RELEASE_BUNDLE_DIR)/TRUEOS.provenance.json
	@if [ -f "$(PROVENANCE_LATEST_SOURCE_MANIFEST)" ]; then \
		cp $(PROVENANCE_LATEST_SOURCE_MANIFEST) $(RELEASE_BUNDLE_DIR)/TRUEOS.source-files.sha256; \
	fi
	cp "$(OVMF_BUNDLE_PATH)" $(RELEASE_BUNDLE_DIR)/$(BUNDLED_OVMF_NAME)
	cp tools/release/run-linux.sh $(RELEASE_BUNDLE_DIR)/run-linux.sh
	cp tools/release/run-macos.sh $(RELEASE_BUNDLE_DIR)/run-macos.sh
	cp tools/release/README-RUN.txt $(RELEASE_BUNDLE_DIR)/README-RUN.txt
	@if [ -n "$(OVMF_LICENSE_PATH)" ] && [ -f "$(OVMF_LICENSE_PATH)" ]; then \
		cp "$(OVMF_LICENSE_PATH)" $(RELEASE_BUNDLE_DIR)/OVMF-LICENSE.txt; \
	fi
	chmod +x $(RELEASE_BUNDLE_DIR)/run-linux.sh $(RELEASE_BUNDLE_DIR)/run-macos.sh
	cd $(RELEASE_BUNDLE_DIR) && 7z a -t7z $(UPDATE_7Z_FLAGS) ../$(notdir $(RELEASE_ARCHIVE)) trueos.iso TRUEOS.provenance.json $$(test -f TRUEOS.source-files.sha256 && printf '%s' TRUEOS.source-files.sha256) $(BUNDLED_OVMF_NAME) run-linux.sh run-macos.sh README-RUN.txt $$(test -f OVMF-LICENSE.txt && printf '%s' OVMF-LICENSE.txt)
	@if [ "$(PUBLISH_RELEASE_SMB)" = "1" ]; then \
		env -u GIO_MODULE_DIR gio mount smb://t4ce@pdjb/home-share || true; \
		env -u GIO_MODULE_DIR gio copy $(RELEASE_ARCHIVE) smb://t4ce@pdjb/home-share/TRUEOS_SITE/; \
	else \
		echo "release: skipping SMB publish (PUBLISH_RELEASE_SMB=$(PUBLISH_RELEASE_SMB))"; \
	fi


SERIAL_CONSOLE_CMD = konsole -e sh -c 'stty -echo -icanon cols 100 rows 100; nc 127.0.0.1 5555; stty sane; echo "Connection closed. Press ENTER to exit..."; read'

dbg: iso
	@killall -9 qemu-system-x86_64 || true
	@$(SERIAL_CONSOLE_CMD) &
	@set -e; \
		$(QEMU_RUN_ENV) $(QEMU_RUNNER) iso-debug -S -s & qemu_pid=$$!; \
		sleep 0.15; \
		echo "GDB stub ready on 127.0.0.1:1234"; \
		wait $$qemu_pid

run: iso
	@killall -9 qemu-system-x86_64 || true
	@($(QEMU_RUN_ENV) $(QEMU_RUNNER) iso & $(SERIAL_CONSOLE_CMD))
