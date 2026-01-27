BUILD_MODE ?= debug
KERNEL_BIN = target/86_64/$(BUILD_MODE)/TRUEOS


ISO_DIR 		:= bld
ISO_PATH 		:= bld/trueos.iso
LIMINE_CFG 		:= limine.conf
LIMINE_SRC 		:= limine
LIMINE_BUILD 	:= bld/limine-build
LIMINE_PREFIX 	:= bld/limine-prefix
LIMINE_STAMP 	:= $(LIMINE_BUILD)/.installed
LIMINE_SHARE 	:= $(LIMINE_PREFIX)/share/limine
LIMINE_BIN 		:= $(LIMINE_PREFIX)/bin/limine

LIMINE_CONFIG_ARGS := --prefix=$(abspath $(LIMINE_PREFIX)) --enable-bios --enable-uefi-x86-64 --enable-uefi-cd

DEPS_SCRIPT := scripts/fetch-deps.sh

QEMU = qemu-system-x86_64
QEMU_BIOS = $(firstword $(wildcard /usr/share/ovmf/OVMF.fd /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd))

QEMU_NET_FLAGS = -netdev user,id=net0,hostfwd=tcp::4243-:4243 -device e1000,netdev=net0 \
	-netdev user,id=net1,hostfwd=tcp::4244-:4244 -device rtl8139,netdev=net1 \
	-netdev user,id=net2,hostfwd=tcp::4245-:4245 -device virtio-net-pci,netdev=net2,disable-modern=on

# Provide a virtio entropy source (virtio-rng). We force transitional/legacy (disable-modern=on)
# to match the kernel's current legacy virtio-pci driver.
QEMU_RNG_FLAGS = -object rng-random,filename=/dev/urandom,id=rng0 \
	-device virtio-rng-pci,rng=rng0,disable-modern=on

QEMU_COMMON_FLAGS = -bios $(QEMU_BIOS) -cdrom $(ISO_PATH) -debugcon stdio -m 2000M -smp cores=4 -cpu qemu64,phys-bits=39 -serial tcp:127.0.0.1:5555,server,nowait $(QEMU_NET_FLAGS) $(QEMU_RNG_FLAGS)

QEMU_USB_FLAGS =  \
	-device nec-usb-xhci,id=xhci,p2=8,p3=8 \
	-device vfio-pci,host=0000:06:00.0 \
	-device usb-mouse,bus=xhci.0,port=1,id=usbmouse0 \
	-device usb-kbd,bus=xhci.0,port=2,id=usbkbd0 \
	-device usb-host,vendorid=0x303a,productid=0x1001,bus=xhci.0,port=3,id=usbhost0 \
	-device usb-host,vendorid=0x0951,productid=0x16a4,bus=xhci.0,port=4,id=usbhypx0 \
	-drive file=disk.img,if=none,format=raw,id=usbdisk0 \
	-device usb-storage,drive=usbdisk0,bus=xhci.0,port=5,id=usbms0 \
# -drive file=disk.img,if=none,format=raw,id=nvme0 \	
# -device nvme,drive=nvme0,serial=deadbeef \

QEMU += $(QEMU_COMMON_FLAGS) $(QEMU_USB_FLAGS)

$(LIMINE_STAMP):
	@$(DEPS_SCRIPT)
	@# Reconfigure/rebuild Limine if configure flags changed.
	@mkdir -p $(LIMINE_BUILD) $(LIMINE_PREFIX)
	@echo '$(LIMINE_CONFIG_ARGS)' > $(LIMINE_BUILD)/.config_args.new
	@if [ -f $(LIMINE_BUILD)/.config_args ]; then \
		if ! cmp -s $(LIMINE_BUILD)/.config_args $(LIMINE_BUILD)/.config_args.new; then \
			echo "Limine config changed; rebuilding..."; \
			rm -rf $(LIMINE_BUILD) $(LIMINE_PREFIX); \
			mkdir -p $(LIMINE_BUILD) $(LIMINE_PREFIX); \
		fi; \
	fi
	@mv -f $(LIMINE_BUILD)/.config_args.new $(LIMINE_BUILD)/.config_args
	@if [ ! -f $(LIMINE_SRC)/configure ]; then \
		if ! command -v autoreconf >/dev/null 2>&1; then \
			echo "error: Limine bootstrap requires 'autoreconf' (autoconf)."; \
			echo "hint: install autoconf + automake (and likely libtool), then retry."; \
			exit 1; \
		fi; \
		(cd $(LIMINE_SRC) && ./bootstrap); \
	fi
	cd $(LIMINE_BUILD) && $(abspath $(LIMINE_SRC))/configure \
		CC_FOR_TARGET=gcc \
		LD_FOR_TARGET=ld \
		OBJCOPY_FOR_TARGET=objcopy \
		OBJDUMP_FOR_TARGET=objdump \
		READELF_FOR_TARGET=readelf \
		$(LIMINE_CONFIG_ARGS)
	$(MAKE) -C $(LIMINE_BUILD)
	$(MAKE) -C $(LIMINE_BUILD) install
	@if [ -f $(LIMINE_BUILD)/bin/limine-bios-hdd.bin ]; then \
		mkdir -p $(LIMINE_SHARE); \
		cp $(LIMINE_BUILD)/bin/limine-bios-hdd.bin $(LIMINE_SHARE)/; \
	fi
	touch $(LIMINE_STAMP)

.PHONY: deps
deps:
	@$(DEPS_SCRIPT)

iso: deps $(LIMINE_STAMP)
	cargo +nightly build $(CARGO_BUILD_FLAGS) -Z build-std=core,compiler_builtins,alloc --target 86_64.json
	rm -rf $(ISO_DIR)/EFI $(ISO_DIR)/TRUEOS.elf $(ISO_DIR)/limine.conf $(ISO_DIR)/limine-uefi-cd.bin
	rm -f $(ISO_PATH)
	mkdir -p $(ISO_DIR)/EFI/BOOT
	cp $(KERNEL_BIN) $(ISO_DIR)/TRUEOS.elf
	cp $(LIMINE_CFG) $(ISO_DIR)/limine.conf
	cp $(LIMINE_SHARE)/BOOTX64.EFI $(ISO_DIR)/EFI/BOOT/BOOTX64.EFI
	cp $(LIMINE_SHARE)/limine-uefi-cd.bin $(ISO_DIR)/
	xorriso -as mkisofs \
		-iso-level 3 -full-iso9660-filenames \
		-R \
		-J -joliet-long \
		-m limine-build \
		-m limine-prefix \
		-m trueos.iso \
		--efi-boot limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		-o $(ISO_PATH) $(ISO_DIR) 

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
	@($(QEMU) & $(SERIAL_CONSOLE_CMD))

dbg: iso-debug
	@($(QEMU) -s -S & $(SERIAL_CONSOLE_CMD))