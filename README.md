# **gdt-cpus** - Game Developer's Toolkit for CPU Management

<p align="center"><b><i>Pin it. Prioritize it. Dominate it.</i></b></p>

<p align="center">
  <a href="#-quick-flex"><img src="https://img.shields.io/badge/Rust-E57324?style=for-the-badge&logo=rust&logoColor=white" alt="Rust"></a>
  <a href="https://crates.io/crates/gdt-cpus"><img src="https://img.shields.io/crates/v/gdt-cpus.svg?style=for-the-badge&color=orange"></a>
  <a href="https://docs.rs/gdt-cpus"><img src="https://img.shields.io/badge/docs_rs-online-orange.svg?style=for-the-badge"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-orange?style=for-the-badge"></a>
</p>

---

You've got cores. *A lot of them.* Stop letting your OS babysit them like it's 2004.

With `gdt-cpus`, you **take control**. Hybrid architectures? P/E cores? SMT voodoo? Handled.

Windows, Linux, macOS? Handled.

Your ego? Also handled - this lib *knows* you're here to squeeze every last nanosecond.

---

## ✨ **Features That Actually Matter**

> *Telemetry dashboards? Out of scope for now.
> NUMA awareness? In the model - because crossing the wrong boundary is how frames go to die.*

* 🗺️ **CPU Topology? Got it.**

  Vendor, model, sockets, cores, logical threads, cache hierarchies. No more guessing what you're running on.

* 🧟‍♂️ **Hybrid-Aware like a Boss**

  Detect and exploit P-cores and E-cores. Be the scheduler your OS wishes it could be.

* 🪢 **Thread Affinity API**

  Pin threads to specific cores. Dominate cache locality. Laugh at poor thread migrations.

* 🎚️ **Thread Priority Control**

  From *lowly background* to *time-critical god mode*.

* 🎮 **Game-Dev First**

  You won't find some academic NUMA experiments here. Just *useful* tools for real-time workloads.

* 🧩 **C FFI Support**

  Because your C++ friends need to know how to party too. (Or just call `gdt-cpus-sys` directly.)
  With full CMake support. No more CMake hell. See `examples/c/basic_info` and `examples/c/priority` for details.

* 🛡️ **Minimal Dependencies You'll Regret**

  Core crate stays tiny: `bitflags`, plus the platform mouthpieces CPUs force on us.

  > (Okay fine… `raw-cpuid`, `windows`, `libc`, optional `serde` - because CPUs still mumble in syscalls.)

---

## 🚀 **Quick Flex**

```rust
use gdt_cpus::*;

fn main() -> gdt_cpus::Result<()> {
    let info = CpuInfo::detect()?;
    println!("Physical cores: {}", info.num_physical_cores());
    println!("Logical cores: {}", info.num_logical_cores());

    if let Err(e) = pin_thread_to_core(0) {
        println!("pinning skipped here: {e}");
    }
    let applied = set_thread_priority(ThreadPriority::AboveNormal)?;
    println!("priority: {applied}");

    if info.is_hybrid() {
        println!("P/E Cores? Oh, we're playing on expert difficulty.");
    }

    Ok(())
}
```

And yes, the topology data is not decorative. On a dual-CCD Ryzen 5950X, the
L3-domain example catches the cliff your scheduler will happily ignore:

```bash
$ cargo run --release --example l3_domains
CPU: AMD Ryzen 9 5950X 16-Core Processor - 16 cores / 32 threads, 2 L3 domain(s):
  domain 0:    32 MiB,  8 cores, 16 threads
  domain 1:    32 MiB,  8 cores, 16 threads

SMT siblings, one core      (lp   0 <-> lp  16):     24.9 ns/round-trip
Two cores, SAME L3 domain   (lp   0 <-> lp   1):     41.9 ns/round-trip
Two cores, CROSS L3 domains (lp   0 <-> lp   8):    191.3 ns/round-trip

Crossing the L3 fabric costs 4.6x an in-domain round trip; pin cooperating threads with l3_domain_mask().
```

---

## 🏎️ **Under The Hood: How We Tame The Silicon Beast**

`gdt-cpus` isn't just calling `num_cpus::get()`. That's for amateurs. We dive deep into OS-specific APIs so you don't have to:

| OS          | API Madness We Handle                                                                                     |
| ----------- | --------------------------------------------------------------------------------------------------------- |
| **Windows** | `GetLogicalProcessorInformationEx`, Registry, `SetThreadGroupAffinity`, CPU Sets                          |
| **Linux**   | `sysfs`, `/proc/cpuinfo`, `cpuid`, `sched_setaffinity`, `setpriority`, rtkit & realtime portal over D-Bus |
| **macOS**   | `sysctl`, QoS, `pthread_setschedparam` (Apple Silicon only)                                               |

All this pain, abstracted away into one beautiful, cross-platform Rust API. We do the dirty work. You reap the rewards.

> "Abstraction without insight is just hiding the problem. `gdt-cpus` gives you both."

---

## 🔥 **Know Your Cores: A Field Guide to Modern Silicon**

Modern CPUs ship a zoo, and the marketing names lie to you. `gdt-cpus` classifies every core into one of three honest kinds - by what the *kernel* says about them, not what the box art does:

### Performance - the raid team

The big ones. Intel P-cores, AMD's everything, Apple's P-cluster, ARM's big *and* medium cores (yes, both - a Cortex-A720 binned 200 MHz lower is still a raid-geared A720, not a different class). Main thread, render thread, simulation workers, anything with a deadline lives here.

### Efficiency - the dungeon levelers

Genuinely mid-tier cores: Intel E-cores (Gracemont and friends), Apple's E-cluster. Slower, but real workers - asset decompression, parallel number crunching that can wait, batch jobs. **Warning: this tier can be EMPTY.** Some chips jump straight from "raid team" to "guy selling fish by the bank" with nothing in between - always write the fallback (`efficiency_core_mask()` empty -> use Performance at `BelowNormal`).

### LpEfficiency - the fishing alts

The low-power island: ARM little cores parked at a fraction of max performance, Intel's LP E-cores on the SoC tile. These are NOT worker cores - they often sit behind weaker interconnects, and putting real work there slows the whole party down (one mini-PC vendor literally tells users to disable them in BIOS; `gdt-cpus` tells your *code* the same thing as data). Telemetry, autosave compression, platform callbacks - trickle work only.

### And within a kind: `perf_hint`

Kinds answer "what class of work" - `Lp::perf_hint` answers "which of these is the FASTEST". Ordinal, machine-local, and only comparable within the same detected machine and core kind. The source scale differs per OS (Linux `cpu_capacity`, Windows `EfficiencyClass`, macOS perflevel order), so treat equal values as indistinguishable and higher values as better, not as portable percentages. On a chip whose Performance tier spans four frequency bins, `max_by_key(perf_hint)` hands your render thread the prime core instead of a lottery ticket.

### The cheat sheet

| Your workload                    | Cores                                                  | Priority                          |
| -------------------------------- | ------------------------------------------------------ | --------------------------------- |
| Main / render thread             | best Performance (`perf_hint`), one per physical core  | `AboveNormal`-`Highest`           |
| Sim / job workers                | remaining Performance primaries, grouped per L3 domain | `Normal`                          |
| Audio / haptics feeder           | any Performance core, never the busiest one            | `TimeCritical` (dedicated thread) |
| Streaming / decompression        | Efficiency if present, else Performance                | `BelowNormal`                     |
| Shader/PSO compiles, bakes       | wherever there's room - throughput, not latency        | `Lowest`                          |
| Telemetry / autosave / callbacks | LpEfficiency island if present, else unpinned          | `Background`                      |

Three laws to rule them: **one heavy thread per physical core** (`primary_thread_mask()` - SMT siblings share execution units), **group cooperating threads by L3 domain** (`l3_domain_mask()` - crossing the fabric costs 3.6× on a dual-CCD Ryzen, we measured), and **don't pin what doesn't need pinning** (the scheduler is smarter than your spreadsheet; pin for latency or cache locality, leave the rest soft).

### Priority that actually works on Linux

Here's the dirty secret of every "set thread priority" crate: on a stock Linux desktop, **negative nice is often forbidden** - so `Highest` quietly becomes `Normal` and everyone pretends the scheduler is mysterious.

`gdt-cpus` does not pretend. `set_thread_priority()` returns an `AppliedPriority`: direct, brokered, clamped, refused, whatever actually happened. If rtkit can lift the thread, great. If policy says "nope", also great - now your engine knows instead of reading tea leaves from frame spikes.

True real-time is kept behind the big red button: `promote_thread_to_realtime(budget)`. That is the "preempt everything, wedge a core if you spin" tier, so the API makes you ask for it out loud.

The full playbook with code lives in the [crate docs](https://docs.rs/gdt-cpus). `gdt-cpus` gives you the intel. Using it to make your app scream (or sip power) is up to you.

---

## 📊 **Examples To Run On Your Hardware**

The examples are synthetic scheduler and topology experiments, not fixed-score
microbenchmarks. They print what this machine actually did, including priority
fallbacks, and compute the takeaway from that run:

| Example               | Command                                           | What it shows                                                                                   |
| --------------------- | ------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| **Basic info**        | `cargo run --example basic_info`                  | Flat LP topology, L3 domains, NUMA ids, per-kind caches, feature bits                           |
| **Thread priorities** | `cargo run --example thread_priorities`           | What each priority request really became: direct, brokered, clamped, fallback                   |
| **Audio latency**     | `cargo run --release --example audio_latency`     | The priority rung your feeder needs before buffers starve                                       |
| **Frame jitter**      | `cargo run --release --example frame_jitter`      | Why a pool sized to physical cores protects a 60 FPS render thread better than an SMT-wide pool |
| **Reserved core**     | `cargo run --release --example reserved_core`     | Why placement beats priority when a latency thread shares a core with hot work                  |
| **L3 domains**        | `cargo run --release --example l3_domains`        | The latency cliff from crossing CCD / L3-domain boundaries                                      |
| **Background budget** | `cargo run --release --example background_budget` | How wide CPU-heavy background work can run before the frame budget gets ugly                    |

Run them on your target hardware and treat the output as framing for your own
budgets, not as portable truth. Captured output from several machines lives in
`docs/examples/`.

---

## 🤔 **gdt-cpus vs. The "Alternatives" (Bless Their Hearts)**

Sure, there are other ways to poke at your CPU. If you like basic, or platform-locked, or just... less.

| Capability                     | `gdt-cpus` (The Pro) | `num_cpus` (The Intern) | `raw-cpuid` (The x86 Nerd) | OS APIs (The DIY Nightmare) |
| ------------------------------ | -------------------- | ----------------------- | -------------------------- | --------------------------- |
| Logical / physical counts      | yes                  | yes                     | current x86 CPU only       | bring snacks                |
| Flat LP topology               | yes                  | no                      | no                         | per-OS archaeology          |
| L3 domains / cache placement   | yes                  | no                      | partial x86                | bring a shovel              |
| P/E/LP-E core classification   | yes                  | no                      | no                         | platform roulette           |
| Affinity control               | hard + soft          | no                      | no                         | possible, enjoy the scars   |
| Priority outcome introspection | yes                  | no                      | no                         | not portable                |

> We ❤️ `num_cpus` - full respect!<br>
> Our brains just speak in sarcasm & memes 🤷‍♂️<br>
> (`num_cpus` paved the way for CPU introspection in Rust - `gdt-cpus` just straps a rocket to it. 🚀)

Choose wisely. Or just choose `gdt-cpus` and be done with it.

---

## 🧠 **The SWOT Analysis (Because We're "Strategic")**

### 💪 Strengths (Obvious Stuff)

* Deep CPU insights, cross-platform.
* P/E core aware. Your hybrid CPU will love you.
* Thread pinning & priority control that *works*.
* Foundation for god-tier task systems (hi, `gdt-jobs`!).
* C FFI via `gdt-cpus-sys`? Check. Your C++ will thank you.

### 📉 Weaknesses (If We *Must*)

* Not magic. You still gotta write good code on top.
* Apple Silicon affinity? Apple says "lol no". We report that accurately.
* Might be overkill if all you need is `num_cpus::get()`. (But why settle?)

### 🚀 Opportunities (World Domination Plans)

* Deeper NUMA policy helpers for server beasts.
* Even *more* detailed cache info. Because why not.
* Your favorite engine using `gdt-cpus` under the hood.

### ⚠️ Threats (The Competition... Kinda)

* OS schedulers *might* get smarter. Someday. Maybe.
* Someone writing an even *more* arrogant README. Unlikely.

---

## 🌐 **Proven on Real Hardware**

Tested across:

* 🐧 Linux with baremetal and containers (LXC-tested, yes, it even respects *your* artificial limits)
* 🪟 Windows (Hyper-Threading chaos? We navigate it.)
* 🍎 macOS (Apple and their obsession with Efficiency Cores; x86_64 macOS intentionally unsupported)

> Curious what it actually prints? Check out docs/examples/basic_info.md for full example output.

---

## **Versioning - CalVer, Deal With It**

> Wait, CalVer for a lib? Ya Idjits or something?<br>
> (Bobby Singer voice, obviously.)

Yep, we timestamp our releases instead of counting up semantic digits. Why? Because we're just built different. And because:

| CalVer Perk            | Why You Care                                                                                                                                                                                                                                                                                                                                                                     |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Instant age check      | `0.2608.0` -> August 2026. No need to diff tags to see if a crate is fossilized or fresh off the compiler.                                                                                                                                                                                                                                                                         |
| Honesty about breakage | New month? Could be a breaking change. You'll know from the number *and* from the migration guide in `docs/migrations/MIGRATION-0.2608.0.md`. We're not shy.                                                                                                                                                                                                                       |
| Works fine with Cargo  | The leading `0.` is load-bearing: in `0.x` land cargo treats every `x` as a breaking epoch, so `gdt-cpus = "0.2608"` pins the August 2026 API line and never silently upgrades you into a new month. (Bare `25.5 -> 25.12` looked like a "minor" bump to cargo and auto-delivered breakage - we learned, we fixed: that's why versions older than `0.2608.0` read like `25.12.0`.) |
| Less bike-shedding     | We'd rather spend time tuning work-steal loops and optimizing P/E core scheduling than debating whether the last commit was “minor” or “patch”. Priorities, people.                                                                                                                                                                                                              |

**TL;DR**:

Each year/month is an API epoch (`0.YYMM.patch`). If we break you, the migration doc shows the fix; if we don't, cargo update is painless.

And if we mess up, the date tells you exactly when to roast us in Issues. 😎

> (We're not idjits - just impatient.)

---

## **How Can I Contribute?**

Find something that's missing, broken, or just less performant than your standards require.

Open an issue. Bonus points if you make a PR. A 🍪 if benchmarks go brrrrr.

But wait, where is the **CODE_OF_CONDUCT**?

**Code of what?** Quoting a famous internet meme:

> “Apologies for the very personal question, but were you homeschooled by a pigeon?”

We're all civilised here. Just don't be an asshole and we're good. 🤞🏻

And hey, mad props to the entire Rust community. Y'all make low-level coding sexy again. This stuff is built with love, for the love of the game (and performant Rust).

---

## 🧩 **Part of the GDT Ecosystem**

`gdt-cpus` is part of the Game Developer's Toolkit - libraries built with years of experience from top-tier studios:

* **gdt-cpus** - Pin it. Prioritize it. Dominate it. You're looking at it right now!
* **gdt-jobs** - High-performance task execution built for games and sims needing serious parallelism.

---

## 📦 **Add to Your Project Like a Professional**

```bash
cargo add gdt-cpus
```

Or just copy-paste like it's still the 90s. We don't judge.

---

## 🔥 **Use Cases**

* Write a physics solver that doesn't feel like it's running on a potato.
* Make sure your background AI calculations stay *in the background*.
* Pin your loading threads to E-cores and gameplay to P-cores. Instant karma.
* Benchmark that ridiculous 64-core Threadripper you overpaid for.

> *Remember*: Your OS works *for* you, not the other way around.
> Pin those threads. Prioritize them. And go write code that makes the fans spin.

---

## ⚖️ **License**

MIT OR Apache-2.0 - because we believe in *freedom of choice* (and legally covering our butts).

---

<p align="center">Made with ❤️ by <a href="https://wildpixelgames.com">Wild Pixel Games</a> - We know CPUs.</p>
<p align="center"><i>"My CPU used to cry itself to sleep. Then I found <code>gdt-cpus</code>."</i> - A Very Smart Developer</p>
