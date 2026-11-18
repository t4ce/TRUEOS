CC ?= cc
CFLAGS ?= -O2 -g
CFLAGS += -std=c11 -Wall -Wextra -Wpedantic
LDLIBS += -l:libOpenCL.so.1 -lpng16 -lXrender -lX11 -lm

REPO_ROOT := ../../..
BUILD_DIR := $(REPO_ROOT)/bld/tools
PROGRAM := $(BUILD_DIR)/spirit_vfx_offline
BACKGROUND_ARTIFACT := $(REPO_ROOT)/crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_background_rgba8.spv
SPRITE_ARTIFACT := $(REPO_ROOT)/crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_sprite_rgba8.spv
OUTPUT ?= bld/spirit-vfx-grid.png
BACKGROUND_OUTPUT ?= bld/spirit-vfx-background-grid.png
PREFIX ?= bld/spirit-vfx-preview
TIME ?= 1.5
CLOCK ?= 36582
LILLY_SOURCE ?= $(abspath $(REPO_ROOT)/../Lily/LilyDS)

.PHONY: all assets run panel render panel-background render-background capture-background clean

all: $(PROGRAM) assets

assets:
	python3 prepare_lilly.py --source "$(LILLY_SOURCE)"

$(PROGRAM): main.c $(BACKGROUND_ARTIFACT) $(SPRITE_ARTIFACT)
	mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) $< -o $@ $(LDLIBS)

run: panel

panel: $(PROGRAM) assets
	cd $(REPO_ROOT) && bld/tools/spirit_vfx_offline --panel

render: $(PROGRAM) assets
	cd $(REPO_ROOT) && bld/tools/spirit_vfx_offline --render-grid $(OUTPUT) $(TIME)

panel-background: $(PROGRAM)
	cd $(REPO_ROOT) && bld/tools/spirit_vfx_offline --panel-background

render-background: $(PROGRAM)
	cd $(REPO_ROOT) && bld/tools/spirit_vfx_offline --render-background-grid "$(BACKGROUND_OUTPUT)" "$(TIME)" "$(CLOCK)"

capture-background: $(PROGRAM)
	cd $(REPO_ROOT) && bld/tools/spirit_vfx_offline --capture-background-grid "$(PREFIX)" "$(TIME)" "$(CLOCK)"

clean:
	rm -f $(PROGRAM)
