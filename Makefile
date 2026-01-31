BUILD_MODE ?= debug
KERNEL_BIN = tgt/86_64/$(BUILD_MODE)/TRUEOS

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

QEMU_NET_FLAGS = -netdev user,id=net1,hostfwd=tcp::4245-:4245,hostfwd=tcp::8080-:80 -device e1000,netdev=net1 \
	#-netdev user,id=net0,hostfwd=tcp::4243-:4243 -device e1000,netdev=net0 \
	#-netdev user,id=net2,hostfwd=tcp::4245-:4245 -device virtio-net-pci,netdev=net2,disable-modern=off

QEMU_RNG_FLAGS = -object rng-random,filename=/dev/urandom,id=rng0 \
	-device virtio-rng-pci,rng=rng0,disable-modern=off

QEMU_ISO_FLAGS = -machine q35 -bios $(QEMU_UEFI_FIRMWARE) -cdrom $(ISO_PATH) -debugcon stdio -m 2000M -smp cores=4 -cpu qemu64,phys-bits=39 -serial tcp:127.0.0.1:5555,server,nowait $(QEMU_NET_FLAGS) $(QEMU_RNG_FLAGS)

QEMU_USB_FLAGS = \
	-device nec-usb-xhci,id=xhci,p2=8,p3=8 \
	-device vfio-pci,host=0000:06:00.0 \
	-device usb-mouse,bus=xhci.0,port=1,id=usbmouse \
	-device usb-kbd,bus=xhci.0,port=2,id=usbkbd \
	-device usb-host,vendorid=0x058f,productid=0x6387,bus=xhci.0,port=6,id=usbpendrive \
	-device usb-host,vendorid=0x303a,productid=0x1001,bus=xhci.0,port=3,id=usbhost \
	-device usb-host,vendorid=0x0951,productid=0x16a4,bus=xhci.0,port=4,id=usbhypx \
	-device usb-host,vendorid=0x1462,productid=0x7e03,bus=xhci.0,port=7,id=usbleds \

#	-drive file=disk.img,if=none,format=raw,id=usbdisk 
#	-device usb-storage,drive=usbdisk,bus=xhci.0,port=5,id=usbms 
#	-drive file=nvme.img,if=none,format=raw,id=nvme0 \
#	-device nvme,drive=nvme0,serial=t4ce

QEMU_ISO = $(QEMU_BIN) $(QEMU_ISO_FLAGS) $(QEMU_USB_FLAGS)

IMG_SIZE ?= 1G

.PHONY: images

images: disk.img nvme.img

disk.img:
	truncate -s $(IMG_SIZE) $@

nvme.img:
	truncate -s $(IMG_SIZE) $@

kernel:
	cargo +nightly build $(CARGO_BUILD_FLAGS) -Z build-std=core,compiler_builtins,alloc --target 86_64.json

iso: kernel images
	rm -rf $(ISO_BOOT_DIR)
	rm -f $(ISO_PATH)
	mkdir -p $(ISO_BOOT_DIR)
	cp $(KERNEL_BIN) $(ISO_BOOT_DIR)/TRUEOS.elf
	strip -s $(ISO_BOOT_DIR)/TRUEOS.elf || true
	cp $(LIMINE_CFG) $(ISO_BOOT_DIR)/limine.conf
	# Stage UEFI netboot files in $(ISO_DIR) for pxe.js (dnsmasq TFTP root).
	mkdir -p $(ISO_DIR)/EFI/BOOT
	cp $(LIMINE_SHARE)/BOOTX64.EFI $(ISO_DIR)/EFI/BOOT/BOOTX64.EFI
	cp $(LIMINE_CFG) $(ISO_DIR)/EFI/BOOT/limine.conf
	cp $(ISO_BOOT_DIR)/TRUEOS.elf $(ISO_DIR)/TRUEOS.elf
	# Also put a standard UEFI removable-media path in the ISO9660 tree as a
	# fallback. Some firmware/OVMF builds will boot this path instead of (or
	# before) the El Torito ESP image.
	mkdir -p $(ISO_BOOT_DIR)/EFI/BOOT
	cp $(LIMINE_SHARE)/BOOTX64.EFI $(ISO_BOOT_DIR)/EFI/BOOT/BOOTX64.EFI
	cp $(LIMINE_CFG) $(ISO_BOOT_DIR)/EFI/BOOT/limine.conf
	rm -f $(ISO_BOOT_DIR)/$(ISO_EFI_IMG)
	dd if=/dev/zero of=$(ISO_BOOT_DIR)/$(ISO_EFI_IMG) bs=1M count=31
	# NOTE: Keep this image < 65535*512 bytes to satisfy El Torito load-size limits.
	# That size is too small to be a standards-compliant FAT32 volume (min 65525 clusters).
	# Use FAT16 here so UEFI and Limine can reliably read limine.conf and the kernel.
	mkfs.vfat -F 16 -n TRUEOS_EFI $(ISO_BOOT_DIR)/$(ISO_EFI_IMG)
	mmd -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) ::/EFI ::/EFI/BOOT
	mcopy -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) $(ISO_BOOT_DIR)/limine.conf ::/limine.conf
	mcopy -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) $(ISO_BOOT_DIR)/limine.conf ::/EFI/BOOT/limine.conf
	mcopy -i $(ISO_BOOT_DIR)/$(ISO_EFI_IMG) $(ISO_BOOT_DIR)/TRUEOS.elf ::/TRUEOS.elf
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

dbg: iso-debug
	@($(QEMU_ISO) -s -S & $(SERIAL_CONSOLE_CMD))

# Boot the installed disk image directly (no installer ISO).
# Useful for validating GPT+ESP+Limine stage installation.
QEMU_DISK_COMMON_FLAGS = -debugcon stdio -m 2000M -smp cores=4 -cpu qemu64,phys-bits=39 -serial tcp:127.0.0.1:5555,server,nowait
QEMU_DISK_DRIVE_FLAGS = -drive file=disk.img,if=virtio,format=raw

run-installed-uefi: iso-debug
	@($(QEMU_BIN) -bios $(QEMU_UEFI_FIRMWARE) $(QEMU_DISK_COMMON_FLAGS) $(QEMU_NET_FLAGS) $(QEMU_RNG_FLAGS) $(QEMU_DISK_DRIVE_FLAGS) & $(SERIAL_CONSOLE_CMD))