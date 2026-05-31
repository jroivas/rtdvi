# rtdvi — convenience targets.
#
# The canonical command is `rtdvi`. `rvi` is an OPT-IN short alias: it is a
# real historical command (restricted vi) and the `r*` restricted-editor
# family ships on most systems, so we never install it by default — you ask
# for it explicitly with `make rvi-alias`.

CARGO_HOME ?= $(HOME)/.cargo
CARGO_BIN  := $(CARGO_HOME)/bin

.PHONY: build test install rvi-alias rvi-unalias help

help:
	@echo "Targets:"
	@echo "  build         cargo build --release"
	@echo "  test          cargo test"
	@echo "  install       cargo install --path . (installs 'rtdvi')"
	@echo "  rvi-alias     opt-in: symlink 'rvi' -> rtdvi in $(CARGO_BIN)"
	@echo "  rvi-unalias   remove the 'rvi' symlink"

build:
	cargo build --release

test:
	cargo test

install:
	cargo install --path .

# Opt-in short command. Relative symlink within the bin dir so it keeps
# working if the directory is moved. Refuses to clobber a real binary that
# isn't already our symlink (e.g. a system restricted-vi).
rvi-alias:
	@test -x "$(CARGO_BIN)/rtdvi" || { echo "rtdvi not installed — run 'make install' first"; exit 1; }
	@if [ -e "$(CARGO_BIN)/rvi" ] && [ ! -L "$(CARGO_BIN)/rvi" ]; then \
		echo "refusing: $(CARGO_BIN)/rvi exists and is not a symlink"; exit 1; \
	fi
	ln -sf rtdvi "$(CARGO_BIN)/rvi"
	@echo "linked $(CARGO_BIN)/rvi -> rtdvi"

rvi-unalias:
	@if [ -L "$(CARGO_BIN)/rvi" ]; then rm -f "$(CARGO_BIN)/rvi"; echo "removed $(CARGO_BIN)/rvi"; \
	else echo "$(CARGO_BIN)/rvi is not our symlink — leaving it alone"; fi
