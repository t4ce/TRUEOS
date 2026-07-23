CC ?= cc
CFLAGS ?= -O2 -g
CFLAGS += -std=c11 -Wall -Wextra -Wpedantic
LDLIBS += -l:libOpenCL.so.1 -lpng16 -lm

REPO_ROOT := ../..
BUILD_DIR := $(REPO_ROOT)/bld/tools
PROGRAM := $(BUILD_DIR)/spirit_sprite_vfx_offline
OUTPUT ?= bld/spirit-sprite-vfx-grid.png
TIME ?= 2.25

.PHONY: all render clean

all: $(PROGRAM)

$(PROGRAM): main.c
	mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) $< -o $@ $(LDLIBS)

render: $(PROGRAM)
	cd $(REPO_ROOT) && bld/tools/spirit_sprite_vfx_offline $(OUTPUT) $(TIME)

clean:
	rm -f $(PROGRAM)
