# Per-user runtime installation

The Agent Skill and Guardian runtime are separate. Installing the skill does not download or install the Windows executable. No runtime release is currently published, so the commands below are for a Guardian executable obtained from a source the user already trusts.

## Install or upgrade

Open PowerShell in the directory containing `guardian.exe`. Supply the SHA-256 value from a trusted release checksum file:

```powershell
& '.\guardian.exe' install --sha256 '<64 hexadecimal characters>'
```

For a Windows x64 MSVC binary built locally from source, calculate a checksum for the exact executable and install that file only after its runtime dependencies have been verified on a clean Windows environment:

```powershell
$exe = (Resolve-Path '.\target\x86_64-pc-windows-msvc\release\guardian.exe').Path
$sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
& $exe install --sha256 $sha256
```

A checksum calculated from a local binary verifies the bytes staged and copied by the installer. It does not establish the binary's publisher or source provenance. A future release should publish a checksum file alongside each architecture-specific executable and establish a separately trusted release channel before recommending this command to general users.

The current local GNU/LLVM development build imports `libunwind.dll`, while the per-user installer copies only `guardian.exe`. Do not install that development binary as a standalone runtime; use `cargo run` during development. The release packager rejects non-system DLLs listed in the normal and delay-load PE import tables, but this screen cannot detect runtime `LoadLibrary` calls and does not prove that an executable is self-contained. The MSVC release binary still needs a clean-environment runtime check.

The installer runs on Windows and copies the currently running executable, so the artifact must already match the host architecture. The executable must be no larger than 100 MiB. The installer validates a 64-character hexadecimal digest, hashes the source, copies it to a uniquely created temporary file in the destination directory, hashes the staged copy again, flushes it, and replaces only the fixed `guardian.exe` destination. It refuses reparse-point source, destination, and install directories. The install location is `%LOCALAPPDATA%\Programs\AgentGuardian\guardian.exe`; the command makes no PATH, startup, service, power-plan, configuration, or log changes and does not request elevation. If a running Guardian instance prevents replacement, close it and retry. A failed checksum or failed replacement leaves the prior executable in place.

Installing a newer executable uses the same command. Run the new executable from its downloaded or build directory and supply that file's trusted digest. The command reports whether it installed, updated, or found the same executable already installed. JSON output is available with `guardian --json install --sha256 ...`.

## Remove

Close Guardian processes, then run the uninstall command from a different executable, such as a local development build or the downloaded release executable:

```powershell
& '.\guardian.exe' uninstall
```

The command removes only `%LOCALAPPDATA%\Programs\AgentGuardian\guardian.exe`, and removes the installation directory only when it is empty. It refuses to delete the executable image that is currently running, so do not launch `uninstall` from the installed copy itself. Configuration, event logs, and exported reports are preserved. The uninstall command does not use delayed reboot deletion because that Windows operation requires administrative-group membership or LocalSystem.

## Verification status

Installer and uninstaller file operations are covered by Windows tests using disposable temporary directories, including a path with spaces and non-ASCII characters, checksum rejection, upgrade, removal, preservation of unrelated files, and protection against self-deletion. No install or uninstall was performed in the current user's profile. A clean-user VM audit, published release checksum verification, and end-to-end upgrade/removal from release artifacts remain **NOT TESTED**.

## Microsoft API references

- [MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)
- [GetFileAttributesW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfileattributesw)
- [Reparse point operations](https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-point-operations)
- [FOLDERID_LocalAppData](https://learn.microsoft.com/en-us/windows/win32/shell/knownfolderid)
