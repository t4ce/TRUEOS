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
# Normal ISO deployment uses the test rig's ESP32-latched physical reset button.
START_BAREMETAL_LOG ?= 1
RELEASE_BUMP_CNT ?= $(if $(CI),0,1)
ISO_DIR := bld
ISO_PATH := bld/trueos.iso
ISO_BOOT_DIR := bld/iso-bootroot
ISO_EFI_IMG := efi.img
UPDATE_7Z_FLAGS ?= -mx=9 -m0=LZMA2 -ms=off
RELEASE_BUNDLE_DIR := $(ISO_DIR)/trueos-release
ISO_ARCHIVE := $(ISO_DIR)/TrueOS.7z
RELEASE_ARCHIVE := $(ISO_ARCHIVE)
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
# Linux i915 selects the TGL GuC 70 firmware family for ADL-S/RKL. GuC is a
# required render/compute dependency, so keep the redistributable blob in-tree
# and fail ISO creation if it is absent.
GUC_FW_HOST_PATH ?= firmware/i915/tgl_guc_70.bin
GUC_FW_ISO_REL_PATH ?= EFI/BOOT/tgl_guc_70.bin
HORIZON_BP_HOST_PATH ?= ../TRUEOS-Blueprints/dist/horizon.bp
HORIZON_BP_ISO_REL_PATH ?= EFI/BOOT/apps/horizon.bp
WEAVE_HELLO_BP_HOST_PATH ?= ../TRUEOS-Blueprints/dist/weave_hello.bp
WEAVE_HELLO_BP_ISO_REL_PATH ?= EFI/BOOT/apps/weave_hello.bp
BLUEPRINTS_DIR ?= ../TRUEOS-Blueprints
BUILDIN_MANIFEST := $(BLUEPRINTS_DIR)/buildins.json
BUILDIN_APP_NAMES := $(shell if [ -f "$(BUILDIN_MANIFEST)" ]; then python3 -c 'import json, sys; print(*json.load(open(sys.argv[1]))["buildins"])' "$(BUILDIN_MANIFEST)" 2>/dev/null; fi)
BUILDIN_BP_FILES := $(addprefix $(BLUEPRINTS_DIR)/dist/,$(addsuffix .bp,$(BUILDIN_APP_NAMES)))
BUILDIN_COMMON_INPUTS := $(shell if [ -d "$(BLUEPRINTS_DIR)" ]; then find "$(BLUEPRINTS_DIR)/src" "$(BLUEPRINTS_DIR)/api" "$(BLUEPRINTS_DIR)/.cargo" -type f 2>/dev/null; fi) $(wildcard $(BLUEPRINTS_DIR)/Cargo.toml $(BLUEPRINTS_DIR)/rust-toolchain.toml $(BLUEPRINTS_DIR)/apps.json)
ENABLE_BLUEPRINTS ?= 0
ENABLE_WEAVE_HELLO ?= 0
# When enabled, install FirmwareScout.efi as BOOTX64.EFI (preserving the
# original Limine loader as LIMINE.EFI) on both the TFTP boot tree and the
# embedded EFI System Partition image, so the physical test rig actually
# chainloads through FirmwareScout instead of Limine directly.
ENABLE_FIRMWARE_SCOUT ?= 0
FIRMWARE_SCOUT_BUILD_SCRIPT := tools/firmware-scout/build.sh
FIRMWARE_SCOUT_STAGE_TREE_SCRIPT := tools/firmware-scout/stage-tree.sh
FIRMWARE_SCOUT_STAGE_EFI_IMAGE_SCRIPT := tools/firmware-scout/stage-efi-image.sh
FIRMWARE_SCOUT_EFI := bld/firmware-scout-target/x86_64-unknown-uefi/release/trueos-firmware-scout.efi
QEMU_RUNNER := tools/qemu/run.sh
QEMU_BIN ?= qemu-system-x86_64
QEMU_MEMORY ?= 12000M
QEMU_SMP ?= 14
QEMU_NIC_DEVICE ?= virtio-net-pci,disable-modern=off
QEMU_SERIAL ?= tcp:127.0.0.1:5555,server,nowait
QEMU_UEFI_FIRMWARE = $(OVMF_BUNDLE_PATH)
NVME_IMG := tools/nvme.img
CNT_FILE := tools/cnt
QEMU_BRIDGE ?= br0
QEMU_BRIDGE_HELPER ?= $(firstword $(wildcard /usr/lib/qemu/qemu-bridge-helper /usr/libexec/qemu-bridge-helper /usr/lib/qemu-bridge-helper))
QEMU_HDA_AUDIODEV ?= none,id=snd0
QEMU_HOST_TCP_PORT_3 ?= 10003
QEMU_HOST_TCP_PORT_4 ?= 10004
QEMU_HOST_TCP_PORT_100 ?= 10100
QEMU_HOST_TCP_PORT_80 ?= 8080
QEMU_HOST_TCP_PORT_54321 ?= 15432
QEMU_HOST_TCP_PORT_32123 ?= 32123
QEMU_HOST_TCP_PORT_NET_SHELL ?= 14245
QEMU_HOST_UDP_PORT_32343 ?= 32343
QEMU_RUN_ENV = ISO_PATH="$(ISO_PATH)" QEMU_BIN="$(QEMU_BIN)" QEMU_MEMORY="$(QEMU_MEMORY)" QEMU_SMP="$(QEMU_SMP)" QEMU_NIC_DEVICE="$(QEMU_NIC_DEVICE)" QEMU_SERIAL="$(QEMU_SERIAL)" QEMU_UEFI_FIRMWARE="$(QEMU_UEFI_FIRMWARE)" QEMU_NVME_IMG="$(NVME_IMG)" QEMU_BRIDGE="$(QEMU_BRIDGE)" QEMU_BRIDGE_HELPER="$(QEMU_BRIDGE_HELPER)" QEMU_HDA_AUDIODEV="$(QEMU_HDA_AUDIODEV)" QEMU_HOST_TCP_PORT_3="$(QEMU_HOST_TCP_PORT_3)" QEMU_HOST_TCP_PORT_4="$(QEMU_HOST_TCP_PORT_4)" QEMU_HOST_TCP_PORT_100="$(QEMU_HOST_TCP_PORT_100)" QEMU_HOST_TCP_PORT_80="$(QEMU_HOST_TCP_PORT_80)" QEMU_HOST_TCP_PORT_54321="$(QEMU_HOST_TCP_PORT_54321)" QEMU_HOST_TCP_PORT_32123="$(QEMU_HOST_TCP_PORT_32123)" QEMU_HOST_TCP_PORT_NET_SHELL="$(QEMU_HOST_TCP_PORT_NET_SHELL)" QEMU_HOST_UDP_PORT_32343="$(QEMU_HOST_UDP_PORT_32343)"
BAREMETAL_LOG_DRAIN := tools/baremetal-log-drain.sh
TESTRIG_PHYSICAL_RESET_HELPER := tools/testrig-physical-reset-button.py
BAREMETAL_LOG_HOST ?= 192.168.178.94
BAREMETAL_LOG_PORT ?= 1
BAREMETAL_LOG_DELAY ?= 5
BAREMETAL_LOG_RETRY_DELAY ?= 1
BAREMETAL_LOG_DIR ?= bld/baremetal-logs
BAREMETAL_LOG_SLOTS ?= 3
BAREMETAL_LOG_WAIT_TIMEOUT ?= 180
BAREMETAL_BOOT_MARKER ?= [service] [important] spawn-svc: started net-shell-listener
# Test-rig reset contract: the ESP32 sends the bytes "probe" to host UDP/7777.
# Replying "ack" to ESP32 UDP/7777 latches its physical reset-button circuit.
# This hardware button is the normal reboot path; it is not a Shell2 command.
TESTRIG_PHYSICAL_RESET_BIND_HOST ?= 0.0.0.0
TESTRIG_PHYSICAL_RESET_PORT ?= 7777
TESTRIG_PHYSICAL_RESET_RESPONSE_PORT ?= $(TESTRIG_PHYSICAL_RESET_PORT)
TESTRIG_PHYSICAL_RESET_PROBE_TIMEOUT ?= 30
BAREMETAL_TFTP_READ_TIMEOUT ?= 240
BAREMETAL_TFTP_VERIFY ?= 1
BAREMETAL_TFTP_BOOTFILE ?= $(ISO_DIR)/EFI/BOOT/BOOTX64.EFI
BAREMETAL_TFTP_KERNEL ?= $(ISO_DIR)/TRUEOS.elf
TESTRIG_PHYSICAL_RESET_RECEIPT ?= $(ISO_DIR)/testrig-physical-reset-receipt.json
EMULATOR_LOG_CAPTURE := tools/emulator-log-capture.sh
EMULATOR_LOG_DIR ?= bld/emulator-logs
EMULATOR_LOG_SLOTS ?= 3

CARGO_BUILD_FLAGS ?=
CARGO_GFX_FLAGS =
TRUEOS_TTSTT_HOST_TARGET ?= $(shell rustc -vV | sed -n 's/^host: //p')
TRUEOS_TTSTT_TARGET_DIR ?= tools/trueos-ttstt/target

INTEL_GPU_BAKERY_DIR := tools/intel-gpu-bakery
INTEL_GPU_CPP_ARTIFACT_DIR := crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp
INTEL_GPU_CPP_COPY_BIN := $(INTEL_GPU_CPP_ARTIFACT_DIR)/copy_rect_rgba8.bin
INTEL_GPU_CPP_REQUIRED_BINS := $(filter-out $(INTEL_GPU_CPP_COPY_BIN),$(wildcard $(INTEL_GPU_CPP_ARTIFACT_DIR)/*.bin))
INTEL_GPU_BAKERY_PYTHON ?= python3
INTEL_GPU_LINKED_ELF ?= $(KERNEL_BIN)
INTEL_GPU_CPP_PROBE_LOG ?=
AARCH64_KERNEL_BAKERY_DIR := tools/aarch64-kernel-bakery
AARCH64_KERNEL_ARTIFACT_DIR ?= bld/aarch64-kernel-artifacts
AARCH64_KERNEL_PYTHON ?= python3
AARCH64_KERNEL_CLANG ?= clang
INTEL_GPU_ARTIFACT_FRONTEND := cpp-for-opencl
INTEL_GPU_SELECTED_COPY_BIN := $(INTEL_GPU_CPP_COPY_BIN)
INTEL_GPU_PREBUILD_VERIFY := intel-gpu-verify-cpp-artifacts picasso-verify-artifacts
CARGO_EFFECTIVE_FLAGS = $(strip $(CARGO_BUILD_FLAGS))

IMG_SIZE ?= 25G

.PHONY: images empty-libs buildins kernel trueos-ttstt-host trueos-ttstt-ubuntu cpp aarch64-kernels aarch64-kernel-copy aarch64-kernel-verify aarch64-kernel-test lfm25-cpp lfm25-cpp-verify lfm25-packed-isa-verify lfm25-igpu-packed-verify intel-gpu-bake-migrated-cpp intel-gpu-bake-copy-cpp intel-gpu-bake-cpp-demo intel-gpu-bake-audio-visualizer-cpp intel-gpu-bake-particle-craft-cpp intel-gpu-bake-shadertoy-cpp intel-gpu-bake-font-instance-cpp intel-gpu-bake-lfm25-q8-packed-cpp intel-gpu-bake-spirit-cpp intel-gpu-bake-subset-sum-cpp intel-gpu-bake-cpp-artifacts intel-gpu-refresh-cpp-artifacts intel-gpu-verify-cpp-artifacts intel-gpu-verify-copy-cpp intel-gpu-verify-copy-cpp-hardware-log intel-gpu-verify-linked-copy intel-gpu-verify-linked-copy-cpp intel-gpu-verify-packaged-copy intel-gpu-verify-packaged-copy-cpp helio-build-simple-cube helio-build-churn-forward helio-build-gbuffer picasso-refresh-artifacts picasso-verify-artifacts helio-refresh-artifacts helio-verify-artifacts artifacts limine testrig-physical-reset-log baremetal-reboot-log iso provenance-git-clean provenance verify-provenance release-git-clean release-count release dbg run

images: $(NVME_IMG)

$(NVME_IMG):
	mkdir -p $(@D)
	truncate -s $(IMG_SIZE) $@

empty-libs:
	mkdir -p $(KERNEL_EMPTY_LIB_DIR)
	rm -f $(KERNEL_EMPTY_LIB_DIR)/empty.o
	ar crs $(KERNEL_EMPTY_LIB_DIR)/libc.a
	ar crs $(KERNEL_EMPTY_LIB_DIR)/libgcc_s.a

define BUILDIN_APP_RULE
$(BLUEPRINTS_DIR)/dist/$(1).bp: $(BUILDIN_MANIFEST) $(BUILDIN_COMMON_INPUTS) $(shell if [ -d "$(BLUEPRINTS_DIR)/buildins/$(1)" ]; then find "$(BLUEPRINTS_DIR)/buildins/$(1)" -type f 2>/dev/null; fi)
	@test -f "$(BUILDIN_MANIFEST)" || { echo "error: Blueprint build-in manifest not found at $(BUILDIN_MANIFEST)"; exit 1; }
	cd "$(BLUEPRINTS_DIR)" && TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp "$(1)"
endef

$(foreach app,$(BUILDIN_APP_NAMES),$(eval $(call BUILDIN_APP_RULE,$(app))))

buildins: $(BUILDIN_BP_FILES)
	@test -n "$(BUILDIN_APP_NAMES)" || { echo "error: no Blueprint build-ins found in $(BUILDIN_MANIFEST)"; exit 1; }
	@echo "buildins: ready $(BUILDIN_APP_NAMES)"

# Host-only compatibility utility. It is deliberately absent from the normal
# TRUEOS image, kernel, and release dependency graphs.
trueos-ttstt-host:
	cd "$${TMPDIR:-/tmp}" && cargo build --manifest-path "$(abspath tools/trueos-ttstt/Cargo.toml)" --release --locked --target "$(TRUEOS_TTSTT_HOST_TARGET)" --target-dir "$(abspath $(TRUEOS_TTSTT_TARGET_DIR))"

trueos-ttstt-ubuntu:
	$(MAKE) --no-print-directory trueos-ttstt-host TRUEOS_TTSTT_HOST_TARGET=x86_64-unknown-linux-gnu

kernel: buildins empty-libs $(INTEL_GPU_PREBUILD_VERIFY)
	TRUEOS_BLUEPRINTS_DIR="$(abspath $(BLUEPRINTS_DIR))" TRUEOS_REQUIRE_BUILDINS=1 cargo build $(CARGO_GFX_FLAGS) $(CARGO_EFFECTIVE_FLAGS) -Z build-std=core,compiler_builtins,alloc,panic_abort -Z json-target-spec --target .cargo/x86_64-unknown-trueos.json
	$(MAKE) --no-print-directory INTEL_GPU_LINKED_ELF="$(KERNEL_BIN)" intel-gpu-verify-linked-copy

helio-build-simple-cube:
	tools/helio-build/build-simple-cube.sh

helio-build-churn-forward:
	tools/helio-build/build-churn-forward.sh

helio-build-gbuffer:
	tools/helio-build/build-gbuffer.sh

picasso-refresh-artifacts: helio-build-simple-cube helio-build-churn-forward helio-build-gbuffer

picasso-verify-artifacts:
	tools/helio-build/build-simple-cube.sh --validate-only
	tools/helio-build/build-churn-forward.sh --validate-only
	tools/helio-build/build-gbuffer.sh --validate-only

# The capture utilities remain Helio-named; retain these aliases for local
# workflows while the build and asset contract is Picasso-owned.
helio-refresh-artifacts: picasso-refresh-artifacts

helio-verify-artifacts: picasso-verify-artifacts

intel-gpu-bake-migrated-cpp:
	PYTHON="$(INTEL_GPU_BAKERY_PYTHON)" "$(INTEL_GPU_BAKERY_DIR)/bake_adls_cpp_migrated.sh"

intel-gpu-bake-copy-cpp:
	PYTHON="$(INTEL_GPU_BAKERY_PYTHON)" "$(INTEL_GPU_BAKERY_DIR)/bake_adls_cpp_copy_rect.sh"

intel-gpu-bake-cpp-demo:
	PYTHON="$(INTEL_GPU_BAKERY_PYTHON)" "$(INTEL_GPU_BAKERY_DIR)/bake_adls_cpp_demo.sh"

intel-gpu-bake-audio-visualizer-cpp:
	PYTHON="$(INTEL_GPU_BAKERY_PYTHON)" "$(INTEL_GPU_BAKERY_DIR)/bake_adls_cpp_audio_visualizer.sh"

intel-gpu-bake-particle-craft-cpp:
	PYTHON="$(INTEL_GPU_BAKERY_PYTHON)" "$(INTEL_GPU_BAKERY_DIR)/bake_adls_cpp_particle_craft.sh"

intel-gpu-bake-shadertoy-cpp:
	PYTHON="$(INTEL_GPU_BAKERY_PYTHON)" "$(INTEL_GPU_BAKERY_DIR)/bake_adls_cpp_shadertoy.sh"

intel-gpu-bake-font-instance-cpp:
	PYTHON="$(INTEL_GPU_BAKERY_PYTHON)" "$(INTEL_GPU_BAKERY_DIR)/bake_adls_cpp_font_instance.sh"

intel-gpu-bake-lfm25-q8-packed-cpp:
	PYTHON="$(INTEL_GPU_BAKERY_PYTHON)" "$(INTEL_GPU_BAKERY_DIR)/bake_adls_cpp_lfm25_q8_packed.sh"

intel-gpu-bake-kokoro-conv1d-cpp:
	PYTHON="$(INTEL_GPU_BAKERY_PYTHON)" "$(INTEL_GPU_BAKERY_DIR)/bake_adls_cpp_kokoro_conv1d.sh"

intel-gpu-bake-kokoro-qgemm-cpp:
	PYTHON="$(INTEL_GPU_BAKERY_PYTHON)" bash "$(INTEL_GPU_BAKERY_DIR)/bake_adls_cpp_kokoro_qgemm.sh"

intel-gpu-bake-spirit-cpp:
	PYTHON="$(INTEL_GPU_BAKERY_PYTHON)" "$(INTEL_GPU_BAKERY_DIR)/bake_adls_cpp_spirit.sh"

intel-gpu-bake-subset-sum-cpp:
	PYTHON="$(INTEL_GPU_BAKERY_PYTHON)" "$(INTEL_GPU_BAKERY_DIR)/bake_adls_cpp_subset_sum.sh"

intel-gpu-bake-cpp-artifacts:
	$(MAKE) --no-print-directory intel-gpu-bake-migrated-cpp
	$(MAKE) --no-print-directory intel-gpu-bake-copy-cpp
	$(MAKE) --no-print-directory intel-gpu-bake-cpp-demo
	$(MAKE) --no-print-directory intel-gpu-bake-audio-visualizer-cpp
	$(MAKE) --no-print-directory intel-gpu-bake-particle-craft-cpp
	$(MAKE) --no-print-directory intel-gpu-bake-shadertoy-cpp
	$(MAKE) --no-print-directory intel-gpu-bake-font-instance-cpp
	$(MAKE) --no-print-directory intel-gpu-bake-lfm25-q8-packed-cpp
	$(MAKE) --no-print-directory intel-gpu-bake-kokoro-conv1d-cpp
	$(MAKE) --no-print-directory intel-gpu-bake-kokoro-qgemm-cpp
	$(MAKE) --no-print-directory intel-gpu-bake-spirit-cpp
	$(MAKE) --no-print-directory intel-gpu-bake-subset-sum-cpp

intel-gpu-refresh-cpp-artifacts: intel-gpu-bake-cpp-artifacts
	$(MAKE) --no-print-directory intel-gpu-verify-cpp-artifacts

cpp: intel-gpu-refresh-cpp-artifacts

aarch64-kernel-copy:
	PYTHON="$(AARCH64_KERNEL_PYTHON)" ARM_CLANG="$(AARCH64_KERNEL_CLANG)" ARM_KERNEL_PUBLISH_DIR="$(abspath $(AARCH64_KERNEL_ARTIFACT_DIR))" "$(AARCH64_KERNEL_BAKERY_DIR)/bake_copy_rect.sh"

aarch64-kernel-verify:
	$(AARCH64_KERNEL_PYTHON) -B "$(AARCH64_KERNEL_BAKERY_DIR)/verify.py" --artifact-dir "$(AARCH64_KERNEL_ARTIFACT_DIR)"

aarch64-kernel-test:
	$(AARCH64_KERNEL_PYTHON) -B -m unittest discover -s "$(AARCH64_KERNEL_BAKERY_DIR)" -p 'test_*.py'

aarch64-kernels: aarch64-kernel-copy aarch64-kernel-verify

lfm25-cpp:
	./tools/lfm2.5-350m/build_cpp.sh

lfm25-cpp-verify:
	./tools/lfm2.5-350m/verify_cpp.sh

lfm25-packed-isa-verify:
	python3 ./tools/lfm2.5-350m/verify_packed_isa.py

lfm25-igpu-packed-verify:
	./tools/lfm2.5-350m/verify_igpu_packed.sh

intel-gpu-verify-cpp-artifacts: lfm25-packed-isa-verify
	$(INTEL_GPU_BAKERY_PYTHON) -B "$(INTEL_GPU_BAKERY_DIR)/verify.py" --artifact-dir "$(INTEL_GPU_CPP_ARTIFACT_DIR)"
	$(INTEL_GPU_BAKERY_PYTHON) -B -m unittest discover -s "$(INTEL_GPU_BAKERY_DIR)" -p 'test_*.py'

intel-gpu-verify-copy-cpp: intel-gpu-verify-cpp-artifacts

intel-gpu-verify-copy-cpp-hardware-log:
	@test -n "$(INTEL_GPU_CPP_PROBE_LOG)" || { \
		echo "error: set INTEL_GPU_CPP_PROBE_LOG=/path/to/copy-rect-probe.log"; \
		exit 2; \
	}
	$(INTEL_GPU_BAKERY_PYTHON) -B "$(INTEL_GPU_BAKERY_DIR)/verify_probe_log.py" "$(INTEL_GPU_CPP_PROBE_LOG)"

intel-gpu-verify-linked-copy:
	$(INTEL_GPU_BAKERY_PYTHON) -B "$(INTEL_GPU_BAKERY_DIR)/verify_linked.py" --elf "$(INTEL_GPU_LINKED_ELF)" --selected-bin "$(INTEL_GPU_SELECTED_COPY_BIN)" $(foreach bin,$(INTEL_GPU_CPP_REQUIRED_BINS),--required-bin "$(bin)")

intel-gpu-verify-linked-copy-cpp:
	$(MAKE) --no-print-directory INTEL_GPU_LINKED_ELF="$(INTEL_GPU_LINKED_ELF)" intel-gpu-verify-linked-copy

intel-gpu-verify-packaged-copy:
	$(INTEL_GPU_BAKERY_PYTHON) -B "$(INTEL_GPU_BAKERY_DIR)/verify_packaged.py" --runtime-elf "$(ARTIFACT_RUNTIME_ELF)" --staged-elf "$(ISO_BOOT_DIR)/TRUEOS.elf" --iso "$(ISO_PATH)" --selected-bin "$(INTEL_GPU_SELECTED_COPY_BIN)" $(foreach bin,$(INTEL_GPU_CPP_REQUIRED_BINS),--required-bin "$(bin)")

intel-gpu-verify-packaged-copy-cpp:
	$(MAKE) --no-print-directory ARTIFACT_DIR="$(ARTIFACT_DIR)" ISO_BOOT_DIR="$(ISO_BOOT_DIR)" ISO_PATH="$(ISO_PATH)" intel-gpu-verify-packaged-copy

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
		printf "cargo_build_flags=%s\n" "$(CARGO_EFFECTIVE_FLAGS)"; \
		printf "intel_gpu_kernel_architecture=cpp-for-opencl\n"; \
		printf "intel_gpu_artifact_frontend=%s\n" "$(INTEL_GPU_ARTIFACT_FRONTEND)"; \
		printf "intel_gpu_copy_artifact=%s\n" "$(INTEL_GPU_SELECTED_COPY_BIN)"; \
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

# Compatibility entry point for older invocations.
baremetal-reboot-log: testrig-physical-reset-log

testrig-physical-reset-log:
	@set -eu; \
	case "$(BAREMETAL_TFTP_VERIFY)" in \
		0|1) ;; \
		*) echo "error: BAREMETAL_TFTP_VERIFY must be 0 or 1, got '$(BAREMETAL_TFTP_VERIFY)'" >&2; exit 2 ;; \
	esac; \
	for required in \
		"$(ARTIFACT_RUNTIME_ELF)" \
		"$(ISO_PATH)" \
		"$(LIMINE_BOOTX64)" \
		"$(BAREMETAL_TFTP_BOOTFILE)" \
		"$(BAREMETAL_TFTP_KERNEL)"; do \
		test -f "$$required" || { echo "error: baremetal deploy input is missing: $$required" >&2; exit 1; }; \
	done; \
	runtime_sha=$$(sha256sum "$(ARTIFACT_RUNTIME_ELF)" | cut -d ' ' -f1); \
	iso_sha=$$(sha256sum "$(ISO_PATH)" | cut -d ' ' -f1); \
	bootfile_sha=$$(sha256sum "$(BAREMETAL_TFTP_BOOTFILE)" | cut -d ' ' -f1); \
	TRUEOS_BAREMETAL_LOG_HOST="$(BAREMETAL_LOG_HOST)" \
	TRUEOS_BAREMETAL_LOG_PORT="$(BAREMETAL_LOG_PORT)" \
	TRUEOS_BAREMETAL_LOG_DELAY="$(BAREMETAL_LOG_DELAY)" \
	TRUEOS_BAREMETAL_LOG_RETRY_DELAY="$(BAREMETAL_LOG_RETRY_DELAY)" \
	TRUEOS_BAREMETAL_LOG_DIR="$(BAREMETAL_LOG_DIR)" \
	TRUEOS_BAREMETAL_LOG_SLOTS="$(BAREMETAL_LOG_SLOTS)" \
	TRUEOS_BAREMETAL_LOG_WAIT_TIMEOUT="$(BAREMETAL_LOG_WAIT_TIMEOUT)" \
	TRUEOS_BAREMETAL_BOOT_MARKER="$(BAREMETAL_BOOT_MARKER)" \
	"$(BAREMETAL_LOG_DRAIN)" stop; \
	rm -f -- "$(TESTRIG_PHYSICAL_RESET_RECEIPT)"; \
	if [ "$(BAREMETAL_TFTP_VERIFY)" = "1" ]; then \
		python3 "$(TESTRIG_PHYSICAL_RESET_HELPER)" \
			--bind-host "$(TESTRIG_PHYSICAL_RESET_BIND_HOST)" \
			--listen-port "$(TESTRIG_PHYSICAL_RESET_PORT)" \
			--response-port "$(TESTRIG_PHYSICAL_RESET_RESPONSE_PORT)" \
			--probe-timeout "$(TESTRIG_PHYSICAL_RESET_PROBE_TIMEOUT)" \
			--tftp-timeout "$(BAREMETAL_TFTP_READ_TIMEOUT)" \
			--watch "$(BAREMETAL_TFTP_BOOTFILE)=$$bootfile_sha" \
			--watch "$(BAREMETAL_TFTP_KERNEL)=$$runtime_sha" \
			--identity "runtime_elf_sha256=$$runtime_sha" \
			--identity "iso_sha256=$$iso_sha" \
			--receipt "$(TESTRIG_PHYSICAL_RESET_RECEIPT)"; \
	else \
		echo "testrig-physical-reset-log: PXE read verification explicitly disabled (BAREMETAL_TFTP_VERIFY=0)"; \
		python3 "$(TESTRIG_PHYSICAL_RESET_HELPER)" \
			--bind-host "$(TESTRIG_PHYSICAL_RESET_BIND_HOST)" \
			--listen-port "$(TESTRIG_PHYSICAL_RESET_PORT)" \
			--response-port "$(TESTRIG_PHYSICAL_RESET_RESPONSE_PORT)" \
			--probe-timeout "$(TESTRIG_PHYSICAL_RESET_PROBE_TIMEOUT)" \
			--identity "runtime_elf_sha256=$$runtime_sha" \
			--identity "iso_sha256=$$iso_sha" \
			--receipt "$(TESTRIG_PHYSICAL_RESET_RECEIPT)"; \
	fi; \
	TRUEOS_BAREMETAL_LOG_HOST="$(BAREMETAL_LOG_HOST)" \
	TRUEOS_BAREMETAL_LOG_PORT="$(BAREMETAL_LOG_PORT)" \
	TRUEOS_BAREMETAL_LOG_DELAY="$(BAREMETAL_LOG_DELAY)" \
	TRUEOS_BAREMETAL_LOG_RETRY_DELAY="$(BAREMETAL_LOG_RETRY_DELAY)" \
	TRUEOS_BAREMETAL_LOG_DIR="$(BAREMETAL_LOG_DIR)" \
	TRUEOS_BAREMETAL_LOG_SLOTS="$(BAREMETAL_LOG_SLOTS)" \
	TRUEOS_BAREMETAL_LOG_WAIT_TIMEOUT="$(BAREMETAL_LOG_WAIT_TIMEOUT)" \
	TRUEOS_BAREMETAL_BOOT_MARKER="$(BAREMETAL_BOOT_MARKER)" \
	TRUEOS_BAREMETAL_EXPECTED_ELF_SHA256="$$runtime_sha" \
	TRUEOS_BAREMETAL_EXPECTED_ISO_SHA256="$$iso_sha" \
	TRUEOS_TESTRIG_PHYSICAL_RESET_RECEIPT="$(TESTRIG_PHYSICAL_RESET_RECEIPT)" \
	"$(BAREMETAL_LOG_DRAIN)" start; \
	TRUEOS_BAREMETAL_LOG_HOST="$(BAREMETAL_LOG_HOST)" \
	TRUEOS_BAREMETAL_LOG_PORT="$(BAREMETAL_LOG_PORT)" \
	TRUEOS_BAREMETAL_LOG_DELAY="$(BAREMETAL_LOG_DELAY)" \
	TRUEOS_BAREMETAL_LOG_RETRY_DELAY="$(BAREMETAL_LOG_RETRY_DELAY)" \
	TRUEOS_BAREMETAL_LOG_DIR="$(BAREMETAL_LOG_DIR)" \
	TRUEOS_BAREMETAL_LOG_SLOTS="$(BAREMETAL_LOG_SLOTS)" \
	TRUEOS_BAREMETAL_LOG_WAIT_TIMEOUT="$(BAREMETAL_LOG_WAIT_TIMEOUT)" \
	TRUEOS_BAREMETAL_BOOT_MARKER="$(BAREMETAL_BOOT_MARKER)" \
	"$(BAREMETAL_LOG_DRAIN)" wait; \
	echo "testrig-physical-reset-log: verified runtime_elf_sha256=$$runtime_sha iso_sha256=$$iso_sha receipt=$(TESTRIG_PHYSICAL_RESET_RECEIPT)"

FORCE:

iso: artifacts images limine
	rm -rf $(ISO_BOOT_DIR)
	rm -f $(ISO_PATH)
	mkdir -p $(ISO_BOOT_DIR)
	cp $(ARTIFACT_RUNTIME_ELF) $(ISO_BOOT_DIR)/TRUEOS.elf
	mkdir -p $(ISO_DIR)/EFI/BOOT
	cp $(LIMINE_BOOTX64) $(ISO_DIR)/EFI/BOOT/BOOTX64.EFI
	@if [ "$(ENABLE_FIRMWARE_SCOUT)" = "1" ]; then \
		"$(FIRMWARE_SCOUT_BUILD_SCRIPT)"; \
		"$(FIRMWARE_SCOUT_STAGE_TREE_SCRIPT)" "$(ISO_DIR)/EFI/BOOT" "$(FIRMWARE_SCOUT_EFI)"; \
		boot_sha=$$(sha256sum "$(ISO_DIR)/EFI/BOOT/BOOTX64.EFI" | cut -d' ' -f1); \
		limine_sha=$$(sha256sum "$(ISO_DIR)/EFI/BOOT/LIMINE.EFI" | cut -d' ' -f1); \
		if [ "$$boot_sha" = "$$limine_sha" ]; then \
			echo "error: FirmwareScout staging left $(ISO_DIR)/EFI/BOOT/BOOTX64.EFI identical to LIMINE.EFI" >&2; \
			exit 1; \
		fi; \
		echo "iso: FirmwareScout staged TFTP tree BOOTX64.EFI=$$boot_sha LIMINE.EFI=$$limine_sha"; \
	else \
		echo "iso: skipping FirmwareScout staging (ENABLE_FIRMWARE_SCOUT=0)"; \
	fi
	@if [ ! -f "$(GUC_FW_HOST_PATH)" ]; then \
		echo "error: required GuC firmware not found at $(GUC_FW_HOST_PATH)"; \
		exit 1; \
	fi
	mkdir -p "$(ISO_DIR)/$(dir $(GUC_FW_ISO_REL_PATH))"
	@case "$(GUC_FW_HOST_PATH)" in \
		*.zst) \
			command -v zstd >/dev/null 2>&1 || { echo "error: zstd command not found; cannot unpack $(GUC_FW_HOST_PATH)"; exit 1; }; \
			zstd -q -d -c "$(GUC_FW_HOST_PATH)" > "$(ISO_DIR)/$(GUC_FW_ISO_REL_PATH)"; \
			;; \
		*) \
			cp "$(GUC_FW_HOST_PATH)" "$(ISO_DIR)/$(GUC_FW_ISO_REL_PATH)"; \
			;; \
	esac
	mkdir -p "$(ISO_BOOT_DIR)/$(dir $(GUC_FW_ISO_REL_PATH))"
	cp "$(ISO_DIR)/$(GUC_FW_ISO_REL_PATH)" "$(ISO_BOOT_DIR)/$(GUC_FW_ISO_REL_PATH)"
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
	@if [ "$(ENABLE_WEAVE_HELLO)" = "1" ]; then \
		if [ ! -f "$(WEAVE_HELLO_BP_HOST_PATH)" ]; then \
			echo "error: Weave hello blueprint not found at $(WEAVE_HELLO_BP_HOST_PATH)"; \
			echo "       run: cd ../TRUEOS-Blueprints && cargo bp weave_hello"; \
			exit 1; \
		fi; \
		mkdir -p "$(ISO_BOOT_DIR)/$(dir $(WEAVE_HELLO_BP_ISO_REL_PATH))"; \
		cp "$(WEAVE_HELLO_BP_HOST_PATH)" "$(ISO_BOOT_DIR)/$(WEAVE_HELLO_BP_ISO_REL_PATH)"; \
		mkdir -p "$(ISO_DIR)/$(dir $(WEAVE_HELLO_BP_ISO_REL_PATH))"; \
		cp "$(WEAVE_HELLO_BP_HOST_PATH)" "$(ISO_DIR)/$(WEAVE_HELLO_BP_ISO_REL_PATH)"; \
	else \
		echo "iso: skipping Weave hello Blueprint (ENABLE_WEAVE_HELLO=0)"; \
	fi
	cp "$(LIMINE_CFG)" "$(LIMINE_CFG_GENERATED)"
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
	@if [ "$(ENABLE_WEAVE_HELLO)" = "1" ]; then \
		printf '%s\n%s\n' \
			"module_path: boot():/$(WEAVE_HELLO_BP_ISO_REL_PATH)" \
			"module_string: trueos.app.weave_hello" \
			>> "$(LIMINE_CFG_GENERATED)"; \
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
	@if [ "$(ENABLE_FIRMWARE_SCOUT)" = "1" ]; then \
		"$(FIRMWARE_SCOUT_STAGE_EFI_IMAGE_SCRIPT)" "$(ISO_BOOT_DIR)/$(ISO_EFI_IMG)" "$(FIRMWARE_SCOUT_EFI)"; \
	else \
		echo "iso: skipping FirmwareScout staging of $(ISO_EFI_IMG) (ENABLE_FIRMWARE_SCOUT=0)"; \
	fi
	cp $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) $(ISO_DIR)/$(ISO_EFI_IMG)
	xorriso -as mkisofs \
		-iso-level 3 -full-iso9660-filenames \
		-R \
		-r \
		-J -joliet-long \
		-e $(ISO_EFI_IMG) -no-emul-boot \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		-o $(ISO_PATH) $(ISO_BOOT_DIR)
	$(MAKE) --no-print-directory ARTIFACT_DIR="$(ARTIFACT_DIR)" ISO_BOOT_DIR="$(ISO_BOOT_DIR)" ISO_PATH="$(ISO_PATH)" intel-gpu-verify-packaged-copy
	rm -f $(ISO_ARCHIVE)
	cd $(ISO_DIR) && 7z a -t7z $(UPDATE_7Z_FLAGS) $(notdir $(ISO_ARCHIVE)) $(notdir $(ISO_PATH))
	@if [ "$(PUBLISH_RELEASE_SMB)" = "1" ]; then \
		env -u GIO_MODULE_DIR gio mount smb://t4ce@pdjb/home-share || true; \
		env -u GIO_MODULE_DIR gio copy $(ISO_ARCHIVE) smb://t4ce@pdjb/home-share/TRUEOS_SITE/; \
	else \
		echo "iso: skipping SMB publish (PUBLISH_RELEASE_SMB=$(PUBLISH_RELEASE_SMB))"; \
	fi
	@case "$(START_BAREMETAL_LOG)" in \
		1) \
			mkdir -p "$(BAREMETAL_LOG_DIR)"; \
			setsid -f $(MAKE) --no-print-directory testrig-physical-reset-log \
				</dev/null >"$(BAREMETAL_LOG_DIR)/testrig-physical-reset-log.log" 2>&1; \
			echo "iso: dispatched baremetal reset/log verification (output=$(BAREMETAL_LOG_DIR)/testrig-physical-reset-log.log)" \
			;; \
		0) echo "iso: skipping baremetal deploy/log verification (START_BAREMETAL_LOG=0)" ;; \
		*) echo "error: START_BAREMETAL_LOG must be 0 or 1, got '$(START_BAREMETAL_LOG)'" >&2; exit 2 ;; \
	esac

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

release-count:
	@mkdir -p "$$(dirname "$(CNT_FILE)")"; \
	count=$$(cat "$(CNT_FILE)" 2>/dev/null || echo 0); \
	count=$${count:-0}; \
	next=$$((count + 1)); \
	printf '%s\n' "$$next" > "$(CNT_FILE)"; \
	printf 'release counter: %s -> %s (%s)\n' "$$count" "$$next" "$(CNT_FILE)"

release: BUILD_MODE := release
release: CARGO_BUILD_FLAGS += --release
release: release-git-clean
	@if [ "$(RELEASE_BUMP_CNT)" = "1" ]; then \
		$(MAKE) --no-print-directory release-count; \
	else \
		echo "release: using committed counter (RELEASE_BUMP_CNT=$(RELEASE_BUMP_CNT))"; \
	fi
	@if [ "$(RELEASE_BUMP_CNT)" = "1" ]; then \
		$(MAKE) --no-print-directory BUILD_MODE="$(BUILD_MODE)" CARGO_BUILD_FLAGS="$(CARGO_BUILD_FLAGS)" PUBLISH_RELEASE_SMB=0 START_BAREMETAL_LOG=0 PROVENANCE_CLEAN_FLAG=--allow-dirty PROVENANCE_SOURCE_MANIFEST=git-index provenance; \
	else \
		$(MAKE) --no-print-directory BUILD_MODE="$(BUILD_MODE)" CARGO_BUILD_FLAGS="$(CARGO_BUILD_FLAGS)" PUBLISH_RELEASE_SMB=0 START_BAREMETAL_LOG=0 PROVENANCE_CLEAN_FLAG=--require-clean PROVENANCE_SOURCE_MANIFEST=git-commit provenance; \
	fi
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

run: START_BAREMETAL_LOG=0
run: iso
	@killall -9 qemu-system-x86_64 || true
	@$(QEMU_RUN_ENV) $(QEMU_RUNNER) iso & qemu_pid=$$!; \
		trap 'kill "$$qemu_pid" 2>/dev/null || true; exit 130' INT TERM; \
		if ! TRUEOS_EMULATOR_LOG_DIR="$(EMULATOR_LOG_DIR)" TRUEOS_EMULATOR_LOG_SLOTS="$(EMULATOR_LOG_SLOTS)" "$(EMULATOR_LOG_CAPTURE)" "$$qemu_pid"; then \
			kill "$$qemu_pid" 2>/dev/null || true; \
			wait "$$qemu_pid" 2>/dev/null || true; \
			exit 1; \
		fi; \
		wait "$$qemu_pid"
