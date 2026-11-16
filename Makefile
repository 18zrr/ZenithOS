# Makefile for Zenith OS

TARGET := x86_64-unknown-none

.PHONY: all clean build_kernel build_userland test check

all: build_userland build_kernel

build_userland:
	@echo "Building userland applications..."
	cargo build --release -p filemanager -p systemsettings -p calculator \
		-p browser -p mediaplayer -p systemmonitor -p terminal

build_kernel:
	@echo "Building kernel..."
	./build_kernel.sh

test:
	./test_build.sh

check:
	cargo check --workspace

clean:
	rm -rf target kernel/build kernel/kernel.bin
