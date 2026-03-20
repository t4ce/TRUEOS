BUILD_MODE ?= debug
KERNEL_BIN = tgt/86_64/$(BUILD_MODE)/TRUEOS
ARTIFACT_BUILD_ID ?= $(shell git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)
ARTIFACT_DIR := bld/artifacts/$(BUILD_MODE)-$(ARTIFACT_BUILD_ID)
ARTIFACT_FULL_ELF := $(ARTIFACT_DIR)/TRUEOS.full.elf
ARTIFACT_RUNTIME_ELF := $(ARTIFACT_DIR)/TRUEOS.elf
ARTIFACT_BUILD_INFO := $(ARTIFACT_DIR)/BUILD_INFO


ISO_DIR := bld
ISO_PATH := bld/trueos.iso
ISO_BOOT_DIR := bld/iso-bootroot
ISO_EFI_IMG := efi.img
UPDATE_7Z_FLAGS ?= -mx=9 -m0=LZMA2 -ms=off

# Size of the EFI System Partition image that gets embedded into the ISO.
# Keep this small: the kernel and Limine config live on the ISO9660 filesystem,
# while the ESP only needs to contain the EFI loader.
EFI_IMG_SIZE_KIB ?= 512

LIMINE_CFG := limine.conf
LIMINE_PREFIX := bld/limine-prefix
LIMINE_SHARE := $(LIMINE_PREFIX)/share/limine

QEMU_ENV = env -i HOME="$(HOME)" PATH="/usr/bin:/bin" TERM="$${TERM:-xterm}" LANG="$${LANG:-C.UTF-8}" DISPLAY="$${DISPLAY:-}" WAYLAND_DISPLAY="$${WAYLAND_DISPLAY:-}" XDG_RUNTIME_DIR="$${XDG_RUNTIME_DIR:-}" XAUTHORITY="$${XAUTHORITY:-}"
QEMU_BIN = $(QEMU_ENV) qemu-system-x86_64 -no-shutdown
# QEMU uses a firmware image for UEFI boot. This is OVMF (not legacy BIOS/SeaBIOS).
QEMU_UEFI_FIRMWARE = $(firstword $(wildcard /usr/share/ovmf/OVMF.fd /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd))

GFX_MODE ?= virgl
INTEL_GPU_PCI ?= 0000:00:02.0
# `x-no-mmap=on` avoids QEMU trying to mmap the passed-through IGD BAR into the
# guest address space directly, which currently trips VFIO DMA-map warnings in our setup.
INTEL_GPU_VFIO_PROPS ?= ,x-igd-opregion=on,x-no-mmap=on

# Enabling vhost-net can significantly improve virtio-net throughput.
# Use `make run QEMU_VHOST=on` if your host supports it (permissions on /dev/vhost-net).
QEMU_VHOST ?= off

QEMU_NET_FLAGS = -netdev tap,id=net1,ifname=tap0,script=no,downscript=no,vhost=$(QEMU_VHOST) -device virtio-net-pci,netdev=net1,disable-modern=off,bus=pcie.0,addr=0x3 \
	#-netdev user,id=net1,net=10.0.2.0/24,dhcpstart=10.0.2.15,hostfwd=tcp::4245-:4245,hostfwd=tcp::8080-:80 -device e1000,netdev=net1 \
	#-netdev user,id=net0,hostfwd=tcp::4243-:4243 -device e1000,netdev=net0 \
	#-netdev user,id=net2,hostfwd=tcp::4245-:4245 -device virtio-net-pci,netdev=net2,disable-modern=off

QEMU_RNG_FLAGS = -object rng-random,filename=/dev/urandom,id=rng0 \
	-device virtio-rng-pci,rng=rng0,disable-modern=off,bus=pcie.0,addr=0x4

CARGO_BUILD_FLAGS ?=

ifeq ($(GFX_MODE),virgl)
CARGO_GFX_FLAGS = --no-default-features --features gfx_virgl
QEMU_GFX_FLAGS = -display sdl,gl=on -vga none -device virtio-gpu-gl-pci,disable-modern=off,xres=1280,yres=800
else ifeq ($(GFX_MODE),intel)
CARGO_GFX_FLAGS = --no-default-features --features gfx_intel
QEMU_GFX_FLAGS = -display none -vga none -device vfio-pci,host=$(INTEL_GPU_PCI),bus=pcie.0,addr=0x2$(INTEL_GPU_VFIO_PROPS)
else ifeq ($(GFX_MODE),none)
CARGO_GFX_FLAGS = --no-default-features
QEMU_GFX_FLAGS = -display sdl,gl=off -vga std
else
$(error Unsupported GFX_MODE '$(GFX_MODE)' (expected virgl, intel, or none))
endif

QEMU_ISO_FLAGS = $(QEMU_GFX_FLAGS) -enable-kvm -machine q35 -bios $(QEMU_UEFI_FIRMWARE) -boot order=d -cdrom $(ISO_PATH) -debugcon stdio -D bld/qemu.log -d int,guest_errors,cpu_reset,unimp -m 2000M -smp cores=8 -cpu host,host-phys-bits=true -serial tcp:127.0.0.1:5555,server,nowait $(QEMU_NET_FLAGS) $(QEMU_RNG_FLAGS)

QEMU_ISO_FLAGS_DBG = $(QEMU_GFX_FLAGS) -machine q35 -bios $(QEMU_UEFI_FIRMWARE) -cdrom $(ISO_PATH) -debugcon stdio -D bld/qemu.log -d int,guest_errors,cpu_reset,unimp -m 2000M -smp cores=4 -cpu qemu64,phys-bits=39 -serial tcp:127.0.0.1:5555,server,nowait $(QEMU_NET_FLAGS) $(QEMU_RNG_FLAGS)

QEMU_UPDATE_TARGET_PCI ?= 0000:08:00.0
QEMU_UPDATE_TARGET_FLAGS = -device vfio-pci,host=$(QEMU_UPDATE_TARGET_PCI),bus=pcie.0,addr=0x6

QEMU_USB_HOST_FLAGS = -device qemu-xhci,id=xhci,p2=8,p3=8,bus=pcie.0,addr=0x5  \
	-device usb-host,vendorid=0x058f,productid=0x6387,bus=xhci.0,port=2,id=usbpendrive \

#	-device usb-host,vendorid=0x0951,productid=0x16a4,bus=xhci.0,port=3,id=usbhypx \
#	-device usb-host,vendorid=0x303a,productid=0x1001,bus=xhci.0,port=1,id=usbtruekey \
#	-drive file=disk.img,if=none,format=raw,id=usbdisk  
#	-device usb-storage,drive=usbdisk,bus=xhci.0,port=4,id=usbms  
#	-device usb-mouse,bus=xhci.0,port=1,id=usbmouse 
#	-device usb-host,vendorid=0x1462,productid=0x7e03,bus=xhci.0,port=2,id=usbleds 
#	-device usb-kbd,bus=xhci.0,port=3,id=usbkbd 
#   -device usb-tablet,bus=xhci.0,port=4,id=usbtablet
#   -device usb-host,vendorid=0x07cf,productid=0x6803,bus=xhci.0,port=0,id=usbpiano

QEMU_USB_FLAGS = $(QEMU_USB_HOST_FLAGS)

QEMU_ISO = $(QEMU_BIN) $(QEMU_ISO_FLAGS) $(QEMU_USB_FLAGS)
QEMU_ISO_WITH_NVME = $(QEMU_BIN) $(QEMU_ISO_FLAGS) $(QEMU_USB_FLAGS) 
QEMU_ISO_DBG = $(QEMU_BIN) $(QEMU_ISO_FLAGS_DBG) $(QEMU_USB_FLAGS)

IMG_SIZE ?= 1G

images: disk.img nvme.img

disk.img:
	truncate -s $(IMG_SIZE) $@

nvme.img:
	truncate -s $(IMG_SIZE) $@

kernel:
	cargo +nightly build $(CARGO_GFX_FLAGS) $(CARGO_BUILD_FLAGS) -Z build-std=core,compiler_builtins,alloc -Z json-target-spec --target 86_64.json

artifacts: kernel
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

iso: artifacts images
	rm -rf $(ISO_BOOT_DIR)
	rm -f $(ISO_PATH)
	mkdir -p $(ISO_BOOT_DIR)
	cp $(ARTIFACT_RUNTIME_ELF) $(ISO_BOOT_DIR)/TRUEOS.elf
	cp $(LIMINE_CFG) $(ISO_BOOT_DIR)/limine.conf
	mkdir -p $(ISO_DIR)/EFI/BOOT
	cp $(LIMINE_SHARE)/BOOTX64.EFI $(ISO_DIR)/EFI/BOOT/BOOTX64.EFI
	cp $(LIMINE_CFG) $(ISO_DIR)/EFI/BOOT/limine.conf
	cp $(ISO_BOOT_DIR)/TRUEOS.elf $(ISO_DIR)/TRUEOS.elf
	mkdir -p $(ISO_BOOT_DIR)/EFI/BOOT
	cp $(LIMINE_SHARE)/BOOTX64.EFI $(ISO_BOOT_DIR)/EFI/BOOT/BOOTX64.EFI
	rm -f $(ISO_BOOT_DIR)/$(ISO_EFI_IMG)
	dd if=/dev/zero of=$(ISO_BOOT_DIR)/$(ISO_EFI_IMG) bs=1k count=$(EFI_IMG_SIZE_KIB)
	mkfs.vfat -n TRUEOS_EFI $(ISO_BOOT_DIR)/$(ISO_EFI_IMG)
	mmd -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) ::/EFI ::/EFI/BOOT
	mcopy -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) $(LIMINE_SHARE)/BOOTX64.EFI ::/EFI/BOOT/BOOTX64.EFI
	# Important: do NOT place limine.conf next to BOOTX64.EFI.
	# Limine prioritizes <EFI app path>/limine.conf; many ISO-to-USB tools copy only
	# /EFI/BOOT into a FAT ESP, which would shadow the intended /limine.conf on ISO.
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
iso-release: iso
	rm -f $(ISO_DIR)/TrueOS.7z
	cd $(ISO_DIR) && 7z a -t7z $(UPDATE_7Z_FLAGS) TrueOS.7z $(notdir $(ISO_PATH))
	env -u GIO_MODULE_DIR gio mount smb://t4ce@pdjb/home-share || true
	env -u GIO_MODULE_DIR gio copy $(ISO_DIR)/TrueOS.7z smb://t4ce@pdjb/home-share/TRUEOS_SITE/
	@count=$$(cat cnt 2>/dev/null || echo 0); count=$${count:-0}; printf '%s\n' $$((count + 1)) | tee cnt

iso-debug: BUILD_MODE := debug
iso-debug: iso

SERIAL_CONSOLE_CMD = konsole -e sh -c 'stty -echo -icanon cols 100 rows 100; nc 127.0.0.1 5555; stty sane'

snipe:
	@killall -9 qemu-system-x86_64 || true

dbg: snipe iso-debug
	@($(QEMU_ISO) & $(SERIAL_CONSOLE_CMD))

dbg-vscode: snipe iso-debug
	@$(SERIAL_CONSOLE_CMD) &
	@set -e; \
		$(QEMU_ISO_DBG) -S -s & qemu_pid=$$!; \
		sleep 0.15; \
		echo "GDB stub ready on 127.0.0.1:1234"; \
		wait $$qemu_pid

# Default quick boot: boot the fresh ISO first while the handed-in NVMe is
# attached for the kernel to probe and mount.
run: snipe iso-debug
	@($(QEMU_ISO_WITH_NVME) & $(SERIAL_CONSOLE_CMD))

run-with-nvme: snipe iso-debug
	@($(QEMU_ISO_WITH_NVME) & $(SERIAL_CONSOLE_CMD))

# Useful for validating GPT+ESP+Limine stage installation.
QEMU_DISK_COMMON_FLAGS = -debugcon stdio -m 2000M -smp cores=4 -cpu qemu64,phys-bits=39 -serial tcp:127.0.0.1:5555,server,nowait
QEMU_DISK_DRIVE_FLAGS = -drive file=disk.img,if=virtio,format=raw

run-installed: snipe iso-debug
	@($(QEMU_BIN) $(QEMU_GFX_FLAGS) -bios $(QEMU_UEFI_FIRMWARE) $(QEMU_DISK_COMMON_FLAGS) $(QEMU_NET_FLAGS) $(QEMU_RNG_FLAGS) & $(SERIAL_CONSOLE_CMD))
