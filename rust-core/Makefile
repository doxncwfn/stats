ARCH := $(shell uname -m)
ifeq ($(ARCH),arm64)
	TARGET := aarch64-apple-darwin
else
	TARGET := x86_64-apple-darwin
endif

CARGO ?= $(HOME)/.cargo/bin/cargo
CARGO_ENV := PATH="$(HOME)/.cargo/bin:$(PATH)"

.PHONY: all build clean universal

all: build

build:
	$(CARGO_ENV) $(CARGO) build --release

universal:
	$(CARGO_ENV) $(CARGO) build --release --target aarch64-apple-darwin
	$(CARGO_ENV) $(CARGO) build --release --target x86_64-apple-darwin
	mkdir -p target/release
	lipo -create -output target/release/libstats_core.a \
		target/aarch64-apple-darwin/release/libstats_core.a \
		target/x86_64-apple-darwin/release/libstats_core.a

clean:
	$(CARGO_ENV) $(CARGO) clean
