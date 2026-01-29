BUILD_MODE ?= debug
KERNEL_BIN = tgt/86_64/$(BUILD_MODE)/TRUEOS

ISO_DIR := bld
ISO_PATH := bld/trueos.iso
ISO_BOOT_DIR := bld/iso-bootroot

LIMINE_CFG := limine.conf
LIMINE_PREFIX := bld/limine-prefix
LIMINE_SHARE := $(LIMINE_PREFIX)/share/limine

QEMU_BIN = qemu-system-x86_64
QEMU_BIOS = $(firstword $(wildcard /usr/share/ovmf/OVMF.fd /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd))

QEMU_NET_FLAGS = -netdev user,id=net0,hostfwd=tcp::4243-:4243 -device e1000,netdev=net0 \
	-netdev user,id=net1,hostfwd=tcp::4244-:4244 -device rtl8139,netdev=net1 \
	-netdev user,id=net2,hostfwd=tcp::4245-:4245 -device virtio-net-pci,netdev=net2,disable-modern=off

QEMU_RNG_FLAGS = -object rng-random,filename=/dev/urandom,id=rng0 \
	-device virtio-rng-pci,rng=rng0,disable-modern=off

QEMU_ISO_FLAGS = -bios $(QEMU_BIOS) -cdrom $(ISO_PATH) -debugcon stdio -m 2000M -smp cores=4 -cpu qemu64,phys-bits=39 -serial tcp:127.0.0.1:5555,server,nowait $(QEMU_NET_FLAGS) $(QEMU_RNG_FLAGS)

QEMU_USB_FLAGS = \
	-device nec-usb-xhci,id=xhci,p2=8,p3=8 \
	-device vfio-pci,host=0000:06:00.0 \
	-device usb-mouse,bus=xhci.0,port=1,id=usbmouse0 \
	-device usb-kbd,bus=xhci.0,port=2,id=usbkbd0 \
	-device usb-host,vendorid=0x303a,productid=0x1001,bus=xhci.0,port=3,id=usbhost0 \
	-device usb-host,vendorid=0x0951,productid=0x16a4,bus=xhci.0,port=4,id=usbhypx0 \
	-device usb-host,vendorid=0x058f,productid=0x6387,bus=xhci.0,port=6,id=usbpendrive0 \
	-drive file=disk.img,if=none,format=raw,id=usbdisk0 \
	-device usb-storage,drive=usbdisk0,bus=xhci.0,port=5,id=usbms0 \
	# -drive file=disk.img,if=none,format=raw,id=nvme0 \
	# -device nvme,drive=nvme0,serial=deadbeef

QEMU_ISO = $(QEMU_BIN) $(QEMU_ISO_FLAGS) $(QEMU_USB_FLAGS)

kernel:
	cargo +nightly build $(CARGO_BUILD_FLAGS) -Z build-std=core,compiler_builtins,alloc --target 86_64.json

iso: kernel
	rm -rf $(ISO_BOOT_DIR)
	rm -f $(ISO_PATH)
	mkdir -p $(ISO_BOOT_DIR)/EFI/BOOT
	cp $(KERNEL_BIN) $(ISO_BOOT_DIR)/TRUEOS.elf
	cp $(LIMINE_CFG) $(ISO_BOOT_DIR)/limine.conf
	cp $(LIMINE_SHARE)/BOOTX64.EFI $(ISO_BOOT_DIR)/EFI/BOOT/BOOTX64.EFI
	cp $(LIMINE_SHARE)/limine-bios.sys $(ISO_BOOT_DIR)/
	cp $(LIMINE_SHARE)/limine-bios-cd.bin $(ISO_BOOT_DIR)/
	cp $(LIMINE_SHARE)/limine-uefi-cd.bin $(ISO_BOOT_DIR)/
	xorriso -as mkisofs \
		-iso-level 3 -full-iso9660-filenames \
		-R \
		-r \
		-J -joliet-long \
		-b limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		-o $(ISO_PATH) $(ISO_BOOT_DIR)
	@# Make the ISO BIOS-bootable as a hybrid image (dd-able to USB/disk).
	$(LIMINE_PREFIX)/bin/limine bios-install $(ISO_PATH)

iso-release: BUILD_MODE := release
iso-release: CARGO_BUILD_FLAGS := --release
iso-release: iso
	7z a -t7z -mx=9 -m0=lzma2 $(ISO_DIR)/TrueOS.7z $(ISO_PATH)
	gio mount smb://t4ce@pdjb/home-share || true
	gio copy $(ISO_DIR)/TrueOS.7z smb://t4ce@pdjb/home-share/TRUEOS_SITE/
	@count=$$(cat cnt 2>/dev/null || echo 0); count=$${count:-0}; printf '%s\n' $$((count + 1)) | tee cnt

iso-debug: BUILD_MODE := debug
iso-debug: iso

SERIAL_CONSOLE_CMD = konsole -e sh -c 'stty -echo -icanon cols 100 rows 80; nc 127.0.0.1 5555; stty sane'

run: iso-debug
	@($(QEMU_ISO) & $(SERIAL_CONSOLE_CMD))











# Boot the installed disk image directly (no installer ISO).
# Useful for validating GPT+ESP+Limine stage installation.
QEMU_DISK_COMMON_FLAGS = -debugcon stdio -m 2000M -smp cores=4 -cpu qemu64,phys-bits=39 -serial tcp:127.0.0.1:5555,server,nowait
QEMU_DISK_DRIVE_FLAGS = -drive file=disk.img,if=virtio,format=raw

run-installed-uefi: iso-debug
	@($(QEMU_BIN) -bios $(QEMU_BIOS) $(QEMU_DISK_COMMON_FLAGS) $(QEMU_NET_FLAGS) $(QEMU_RNG_FLAGS) $(QEMU_DISK_DRIVE_FLAGS) & $(SERIAL_CONSOLE_CMD))

run-installed-bios: iso-debug
	@($(QEMU_BIN) $(QEMU_DISK_COMMON_FLAGS) $(QEMU_NET_FLAGS) $(QEMU_RNG_FLAGS) $(QEMU_DISK_DRIVE_FLAGS) & $(SERIAL_CONSOLE_CMD))

dbg: iso-debug
	@($(QEMU_ISO) -s -S & $(SERIAL_CONSOLE_CMD))