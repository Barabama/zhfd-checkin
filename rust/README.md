# Rust CLI migration

The Rust implementation lives in `rust/`. Python remains the behavioral oracle while the Rust CLI and egui GUI are validated.

Build with the MSVC toolchain:

```powershell
$env:PATH="$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo test --manifest-path .\rust\Cargo.toml
cargo build --release --manifest-path .\rust\Cargo.toml --bins
```

Run commands from the release output directory or copy the executable anywhere:

```text
zhfd-checkin.exe                 # CLI dispatcher (no args launches detached GUI companion)
zhfd-checkin-gui.exe             # standalone GUI; no console window on Windows
zhfd-checkin.exe diagnose
zhfd-checkin.exe report
zhfd-checkin.exe profile detect
zhfd-checkin.exe profile detect --instance-index 1
zhfd-checkin.exe instance list
zhfd-checkin.exe run --dry-run --instance-index 0
zhfd-checkin.exe run --dry-run --instance-index 1
zhfd-checkin.exe run --dry-run --all-instances
zhfd-checkin.exe vision --image .\debug\button_now.png
```

`config.toml`, `logs/`, `reports/`, and `captures/` are created beside the executable. LDPlayer and the APK remain external dependencies. See `docs/RUST_MIGRATION.md` for the current migration boundary and profile status.

## Multiple LDPlayer instances

The legacy single-instance settings remain supported. To configure two instances explicitly, add:

```toml
[[emulator.instances]]
name = "签到实例 0"
instance_index = 0
serial = ""
enabled = true

[[emulator.instances]]
name = "签到实例 1"
instance_index = 1
serial = ""
enabled = true
```

Use `instance list` to inspect discovered LDPlayer instances. `--instance-index` selects one instance, `--serial` overrides its ADB serial, and `--all-instances` runs the selected command sequentially for all discovered instances. Each run keeps its own serial, instance index/name, Profile decision, and logs; no ADB state is shared between instances.
