# Build the markdown render plugin and install it for rtdvi.
#
#   make            # build the .wasm (release)
#   make install    # build + copy to ~/.local/rtdvi/plugins/
#   make test       # run the parser unit tests on the host

WASM := target/wasm32-unknown-unknown/release/markdown.wasm
DEST := $(HOME)/.local/rtdvi/plugins

.PHONY: build install test clean

build:
	cargo build --target wasm32-unknown-unknown --release

install: build
	mkdir -p $(DEST)
	cp $(WASM) $(DEST)/

test:
	cargo test

clean:
	cargo clean
