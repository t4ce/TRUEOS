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

LIMINE_CFG := limine.conf
LIMINE_PREFIX := bld/limine-prefix
LIMINE_SHARE := $(LIMINE_PREFIX)/share/limine

QEMU_BIN = qemu-system-x86_64 
# QEMU uses a firmware image for UEFI boot. This is OVMF (not legacy BIOS/SeaBIOS).
QEMU_UEFI_FIRMWARE = $(firstword $(wildcard /usr/share/ovmf/OVMF.fd /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd))

QEMU_NET_FLAGS = -netdev tap,id=net1,ifname=tap0,script=no,downscript=no,vhost=off -device virtio-net-pci,netdev=net1,disable-modern=off \
	#-netdev user,id=net1,net=10.0.2.0/24,dhcpstart=10.0.2.15,hostfwd=tcp::4245-:4245,hostfwd=tcp::8080-:80 -device e1000,netdev=net1 \
	#-netdev user,id=net0,hostfwd=tcp::4243-:4243 -device e1000,netdev=net0 \
	#-netdev user,id=net2,hostfwd=tcp::4245-:4245 -device virtio-net-pci,netdev=net2,disable-modern=off

QEMU_RNG_FLAGS = -object rng-random,filename=/dev/urandom,id=rng0 \
	-device virtio-rng-pci,rng=rng0,disable-modern=off

CARGO_BUILD_FLAGS ?=

# Default display path: virgl-capable virtio GPU (no legacy VGA).
# Override with `QEMU_ISO_FLAGS=...` if you need a different display device.
QEMU_ISO_FLAGS = -display sdl,gl=on -vga none -device virtio-vga-gl,disable-modern=off -enable-kvm -machine q35 -bios $(QEMU_UEFI_FIRMWARE) -cdrom $(ISO_PATH) -debugcon stdio -D bld/qemu.log -d int,guest_errors,cpu_reset,unimp -m 2000M -smp cores=4 -cpu host,host-phys-bits=true -serial tcp:127.0.0.1:5555,server,nowait $(QEMU_NET_FLAGS) $(QEMU_RNG_FLAGS)

QEMU_USB_FLAGS = \
	-device qemu-xhci,id=xhci,p2=8,p3=8 \
	-device usb-mouse,bus=xhci.0,port=1,id=usbmouse \
	-device usb-kbd,bus=xhci.0,port=2,id=usbkbd \
	-device usb-host,vendorid=0x303a,productid=0x1001,bus=xhci.0,port=3,id=usbhost \
	-device usb-host,vendorid=0x0951,productid=0x16a4,bus=xhci.0,port=4,id=usbhypx \
	-device usb-host,vendorid=0x07cf,productid=0x6803,bus=xhci.0,port=7,id=usbpiano \
	-device usb-host,vendorid=0x058f,productid=0x6387,bus=xhci.0,port=6,id=usbpendrive \
	-drive file=/dev/disk/by-partuuid/2e4e446c-bc9b-4e6c-a657-9ff9a0edccca,if=none,format=raw,id=nvme0 \
	-device nvme,drive=nvme0,serial=t4ce \
	-drive file=disk.img,if=none,format=raw,id=usbdisk  \
	-device usb-storage,drive=usbdisk,bus=xhci.0,port=5,id=usbms 

#-device usb-host,vendorid=0x1462,productid=0x7e03,bus=xhci.0,port=5,id=usbleds \

QEMU_ISO = $(QEMU_BIN) $(QEMU_ISO_FLAGS) $(QEMU_USB_FLAGS)

IMG_SIZE ?= 1G

images: disk.img nvme.img

disk.img:
	truncate -s $(IMG_SIZE) $@

nvme.img:
	truncate -s $(IMG_SIZE) $@

kernel:
	cargo +nightly build $(CARGO_BUILD_FLAGS) -Z build-std=core,compiler_builtins,alloc --target 86_64.json

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
	cp $(LIMINE_CFG) $(ISO_BOOT_DIR)/EFI/BOOT/limine.conf
	rm -f $(ISO_BOOT_DIR)/$(ISO_EFI_IMG)
	dd if=/dev/zero of=$(ISO_BOOT_DIR)/$(ISO_EFI_IMG) bs=1k count=512
	mkfs.vfat -n TRUEOS_EFI $(ISO_BOOT_DIR)/$(ISO_EFI_IMG)
	mmd -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) ::/EFI ::/EFI/BOOT
	mcopy -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) $(LIMINE_SHARE)/BOOTX64.EFI ::/EFI/BOOT/BOOTX64.EFI
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
	7z a -t7z -mx=9 -m0=lzma2 $(ISO_DIR)/TrueOS.7z $(ISO_PATH)
	gio mount smb://t4ce@pdjb/home-share || true
	gio copy $(ISO_DIR)/TrueOS.7z smb://t4ce@pdjb/home-share/TRUEOS_SITE/
	@count=$$(cat cnt 2>/dev/null || echo 0); count=$${count:-0}; printf '%s\n' $$((count + 1)) | tee cnt

iso-debug: BUILD_MODE := debug
iso-debug: iso

SERIAL_CONSOLE_CMD = konsole -e sh -c 'stty -echo -icanon cols 100 rows 30; nc 127.0.0.1 5555; stty sane'

run: iso-debug
	@($(QEMU_ISO) & $(SERIAL_CONSOLE_CMD))

dbg: iso-debug
	@$(SERIAL_CONSOLE_CMD) &
	@echo "Waiting for debugger..."
	@$(QEMU_ISO) -S -s

# Useful for validating GPT+ESP+Limine stage installation.
QEMU_DISK_COMMON_FLAGS = -debugcon stdio -m 2000M -smp cores=4 -cpu qemu64,phys-bits=39 -serial tcp:127.0.0.1:5555,server,nowait
QEMU_DISK_DRIVE_FLAGS = -drive file=disk.img,if=virtio,format=raw

run-installed: iso-debug
	@($(QEMU_BIN) -bios $(QEMU_UEFI_FIRMWARE) $(QEMU_DISK_COMMON_FLAGS) $(QEMU_NET_FLAGS) $(QEMU_RNG_FLAGS) $(QEMU_DISK_DRIVE_FLAGS) & $(SERIAL_CONSOLE_CMD))
