use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    GetFileAttributesW, GetFileInformationByHandle, MOVEFILE_REPLACE_EXISTING,
    MOVEFILE_WRITE_THROUGH, MoveFileExW,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

const MAX_EXECUTABLE_BYTES: u64 = 100 * 1024 * 1024;
const INSTALL_SUBDIRECTORY: &str = "Programs\\AgentGuardian";
const EXECUTABLE_NAME: &str = "guardian.exe";

pub fn install(json: bool, expected_sha256: Option<&str>) -> Result<(), String> {
    let source = std::env::current_exe()
        .map_err(|error| format!("could not locate this executable: {error}"))?;
    let expected = match expected_sha256 {
        Some(value) => parse_sha256(value)?,
        None => hash_file(&source)
            .map_err(|error| format!("could not hash this executable: {error}"))?,
    };
    let local_app_data = local_app_data_path()?;
    let outcome = install_at(&source, &local_app_data, expected)?;

    if json {
        let path = format!(r"%LOCALAPPDATA%\{INSTALL_SUBDIRECTORY}\{EXECUTABLE_NAME}");
        println!(
            "{}",
            serde_json::json!({
                "installed": true,
                "updated": outcome == InstallOutcome::Updated,
                "path": path,
                "publisher_checksum_verified": expected_sha256.is_some(),
            })
        );
    } else {
        match outcome {
            InstallOutcome::Installed => println!(
                "Installed Guardian at %LOCALAPPDATA%\\{INSTALL_SUBDIRECTORY}\\{EXECUTABLE_NAME}."
            ),
            InstallOutcome::Updated => println!(
                "Updated Guardian at %LOCALAPPDATA%\\{INSTALL_SUBDIRECTORY}\\{EXECUTABLE_NAME}."
            ),
            InstallOutcome::AlreadyInstalled => println!(
                "This Guardian executable is already installed at %LOCALAPPDATA%\\{INSTALL_SUBDIRECTORY}\\{EXECUTABLE_NAME}."
            ),
        }
        println!("No PATH, startup, service, configuration, or log changes were made.");
        if expected_sha256.is_none() {
            println!(
                "The installed copy was verified against this executable; publisher identity was not verified."
            );
        }
    }
    Ok(())
}

pub fn uninstall(json: bool) -> Result<(), String> {
    let local_app_data = local_app_data_path()?;
    let current_executable = std::env::current_exe()
        .map_err(|error| format!("could not locate this executable: {error}"))?;
    let destination = local_app_data
        .join(INSTALL_SUBDIRECTORY)
        .join(EXECUTABLE_NAME);
    if require_optional_regular_file(&destination)
        .map_err(|error| format!("installed executable is unsafe: {error}"))?
        && path_eq(&current_executable, &destination)
    {
        schedule_self_uninstall(&local_app_data)?;
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "removed": false,
                    "removal_scheduled": true,
                    "installation_directory_removed": false,
                })
            );
        } else {
            println!("Guardian removal is scheduled after this command exits.");
            println!("Configuration, logs, and reports are preserved.");
        }
        return Ok(());
    }
    let outcome = uninstall_at(&local_app_data, &current_executable)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "removed": outcome.executable_removed,
                "installation_directory_removed": outcome.directory_removed,
            })
        );
    } else if outcome.executable_removed {
        println!("Removed the per-user Guardian executable.");
        if outcome.directory_removed {
            println!("The empty installation directory was removed.");
        } else {
            println!("The installation directory was left in place.");
        }
        println!("Guardian configuration, logs, and reports were preserved.");
    } else {
        println!("No per-user Guardian executable was installed.");
    }
    Ok(())
}

fn schedule_self_uninstall(local_app_data: &Path) -> Result<(), String> {
    let (process, creation_time) =
        super::ProcessHandle::open(unsafe { super::GetCurrentProcessId() })?;
    drop(process);

    let folder_utf16: Vec<u8> = local_app_data
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let folder_base64 = encode_base64(&folder_utf16);
    let script = format!(
        "$targetPid={};$created=[long]{creation_time};$deadline=(Get-Date).AddSeconds(30);while($true){{if((Get-Date) -ge $deadline){{exit 4}};$p=Get-Process -Id $targetPid -ErrorAction SilentlyContinue;if($null -eq $p){{break}};$actual=[long]$p.StartTime.ToUniversalTime().ToFileTimeUtc();if([Math]::Abs($actual-$created) -gt 10000000){{exit 3}};Start-Sleep -Milliseconds 200}};$base=[Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('{}'));$dir=Join-Path $base '{}';$file=Join-Path $dir '{}';if(Test-Path -LiteralPath $file -PathType Leaf){{Remove-Item -LiteralPath $file -Force -ErrorAction Stop}};if(Test-Path -LiteralPath $dir -PathType Container){{try{{Remove-Item -LiteralPath $dir -Force -ErrorAction Stop}}catch{{}}}}",
        unsafe { super::GetCurrentProcessId() },
        folder_base64,
        INSTALL_SUBDIRECTORY,
        EXECUTABLE_NAME,
    );
    let encoded_script = encode_base64(
        &script
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );

    let mut system_directory = vec![0_u16; 32_768];
    let length = unsafe {
        super::GetSystemDirectoryW(system_directory.as_mut_ptr(), system_directory.len() as u32)
    } as usize;
    if length == 0 || length >= system_directory.len() {
        return Err("could not locate the Windows system directory for the cleanup helper".into());
    }
    system_directory.truncate(length);
    let powershell = PathBuf::from(std::ffi::OsString::from_wide(&system_directory))
        .join(r"WindowsPowerShell\v1.0\powershell.exe");
    if !powershell.is_file() {
        return Err("Windows PowerShell is unavailable; Guardian was not removed".into());
    }
    Command::new(powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-EncodedCommand",
            &encoded_script,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0000_0008 | 0x0800_0000)
        .spawn()
        .map_err(|error| format!("could not start the self-removal helper: {error}"))?;
    Ok(())
}

fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        encoded.push(ALPHABET[(first >> 2) as usize] as char);
        encoded.push(ALPHABET[(((first & 0x03) << 4) | (second >> 4)) as usize] as char);
        encoded.push(if chunk.len() > 1 {
            ALPHABET[(((second & 0x0f) << 2) | (third >> 6)) as usize] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            ALPHABET[(third & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    encoded
}

#[derive(Debug, PartialEq, Eq)]
enum InstallOutcome {
    Installed,
    Updated,
    AlreadyInstalled,
}

#[derive(Debug, Default)]
struct UninstallOutcome {
    executable_removed: bool,
    directory_removed: bool,
}

fn local_app_data_path() -> Result<PathBuf, String> {
    let mut raw_path = std::ptr::null_mut();
    // SHGetKnownFolderPath allocates its result with the COM task allocator.
    let result = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_LocalAppData,
            KF_FLAG_DEFAULT as u32,
            std::ptr::null_mut(),
            &mut raw_path,
        )
    };
    if result < 0 || raw_path.is_null() {
        return Err(format!(
            "Windows could not resolve the current user's Local AppData folder (HRESULT 0x{:08X})",
            result as u32
        ));
    }
    let mut length = 0;
    // The known-folder API returns a valid NUL-terminated UTF-16 string.
    unsafe {
        while *raw_path.add(length) != 0 {
            length += 1;
        }
    }
    // Copy the path before releasing the COM-allocated buffer.
    let path = unsafe {
        PathBuf::from(std::ffi::OsString::from_wide(std::slice::from_raw_parts(
            raw_path, length,
        )))
    };
    unsafe { CoTaskMemFree(raw_path.cast()) };
    if !path.is_absolute() {
        return Err("Windows returned a non-absolute Local AppData path".into());
    }
    require_directory(&path)
        .map_err(|error| format!("Local AppData is not a safe existing directory: {error}"))?;
    Ok(path)
}

fn install_at(
    source: &Path,
    local_app_data: &Path,
    expected_sha256: [u8; 32],
) -> Result<InstallOutcome, String> {
    require_regular_file(source).map_err(|error| format!("invalid source executable: {error}"))?;
    let source_digest = hash_file(source)
        .map_err(|error| format!("could not verify source executable: {error}"))?;
    if source_digest != expected_sha256 {
        return Err("source SHA-256 does not match --sha256; nothing was installed".into());
    }

    let install_directory = create_install_directory(local_app_data)?;
    let destination = install_directory.join(EXECUTABLE_NAME);
    if path_eq(source, &destination) {
        return Ok(InstallOutcome::AlreadyInstalled);
    }

    let had_destination = match require_optional_regular_file(&destination) {
        Ok(exists) => exists,
        Err(error) => return Err(format!("existing install target is unsafe: {error}")),
    };

    let (stage_path, mut stage) = create_stage_file(&install_directory)
        .map_err(|error| format!("could not create a temporary install file: {error}"))?;
    let stage_result = copy_and_hash(source, &mut stage)
        .and_then(|digest| {
            if digest == expected_sha256 {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "source changed while it was being copied",
                ))
            }
        })
        .and_then(|()| stage.flush())
        .and_then(|()| stage.sync_all());
    drop(stage);
    if let Err(error) = stage_result {
        let _ = fs::remove_file(&stage_path);
        return Err(format!("could not verify staged executable: {error}"));
    }

    let staged_digest = verify_staged_file(&stage_path, expected_sha256);
    if let Err(error) = staged_digest {
        let _ = fs::remove_file(&stage_path);
        return Err(format!("could not verify staged executable: {error}"));
    }

    if let Err(error) = replace_file(&stage_path, &destination) {
        let _ = fs::remove_file(&stage_path);
        return Err(format!(
            "could not replace the installed executable: {error}. Close running Guardian instances and try again"
        ));
    }

    Ok(if had_destination {
        InstallOutcome::Updated
    } else {
        InstallOutcome::Installed
    })
}

fn uninstall_at(
    local_app_data: &Path,
    current_executable: &Path,
) -> Result<UninstallOutcome, String> {
    require_directory(local_app_data)
        .map_err(|error| format!("LOCALAPPDATA is not a safe existing directory: {error}"))?;
    let install_directory = local_app_data.join(INSTALL_SUBDIRECTORY);
    let programs_directory = local_app_data.join("Programs");
    if !path_exists(&programs_directory).map_err(|error| error.to_string())? {
        return Ok(UninstallOutcome::default());
    }
    require_directory(&programs_directory)
        .map_err(|error| format!("install parent is unsafe: {error}"))?;
    if !path_exists(&install_directory).map_err(|error| error.to_string())? {
        return Ok(UninstallOutcome::default());
    }
    require_directory(&install_directory)
        .map_err(|error| format!("installation directory is unsafe: {error}"))?;

    let destination = install_directory.join(EXECUTABLE_NAME);
    if !require_optional_regular_file(&destination)
        .map_err(|error| format!("installed executable is unsafe: {error}"))?
    {
        return Ok(UninstallOutcome::default());
    }
    if path_eq(current_executable, &destination) {
        return Err(
            "cannot remove the currently running guardian.exe. Run uninstall from a different Guardian executable, such as the downloaded release file, after closing other Guardian instances".into(),
        );
    }

    fs::remove_file(&destination)
        .map_err(|error| format!("could not remove the Guardian executable: {error}"))?;
    let directory_removed = match fs::remove_dir(&install_directory) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => false,
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(_) => false,
    };
    Ok(UninstallOutcome {
        executable_removed: true,
        directory_removed,
    })
}

fn parse_sha256(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("--sha256 must contain exactly 64 hexadecimal characters".into());
    }
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(&value[start..start + 2], 16)
            .map_err(|_| "--sha256 must contain exactly 64 hexadecimal characters".to_string())?;
    }
    Ok(digest)
}

fn hash_file(path: &Path) -> io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_EXECUTABLE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "source must be a regular file no larger than 100 MiB",
        ));
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        if total > MAX_EXECUTABLE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "source exceeds the 100 MiB limit",
            ));
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().into())
}

fn verify_staged_file(path: &Path, expected_sha256: [u8; 32]) -> io::Result<()> {
    require_regular_file(path)?;
    let digest = hash_file(path)?;
    if digest != expected_sha256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "staged file SHA-256 does not match --sha256",
        ));
    }
    Ok(())
}

fn copy_and_hash(source: &Path, destination: &mut File) -> io::Result<[u8; 32]> {
    let mut source = File::open(source)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        if total > MAX_EXECUTABLE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "source exceeds the 100 MiB limit",
            ));
        }
        destination.write_all(&buffer[..count])?;
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().into())
}

fn create_stage_file(directory: &Path) -> io::Result<(PathBuf, File)> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0..32_u32 {
        let path = directory.join(format!(
            ".guardian-install-{}-{nonce}-{attempt}.tmp",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a unique temporary file name",
    ))
}

fn create_install_directory(local_app_data: &Path) -> Result<PathBuf, String> {
    require_directory(local_app_data)
        .map_err(|error| format!("LOCALAPPDATA is not a safe existing directory: {error}"))?;
    let programs = local_app_data.join("Programs");
    create_or_validate_directory(&programs)
        .map_err(|error| format!("could not prepare the per-user Programs directory: {error}"))?;
    let install_directory = programs.join("AgentGuardian");
    create_or_validate_directory(&install_directory)
        .map_err(|error| format!("could not prepare the Guardian install directory: {error}"))?;
    Ok(install_directory)
}

fn create_or_validate_directory(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => require_directory(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path)?;
            require_directory(path)
        }
        Err(error) => Err(error),
    }
}

fn require_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a real directory",
        ));
    }
    let attributes = file_attributes(path)?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || attributes & FILE_ATTRIBUTE_DIRECTORY == 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "directory is a reparse point or not a directory",
        ));
    }
    Ok(())
}

fn require_regular_file(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a regular file",
        ));
    }
    let attributes = file_attributes(path)?;
    if attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file is a reparse point or not a regular file",
        ));
    }
    Ok(())
}

fn require_optional_regular_file(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => require_regular_file(path).map(|()| true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn path_exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn file_attributes(path: &Path) -> io::Result<u32> {
    let path = wide_path(path)?;
    // The path is a valid NUL-terminated UTF-16 string owned by this call.
    let attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
    if attributes == u32::MAX {
        Err(io::Error::last_os_error())
    } else {
        Ok(attributes)
    }
}

fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    let source = wide_path(source)?;
    let destination = wide_path(destination)?;
    // Both paths name files in the same checked, per-user directory. The flags
    // replace only Guardian's fixed executable and wait for the move to finish.
    let success = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains a NUL character",
        ));
    }
    let mut terminated = wide;
    terminated.push(0);
    Ok(terminated)
}

fn path_eq(left: &Path, right: &Path) -> bool {
    let left = fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    if left
        .to_string_lossy()
        .to_lowercase()
        .eq(&right.to_string_lossy().to_lowercase())
    {
        return true;
    }

    let (Ok(left), Ok(right)) = (File::open(left), File::open(right)) else {
        return false;
    };
    let (Some(left_identity), Some(right_identity)) = (file_identity(&left), file_identity(&right))
    else {
        return false;
    };
    left_identity == right_identity
}

fn file_identity(file: &File) -> Option<(u32, u64)> {
    let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // GetFileInformationByHandle fills the structure when it returns nonzero.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) } == 0 {
        return None;
    }
    let information = unsafe { information.assume_init() };
    Some((
        information.dwVolumeSerialNumber,
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "Agent Guardian 测试 {label} {} {nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn digest(bytes: &[u8]) -> [u8; 32] {
        Sha256::digest(bytes).into()
    }

    #[test]
    fn base64_encoder_matches_standard_vectors() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
    }

    #[test]
    fn checksum_parser_accepts_uppercase_and_rejects_malformed_input() {
        let expected = digest(b"guardian");
        let upper = format!("{:X}", Sha256::digest(b"guardian"));
        assert_eq!(parse_sha256(&upper).unwrap(), expected);
        assert!(parse_sha256("abc").is_err());
        assert!(parse_sha256(&"z".repeat(64)).is_err());
    }

    #[test]
    fn install_copies_verified_bytes_to_non_ascii_per_user_path() {
        let root = test_root("install");
        let source = root.join("guardian source.exe");
        let bytes = b"verified guardian executable fixture";
        fs::write(&source, bytes).unwrap();

        let outcome = install_at(&source, &root, digest(bytes)).unwrap();
        let installed = root.join(INSTALL_SUBDIRECTORY).join(EXECUTABLE_NAME);
        assert_eq!(outcome, InstallOutcome::Installed);
        assert_eq!(fs::read(installed).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn checksum_failure_preserves_existing_install() {
        let root = test_root("checksum");
        let source = root.join("guardian source.exe");
        let install_directory = root.join(INSTALL_SUBDIRECTORY);
        fs::create_dir_all(&install_directory).unwrap();
        let destination = install_directory.join(EXECUTABLE_NAME);
        fs::write(&source, b"new executable").unwrap();
        fs::write(&destination, b"existing executable").unwrap();

        assert!(install_at(&source, &root, digest(b"wrong checksum")).is_err());
        assert_eq!(fs::read(destination).unwrap(), b"existing executable");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn upgrade_replaces_only_guardian_executable() {
        let root = test_root("upgrade");
        let source = root.join("guardian newer.exe");
        let bytes = b"newer verified executable";
        fs::write(&source, bytes).unwrap();
        let install_directory = root.join(INSTALL_SUBDIRECTORY);
        fs::create_dir_all(&install_directory).unwrap();
        let destination = install_directory.join(EXECUTABLE_NAME);
        fs::write(&destination, b"old executable").unwrap();
        let user_file = install_directory.join("keep.txt");
        fs::write(&user_file, b"preserve").unwrap();

        let outcome = install_at(&source, &root, digest(bytes)).unwrap();
        assert_eq!(outcome, InstallOutcome::Updated);
        assert_eq!(fs::read(destination).unwrap(), bytes);
        assert_eq!(fs::read(user_file).unwrap(), b"preserve");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uninstall_removes_only_guardian_executable_and_empty_directory() {
        let root = test_root("uninstall");
        let install_directory = root.join(INSTALL_SUBDIRECTORY);
        fs::create_dir_all(&install_directory).unwrap();
        let destination = install_directory.join(EXECUTABLE_NAME);
        fs::write(&destination, b"guardian").unwrap();
        let local_data = root.join("AgentGuardian");
        fs::create_dir_all(&local_data).unwrap();
        let journal = local_data.join("events.jsonl");
        fs::write(&journal, b"preserve local event history").unwrap();
        let current_executable = root.join("maintenance-copy.exe");
        fs::write(&current_executable, b"uninstaller").unwrap();

        let outcome = uninstall_at(&root, &current_executable).unwrap();
        assert!(outcome.executable_removed);
        assert!(outcome.directory_removed);
        assert!(!destination.exists());
        assert!(root.join("Programs").is_dir());
        assert_eq!(fs::read(journal).unwrap(), b"preserve local event history");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uninstall_leaves_unrelated_files_in_the_install_directory() {
        let root = test_root("uninstall-preserve");
        let install_directory = root.join(INSTALL_SUBDIRECTORY);
        fs::create_dir_all(&install_directory).unwrap();
        let destination = install_directory.join(EXECUTABLE_NAME);
        fs::write(&destination, b"guardian").unwrap();
        let unrelated_file = install_directory.join("keep.txt");
        fs::write(&unrelated_file, b"preserve").unwrap();
        let current_executable = root.join("maintenance-copy.exe");
        fs::write(&current_executable, b"uninstaller").unwrap();

        let outcome = uninstall_at(&root, &current_executable).unwrap();
        assert!(outcome.executable_removed);
        assert!(!outcome.directory_removed);
        assert!(!destination.exists());
        assert_eq!(fs::read(unrelated_file).unwrap(), b"preserve");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uninstall_refuses_to_delete_its_running_executable() {
        let root = test_root("self-uninstall");
        let install_directory = root.join(INSTALL_SUBDIRECTORY);
        fs::create_dir_all(&install_directory).unwrap();
        let destination = install_directory.join(EXECUTABLE_NAME);
        fs::write(&destination, b"guardian").unwrap();

        assert!(uninstall_at(&root, &destination).is_err());
        assert!(destination.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uninstall_refuses_to_delete_a_hard_link_to_its_running_executable() {
        let root = test_root("self-uninstall-hard-link");
        let install_directory = root.join(INSTALL_SUBDIRECTORY);
        fs::create_dir_all(&install_directory).unwrap();
        let destination = install_directory.join(EXECUTABLE_NAME);
        fs::write(&destination, b"guardian image").unwrap();
        let running_alias = root.join("running-alias.exe");
        fs::hard_link(&destination, &running_alias).unwrap();

        assert!(uninstall_at(&root, &running_alias).is_err());
        assert!(destination.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn staged_file_is_rehashed_after_copy() {
        let root = test_root("staged-rehash");
        let staged = root.join("guardian-stage.tmp");
        fs::write(&staged, b"staged bytes").unwrap();

        assert!(verify_staged_file(&staged, digest(b"other bytes")).is_err());
        assert!(verify_staged_file(&staged, digest(b"staged bytes")).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
}
