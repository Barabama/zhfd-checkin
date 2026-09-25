# Rust CLI migration

The Rust implementation lives in `rust/`. Python remains the behavioral oracle while the Rust CLI and egui GUI are validated.

Build with the MSVC toolchain:

```powershell
$env:PATH="$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo test --manifest-path .\rust\Cargo.toml
cargo build --release --manifest-path .\rust\Cargo.toml
```

Run commands from the release output directory or copy the executable anywhere:

```text
zhfd-checkin.exe                 # GUI
zhfd-checkin.exe diagnose
zhfd-checkin.exe profile detect
zhfd-checkin.exe run --dry-run
zhfd-checkin.exe vision --image .\debug\button_now.png
```

`config.toml`, `logs/`, and `captures/` are created beside the executable. LDPlayer and the APK remain external dependencies. See `docs/RUST_MIGRATION.md` for the current migration boundary and profile status.
