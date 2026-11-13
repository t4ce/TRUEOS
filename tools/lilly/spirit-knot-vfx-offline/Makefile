PYTHON ?= python3
REPO_ROOT := ../../..
OUTPUT ?= $(REPO_ROOT)/bld/spirit-knot-preview.html

.PHONY: all run
all: $(OUTPUT)

$(OUTPUT): build_preview.py preview.html knot.frag $(REPO_ROOT)/tools/Lilly.tar.lz4
	$(PYTHON) build_preview.py --output $(OUTPUT)

run: all
	xdg-open $(OUTPUT)
