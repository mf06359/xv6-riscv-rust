K=kernel
U=user

# All kernel code is now Rust — there are no `.S` files left, the
# entire kernel is a single object emitted by `cargo rustc -- --emit=obj`.
RUST_OBJS = \
  $K/rust_kernel.o

# riscv64-unknown-elf- or riscv64-linux-gnu-
# perhaps in /opt/riscv/bin
#TOOLPREFIX =

# Try to infer the correct TOOLPREFIX if not set
ifndef TOOLPREFIX
TOOLPREFIX := $(shell if riscv64-unknown-elf-objdump -i 2>&1 | grep 'elf64-big' >/dev/null 2>&1; \
	then echo 'riscv64-unknown-elf-'; \
	elif riscv64-elf-objdump -i 2>&1 | grep 'elf64-big' >/dev/null 2>&1; \
	then echo 'riscv64-elf-'; \
	elif riscv64-none-elf-objdump -i 2>&1 | grep 'elf64-big' >/dev/null 2>&1; \
	then echo 'riscv64-none-elf-'; \
	elif riscv64-linux-gnu-objdump -i 2>&1 | grep 'elf64-big' >/dev/null 2>&1; \
	then echo 'riscv64-linux-gnu-'; \
	elif riscv64-unknown-linux-gnu-objdump -i 2>&1 | grep 'elf64-big' >/dev/null 2>&1; \
	then echo 'riscv64-unknown-linux-gnu-'; \
	else echo "***" 1>&2; \
	echo "*** Error: Couldn't find a riscv64 version of GCC/binutils." 1>&2; \
	echo "*** To turn off this error, run 'gmake TOOLPREFIX= ...'." 1>&2; \
	echo "***" 1>&2; exit 1; fi)
endif

QEMU = qemu-system-riscv64
MIN_QEMU_VERSION = 7.2

# `LD` and `OBJDUMP` come from the riscv64 binutils package — we still
# need them for the final link with linker scripts and for symbol/asm
# dumps. `CC` (gcc) is no longer used: all kernel and user code is Rust,
# the previous `.S` files have been ported to `naked_asm!` / `global_asm!`.
LD = $(TOOLPREFIX)ld
OBJDUMP = $(TOOLPREFIX)objdump

CARGO ?= cargo
RUST_TARGET ?= riscv64gc-unknown-none-elf
CARGO_PROFILE ?= release
CARGO_TARGET_DIR ?= target
CARGO_DEPS_DIR  := $(CARGO_TARGET_DIR)/$(RUST_TARGET)/$(CARGO_PROFILE)/deps
CARGO_HOST_DIR  := $(CARGO_TARGET_DIR)/$(CARGO_PROFILE)
ifeq ($(CARGO_PROFILE),release)
CARGO_FLAG := --release
else
CARGO_FLAG :=
endif

LDFLAGS = -z max-page-size=4096

# ---------------------------------------------------------------------------
# Toolchain sanity check
# ---------------------------------------------------------------------------

check-rust-target:
	@rustup target list --installed | grep -qx "$(RUST_TARGET)" || \
		(echo "*** Error: missing rust target $(RUST_TARGET)."; \
		 echo "*** Run: rustup target add $(RUST_TARGET)"; \
		 exit 1)

# ---------------------------------------------------------------------------
# Kernel
# ---------------------------------------------------------------------------

$K/kernel: $(RUST_OBJS) $K/kernel.ld
	$(LD) $(LDFLAGS) -T $K/kernel.ld -o $K/kernel $(RUST_OBJS)
	$(OBJDUMP) -S $K/kernel > $K/kernel.asm
	$(OBJDUMP) -t $K/kernel | sed '1,/SYMBOL TABLE/d; s/ .* / /; /^$$/d' > $K/kernel.sym

# Build the kernel crate via cargo and copy the resulting object out of
# target/.../deps. The `--emit=obj` flag is passed through `cargo rustc`.
$K/rust_kernel.o: check-rust-target $K/Cargo.toml $(wildcard $K/rust_*.rs) Cargo.toml
	$(CARGO) rustc --target $(RUST_TARGET) $(CARGO_FLAG) -p kernel -- --emit=obj
	@cp -f $$(ls -t $(CARGO_DEPS_DIR)/rust_xv6_kernel-*.o | head -1) $@

# ---------------------------------------------------------------------------
# User binaries
# ---------------------------------------------------------------------------

# Each user binary is a workspace member named "user-<basename>", with a
# self-contained `cargo_root.rs` that re-mods the original ../<name>.rs plus
# the four runtime helpers (ulib/usys/printf/umalloc) via #[path].

# User .o file rule.
$U/%.o: $U/%/Cargo.toml $U/%/cargo_root.rs $U/%.rs $U/rust_user.rs $U/ulib.rs $U/usys.rs $U/printf.rs $U/umalloc.rs Cargo.toml
	$(CARGO) rustc --target $(RUST_TARGET) $(CARGO_FLAG) -p user-$* -- --emit=obj
	@cp -f $$(ls -t $(CARGO_DEPS_DIR)/user_$*-*.o | head -1) $@

# Default user binary link rule. The .o is self-contained, so no separate
# ULIB list is needed any more.
$U/_%: $U/%.o $U/user.ld
	$(LD) $(LDFLAGS) -T $U/user.ld -o $@ $<
	$(OBJDUMP) -S $@ > $U/$*.asm
	$(OBJDUMP) -t $@ | sed '1,/SYMBOL TABLE/d; s/ .* / /; /^$$/d' > $U/$*.sym

# forktest still uses the original "smallest possible" link layout: -N (set
# .text/.data/.bss read-write), entry at `main`, and base address 0. This
# was originally meant to keep the binary small enough to exhaust NPROC
# before exhausting memory; with the self-contained .o it's slightly bigger
# but still works.
$U/_forktest: $U/forktest.o
	$(LD) $(LDFLAGS) -N -e main -Ttext 0 -o $U/_forktest $U/forktest.o
	$(OBJDUMP) -S $U/_forktest > $U/forktest.asm

# ---------------------------------------------------------------------------
# mkfs (host tool, built against std for the host triple)
# ---------------------------------------------------------------------------

mkfs/mkfs: mkfs/Cargo.toml mkfs/mkfs.rs Cargo.toml
	$(CARGO) build $(CARGO_FLAG) -p mkfs
	@cp -f $(CARGO_HOST_DIR)/mkfs $@

# ---------------------------------------------------------------------------
# tags / clean / qemu
# ---------------------------------------------------------------------------

tags: $(OBJS) $(RUST_OBJS)
	etags kernel/*.S kernel/*.rs user/*.rs

# Prevent deletion of intermediate files, e.g. cat.o, after first build, so
# that disk image changes after first build are persistent until clean.  More
# details:
# http://www.gnu.org/software/make/manual/html_node/Chained-Rules.html
.PRECIOUS: %.o $U/%.o

UPROGS=\
	$U/_cat\
	$U/_echo\
	$U/_forktest\
	$U/_grep\
	$U/_init\
	$U/_isty\
	$U/_kill\
	$U/_ln\
	$U/_ls\
	$U/_mkdir\
	$U/_rm\
	$U/_sh\
	$U/_stressfs\
	$U/_usertests\
	$U/_grind\
	$U/_wc\
	$U/_zombie\
	$U/_logstress\
	$U/_forphan\
	$U/_dorphan\
	$U/_life\
	$U/_primes\

fs.img: mkfs/mkfs MEMO $(UPROGS)
	mkfs/mkfs fs.img MEMO $(UPROGS)

clean:
	$(CARGO) clean 2>/dev/null || true
	rm -f *.tex *.dvi *.idx *.aux *.log *.ind *.ilg \
	*/*.o */*.d */*.asm */*.sym \
	$K/kernel fs.img \
	mkfs/mkfs .gdbinit \
	$(UPROGS)

# try to generate a unique GDB port
GDBPORT = $(shell expr `id -u` % 5000 + 25000)
# QEMU's gdb stub command line changed in 0.11
QEMUGDB = $(shell if $(QEMU) -help | grep -q '^-gdb'; \
	then echo "-gdb tcp::$(GDBPORT)"; \
	else echo "-s -p $(GDBPORT)"; fi)
ifndef CPUS
CPUS := 3
endif

QEMUOPTS = -machine virt -bios none -kernel $K/kernel -m 128M -smp $(CPUS) -nographic
QEMUOPTS += -global virtio-mmio.force-legacy=false
QEMUOPTS += -drive file=fs.img,if=none,format=raw,id=x0
QEMUOPTS += -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0

qemu: check-qemu-version $K/kernel fs.img
	$(QEMU) $(QEMUOPTS)

.gdbinit: .gdbinit.tmpl-riscv
	sed "s/:1234/:$(GDBPORT)/" < $^ > $@

qemu-gdb: $K/kernel .gdbinit fs.img
	@echo "*** Now run 'gdb' in another window." 1>&2
	$(QEMU) $(QEMUOPTS) -S $(QEMUGDB)

print-gdbport:
	@echo $(GDBPORT)

QEMU_VERSION := $(shell $(QEMU) --version | head -n 1 | sed -E 's/^QEMU emulator version ([0-9]+\.[0-9]+)\..*/\1/')
check-qemu-version:
	@if [ "$(shell echo "$(QEMU_VERSION) >= $(MIN_QEMU_VERSION)" | bc)" -eq 0 ]; then \
		echo "ERROR: Need qemu version >= $(MIN_QEMU_VERSION)"; \
		exit 1; \
	fi
