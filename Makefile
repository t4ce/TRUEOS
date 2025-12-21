CARGO := cargo
TARGET_JSON := 86_64.json
TARGET_DIR := target/86_64
BUILD_MODE := debug
KERNEL_BIN := $(TARGET_DIR)/$(BUILD_MODE)/falseos

ISO_DIR := bld/isofiles
ISO_PATH := bld/falseos.iso
LIMINE_CFG := limine.cfg
LIMINE_DIR := limine

.PHONY: iso run clean

iso:
	$(CARGO) +nightly build -Z build-std=core,compiler_builtins,alloc --target $(TARGET_JSON)
	rm -rf $(ISO_DIR)
	mkdir -p $(ISO_DIR)/EFI/BOOT
	cp $(KERNEL_BIN) $(ISO_DIR)/kernel.bin
	cp $(LIMINE_CFG) $(ISO_DIR)/limine.cfg
	cp $(LIMINE_DIR)/BOOTX64.EFI $(ISO_DIR)/EFI/BOOT/BOOTX64.EFI
	cp $(LIMINE_DIR)/limine-bios.sys $(ISO_DIR)/
	cp $(LIMINE_DIR)/limine-bios-cd.bin $(ISO_DIR)/
	cp $(LIMINE_DIR)/limine-uefi-cd.bin $(ISO_DIR)/
	xorriso -as mkisofs \
		-b limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		-o $(ISO_PATH) $(ISO_DIR)
	$(LIMINE_DIR)/limine bios-install $(ISO_PATH)

run: iso
	qemu-system-x86_64 -cdrom $(ISO_PATH) -m 2000M -smp cores=4 -debugcon stdio

clean:
	$(CARGO) clean
	rm -rf bld
