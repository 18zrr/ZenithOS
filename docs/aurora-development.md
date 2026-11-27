# ZenithOS v0.4.0 — Aurora development plan

Base: v0.3.2, commit `940c82f`. This is an incremental development branch.

| Order | Existing implementation and dependency | Work and acceptance gate |
|---|---|---|
| 1. Kernel accounting | The cooperative scheduler runs shell/desktop, heartbeat and idle on one CPU. The bump allocator counts allocations forever. `ProcessManager` is not the scheduler. | Reclaim kernel allocations, account time at context switches, sample task and volume statistics without disk I/O. Boot both images and rerun editor/persistence tests. |
| 2. Native desktop tools | Windows live in `kernel/src/wm.rs`; `gui/zenith-ui` supplies shared theme data. The separate `userland` crates aren't the apps launched by Start. | Build on the live Settings/Monitor views. Reuse existing controls. Limit history and refresh only while visible. Read old `desktop.prefs`; commit updates atomically. |
| 3. Application boundaries | The optional ELF runner has its own CPL3 bring-up path; it does not isolate desktop windows. | Define an ABI, bounded IPC and validated copies at the user boundary; test launch, fault, exit and cleanup before exposing user process control. Never offer termination of critical kernel tasks. |
| 4. Explorer and shell | File Manager already supports transactional writes, editor, properties and deletion confirmation on v5. | Add navigation/search first; implement clipboard and multi-file operations with explicit conflict and partial-failure handling. Then build notifications and Control Center on those services. |
| 5. Integration | Existing tests exercise saves, reboot, focus and unrecognized-disk preservation. | Verify settings across boots, monitor overhead, multi-window input and isolation where implemented. Record measured results and remaining limitations before release. |

Native desktop code remains privileged. Task CPU share describes the one
scheduled CPU; kernel heap usage is not whole-system physical RAM usage.
Window counts must not be reported as process counts.

## Implementation status

- **Kernel:** reclaiming heap, residency accounting, ATA transfer counters,
  directory validation and transactional moves. Mount checks directory names,
  reachability and unique inode ownership after replay. Small edits journal
  only touched directory and inode-table sectors. No FS migration required.
- **Applications:** eight Settings pages, bounded Monitor graphs and task
  filter/sort, responsive Explorer, navigation tabs and batch transfers.
- **Desktop:** snapping, text clipboard, Control Center, notification history.
- **User-mode foundation:** [experimental ABI and executable example](native-app-abi.md).
  A boot module can read statistics and notify the desktop; interactive
  windows, independent concurrent user apps and general IPC remain future work.
- **Limits:** no recursive directory copies, multi-volume Explorer, GPU
  utilization, per-process RSS, or virtual workspaces. Transfers stop at the
  first failed item; atomicity is per item, not across the entire batch.
