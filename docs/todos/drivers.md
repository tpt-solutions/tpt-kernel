# tpt-k-drivers — Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0`
Master plan and milestones: [tpt-kernel/todo.md](../tpt-kernel/todo.md) · Spec: [spec.txt](../tpt-kernel/spec.txt)

Scope: user-space drivers. Each driver runs as a WASM guest inside a driver host
that embeds `tpt-mcpbox`. The driver manager restarts drivers that crash.

Rules: drivers reach hardware only through capabilities granted by `tpt-k-root`.

---

## Phase 3 — Drivers & sandbox

- [ ] Driver-host process embedding `tpt-mcpbox` (WASM)
- [ ] Capability-checked WASM host functions: `mmio_read/write`, `dma_map`, `irq_wait`
- [ ] `tpt-k-drivers-virtio`: VirtIO console (or keyboard) driver
- [ ] Driver receives interrupts via IPC notification
- [ ] `tpt-k-drivers`: driver manager that monitors health and restarts crashed drivers

### Verification
- [ ] Test: misbehaving driver traps or crashes without a kernel panic
- [ ] Test: driver cannot access memory or IRQs outside its granted capabilities
- [ ] Milestone M3 (driver side)

## Phase 4 — Storage

- [ ] `tpt-k-drivers-virtio`: VirtIO-blk driver
- [ ] `tpt-k-drivers-nvme`: user-space NVMe driver
- [ ] Milestone M4 (driver side)

## Phase 7 — Networking

- [ ] VirtIO-net driver in `tpt-k-drivers-virtio`

## Phase 8 — Driver expansion

- [ ] `tpt-k-drivers-usb-hid`: USB HID driver (keyboard/mouse)
- [ ] `tpt-k-drivers-synth`: hardware-ID matching against known layouts
- [ ] `tpt-k-drivers-synth`: generate and load WASM micro-drivers dynamically
- [ ] Sandbox and capability review for generated drivers
- [ ] Datasheet-driven generation pipeline (AI/tooling boilerplate reviewed via `tpt-semadiff`)

## Phase 9 — Bare metal

- [ ] NVMe driver on real hardware
