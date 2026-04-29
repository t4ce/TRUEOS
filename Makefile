BUILD_MODE ?= debug
KERNEL_TARGET_DIR = x86_64-unknown-trueos
KERNEL_BIN = tgt/$(KERNEL_TARGET_DIR)/$(BUILD_MODE)/TRUEOS
ARTIFACT_BUILD_ID ?= $(shell git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)
ARTIFACT_DIR = bld/artifacts/$(BUILD_MODE)-$(ARTIFACT_BUILD_ID)
ARTIFACT_FULL_ELF = $(ARTIFACT_DIR)/TRUEOS.full.elf
ARTIFACT_RUNTIME_ELF = $(ARTIFACT_DIR)/TRUEOS.elf
ARTIFACT_BUILD_INFO = $(ARTIFACT_DIR)/BUILD_INFO
ISO_DIR := bld
ISO_PATH := bld/trueos.iso
ISO_BOOT_DIR := bld/iso-bootroot
ISO_EFI_IMG := efi.img
UPDATE_7Z_FLAGS ?= -mx=9 -m0=LZMA2 -ms=off
RELEASE_BUNDLE_DIR := $(ISO_DIR)/trueos-release
RELEASE_ARCHIVE := $(ISO_DIR)/TrueOS.7z
BUNDLED_OVMF_NAME := ovmf-code-x86_64.fd
OVMF_BUNDLE_PATH ?= $(firstword $(wildcard /usr/share/ovmf/OVMF.fd /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd /opt/homebrew/share/qemu/edk2-x86_64-code.fd /usr/local/share/qemu/edk2-x86_64-code.fd))
OVMF_LICENSE_PATH ?= $(firstword $(wildcard /usr/share/doc/ovmf/copyright /opt/homebrew/share/doc/qemu/LICENSE /usr/local/share/doc/qemu/LICENSE))
# Extra slack added on top of the staged EFI payload when sizing the embedded
# EFI System Partition image. This keeps the image close to minimal while
# leaving room for FAT metadata and small growth in embedded artifacts.
EFI_IMG_OVERHEAD_KIB ?= 1024
EFI_IMG_MIN_SIZE_KIB ?= 0
LIMINE_CFG := limine.conf
LIMINE_CFG_GENERATED := $(ISO_DIR)/limine.generated.conf
LIMINE_PREFIX := bld/limine-prefix
LIMINE_SHARE := $(LIMINE_PREFIX)/share/limine
GUC_FW_HOST_PATH ?= /lib/firmware/i915/adlp_guc_70.bin.zst
GUC_FW_ISO_REL_PATH ?= EFI/BOOT/adlp_guc_70.bin
BLUEPRINTS_ROOT ?= ../TRUEOS Blueprints
BP_MANIFEST ?= $(BLUEPRINTS_ROOT)/Cargo.toml
BP_NAMES ?= $(strip $(shell awk 'BEGIN { in_example = 0 } /^\[\[example\]\]$$/ { in_example = 1; next } /^\[/ { in_example = 0 } in_example && /^[[:space:]]*name[[:space:]]*=/ { line = $$0; sub(/^[^=]*=[[:space:]]*"/, "", line); sub(/"[[:space:]]*$$/, "", line); print line }' "$(BP_MANIFEST)" 2>/dev/null))
BP_DIST_DIR ?= $(BLUEPRINTS_ROOT)/dist
BP_ISO_DIR_REL ?= EFI/BOOT/apps
BP_EXAMPLE_PAIRS ?= $(strip $(shell awk 'BEGIN { in_example = 0; name = ""; path = "" } /^\[\[example\]\]$$/ { if (in_example && name != "" && path != "") print name ":" path; in_example = 1; name = ""; path = ""; next } /^\[/ { if (in_example && name != "" && path != "") print name ":" path; in_example = 0; next } in_example && /^[[:space:]]*name[[:space:]]*=/ { line = $$0; sub(/^[^=]*=[[:space:]]*"/, "", line); sub(/"[[:space:]]*$$/, "", line); name = line; next } in_example && /^[[:space:]]*path[[:space:]]*=/ { line = $$0; sub(/^[^=]*=[[:space:]]*"/, "", line); sub(/"[[:space:]]*$$/, "", line); path = line; next } END { if (in_example && name != "" && path != "") print name ":" path }' "$(BP_MANIFEST)" 2>/dev/null))
# Blueprint build/embed switches.
# Set BP_SKIP_BUILD to 1 to reuse existing dist/*.bp files even when stale.
# Set BP_SKIP_EMBED to 1 to omit blueprints from the ISO.
# Set both to 1 to stop both blueprint build and embedding.
BP_SKIP_BUILD := 0
BP_SKIP_EMBED := 0
QEMU_RUNNER := tools/qemu/run.sh
QEMU_BIN ?= qemu-system-x86_64
QEMU_UEFI_FIRMWARE = $(OVMF_BUNDLE_PATH)
QEMU_BRIDGE ?= br0
QEMU_BRIDGE_HELPER ?= $(firstword $(wildcard /usr/lib/qemu/qemu-bridge-helper /usr/libexec/qemu-bridge-helper /usr/lib/qemu-bridge-helper))
QEMU_HDA_AUDIODEV ?= none,id=snd0
QEMU_RUN_ENV = ISO_PATH="$(ISO_PATH)" QEMU_BIN="$(QEMU_BIN)" QEMU_UEFI_FIRMWARE="$(QEMU_UEFI_FIRMWARE)" QEMU_BRIDGE="$(QEMU_BRIDGE)" QEMU_BRIDGE_HELPER="$(QEMU_BRIDGE_HELPER)" QEMU_HDA_AUDIODEV="$(QEMU_HDA_AUDIODEV)"
BAREMETAL_LOG_DRAIN := tools/baremetal-log-drain.sh
BAREMETAL_LOG_HOST ?= 192.168.178.94
BAREMETAL_LOG_PORT ?= 1
BAREMETAL_LOG_DELAY ?= 15
BAREMETAL_LOG_DIR ?= bld/baremetal-logs

CARGO_BUILD_FLAGS ?=

CARGO_GFX_FLAGS =

IMG_SIZE ?= 1G

.PHONY: kernel blueprints artifacts kernel-stages baremetal-reboot-log iso iso-build iso-release iso-debug snipe dbg dbg-vscode run run-with-nvme run-installed lc

images: disk.img nvme.img

disk.img:
	truncate -s $(IMG_SIZE) $@

nvme.img:
	truncate -s $(IMG_SIZE) $@

kernel:
	cargo +nightly build $(CARGO_GFX_FLAGS) $(CARGO_BUILD_FLAGS) -Z build-std=core,compiler_builtins,alloc,std,panic_abort -Z json-target-spec --target .cargo/x86_64-unknown-trueos.json

blueprints:
	@if [ "$(BP_SKIP_BUILD)" = "1" ]; then \
		echo "blueprints: skipping build"; \
	elif [ -z "$(strip $(BP_EXAMPLE_PAIRS))" ]; then \
		echo "blueprints: no blueprint outputs configured"; \
	else \
		mkdir -p "$(BP_DIST_DIR)"; \
		for bp_entry in $(BP_EXAMPLE_PAIRS); do \
			bp=$${bp_entry%%:*}; \
			src_rel=$${bp_entry#*:}; \
			out="$(BP_DIST_DIR)/$$bp.bp"; \
			needs_build=0; \
			if [ ! -f "$$out" ]; then \
				needs_build=1; \
			elif [ "$(BP_MANIFEST)" -nt "$$out" ] || [ "$(BLUEPRINTS_ROOT)/$$src_rel" -nt "$$out" ]; then \
				needs_build=1; \
			elif [ -f "$(BLUEPRINTS_ROOT)/Cargo.lock" ] && [ "$(BLUEPRINTS_ROOT)/Cargo.lock" -nt "$$out" ]; then \
				needs_build=1; \
			elif [ -f "$(BLUEPRINTS_ROOT)/target.json" ] && [ "$(BLUEPRINTS_ROOT)/target.json" -nt "$$out" ]; then \
				needs_build=1; \
			elif [ -f "$(BLUEPRINTS_ROOT)/trueos/Cargo.toml" ] && [ "$(BLUEPRINTS_ROOT)/trueos/Cargo.toml" -nt "$$out" ]; then \
				needs_build=1; \
			elif [ -f "$(BLUEPRINTS_ROOT)/trueos-sys/Cargo.toml" ] && [ "$(BLUEPRINTS_ROOT)/trueos-sys/Cargo.toml" -nt "$$out" ]; then \
				needs_build=1; \
			elif find "$(BLUEPRINTS_ROOT)/src" "$(BLUEPRINTS_ROOT)/trueos/src" "$(BLUEPRINTS_ROOT)/trueos-sys/src" -type f -name '*.rs' -newer "$$out" -print -quit 2>/dev/null | grep -q .; then \
				needs_build=1; \
			fi; \
			if [ "$$needs_build" = "1" ]; then \
				cd "$(BLUEPRINTS_ROOT)" && cargo apps "$$bp"; \
			else \
				echo "blueprints: $$bp is up to date"; \
			fi; \
		done; \
	fi

artifacts: blueprints kernel
	mkdir -p $(ARTIFACT_DIR)
	cp $(KERNEL_BIN) $(ARTIFACT_FULL_ELF)
	cp $(KERNEL_BIN) $(ARTIFACT_RUNTIME_ELF)
	strip -s $(ARTIFACT_RUNTIME_ELF) || true
	@{ \
		commit=$$(git rev-parse HEAD 2>/dev/null || echo unknown); \
		ts=$$(date -u +"%Y-%m-%dT%H:%M:%SZ"); \
		printf "build_mode=%s\n" "$(BUILD_MODE)"; \
		printf "build_id=%s\n" "$(ARTIFACT_BUILD_ID)"; \
		printf "commit=%s\n" "$$commit"; \
		printf "timestamp_utc=%s\n" "$$ts"; \
		printf "full_elf=%s\n" "$(ARTIFACT_FULL_ELF)"; \
		printf "runtime_elf=%s\n" "$(ARTIFACT_RUNTIME_ELF)"; \
	} > $(ARTIFACT_BUILD_INFO)

kernel-stages: artifacts

baremetal-reboot-log:
	-fuser -k 7777/udp || true
	python3 -c "import socket; s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); s.bind(('',7777)); exec(\"while True:\n d,a=s.recvfrom(2048)\n if d==b'probe': s.sendto(b'ack',(a[0],7777)); break\")" &
	@TRUEOS_BAREMETAL_LOG_HOST="$(BAREMETAL_LOG_HOST)" TRUEOS_BAREMETAL_LOG_PORT="$(BAREMETAL_LOG_PORT)" TRUEOS_BAREMETAL_LOG_DELAY="$(BAREMETAL_LOG_DELAY)" TRUEOS_BAREMETAL_LOG_DIR="$(BAREMETAL_LOG_DIR)" "$(BAREMETAL_LOG_DRAIN)" start

iso: baremetal-reboot-log
	@$(MAKE) iso-build

iso-build: artifacts images
	rm -rf $(ISO_BOOT_DIR)
	rm -f $(ISO_PATH)
	mkdir -p $(ISO_BOOT_DIR)
	cp $(ARTIFACT_RUNTIME_ELF) $(ISO_BOOT_DIR)/TRUEOS.elf
	mkdir -p $(ISO_DIR)/EFI/BOOT
	@if [ "$(BP_SKIP_EMBED)" != "1" ]; then \
		for bp in $(BP_NAMES); do \
			if [ ! -f "$(BP_DIST_DIR)/$$bp.bp" ]; then \
				echo "error: blueprint not found at $(BP_DIST_DIR)/$$bp.bp"; \
				exit 1; \
			fi; \
		done; \
	else \
		echo "iso: skipping blueprint embed"; \
	fi
	@if [ ! -f "$(GUC_FW_HOST_PATH)" ]; then \
		echo "error: GUC firmware not found at $(GUC_FW_HOST_PATH)"; \
		exit 1; \
	fi
	@case "$(GUC_FW_HOST_PATH)" in \
		*.zst) \
			command -v zstd >/dev/null 2>&1 || { echo "error: zstd command not found; cannot decompress $(GUC_FW_HOST_PATH)"; exit 1; }; \
			zstd -d -c "$(GUC_FW_HOST_PATH)" > "$(ISO_DIR)/EFI/BOOT/adlp_guc_70.bin"; \
			;; \
		*) \
			cp "$(GUC_FW_HOST_PATH)" "$(ISO_DIR)/EFI/BOOT/adlp_guc_70.bin"; \
			;; \
	esac
	mkdir -p $(ISO_BOOT_DIR)/$(dir $(GUC_FW_ISO_REL_PATH))
	cp $(ISO_DIR)/EFI/BOOT/adlp_guc_70.bin $(ISO_BOOT_DIR)/$(GUC_FW_ISO_REL_PATH)
	@if [ "$(BP_SKIP_EMBED)" != "1" ]; then \
		mkdir -p $(ISO_BOOT_DIR)/$(BP_ISO_DIR_REL); \
		for bp in $(BP_NAMES); do \
			cp "$(BP_DIST_DIR)/$$bp.bp" "$(ISO_BOOT_DIR)/$(BP_ISO_DIR_REL)/$$bp.bp"; \
		done; \
		mkdir -p $(ISO_DIR)/$(BP_ISO_DIR_REL); \
		for bp in $(BP_NAMES); do \
			cp "$(BP_DIST_DIR)/$$bp.bp" "$(ISO_DIR)/$(BP_ISO_DIR_REL)/$$bp.bp"; \
		done; \
	fi
	@awk 'BEGIN { skip_app_string = 0 } skip_app_string && /^module_string: trueos\.app\./ { skip_app_string = 0; next } skip_app_string { skip_app_string = 0 } /^module_path: boot\(\):\/EFI\/BOOT\/apps\/.*\.bp$$/ { skip_app_string = 1; next } { print }' "$(LIMINE_CFG)" > "$(LIMINE_CFG_GENERATED)"
	@if [ "$(BP_SKIP_EMBED)" != "1" ]; then \
		for bp in $(BP_NAMES); do \
			printf '%s\n%s\n' \
				"module_path: boot():/$(BP_ISO_DIR_REL)/$$bp.bp" \
				"module_string: trueos.app.$$bp" \
				>> "$(LIMINE_CFG_GENERATED)"; \
		done; \
	fi
	@if [ "$(GUC_FW_ISO_REL_PATH)" != "EFI/BOOT/adlp_guc_70.bin" ]; then \
		mkdir -p $(ISO_BOOT_DIR)/EFI/BOOT; \
		cp $(ISO_DIR)/EFI/BOOT/adlp_guc_70.bin $(ISO_BOOT_DIR)/EFI/BOOT/adlp_guc_70.bin; \
	fi
	cp $(LIMINE_CFG_GENERATED) $(ISO_BOOT_DIR)/limine.conf
	cp $(LIMINE_SHARE)/BOOTX64.EFI $(ISO_DIR)/EFI/BOOT/BOOTX64.EFI
	cp $(LIMINE_CFG_GENERATED) $(ISO_DIR)/EFI/BOOT/limine.conf
	cp $(ISO_BOOT_DIR)/TRUEOS.elf $(ISO_DIR)/TRUEOS.elf
	mkdir -p $(ISO_BOOT_DIR)/EFI/BOOT
	cp $(LIMINE_SHARE)/BOOTX64.EFI $(ISO_BOOT_DIR)/EFI/BOOT/BOOTX64.EFI
	rm -f $(ISO_BOOT_DIR)/$(ISO_EFI_IMG)
	@efi_payload_kib=$$(du -sk "$(ISO_BOOT_DIR)/EFI" | cut -f1); \
		efi_img_size_kib=$$((efi_payload_kib + $(EFI_IMG_OVERHEAD_KIB))); \
		if [ "$$efi_img_size_kib" -lt "$(EFI_IMG_MIN_SIZE_KIB)" ]; then \
			efi_img_size_kib="$(EFI_IMG_MIN_SIZE_KIB)"; \
		fi; \
		echo "iso: sizing $(ISO_EFI_IMG) to $${efi_img_size_kib} KiB (payload=$${efi_payload_kib} KiB, overhead=$(EFI_IMG_OVERHEAD_KIB) KiB)"; \
		dd if=/dev/zero of=$(ISO_BOOT_DIR)/$(ISO_EFI_IMG) bs=1k count=$$efi_img_size_kib
	mkfs.vfat -n TRUEOS_EFI $(ISO_BOOT_DIR)/$(ISO_EFI_IMG)
	mmd -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) ::/EFI ::/EFI/BOOT
	@if [ "$(BP_SKIP_EMBED)" != "1" ]; then \
		mmd -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) ::/EFI/BOOT/apps; \
	fi
	mcopy -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) $(LIMINE_SHARE)/BOOTX64.EFI ::/EFI/BOOT/BOOTX64.EFI
	mcopy -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) $(ISO_DIR)/EFI/BOOT/adlp_guc_70.bin ::/EFI/BOOT/adlp_guc_70.bin
	@if [ "$(BP_SKIP_EMBED)" != "1" ]; then \
		for bp in $(BP_NAMES); do \
			mcopy -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) "$(ISO_BOOT_DIR)/$(BP_ISO_DIR_REL)/$$bp.bp" ::/$(BP_ISO_DIR_REL)/$$bp.bp; \
		done; \
	fi
	xorriso -as mkisofs \
		-iso-level 3 -full-iso9660-filenames \
		-R \
		-r \
		-J -joliet-long \
		-e $(ISO_EFI_IMG) -no-emul-boot \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		-o $(ISO_PATH) $(ISO_BOOT_DIR)


iso-release: BUILD_MODE := release
iso-release: CARGO_BUILD_FLAGS += --release
iso-release: iso-build
	@if [ -z "$(OVMF_BUNDLE_PATH)" ] || [ ! -f "$(OVMF_BUNDLE_PATH)" ]; then \
		echo "error: no OVMF firmware found to bundle"; \
		echo "       install OVMF/edk2-ovmf or run: make iso-release OVMF_BUNDLE_PATH=/path/to/ovmf-code-x86_64.fd"; \
		exit 1; \
	fi
	rm -rf $(RELEASE_BUNDLE_DIR)
	rm -f $(RELEASE_ARCHIVE)
	mkdir -p $(RELEASE_BUNDLE_DIR)
	cp $(ISO_PATH) $(RELEASE_BUNDLE_DIR)/trueos.iso
	cp "$(OVMF_BUNDLE_PATH)" $(RELEASE_BUNDLE_DIR)/$(BUNDLED_OVMF_NAME)
	cp tools/release/run-linux.sh $(RELEASE_BUNDLE_DIR)/run-linux.sh
	cp tools/release/run-macos.sh $(RELEASE_BUNDLE_DIR)/run-macos.sh
	cp tools/release/README-RUN.txt $(RELEASE_BUNDLE_DIR)/README-RUN.txt
	@if [ -n "$(OVMF_LICENSE_PATH)" ] && [ -f "$(OVMF_LICENSE_PATH)" ]; then \
		cp "$(OVMF_LICENSE_PATH)" $(RELEASE_BUNDLE_DIR)/OVMF-LICENSE.txt; \
	fi
	chmod +x $(RELEASE_BUNDLE_DIR)/run-linux.sh $(RELEASE_BUNDLE_DIR)/run-macos.sh
	cd $(RELEASE_BUNDLE_DIR) && 7z a -t7z $(UPDATE_7Z_FLAGS) ../$(notdir $(RELEASE_ARCHIVE)) trueos.iso $(BUNDLED_OVMF_NAME) run-linux.sh run-macos.sh README-RUN.txt $$(test -f OVMF-LICENSE.txt && printf '%s' OVMF-LICENSE.txt)
	env -u GIO_MODULE_DIR gio mount smb://t4ce@pdjb/home-share || true
	env -u GIO_MODULE_DIR gio copy $(RELEASE_ARCHIVE) smb://t4ce@pdjb/home-share/TRUEOS_SITE/
	@count=$$(cat cnt 2>/dev/null || echo 0); count=$${count:-0}; printf '%s\n' $$((count + 1)) | tee cnt

iso-debug: BUILD_MODE := debug
iso-debug: iso-build

SERIAL_CONSOLE_CMD = konsole -e sh -c 'stty -echo -icanon cols 100 rows 100; nc 127.0.0.1 5555; stty sane; echo "Connection closed. Press ENTER to exit..."; read'

snipe:
	@killall -9 qemu-system-x86_64 || true

dbg: snipe iso-debug
	@($(QEMU_RUN_ENV) $(QEMU_RUNNER) iso & $(SERIAL_CONSOLE_CMD))

dbg-vscode: snipe iso-debug
	@$(SERIAL_CONSOLE_CMD) &
	@set -e; \
		$(QEMU_RUN_ENV) $(QEMU_RUNNER) iso-debug -S -s & qemu_pid=$$!; \
		sleep 0.15; \
		echo "GDB stub ready on 127.0.0.1:1234"; \
		wait $$qemu_pid

# Default quick boot: rebuild the emulator ISO, then restart only QEMU.
run: snipe iso-debug
	@($(QEMU_RUN_ENV) $(QEMU_RUNNER) iso & $(SERIAL_CONSOLE_CMD))

lc:
	@./lc $(ARGS)

run-with-nvme: snipe iso-debug
	@($(QEMU_RUN_ENV) $(QEMU_RUNNER) iso & $(SERIAL_CONSOLE_CMD))

run-installed: snipe iso-debug
	@($(QEMU_RUN_ENV) $(QEMU_RUNNER) installed & $(SERIAL_CONSOLE_CMD))
