CC ?= cc
CFLAGS ?= -O2 -g
CFLAGS += -std=c11 -Wall -Wextra -Wpedantic
LDLIBS += -l:libOpenCL.so.1 -lpng16 -lm

REPO_ROOT := ../..
BUILD_DIR := $(REPO_ROOT)/bld/tools
PROGRAM := $(BUILD_DIR)/cpp_audio_visualizer_offline
OUTPUT ?= bld/cpp-audio-visualizer-1440p.png
WIDTH ?= 2560
HEIGHT ?= 1440
TIME ?= 14.25

.PHONY: all render clean

all: $(PROGRAM)

$(PROGRAM): main.c
	mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) $< -o $@ $(LDLIBS)

render: $(PROGRAM)
	cd $(REPO_ROOT) && bld/tools/cpp_audio_visualizer_offline \
		$(OUTPUT) $(WIDTH) $(HEIGHT) $(TIME)

clean:
	rm -f $(PROGRAM)
