CC ?= cc
CFLAGS ?= -O2 -g
CFLAGS += -std=c11 -Wall -Wextra -Wpedantic
LDLIBS += -l:libOpenCL.so.1 -lpng16 -lXrender -lX11 -lm

REPO_ROOT := ../..
BUILD_DIR := $(REPO_ROOT)/bld/tools
PROGRAM := $(BUILD_DIR)/spirit_sprite_vfx_offline
OUTPUT ?= bld/spirit-sprite-vfx-grid.png
TIME ?= 2.25

.PHONY: all run panel render clean

all: $(PROGRAM)

$(PROGRAM): main.c
	mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) $< -o $@ $(LDLIBS)

run: panel

panel: $(PROGRAM)
	cd $(REPO_ROOT) && bld/tools/spirit_sprite_vfx_offline --panel

render: $(PROGRAM)
	cd $(REPO_ROOT) && bld/tools/spirit_sprite_vfx_offline --render-grid $(OUTPUT) $(TIME)

clean:
	rm -f $(PROGRAM)
