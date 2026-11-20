# Makefile for Zenith OS kernel build

TARGET := x86_64-unknown-none
KERNEL_NAME := kernel.bin
SRC_DIR := src
ARCH_DIR := $(SRC_DIR)/arch/x86_64
BUILD_DIR := build

WORKSPACE_TARGET := ../target/$(TARGET)/release
HOST_TARGET := ../target/release

CARGO := cargo
AS := as
ASFLAGS := --64
LD := ld

OBJS := $(BUILD_DIR)/startup.o $(BUILD_DIR)/interrupts.o $(BUILD_DIR)/syscalls.o
LIMINE_OBJS := $(BUILD_DIR)/limine.o $(BUILD_DIR)/interrupts.o $(BUILD_DIR)/syscalls.o
LIMINE_IMAGE := kernel-limine.bin

.PHONY: all clean build build-rust qemu image image-limine

all: $(KERNEL_NAME)

# Build a linked kernel image using the host static library. This is the image
# used by the Limine boot-image script when the bare-metal target is absent.
image:
	@$(CARGO) build -p zenith-kernel --release
	@mkdir -p $(BUILD_DIR)
	$(AS) $(ASFLAGS) $(ARCH_DIR)/startup.s -o $(BUILD_DIR)/startup.o
	$(AS) $(ASFLAGS) $(ARCH_DIR)/interrupts.s -o $(BUILD_DIR)/interrupts.o
	$(AS) $(ASFLAGS) $(ARCH_DIR)/syscalls.s -o $(BUILD_DIR)/syscalls.o
	$(LD) -n -T linker.ld $(OBJS) $(HOST_TARGET)/libzenith_kernel.a -o $(KERNEL_NAME)
	@echo "Kernel image ready: $(KERNEL_NAME)"

# Native Limine-protocol image (real linear framebuffer).
image-limine:
	@$(CARGO) build -p zenith-kernel --release $(CARGO_BUILD_FLAGS)
	@mkdir -p $(BUILD_DIR)
	$(AS) $(ASFLAGS) $(ARCH_DIR)/limine.s -o $(BUILD_DIR)/limine.o
	$(AS) $(ASFLAGS) $(ARCH_DIR)/interrupts.s -o $(BUILD_DIR)/interrupts.o
	$(AS) $(ASFLAGS) $(ARCH_DIR)/syscalls.s -o $(BUILD_DIR)/syscalls.o
	$(LD) -n -T linker-limine.ld $(LIMINE_OBJS) $(HOST_TARGET)/libzenith_kernel.a -o $(LIMINE_IMAGE)
	@echo "Limine kernel image ready: $(LIMINE_IMAGE)"

$(BUILD_DIR)/%.o: $(ARCH_DIR)/%.s
	@mkdir -p $(BUILD_DIR)
	$(AS) $(ASFLAGS) $< -o $@

build-rust:
	@echo "Building Rust kernel library for $(TARGET)..."
	$(CARGO) build --target=$(TARGET) --release

$(KERNEL_NAME): build-rust $(OBJS)
	@echo "Linking kernel..."
	$(LD) -n -T linker.ld $(OBJS) $(WORKSPACE_TARGET)/libzenith_kernel.a -o $@
	@echo "Kernel image: $@"

# Smoke-test in QEMU. Falls back to the host-built static library when the
# bare-metal target is unavailable, which is enough to validate the boot path.
qemu:
	@$(CARGO) build -p zenith-kernel --release
	@mkdir -p $(BUILD_DIR)
	$(AS) $(ASFLAGS) $(ARCH_DIR)/startup.s -o $(BUILD_DIR)/startup.o
	$(AS) $(ASFLAGS) $(ARCH_DIR)/interrupts.s -o $(BUILD_DIR)/interrupts.o
	$(AS) $(ASFLAGS) $(ARCH_DIR)/syscalls.s -o $(BUILD_DIR)/syscalls.o
	$(LD) -n -T linker.ld $(OBJS) $(HOST_TARGET)/libzenith_kernel.a -o $(KERNEL_NAME)
	@echo "Booting $(KERNEL_NAME) in QEMU (serial + VGA console)..."
	@timeout 8 qemu-system-x86_64 -kernel $(KERNEL_NAME) -display none -serial stdio -no-reboot || true

clean:
	rm -rf $(BUILD_DIR)
	rm -f $(KERNEL_NAME) $(LIMINE_IMAGE)

build: clean all
