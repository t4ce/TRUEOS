PYTHON ?= python3
CLANG ?= clang
BUILD_DIR ?= ../../bld/intel-gpu-primitives-cloud

.PHONY: test cloud-verify bake-adls-candidates clean

test:
	$(PYTHON) -B -m unittest discover -s . -p 'test_*.py'

cloud-verify: test
	CLANG="$(CLANG)" $(PYTHON) -B cloud_verify.py --out-dir "$(BUILD_DIR)"

bake-adls-candidates:
	PYTHON="$(PYTHON)" ./bake_adls_candidates.sh

clean:
	rm -rf "$(BUILD_DIR)" __pycache__
