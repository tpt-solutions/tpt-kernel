# Root server design (draft for review)

Status: draft. Owner: tpt-k-root. Covers M2 items 3 to 5 of `todo.md`: the root
server, interrupt-capability grants from the root, and ELF-loaded children.

## 1. What the root is

The root server is the first user-space process. The kernel starts it with a
fixed set of capabilities, and from then on it is the only component that
creates, grants and revokes capabilities for other processes. Drivers, services
and userland all receive their authority from the root, which keeps the
capability graph auditable in one place.

Today the root is a stub (`crates/tpt-k-root/src/main.rs`, 4 lines), and the
kernel demo plays its part with hand-written assembly (`root_program`). This
design replaces that with a real process.

## 2. Initial capabilities

The kernel hands the root, through its entry arguments and a boot record:

| Capability | Object | Rights | Purpose |
|---|---|---|---|
| `spawn` | spawn authority | create children | Start a child from an ELF image |
| `memory` | memory-object factory | create objects | Allocate pages for children and drivers |
| `irq[n]` | interrupt object for GSI `n` | READ | One per routed line (RTC today) |
| `channel` | root control channel | all | Talk to children |

The kernel creates these at boot and installs them in the root's table only.
Nothing else holds `spawn` or `irq`, so the root is the only grantor.

## 3. Spawn

`SYS_SPAWN_IMAGE` exists today and takes a position-independent image in the
caller's code pages. The root needs more than that:

1. **Load ELF.** Parse an ELF64 image (the parser exists in `tpt-k-boot-fs::elf`,
   host-tested) and place its `PT_LOAD` segments into fresh memory objects, at
   the segment addresses, in a new address space.
2. **Build the child space.** A new space forked from the kernel's boot space,
   with the segments mapped as user pages. This replaces the one-page image
   limit of `SYS_SPAWN_IMAGE`.
3. **Grant.** Copy the capabilities the child needs from the root's table into
   the child's table, with rights narrowed. Each copy keeps its origin, so the
   root can revoke a child's authority with `revoke_across`.
4. **Start.** Enter the child at its ELF entry point, with its capability
   handles as entry arguments.

Slots: the kernel has 2 spawn slots and 16 scheduler threads. The root needs
enough slots for its drivers and services, so the spawn slot count becomes a
parameter (target 8), and the scheduler limit is raised with it.

Program source: until a FAT reader exists (boot todo), the kernel image embeds
the root's ELF and the children's ELF images, and the root loads children from
those blobs. A later step reads them from the boot volume.

## 4. Interrupt capabilities

The root receives `irq[n]` objects from the kernel. When it starts a driver it
grants that driver the matching `irq[n]` with READ only. The driver waits with
`SYS_IRQ_WAIT`. Only the root can grant an interrupt object, so a driver cannot
claim a line it was not given.

This moves the interrupt grant out of the bootstrap (`kmain`), which today mints
the RTC object into the driver thread's table directly.

## 5. Memory

Children and drivers get memory from the root. A grant of a memory object
transfers the pages, and the root can later revoke them. Freeing objects (item 1
of the approved list) is a precondition for long-running services. Part of it is
built: `SYS_MEM_FREE` exists, and an object is reclaimed when its last handle is
revoked, with its mappings removed first (claim `MEM-1`). What is still open is
listed in `todo.md`.

## 6. Protocol with the kernel

The root uses only the existing syscall surface: `SYS_SPAWN_IMAGE` (later
`SYS_SPAWN_ELF`), `SYS_GRANT`, `SYS_REVOKE`, `SYS_MEM_CREATE`, `SYS_MAP`,
`SYS_SEND`/`SYS_RECV` (and the `_CAP` and `_BULK` forms), and `SYS_IRQ_WAIT`.
New kernel calls are added only when a root operation cannot be built from
these.

## 7. Milestones

1. **Root as a process.** Replace `root_program` with a ring-3 binary that
   receives its capabilities, spawns one child from an embedded image, and
   reports the child's exit. Test: the smoke test's existing lines stay, and the
   root's output is checked.
2. **ELF children.** Add `SYS_SPAWN_ELF` (or a loader in the root) and spawn a
   child from an embedded ELF with two segments. Test: the child reads a value
   from each segment.
3. **Interrupt grant.** The root grants `irq[8]` to the driver. Remove the
   bootstrap's direct mint. Test: the driver wakes, and a driver without the
   grant cannot wait on the line.
4. **Revocation.** The root revokes a child's copies. Test: the child's next
   syscall with the revoked handle fails.

## 8. Decisions needed

- **Where the root's code runs.** Ring 3 as a `no_std` binary (proposed), or the
  `std` host scaffold that `tpt-k-root` is today. Ring 3 is needed for the
  capability model to mean anything.
- **Embedded or loaded images.** Embedded until the boot volume reader exists
  (proposed).
- **Spawn slot count.** 8 (proposed), with a scheduler limit of 16 kept.

## 9. Out of scope here

Scheduling policy, the fabric intent model (`tpt-fabric`), and the userland
shell. The root only creates and grants authority. Policy about who should get
what comes later.
