# Migration Guide: 0.2606.1 -> 0.2608.0

Short version: **most Rust code needs no changes** - if you call `set_thread_priority` /
`promote_thread_to_realtime` and read the result through `reason()` / `broker_error()` / `degraded()`,
everything compiles as-is, on more targets than before. Two mechanical breaks affect code that names
`FallbackReason::BrokerRefused` in a pattern or calls `AppliedPriority::from_parts`. The **C ABI is
unchanged** - no recompile, no header change.

## Rust: two mechanical fixes

### `FallbackReason::BrokerRefused` carries its `BrokerError` now

The refusal detail is the variant's payload instead of a sibling field, so a broker error without a
broker-refused reason can no longer exist, in code or in serialized data.

```rust
// 0.2606.1
match applied.reason() {
    Some(FallbackReason::BrokerRefused) => {
        if let Some(e) = applied.broker_error() { retry_or_give_up(e); }
    }
    _ => {}
}

// 0.2608.0
match applied.reason() {
    Some(FallbackReason::BrokerRefused(e)) => retry_or_give_up(e),
    _ => {}
}
```

The `broker_error()` accessor still exists and now reads the payload, so code using the accessor
instead of a pattern needs no change.

### `AppliedPriority::from_parts` is 5 arguments and infallible

The trailing `broker_error: Option<BrokerError>` parameter is gone - the detail rides inside the
reason - and with no contradictory combination left to reject, the constructor returns `Self`
directly.

```rust
// 0.2606.1
let applied = AppliedPriority::from_parts(req, eff, grant, reason, mech, broker_error)
    .expect("valid combination");

// 0.2608.0
let applied = AppliedPriority::from_parts(req, eff, grant, reason, mech);
```

### serde: re-serialize, do not replay old data

The serialized `AppliedPriority` shape changed with the fold (the `broker_error` field is gone; a
refusal serializes as the `BrokerRefused` variant with its payload). Values serialized by 0.2606.1 do
not deserialize under 0.2608.0. If you persist or ship these across a version boundary - telemetry
pipelines are the expected case - cut over producer and consumer together.

## Rust: what got wider, not different

The fallback vocabulary (`FallbackReason`, `BrokerError`, `Grant::Brokered`, and the
`reason()` / `broker_error()` / `degraded()` accessors) now compiles on **every** target and feature
set. Portable code that inspects priority outcomes no longer needs any `cfg` and never did need one
per platform - on targets without a broker the accessors simply return `None` / `false`. Windows and
macOS produce clean grants or hard errors; the Linux family produces the fallback reports.

New surface, purely additive: `AffinityMask` implements `FromStr` (both `Display` spellings and bare
kernel range lists parse), and `{:#}` renders the exact kernel spelling with no whitespace. Android
(`aarch64-linux-android`) is a supported target.

## C ABI: nothing to do

`GdtCpusAppliedPriority` keeps its flat `reason` + `broker_error` field pair - the fold is flattened
at the conversion boundary - and no `#[repr(C)]` layout, enum value, or function signature changed.
Binaries built against the 0.2606.1 header keep working.
