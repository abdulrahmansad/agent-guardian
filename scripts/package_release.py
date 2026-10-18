#!/usr/bin/env python3
"""Create a deterministic local Windows x64 runtime archive and SHA-256 sidecar."""

from __future__ import annotations

import argparse
import hashlib
import io
import os
import re
import struct
import sys
import tempfile
import zipfile
from pathlib import Path

MAX_EXECUTABLE_BYTES = 100 * 1024 * 1024
SEMVER = re.compile(r"\A(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?\Z")
PACKAGE_FILES = {
    "INSTALLATION.md": Path("docs/development/installation.md"),
    "LICENSE": Path("LICENSE"),
    "README.md": Path("README.md"),
}
WINDOWS_SYSTEM_DLLS = {
    "advapi32.dll",
    "bcryptprimitives.dll",
    "combase.dll",
    "crypt32.dll",
    "gdi32.dll",
    "iphlpapi.dll",
    "kernel32.dll",
    "kernelbase.dll",
    "msvcrt.dll",
    "ntdll.dll",
    "ole32.dll",
    "oleaut32.dll",
    "powrprof.dll",
    "psapi.dll",
    "rpcrt4.dll",
    "secur32.dll",
    "setupapi.dll",
    "shell32.dll",
    "shlwapi.dll",
    "user32.dll",
    "userenv.dll",
    "ucrtbase.dll",
    "version.dll",
    "winhttp.dll",
    "wininet.dll",
    "winmm.dll",
    "wintrust.dll",
    "ws2_32.dll",
}


def workspace_version(repository_root: Path) -> str:
    cargo_toml = (repository_root / "Cargo.toml").read_text(encoding="utf-8")
    workspace_section = re.search(r"(?ms)^\[workspace\.package\]\s*(.*?)(?=^\[|\Z)", cargo_toml)
    if workspace_section is None:
        raise ValueError("Cargo.toml has no [workspace.package] section")
    version = re.search(r'^version\s*=\s*"([^"]+)"\s*$', workspace_section.group(1), re.MULTILINE)
    if version is None or not SEMVER.fullmatch(version.group(1)):
        raise ValueError("workspace package version is missing or is not semantic versioning")
    return version.group(1)


def pe_imports(binary: bytes) -> set[str]:
    if len(binary) < 64 or binary[:2] != b"MZ":
        raise ValueError("runtime binary is not a valid Windows PE executable")
    pe_offset = struct.unpack_from("<I", binary, 0x3C)[0]
    if pe_offset + 24 > len(binary) or binary[pe_offset : pe_offset + 4] != b"PE\0\0":
        raise ValueError("runtime binary has no valid PE header")
    machine = struct.unpack_from("<H", binary, pe_offset + 4)[0]
    if machine != 0x8664:
        raise ValueError(f"only Windows x64 executables can be packaged; PE machine is 0x{machine:04X}")
    section_count = struct.unpack_from("<H", binary, pe_offset + 6)[0]
    optional_size = struct.unpack_from("<H", binary, pe_offset + 20)[0]
    optional_offset = pe_offset + 24
    if optional_size < 240 or optional_offset + optional_size > len(binary):
        raise ValueError("runtime executable has an invalid PE32+ optional header")
    if struct.unpack_from("<H", binary, optional_offset)[0] != 0x20B:
        raise ValueError("runtime executable is not a PE32+ image")

    directory_count = struct.unpack_from("<I", binary, optional_offset + 108)[0]
    available_directories = (optional_size - 112) // 8
    if directory_count > available_directories:
        raise ValueError("runtime executable has an invalid data-directory count")

    def directory(index: int) -> tuple[int, int]:
        if index >= directory_count:
            return (0, 0)
        return struct.unpack_from("<II", binary, optional_offset + 112 + index * 8)

    section_offset = optional_offset + optional_size
    sections = []
    for index in range(section_count):
        offset = section_offset + index * 40
        if offset + 40 > len(binary):
            raise ValueError("runtime executable has a truncated section table")
        virtual_size, virtual_address, raw_size, raw_offset = struct.unpack_from("<IIII", binary, offset + 8)
        sections.append((virtual_address, max(virtual_size, raw_size), raw_size, raw_offset))

    def rva_to_offset(rva: int, size: int = 1) -> int:
        for virtual_address, span, raw_size, raw_offset in sections:
            delta = rva - virtual_address
            if 0 <= delta and delta + size <= min(span, raw_size):
                file_offset = raw_offset + delta
                if file_offset + size <= len(binary):
                    return file_offset
        raise ValueError("runtime executable import directory points outside its sections")

    def imported_name(name_rva: int) -> str:
        name_offset = rva_to_offset(name_rva)
        name_end = binary.find(b"\0", name_offset, min(name_offset + 260, len(binary)))
        if name_end < 0:
            raise ValueError("runtime executable has an invalid imported DLL name")
        try:
            return binary[name_offset:name_end].decode("ascii").lower()
        except UnicodeDecodeError as error:
            raise ValueError("runtime executable has a non-ASCII imported DLL name") from error

    imports: set[str] = set()
    import_rva, import_size = directory(1)
    if import_rva != 0 or import_size != 0:
        if import_rva == 0 or import_size < 20:
            raise ValueError("runtime executable has an invalid import directory")
        terminated = False
        for descriptor_index in range(min(import_size // 20, 4096)):
            descriptor_offset = rva_to_offset(import_rva + descriptor_index * 20, 20)
            descriptor = struct.unpack_from("<IIIII", binary, descriptor_offset)
            if descriptor == (0, 0, 0, 0, 0):
                terminated = True
                break
            imports.add(imported_name(descriptor[3]))
        if not terminated:
            raise ValueError("runtime executable import table is not terminated")

    delay_rva, delay_size = directory(13)
    if delay_rva != 0 or delay_size != 0:
        if delay_rva == 0 or delay_size < 32:
            raise ValueError("runtime executable has an invalid delay-import directory")
        image_base = struct.unpack_from("<Q", binary, optional_offset + 24)[0]
        terminated = False
        for descriptor_index in range(min(delay_size // 32, 4096)):
            descriptor_offset = rva_to_offset(delay_rva + descriptor_index * 32, 32)
            descriptor = struct.unpack_from("<IIIIIIII", binary, descriptor_offset)
            if descriptor == (0, 0, 0, 0, 0, 0, 0, 0):
                terminated = True
                break
            attributes, name = descriptor[:2]
            if attributes & ~1:
                raise ValueError("runtime executable has invalid delay-import attributes")
            if attributes & 1:
                name_rva = name
            else:
                if name < image_base:
                    raise ValueError("runtime executable has an invalid delay-import DLL address")
                name_rva = name - image_base
            imports.add(imported_name(name_rva))
        if not terminated:
            raise ValueError("runtime executable delay-import table is not terminated")

    return imports


def pe_architecture(binary: bytes) -> str:
    if len(binary) < 64 or binary[:2] != b"MZ":
        raise ValueError("runtime binary is not a valid Windows PE executable")
    pe_offset = struct.unpack_from("<I", binary, 0x3C)[0]
    if pe_offset + 6 > len(binary) or binary[pe_offset : pe_offset + 4] != b"PE\0\0":
        raise ValueError("runtime binary has no valid PE header")
    machine = struct.unpack_from("<H", binary, pe_offset + 4)[0]
    if machine != 0x8664:
        raise ValueError(f"only Windows x64 executables can be packaged; PE machine is 0x{machine:04X}")
    imports = pe_imports(binary)
    non_system_imports = sorted(
        name
        for name in imports
        if name not in WINDOWS_SYSTEM_DLLS
        and not name.startswith(("api-ms-win-", "ext-ms-win-"))
    )
    if non_system_imports:
        joined = ", ".join(non_system_imports)
        raise ValueError(f"runtime executable requires non-system DLLs: {joined}")
    return "windows-x86_64"


def normalized_text(path: Path) -> bytes:
    return path.read_text(encoding="utf-8").replace("\r\n", "\n").replace("\r", "\n").encode("utf-8")


def package_release(
    binary_path: Path, repository_root: Path, output_dir: Path
) -> tuple[Path, Path, Path, str]:
    if binary_path.is_symlink() or not binary_path.is_file():
        raise ValueError("--binary must name an existing regular executable file")
    binary = binary_path.read_bytes()
    if not binary or len(binary) > MAX_EXECUTABLE_BYTES:
        raise ValueError("runtime executable must be no larger than 100 MiB")
    platform = pe_architecture(binary)
    version = workspace_version(repository_root)

    entries = {name: normalized_text(repository_root / source) for name, source in PACKAGE_FILES.items()}
    entries["VERSION.txt"] = f"Agent Guardian {version}\n{platform}\nLocal development package; not an official release.\n".encode()
    entries["guardian.exe"] = binary
    executable_digest = hashlib.sha256(binary).hexdigest()
    executable_checksum = f"{executable_digest}  guardian.exe\n".encode("ascii")
    entries["guardian.exe.sha256"] = executable_checksum

    artifact_stem = f"agent-guardian-{version}-{platform}-local"
    archive_name = f"{artifact_stem}.zip"
    output_dir.mkdir(parents=True, exist_ok=True)
    archive_path = output_dir / archive_name
    checksum_path = output_dir / f"{archive_name}.sha256"
    executable_checksum_path = output_dir / f"{artifact_stem}.exe.sha256"

    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, mode="w", compression=zipfile.ZIP_STORED) as archive:
        archive.comment = b""
        for name in sorted(entries):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_STORED
            info.create_system = 0
            info.external_attr = 0
            info.flag_bits |= 0x800
            archive.writestr(info, entries[name])
    archive_bytes = buffer.getvalue()

    with zipfile.ZipFile(io.BytesIO(archive_bytes), mode="r") as archive:
        if archive.testzip() is not None or set(archive.namelist()) != set(entries):
            raise ValueError("generated archive failed its integrity check")
        if any(archive.read(name) != expected for name, expected in entries.items()):
            raise ValueError("generated archive content does not match its inputs")

    digest = hashlib.sha256(archive_bytes).hexdigest()
    checksum = f"{digest}  {archive_name}\n".encode("ascii")
    atomic_write(archive_path, archive_bytes)
    atomic_write(checksum_path, checksum)
    atomic_write(executable_checksum_path, executable_checksum)
    if hashlib.sha256(archive_path.read_bytes()).hexdigest() != digest:
        raise ValueError("written archive checksum does not match its verified contents")
    if executable_checksum_path.read_bytes() != executable_checksum:
        raise ValueError("written executable checksum does not match its verified contents")
    return archive_path, checksum_path, executable_checksum_path, digest


def atomic_write(path: Path, content: bytes) -> None:
    temporary_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix=f".{path.name}.", suffix=".tmp", delete=False) as temporary:
            temporary_path = Path(temporary.name)
            temporary.write(content)
            temporary.flush()
            os.fsync(temporary.fileno())
        os.replace(temporary_path, path)
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path, help="Windows x64 guardian.exe to package")
    parser.add_argument("--output-dir", type=Path, default=Path("dist"), help="local artifact directory (default: dist)")
    args = parser.parse_args()

    repository_root = Path(__file__).resolve().parent.parent
    binary_path = args.binary if args.binary.is_absolute() else Path.cwd() / args.binary
    output_dir = args.output_dir if args.output_dir.is_absolute() else Path.cwd() / args.output_dir
    try:
        archive, archive_checksum, executable_checksum, digest = package_release(
            binary_path, repository_root, output_dir
        )
    except (OSError, ValueError, zipfile.BadZipFile) as error:
        print(f"package-release: {error}", file=sys.stderr)
        return 2
    print(f"Local package: {archive}")
    print(f"Archive hash:  {archive_checksum}")
    print(f"Executable:    {executable_checksum}")
    print(f"Archive SHA:   {digest}")
    print("This package is a local development artifact, not an official release.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
