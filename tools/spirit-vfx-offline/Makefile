CC ?= cc
CFLAGS ?= -O2 -g
CFLAGS += -std=c11 -Wall -Wextra -Wpedantic
LDLIBS += -l:libOpenCL.so.1 -lpng16 -lXrender -lX11 -lm

REPO_ROOT := ../..
BUILD_DIR := $(REPO_ROOT)/bld/tools
PROGRAM := $(BUILD_DIR)/spirit_vfx_offline
BACKGROUND_ARTIFACT := $(REPO_ROOT)/crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_background_rgba8.spv
SPRITE_ARTIFACT := $(REPO_ROOT)/crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_sprite_rgba8.spv
OUTPUT ?= bld/spirit-vfx-grid.png
TIME ?= 1.5

.PHONY: all run panel render clean

all: $(PROGRAM)

$(PROGRAM): main.c $(BACKGROUND_ARTIFACT) $(SPRITE_ARTIFACT)
	mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) $< -o $@ $(LDLIBS)

run: panel

panel: $(PROGRAM)
	cd $(REPO_ROOT) && bld/tools/spirit_vfx_offline --panel

render: $(PROGRAM)
	cd $(REPO_ROOT) && bld/tools/spirit_vfx_offline --render-grid $(OUTPUT) $(TIME)

clean:
	rm -f $(PROGRAM)
