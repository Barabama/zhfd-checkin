# Rust CLI migration and user entry point

The Rust implementation lives in `rust/`. Python remains the behavioral oracle while the Rust CLI and egui GUI are validated.

## First-time users

If you are using the portable package rather than building from source, start with [docs/QUICK_START.md](../docs/QUICK_START.md). It covers the safe order for first setup, environment diagnosis, a single-instance dry-run, result interpretation, and common recovery actions. The GUI also contains the same task-oriented guide under **快速使用**.

The shortest safe first-run path is:

1. Start LDPlayer and manually sign in to the target App.
2. Open `zhfd-checkin-gui.exe`, complete **首次设置 / 配置修复**, and validate the `ldconsole.exe`/`adb.exe` paths.
3. Use **环境诊断** to confirm the selected instance and ADB connection.
4. Run one selected instance in **dry-run**; do not start with `--all-instances` or live mode.
5. Read the result, `error_code`, event timeline, and screenshots before changing scope.

`dry-run` never taps the sign-in control, but it can start the App, inspect pages, navigate, and wait on the target emulator. The program does not enter credentials, accept agreements, or dismiss advertisements automatically.

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

`config.toml`, `logs/`, `reports/`, and `captures/` are created beside the executable. LDPlayer and the APK remain external dependencies. See `../docs/README.md` for the current documentation index, `../docs/QUICK_START.md` for the first-run guide, `../docs/OPERATIONS.md` for deployment and commands, `../docs/CHECKIN_SAFETY.md` for behavior/safety rules, and `../docs/ENGINEERING_AND_RELEASE.md` for build/release state. Historical migration notes are preserved under `../docs/archive/2026-10-11/architecture/`.

The current default sign-in window is **21:00–23:59 (Asia/Shanghai)**. Existing historical validation records may mention the former 21:30 start time; they describe the window that was in effect at the time and are not current defaults.

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
## Windows release package

Build and package the two portable Windows binaries:

```powershell
cargo build --release --manifest-path .\rust\Cargo.toml --bins
.\scripts\portable_smoke.ps1
.\scripts\package_release.ps1
```

The package contains only `zhfd-checkin.exe`, `zhfd-checkin-gui.exe`, and a usage note. It does not contain `config.toml`, accounts, tokens, logs, screenshots, or emulator state. The default scheduled task remains dry-run; do not create a live scheduled task without an explicit operational decision.