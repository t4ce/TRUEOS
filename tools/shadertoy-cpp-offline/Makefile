CC ?= cc
CFLAGS ?= -O2 -g
CFLAGS += -std=c11 -Wall -Wextra -Wpedantic
CPPFLAGS += -DTRUEOS_REPO_ROOT='"$(abspath ../..)"'
LDLIBS += -l:libOpenCL.so.1 -l:libX11.so.6 -lm

REPO_ROOT := ../..
BUILD_DIR := $(REPO_ROOT)/bld/tools
PROGRAM := $(BUILD_DIR)/shadertoy_cpp_offline

.PHONY: all run test toolchain clean

all: $(PROGRAM)

$(PROGRAM): main.c
	mkdir -p $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< -o $@ $(LDLIBS)

run: $(PROGRAM)
	./run.sh

test:
	python3 -B -m unittest -v test_adapter.py

toolchain:
	./bootstrap_toolchain.sh

clean:
	rm -f $(PROGRAM)
