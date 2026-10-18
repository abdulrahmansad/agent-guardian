import hashlib
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

from package_release import package_release  # noqa: E402


def synthetic_pe(
    machine: int,
    import_name: str | None = None,
    delay_import_name: str | None = None,
    delay_name_is_rva: bool = True,
) -> bytes:
    pe_offset = 64
    optional_offset = pe_offset + 24
    optional_size = 240
    section_count = int(import_name is not None or delay_import_name is not None)
    section_offset = optional_offset + optional_size
    data = bytearray(section_offset + section_count * 40)
    if section_count:
        data.extend(b"\0" * (0x300 - len(data)))
    data[:2] = b"MZ"
    data[0x3C:0x40] = pe_offset.to_bytes(4, "little")
    data[pe_offset : pe_offset + 4] = b"PE\0\0"
    data[pe_offset + 4 : pe_offset + 6] = machine.to_bytes(2, "little")
    data[pe_offset + 6 : pe_offset + 8] = section_count.to_bytes(2, "little")
    data[pe_offset + 20 : pe_offset + 22] = optional_size.to_bytes(2, "little")
    data[optional_offset : optional_offset + 2] = (0x20B).to_bytes(2, "little")
    image_base = 0x400000 if delay_import_name is not None and not delay_name_is_rva else 0x140000000
    data[optional_offset + 24 : optional_offset + 32] = image_base.to_bytes(8, "little")
    data[optional_offset + 108 : optional_offset + 112] = (16).to_bytes(4, "little")
    if section_count:
        data[section_offset + 8 : section_offset + 12] = (0x100).to_bytes(4, "little")
        data[section_offset + 12 : section_offset + 16] = (0x1000).to_bytes(4, "little")
        data[section_offset + 16 : section_offset + 20] = (0x100).to_bytes(4, "little")
        data[section_offset + 20 : section_offset + 24] = (0x200).to_bytes(4, "little")
    if import_name is not None:
        data[optional_offset + 120 : optional_offset + 124] = (0x1000).to_bytes(4, "little")
        data[optional_offset + 124 : optional_offset + 128] = (40).to_bytes(4, "little")
        data[0x20C:0x210] = (0x1030).to_bytes(4, "little")
        data[0x230 : 0x230 + len(import_name) + 1] = import_name.encode("ascii") + b"\0"
    if delay_import_name is not None:
        data[optional_offset + 216 : optional_offset + 220] = (0x1040).to_bytes(4, "little")
        data[optional_offset + 220 : optional_offset + 224] = (64).to_bytes(4, "little")
        name_value = 0x1090 if delay_name_is_rva else image_base + 0x1090
        data[0x240:0x244] = (int(delay_name_is_rva)).to_bytes(4, "little")
        data[0x244:0x248] = name_value.to_bytes(4, "little")
        data[0x290 : 0x290 + len(delay_import_name) + 1] = delay_import_name.encode("ascii") + b"\0"
    return bytes(data)


class PackageReleaseTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        (self.root / "docs/development").mkdir(parents=True)
        for path, content in {
            "Cargo.toml": '[workspace.package]\nversion = "0.1.0"\n',
            "docs/development/installation.md": "# Install\r\n",
            "LICENSE": "MIT\r\n",
            "README.md": "# Agent Guardian\n",
        }.items():
            target = self.root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content, encoding="utf-8")
        self.binary = self.root / "guardian.exe"
        self.binary.write_bytes(synthetic_pe(0x8664))

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_same_inputs_create_identical_archive_and_checksum(self) -> None:
        output = self.root / "dist"
        first = package_release(self.binary, self.root, output)
        first_bytes = first[0].read_bytes()
        second = package_release(self.binary, self.root, output)

        self.assertEqual(first[3], second[3])
        self.assertEqual(first_bytes, second[0].read_bytes())
        self.assertIn("local.zip", second[0].name)
        self.assertEqual(second[1].read_text(encoding="ascii").split()[0], second[3])
        self.assertEqual(
            second[2].read_text(encoding="ascii").split()[0],
            hashlib.sha256(self.binary.read_bytes()).hexdigest(),
        )

    def test_non_x64_executable_is_rejected(self) -> None:
        self.binary.write_bytes(synthetic_pe(0xAA64))

        with self.assertRaisesRegex(ValueError, "only Windows x64"):
            package_release(self.binary, self.root, self.root / "dist")

    def test_non_system_runtime_dependency_is_rejected(self) -> None:
        self.binary.write_bytes(synthetic_pe(0x8664, "libunwind.dll"))

        with self.assertRaisesRegex(ValueError, "non-system DLLs: libunwind.dll"):
            package_release(self.binary, self.root, self.root / "dist")

    def test_non_system_delay_loaded_dependency_is_rejected(self) -> None:
        self.binary.write_bytes(synthetic_pe(0x8664, delay_import_name="libunwind.dll"))

        with self.assertRaisesRegex(ValueError, "non-system DLLs: libunwind.dll"):
            package_release(self.binary, self.root, self.root / "dist")

    def test_legacy_delay_import_address_is_resolved(self) -> None:
        self.binary.write_bytes(
            synthetic_pe(0x8664, delay_import_name="kernel32.dll", delay_name_is_rva=False)
        )

        package_release(self.binary, self.root, self.root / "dist")

    def test_native_wifi_api_is_a_windows_system_dependency(self) -> None:
        self.binary.write_bytes(synthetic_pe(0x8664, import_name="wlanapi.dll"))

        package_release(self.binary, self.root, self.root / "dist")


if __name__ == "__main__":
    unittest.main()
