# ctop

A minimal, calm terminal monitor for Linux on Intel CPUs. It shows four things
and nothing else:

- per-core CPU usage
- each core's temperature, right next to its usage
- RAM usage (with page cache as a lighter segment)
- swap usage

There is no process list. It is a dashboard you can read at a glance, not an
htop replacement.

Text capture from an i5-8500T with two busy cores (colors not shown):

```
╭ ctop ────────────────────────────────────────────────────────────────────────────────────╮
│  Intel Core i5-8500T @ 2.10GHz                                                up 5h 42m  │
│  CPU  34%    Package 61°C                                                           ▃▄▃  │
╰──────────────────────────────────────────────────────────────────────────────────────────╯
╭ Cores ────────────────────────────────────────────────────────────── 6 cores · 6 threads ╮
│  cpu0   ███████████████████████████████████████████████████████████████████ 100%   61°C  │
│  cpu1   ▋··································································   1%   49°C  │
│  cpu2   ██▋································································   4%   50°C  │
│  cpu3   ███████████████████████████████████████████████████████████████████ 100%   58°C  │
│  cpu4   █▍·································································   2%   47°C  │
│  cpu5   ···································································   0%   46°C  │
╰──────────────────────────────────────────────────────────────────────────────────────────╯
╭ Memory ───────────────────────────────────────────────────────────────── █ used  █ cache ╮
│  RAM   ███████████████████████████▍███████████████████████▎    8.1 GiB / 15.3 GiB   53%  │
│  Swap  ····················································    0.0 MiB / 12.3 GiB    0%  │
╰──────────────────────────────────────────────────────────────────────────────────────────╯

  q quit    +/- interval                                                          every 1s
```

With Hyper-Threading, sibling threads are drawn as a bracketed pair. On hybrid
CPUs (Alder Lake and newer), P-cores and E-cores get their own groups. This is
the test render of the 12700H fixture at 120 columns, with every core 60 % busy:

```
╭ Cores ─────────────────────────────────────────────────────────────────────────────────── 6P + 8E cores · 20 threads ╮
│  P  Performance   6 cores · 12 threads                                                                               │
│  cpu0  ╭ █████████████████████··············  60%   50°C    cpu6  ╭ █████████████████████··············  60%   62°C  │
│  cpu1  ╰ █████████████████████··············  60%   50°C    cpu7  ╰ █████████████████████··············  60%   62°C  │
│  cpu2  ╭ █████████████████████··············  60%   54°C    cpu8  ╭ █████████████████████··············  60%   66°C  │
│  cpu3  ╰ █████████████████████··············  60%   54°C    cpu9  ╰ █████████████████████··············  60%   66°C  │
│  cpu4  ╭ █████████████████████··············  60%   58°C    cpu10 ╭ █████████████████████··············  60%   70°C  │
│  cpu5  ╰ █████████████████████··············  60%   58°C    cpu11 ╰ █████████████████████··············  60%   70°C  │
│                                                                                                                      │
│  E  Efficiency   8 cores · 8 threads                                                                                 │
│  cpu12   █████████████████████··············  60%   34°C    cpu16   █████████████████████··············  60%   38°C  │
│  cpu13   █████████████████████··············  60%   35°C    cpu17   █████████████████████··············  60%   39°C  │
│  cpu14   █████████████████████··············  60%   36°C    cpu18   █████████████████████··············  60%   40°C  │
│  cpu15   █████████████████████··············  60%   37°C    cpu19   █████████████████████··············  60%   41°C  │
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

## Build and run

You need a stable Rust toolchain, 1.88 or newer.

```sh
cargo build --release
./target/release/ctop
```

Or install it into `~/.cargo/bin`:

```sh
cargo install --path .
```

It runs as a normal user. Root is never needed.

| Key                  | Action                                         |
|----------------------|------------------------------------------------|
| `q`, `Esc`, `Ctrl-C` | quit                                           |
| `+` (or `=`)         | longer refresh interval                        |
| `-`                  | shorter refresh interval                       |

The interval steps through 250 ms, 500 ms, 750 ms, 1 s (the default), 1.5 s, 2 s,
3 s, 4 s and 5 s. The current value is shown in the footer.

### Options and environment

- `ctop --once` prints one plain-text snapshot to stdout and exits. It takes two
  samples 0.5 s apart, so the usage figures are real. Useful for scripts and for
  checking what the data layer sees.
- `NO_COLOR=1 ctop` turns off all color. Bars and text stay; the cache segment
  is drawn as `░` so it can still be told apart from used memory.
- `COLORTERM=truecolor` (or `24bit`) enables 24-bit color. Without it, colors
  are mapped to the nearest xterm-256 entry.

## Tests

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

All tests run on string fixtures held in an in-memory filesystem. They never
read the live `/proc` or `/sys`, so they pass without root, without Intel
hardware, and inside containers. The fixtures describe whole machines:

- a quad-core i7 with HT
- a hybrid i7-12700H (6P + 8E)
- a KVM guest with no hwmon, where one CPU is missing its topology files
- a laptop with no swap
- a dual-socket box where both packages have a `Core 0`

UI tests render into ratatui's `TestBackend` and check the layout, column flow,
HT pairing, the overflow note, the "too small" screen, and that `NO_COLOR` leaves
every cell uncolored.

## Design notes

### Data sources

Every value comes from plain text files, read with `std::fs`.

| Module        | Source                                    | Notes |
|---------------|-------------------------------------------|-------|
| `cpu.rs`      | `/proc/stat`, `/proc/cpuinfo`             | busy = user+nice+system+irq+softirq+steal, idle = idle+iowait; usage = Δbusy / (Δbusy+Δidle). CPUs are keyed by number, so a CPU going offline never shifts the others. |
| `temp.rs`     | `/sys/class/hwmon/hwmon*/`                | Finds `coretemp` by the `name` file, never by hwmon number. `Package id X` and `Core N` labels; `temp*_max` / `temp*_crit` give the thresholds. |
| `topology.rs` | `/sys/devices/system/cpu/cpuN/topology/`, `/sys/devices/cpu_{core,atom}/cpus` | `core_id` and `physical_package_id`; P/E detection; CPU range-list parser (`0-3,8,10-11`). |
| `mem.rs`      | `/proc/meminfo`                           | used = MemTotal − MemAvailable; cache = Buffers + Cached + SReclaimable − Shmem; swap used = SwapTotal − SwapFree. |
| `collect.rs`  | `/proc/uptime`                            | Combines everything into a plain `Snapshot`. |

`sysfs.rs` holds the small `Fs` trait (read a file, list a directory). The
collectors read through it, which is what lets the tests use in-memory
fixtures.

### Mapping a logical CPU to its temperature

`coretemp` reports one temperature per physical core, labelled by `core_id`, not
by logical CPU number. On a 12700H, for example, cpu2 is `Core 4` and E-core
cpu19 is `Core 31`. Each logical CPU is therefore resolved through
`topology/core_id` and `physical_package_id` to the key `(package, core)`.
Hyper-Threading siblings share that key, so they share a reading. On multi-socket
machines, each coretemp instance's `Package id` label says which package its
cores belong to.

If there is no coretemp (an AMD CPU, a VM, or the module not loaded), usage,
memory and swap still work, and temperatures show `—`.

### Colors relative to the chip's own limits

Temperature color is not tied to fixed numbers. Each reading carries its
sensor's `high` (`temp*_max`) and `crit` (TjMax, `temp*_crit`). The scale runs
from cool at 40 % of TjMax to hot at `high`. Readings at or above `crit` are also
shown in bold. If the files are missing, TjMax falls back to 100 °C and `high`
to TjMax − 10.

Usage bars use a green → yellow → orange → red gradient. Each cell is colored by
its position along the bar, so a full bar shows the whole scale, and a mostly
idle core stays calm green.

The palette is muted (Nord-like), so the screen stays quiet and the numbers
stand out.

### Bars and layout

- Bars use the eighth blocks `▏▎▍▌▋▊▉█`, which gives 1/8-cell precision. In
  stacked bars (RAM used + cache), the boundary cell uses the next segment's
  color as its background, so the edge stays smooth.
- Cards are only as tall as their content, and on machines with fewer than 16
  logical CPUs the dashboard is capped at 120 columns and centered.
- Cores flow into columns column-major. With 16 or more logical CPUs there are
  at least two columns. More are added only when the rows wouldn't fit the
  height, and no column is narrower than 34 cells. Column breaks always fall
  between physical cores, so an HT pair is never split.
- If even the widest layout can't show every core, the visible ones are a
  contiguous run and a note says how many are hidden. Below 50×12, a friendly
  "terminal too small" message replaces the layout.

### Runtime behaviour

- The loop blocks in `crossterm::event::poll` until a key, a resize, or the next
  tick, so there is no busy loop. Coretemp discovery runs once at startup, and
  each tick re-reads only `temp*_input`. Topology is re-read only when the set
  of online CPUs changes. Measured overhead is about 0.3 % of one core at the
  1 s default (27 ms of CPU time over 10 s), and about 1 % at 250 ms.
- The first frame says "measuring…" instead of showing a fake 0 %. The second
  sample is taken after 250 ms, so real numbers appear almost at once.
- The terminal is restored (raw mode off, alternate screen left, cursor shown)
  in three cases: by a drop guard on normal exit and on `?` errors, and by a
  panic hook. The hook restores the terminal *before* printing the panic
  message, so the message stays visible.

### Safety and robustness

- The crate root has `#![forbid(unsafe_code)]`. There is no FFI and no `libc` of
  our own.
- There is no `unwrap()` / `expect()` on runtime data. A missing or malformed
  file makes that one field "unavailable" or `—`; it never stops the program.
  For example, an unreadable `/proc/meminfo` shows "unavailable" in the memory
  card, and one unreadable sensor drops only that core's temperature.

### Dependencies

Only `ratatui` and `crossterm`, nothing else. `ratatui` is built with
`default-features = false` and only the `crossterm_0_29` backend feature, which
drops the macros, the layout cache and extra widgets. It is pinned to the same
crossterm 0.29 that `ctop` uses directly, so only one copy is compiled.

## Limitations

- `SIGTERM` / `SIGHUP` end the process without the cleanup above, so if ctop is
  killed from outside, run `reset`. Catching signals would need `libc` or a crate
  such as `signal-hook`, which the no-FFI, minimal-dependency rules exclude.
  Keyboard exits and panics are covered.
- Temperatures are Intel `coretemp` only, by design. Other hwmon drivers
  (`k10temp`, `acpitz`, …) are ignored.

## License

MIT. See [LICENSE](LICENSE).
