# Local release preparation

This checklist describes preparation only. It does not authorize publishing, tagging, signing, or uploading an artifact. The current runtime remains experimental, and this repository has no verified public release.

## Build and package

Start from a reviewed commit and record its full Git revision. Use the Windows x64 MSVC target on a clean Windows runner with Rust 1.89 or newer, the MSVC C++ build tools, Windows SDK, and the pinned `Cargo.lock`:

```powershell
cargo build --release --locked --target x86_64-pc-windows-msvc
python scripts/package_release.py --binary target/x86_64-pc-windows-msvc/release/guardian.exe
```

The checked-in Cargo target configuration enables `crt-static` for this MSVC target so the executable does not depend on a separately installed Visual C++ runtime. Keep the import-table check in the workflow: it verifies the produced binary rather than assuming the configuration took effect.

The packager checks the PE architecture and rejects non-system DLLs named in the normal and delay-load import tables. It creates a local ZIP with fixed timestamps and uncompressed entries, reopens and verifies the archive contents, and writes separate SHA-256 sidecars for the ZIP and `guardian.exe`. It also includes the executable checksum inside the ZIP. It normalizes bundled text files to UTF-8 with LF endings. The artifacts go to the local `dist/` directory, which Git ignores. The ZIP name ends in `-local.zip` and includes a notice that it is not an official release. The current local GNU/LLVM build imports `libunwind.dll`, so the packager correctly rejects it.

The import-table check is a dependency screen, not proof that the executable is self-contained: it cannot see DLLs loaded dynamically at runtime. CI has built, packaged, and launched the MSVC executable with `--version`, but clean-user install, upgrade, removal, and runtime behavior are still unverified. Verify the packaged binary on a clean Windows runner and user account before recommending installation.

The archive is deterministic for the same executable and repository text inputs. This does not prove that Rust binary builds are reproducible. Before claiming reproducible binaries, build the same commit in two clean environments and compare the executable hashes. The generated local checksum does not authenticate the publisher; a public release must distribute checksums through a separately trusted channel.

Verify the output locally:

```powershell
$archive = Get-ChildItem dist\agent-guardian-*-local.zip
$expected = (Get-Content "$($archive.FullName).sha256" -Raw).Split()[0]
$actual = (Get-FileHash -LiteralPath $archive.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw 'Archive checksum mismatch' }
tar -tf $archive.FullName
```

Do not publish the generated local package as a release artifact. An official release still needs the acceptance gates below and explicit publication authorization.

## Release gate checklist

- [ ] The proposed version matches the documented stability level and the source commit is reviewed.
- [ ] GitHub CI passes on the supported Windows runner, including the declared Rust 1.89 minimum.
- [ ] The Agent Skills specification validator and the standard Skills CLI installation flow pass against the published repository.
- [ ] A clean-user Windows VM completes install, upgrade, launch, and uninstall using a separately trusted checksum.
- [ ] Power behavior passes real hardware tests for idle sleep, display-off, explicit sleep, process exit/crash, AC/battery transition, and locked session on each claimed sleep model.
- [ ] Connectivity monitoring and any future recovery pass a controlled isolated-network test; no real adapter mutation is enabled before its separate authorization and recovery controls are reviewed.
- [ ] Security review, dependency scan, performance measurements, compatibility matrix, and demo evidence are current.
- [ ] Release assets are built from the reviewed commit, checksums are verified independently, and signing/provenance decisions are documented.
- [ ] Repository owner authorizes the exact tag, assets, and publication action.

Until these gates are complete, keep the repository and artifacts labeled experimental or local development. Power and adapter behavior that has not been exercised on hardware stays **NOT TESTED**.

The current [launch copy](launch-copy.md) is a draft for use only after the gates pass.
