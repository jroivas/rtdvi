PROFILE ?= debug
OUT := target/wasm32-unknown-emscripten/$(PROFILE)/mlua_wasm.wasm

# ── Binaryen (wasm-opt) ───────────────────────────────────────────────────────
# Download binaryen 129 once and cache under target/binaryen/.

BINARYEN_VERSION := 129
BINARYEN_DIR     := target/binaryen
BINARYEN_STAMP   := $(BINARYEN_DIR)/.stamp-$(BINARYEN_VERSION)
WASM_OPT         := $(BINARYEN_DIR)/bin/wasm-opt

ARCH := $(shell uname -m)
OS   := $(shell uname -s | tr '[:upper:]' '[:lower:]')
ifeq ($(OS),darwin)
  OS := macos
endif
BINARYEN_URL := https://github.com/WebAssembly/binaryen/releases/download/version_$(BINARYEN_VERSION)/binaryen-version_$(BINARYEN_VERSION)-$(ARCH)-$(OS).tar.gz

# ── Targets ───────────────────────────────────────────────────────────────────

.PHONY: all clean
.DEFAULT_GOAL := all

all: $(BINARYEN_STAMP)
	# -fwasm-exceptions: switch C++ exception handling from JS invoke_* trampolines
	# to native WASM EH (try_table/exnref in LLVM 23). mlua-sys compiles Lua as
	# C++ with -fexceptions; -fwasm-exceptions overrides that to use WASM EH,
	# eliminating the invoke_* imports that require a JS host.
	EMCC_CFLAGS="-fwasm-exceptions" cargo build $(if $(filter release,$(PROFILE)),--release,)
	@echo "[wasm-opt] translating old-format EH → exnref …"
	$(WASM_OPT) \
	  --translate-to-exnref \
	  --enable-exception-handling \
	  --enable-reference-types \
	  $(OUT) -o $(OUT)
	@echo "[done] $(OUT)"

$(BINARYEN_STAMP):
	@mkdir -p $(BINARYEN_DIR)
	@echo "[binaryen] downloading version $(BINARYEN_VERSION) for $(ARCH)-$(OS) …"
	@curl -fsSL "$(BINARYEN_URL)" \
	  | tar -xz --strip-components=1 -C $(BINARYEN_DIR)
	@touch $@

clean:
	cargo clean
	rm -rf $(BINARYEN_DIR)
