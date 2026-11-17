#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# QEMU boot smoke test (M1). Builds the kernel and the UEFI loader, stages an
# EFI System Partition directory, boots it under OVMF, and checks that the
# kernel prints "Hello tpt-k" and exits through the debug-exit device with the
# success code.
#
# Usage: scripts/qemu-smoke.sh
#
# Environment overrides:
#   QEMU         qemu-system-x86_64 binary
#   OVMF_CODE    OVMF code image, read-only
#   OVMF_VARS    OVMF vars template, copied per run
#   OUT          scratch directory (default: a fresh temp directory)
#   TIMEOUT      seconds before QEMU is killed (default: 60)

set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
TIMEOUT="${TIMEOUT:-60}"
OUT="${OUT:-$(mktemp -d)}"

# Exit status the kernel writes to the debug-exit port (0x10): QEMU reports
# (0x10 << 1) | 1.
EXPECTED_QEMU_STATUS=33
# Output lines the run must contain: the greeting, the breakpoint that proves
# the IDT dispatches traps and returns, the interleaved output of two demo
# threads (context switch and round robin), the capability check and the
# request/reply over endpoints, timer ticks, and the scheduler's completion line.
EXPECTED_TEXT=(
    "Hello tpt-k"
    "handoff: rsdp at 0x"
    "handoff: framebuffer "
    "ioapic: rtc irqs=2"
    "irq: driver woke"
    "paging: remap ok"
    "hello from ring 3"
    "apic: timer"
    "handles: process tables are isolated"
    "handles: grant attenuates rights"
    "user: exit 10"
    "user: exit 2"
    "user: exit 12648430"
    "spawn: child in slot"
    "spawn: image child in slot"
    "user: exit 7"
    "user: exit 90"
    "user: exit 5"
    "trap 3 (breakpoint)"
    "ipc: server got 1 2 3 4"
    "preempt: ticks="
    "scheduler: ok"
)

die() {
    echo "qemu-smoke: $*" >&2
    exit 2
}

# First existing file from the arguments, or empty.
first_file() {
    for candidate in "$@"; do
        if [ -n "$candidate" ] && [ -f "$candidate" ]; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

find_qemu() {
    if [ -n "${QEMU:-}" ]; then
        echo "$QEMU"
        return
    fi
    if command -v qemu-system-x86_64 >/dev/null 2>&1; then
        command -v qemu-system-x86_64
        return
    fi
    first_file "/c/Program Files/qemu/qemu-system-x86_64.exe" ||
        die "qemu-system-x86_64 not found; set QEMU"
}

# Windows QEMU needs Windows-style paths. Converts when cygpath is present.
native_path() {
    if command -v cygpath >/dev/null 2>&1; then
        cygpath -m "$1"
    else
        echo "$1"
    fi
}

QEMU_BIN="$(find_qemu)"
OVMF_CODE="${OVMF_CODE:-$(first_file \
    /usr/share/OVMF/OVMF_CODE_4M.fd \
    /usr/share/OVMF/OVMF_CODE.fd \
    /usr/share/edk2/x64/OVMF_CODE.4m.fd \
    /usr/share/edk2-ovmf/x64/OVMF_CODE.fd \
    "/c/Program Files/qemu/share/edk2-x86_64-code.fd")}"
OVMF_VARS="${OVMF_VARS:-$(first_file \
    /usr/share/OVMF/OVMF_VARS_4M.fd \
    /usr/share/OVMF/OVMF_VARS.fd \
    /usr/share/edk2/x64/OVMF_VARS.4m.fd \
    /usr/share/edk2-ovmf/x64/OVMF_VARS.fd \
    "/c/Program Files/qemu/share/edk2-i386-vars.fd")}"
[ -n "$OVMF_CODE" ] || die "OVMF code image not found; set OVMF_CODE"
[ -n "$OVMF_VARS" ] || die "OVMF vars template not found; set OVMF_VARS"

echo "qemu-smoke: building kernel"
(cd "$ROOT" && bash scripts/build-kernel.sh) || die "kernel build failed"
echo "qemu-smoke: building loader"
(cd "$ROOT" && cargo build -p tpt-k-boot --target x86_64-unknown-uefi) ||
    die "loader build failed"

KERNEL_ELF="$ROOT/target/x86_64-unknown-none/debug/tpt-k-kernel"
LOADER_EFI="$ROOT/target/x86_64-unknown-uefi/debug/tpt-k-boot.efi"
[ -f "$KERNEL_ELF" ] || die "missing $KERNEL_ELF"
[ -f "$LOADER_EFI" ] || die "missing $LOADER_EFI"

# The ESP layout the loader expects: the removable-media loader path and the
# kernel at TPTK\KERNEL.ELF.
ESP="$OUT/esp"
mkdir -p "$ESP/EFI/BOOT" "$ESP/TPTK" || die "cannot create $ESP"
cp "$LOADER_EFI" "$ESP/EFI/BOOT/BOOTX64.EFI" || die "copy loader failed"
cp "$KERNEL_ELF" "$ESP/TPTK/KERNEL.ELF" || die "copy kernel failed"
cp "$OVMF_CODE" "$OUT/code.fd" || die "copy OVMF code failed"
cp "$OVMF_VARS" "$OUT/vars.fd" || die "copy OVMF vars failed"

SERIAL="$OUT/serial.log"
rm -f "$SERIAL"

echo "qemu-smoke: booting (timeout ${TIMEOUT}s)"
timeout "$TIMEOUT" "$QEMU_BIN" \
    -machine q35 \
    -m 256M \
    -display none \
    -no-reboot \
    -serial "file:$(native_path "$SERIAL")" \
    -drive "if=pflash,format=raw,unit=0,readonly=on,file=$(native_path "$OUT/code.fd")" \
    -drive "if=pflash,format=raw,unit=1,file=$(native_path "$OUT/vars.fd")" \
    -drive "file=fat:rw:$(native_path "$ESP"),format=raw,media=disk" \
    -device "isa-debug-exit,iobase=0xf4,iosize=0x04"
status=$?

echo "qemu-smoke: QEMU exit status $status"
if [ -f "$SERIAL" ]; then
    echo "--- serial output ---"
    tr -d '\r' < "$SERIAL"
    echo "--- end serial output ---"
fi

if [ "$status" -ne "$EXPECTED_QEMU_STATUS" ]; then
    echo "qemu-smoke: FAIL (expected status $EXPECTED_QEMU_STATUS)" >&2
    exit 1
fi
for text in "${EXPECTED_TEXT[@]}"; do
    if ! grep -aq "$text" "$SERIAL" 2>/dev/null; then
        echo "qemu-smoke: FAIL (serial output lacks '$text')" >&2
        exit 1
    fi
done
# The first user process exits with 1. The text "user: exit 1" is also a prefix
# of "user: exit 10", so the check is anchored to the end of the line.
if ! grep -aq 'user: exit 1[[:space:]]*$' "$SERIAL"; then
    echo "qemu-smoke: FAIL (process 1 did not exit with its number)" >&2
    exit 1
fi
# The kernel reports the free frames it found in the loader's memory map.
if ! grep -aq 'memory: [1-9][0-9]* free frames' "$SERIAL"; then
    echo "qemu-smoke: FAIL (no free-frame count from the memory map)" >&2
    exit 1
fi
# The two workers print three tagged letters each. Their interleaving depends on
# timer ticks, so only the counts are fixed.
count_a="$(grep -ao '<A>' "$SERIAL" | wc -l | tr -d ' ')"
count_b="$(grep -ao '<B>' "$SERIAL" | wc -l | tr -d ' ')"
if [ "$count_a" != 3 ] || [ "$count_b" != 3 ]; then
    echo "qemu-smoke: FAIL (worker prints A=$count_a B=$count_b, expected 3 and 3)" >&2
    exit 1
fi
# The run must include at least one switch made by the timer.
if ! grep -aq 'timer switches=[1-9]' "$SERIAL"; then
    echo "qemu-smoke: FAIL (no timer-driven preemption in the serial output)" >&2
    exit 1
fi
echo "qemu-smoke: PASS"
