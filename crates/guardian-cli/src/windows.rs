use std::ffi::OsString;
use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use guardian_core::{
    Config, ConnectivityEvidence, ConnectivityState, EndpointEvidence, EventStore, LeaseBook,
    LeaseId, LocalAdapterObservation, LocalNetworkSummary, PowerLeaseDecision, PowerLeasePolicy,
    PowerLeasePolicyError, PowerSourceClass, ProcessIdentity, ReleaseReason, SessionEvent,
    SessionEventKind, SessionReport, WifiReconnectResult, classify_local_evidence,
};
use sha2::{Digest, Sha256};
use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, LocalFree, SetLastError, WAIT_FAILED,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
    GAA_FLAG_SKIP_FRIENDLY_NAME, GAA_FLAG_SKIP_MULTICAST, GAA_FLAG_SKIP_UNICAST,
    GetAdaptersAddresses, IF_TYPE_IEEE80211, IF_TYPE_TUNNEL, IP_ADAPTER_ADDRESSES_LH,
};
use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows_sys::Win32::Networking::WinSock::AF_UNSPEC;
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    ConvertStringSidToSidW, GetSecurityInfo, SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL_SIZE_INFORMATION, AclSizeInformation,
    DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation, GetSecurityDescriptorControl,
    GetSecurityDescriptorDacl, GetTokenInformation, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenSessionId, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::READ_CONTROL;
use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
use windows_sys::Win32::System::Threading::{
    CREATE_EVENT_MANUAL_RESET, CreateEventExW, EVENT_MODIFY_STATE, GetCurrentProcess, OpenEventW,
    OpenProcessToken, SYNCHRONIZATION_SYNCHRONIZE, SetEvent, WaitForMultipleObjects,
};

mod install;
mod monitor;
mod probes;
mod recovery;
mod signals;

const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const POWER_REQUEST_SYSTEM_REQUIRED: i32 = 1;
const BATTERY_LEASE_LIMIT: Duration = Duration::from_secs(5 * 60);
const STOP_EVENT_ACCESS: u32 = EVENT_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE | READ_CONTROL;
const POWER_POLL_INTERVAL: Duration = Duration::from_secs(10);
const ERROR_BUFFER_OVERFLOW: u32 = 111;
const ERROR_NO_DATA: u32 = 232;
const NETWORK_BUFFER_START_BYTES: usize = 15 * 1024;
const NETWORK_BUFFER_MAX_BYTES: usize = 1024 * 1024;
type Handle = *mut c_void;

#[repr(C)]
struct FileTime {
    low: u32,
    high: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DetailedReason {
    localized_reason_module: Handle,
    localized_reason_id: u32,
    reason_string_count: u32,
    reason_strings: *const *const u16,
}

#[repr(C)]
union Reason {
    detailed: DetailedReason,
    simple_reason: *const u16,
}

#[repr(C)]
struct ReasonContext {
    version: u32,
    flags: u32,
    reason: Reason,
}

#[repr(C)]
#[derive(Default)]
struct SystemPowerStatus {
    ac_line_status: u8,
    battery_flag: u8,
    battery_life_percent: u8,
    system_status_flag: u8,
    battery_life_time: u32,
    battery_full_life_time: u32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn PowerCreateRequest(context: *const ReasonContext) -> Handle;
    fn PowerSetRequest(request: Handle, request_type: i32) -> i32;
    fn PowerClearRequest(request: Handle, request_type: i32) -> i32;
    fn OpenProcess(access: u32, inherit_handle: i32, process_id: u32) -> Handle;
    fn GetProcessTimes(
        process: Handle,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn CloseHandle(handle: Handle) -> i32;
    fn GetLastError() -> u32;
    fn GetCurrentProcessId() -> u32;
    fn GetSystemPowerStatus(status: *mut SystemPowerStatus) -> i32;
    fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
}

struct ProcessHandle(Handle);

struct TokenHandle(Handle);

impl TokenHandle {
    fn open(process: Handle) -> Result<Self, String> {
        let mut token = std::ptr::null_mut();
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
            return Err(format!(
                "could not verify process owner and session (Windows error {})",
                unsafe { GetLastError() }
            ));
        }
        if token.is_null() || token as isize == -1 {
            return Err("Windows returned an invalid process token handle".into());
        }
        Ok(Self(token))
    }

    fn user_buffer(&self) -> Result<Vec<usize>, String> {
        let mut required_bytes = 0;
        let first_result = unsafe {
            GetTokenInformation(
                self.0,
                TokenUser,
                std::ptr::null_mut(),
                0,
                &mut required_bytes,
            )
        };
        if first_result == 0
            && (required_bytes == 0 || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER)
        {
            return Err(format!(
                "could not query process owner (Windows error {})",
                unsafe { GetLastError() }
            ));
        }
        if required_bytes == 0 || required_bytes > 64 * 1024 {
            return Err("Windows returned an invalid process-owner buffer size".into());
        }
        let word_count = (required_bytes as usize).div_ceil(std::mem::size_of::<usize>());
        let mut buffer = vec![0_usize; word_count];
        let mut returned_bytes = 0;
        if unsafe {
            GetTokenInformation(
                self.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * std::mem::size_of::<usize>()) as u32,
                &mut returned_bytes,
            )
        } == 0
        {
            return Err(format!(
                "could not query process owner (Windows error {})",
                unsafe { GetLastError() }
            ));
        }
        if returned_bytes < std::mem::size_of::<TOKEN_USER>() as u32 {
            return Err("Windows returned incomplete process-owner information".into());
        }
        Ok(buffer)
    }

    fn session_id(&self) -> Result<u32, String> {
        let mut session_id = 0_u32;
        let mut returned_bytes = 0;
        if unsafe {
            GetTokenInformation(
                self.0,
                TokenSessionId,
                (&mut session_id as *mut u32).cast(),
                std::mem::size_of::<u32>() as u32,
                &mut returned_bytes,
            )
        } == 0
        {
            return Err(format!(
                "could not query process session (Windows error {})",
                unsafe { GetLastError() }
            ));
        }
        if returned_bytes != std::mem::size_of::<u32>() as u32 {
            return Err("Windows returned incomplete process-session information".into());
        }
        Ok(session_id)
    }
}

impl Drop for TokenHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn token_user_sid(buffer: &[usize]) -> Result<*mut c_void, String> {
    let token_user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
    if token_user.User.Sid.is_null() {
        return Err("Windows returned an invalid process-owner SID".into());
    }
    Ok(token_user.User.Sid)
}

fn validate_process_context(
    same_user: bool,
    target_session_id: u32,
    current_session_id: u32,
) -> Result<(), String> {
    if same_user && target_session_id == current_session_id {
        Ok(())
    } else {
        Err("target process must belong to the current Windows user and session".into())
    }
}

fn verify_process_owner_and_session(target_process: Handle) -> Result<(), String> {
    let target_token = TokenHandle::open(target_process)?;
    let current_token = TokenHandle::open(unsafe { GetCurrentProcess() })?;
    let target_user = target_token.user_buffer()?;
    let current_user = current_token.user_buffer()?;
    let same_user = unsafe {
        EqualSid(
            token_user_sid(&target_user)?,
            token_user_sid(&current_user)?,
        ) != 0
    };
    validate_process_context(
        same_user,
        target_token.session_id()?,
        current_token.session_id()?,
    )
}

impl ProcessHandle {
    fn open(process_id: u32) -> Result<(Self, u64), String> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                0,
                process_id,
            )
        };
        if handle.is_null() || handle as isize == -1 {
            return Err(format!(
                "could not open process {process_id} (Windows error {})",
                unsafe { GetLastError() }
            ));
        }
        let process = Self(handle);
        let mut creation = FileTime { low: 0, high: 0 };
        let mut exit = FileTime { low: 0, high: 0 };
        let mut kernel = FileTime { low: 0, high: 0 };
        let mut user = FileTime { low: 0, high: 0 };
        let succeeded =
            unsafe { GetProcessTimes(process.0, &mut creation, &mut exit, &mut kernel, &mut user) };
        if succeeded == 0 {
            return Err(format!(
                "could not read process identity (Windows error {})",
                unsafe { GetLastError() }
            ));
        }
        let created = (u64::from(creation.high) << 32) | u64::from(creation.low);
        verify_process_owner_and_session(process.0)?;
        Ok((process, created))
    }

    fn wait(&self, timeout_ms: u32) -> Result<bool, String> {
        match unsafe { WaitForSingleObject(self.0, timeout_ms) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(format!(
                "could not monitor target process (Windows error {})",
                unsafe { GetLastError() }
            )),
        }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(self.0);
        }
    }
}

fn stop_event_name_for(session_id: u32, sid: &str) -> String {
    let digest = Sha256::digest(sid.as_bytes());
    let suffix = digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("Local\\AgentGuardian.Stop.{session_id}.{suffix}")
}

fn stop_event_identity() -> Result<(Vec<u16>, String), String> {
    let token = TokenHandle::open(unsafe { GetCurrentProcess() })?;
    let session_id = token.session_id()?;
    let user_buffer = token.user_buffer()?;
    let sid = token_user_sid(&user_buffer)?;
    let mut sid_string = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut sid_string) } == 0 {
        return Err(format!(
            "could not identify the current Windows user for stop signaling (Windows error {})",
            unsafe { GetLastError() }
        ));
    }
    if sid_string.is_null() {
        return Err("Windows returned an invalid current-user identifier".into());
    }
    let sid_allocation = LocalAllocation(sid_string.cast());
    let mut sid_length = 0_usize;
    while sid_length < 192 && unsafe { *sid_string.add(sid_length) } != 0 {
        sid_length += 1;
    }
    if sid_length == 0 || sid_length == 192 {
        return Err("Windows returned an invalid current-user identifier".into());
    }
    let sid_text =
        String::from_utf16(unsafe { std::slice::from_raw_parts(sid_string, sid_length) })
            .map_err(|_| "Windows returned an invalid current-user identifier".to_string())?;
    if !sid_text.starts_with("S-")
        || !sid_text
            .chars()
            .all(|character| character == 'S' || character == '-' || character.is_ascii_digit())
    {
        return Err("Windows returned an invalid current-user identifier".into());
    }
    let _ = sid_allocation;
    let name = stop_event_name_for(session_id, &sid_text);
    let wide_name = name.encode_utf16().chain(std::iter::once(0)).collect();
    Ok((wide_name, sid_text))
}

struct SecurityDescriptor(LocalAllocation);

impl SecurityDescriptor {
    fn for_user(sid: &str) -> Result<Self, String> {
        // Set the event owner explicitly. On elevated Windows runners the
        // token's default owner can be Administrators rather than TokenUser.
        let sddl = format!("O:{sid}D:P(A;;0x{:08x};;;{sid})", STOP_EVENT_ACCESS);
        Self::from_sddl(&sddl)
    }

    fn from_sddl(sddl: &str) -> Result<Self, String> {
        let wide_sddl: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide_sddl.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        };
        if converted == 0 {
            if !descriptor.is_null() {
                let _ = LocalAllocation(descriptor.cast());
            }
            return Err(format!(
                "could not secure the Guardian stop signal (Windows error {})",
                unsafe { GetLastError() }
            ));
        }
        if descriptor.is_null() {
            return Err("Windows returned an invalid stop-signal security descriptor".into());
        }
        Ok(Self(LocalAllocation(descriptor)))
    }
}

fn sid_from_text(sid: &str) -> Result<LocalAllocation, String> {
    let wide_sid: Vec<u16> = sid.encode_utf16().chain(std::iter::once(0)).collect();
    let mut binary_sid = std::ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(wide_sid.as_ptr(), &mut binary_sid) } == 0 {
        if !binary_sid.is_null() {
            let _ = LocalAllocation(binary_sid.cast());
        }
        return Err(format!(
            "could not validate the Guardian stop signal identity (Windows error {})",
            unsafe { GetLastError() }
        ));
    }
    if binary_sid.is_null() {
        return Err("Windows returned an invalid stop-signal identity".into());
    }
    Ok(LocalAllocation(binary_sid.cast()))
}

fn stop_event_security_matches(handle: Handle, sid: &str) -> Result<bool, String> {
    let expected_sid = sid_from_text(sid)?;
    let mut owner = std::ptr::null_mut();
    let mut dacl = std::ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let error = unsafe {
        GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if error != 0 {
        if !descriptor.is_null() {
            let _ = LocalAllocation(descriptor.cast());
        }
        return Err(format!(
            "could not inspect the Guardian stop signal security (Windows error {error})"
        ));
    }
    if descriptor.is_null() {
        return Err("Windows returned an invalid stop-signal security descriptor".into());
    }
    let _descriptor = LocalAllocation(descriptor.cast());
    if owner.is_null() || unsafe { EqualSid(owner, expected_sid.0) } == 0 {
        return Ok(false);
    }

    let mut control = 0_u16;
    let mut revision = 0_u32;
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
        return Err(format!(
            "could not inspect the Guardian stop signal ACL (Windows error {})",
            unsafe { GetLastError() }
        ));
    }
    if control & SE_DACL_PROTECTED == 0 {
        return Ok(false);
    }

    let mut dacl_present = 0;
    let mut dacl_defaulted = 0;
    let mut descriptor_dacl = std::ptr::null_mut();
    if unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &mut dacl_present,
            &mut descriptor_dacl,
            &mut dacl_defaulted,
        )
    } == 0
    {
        return Err(format!(
            "could not inspect the Guardian stop signal ACL (Windows error {})",
            unsafe { GetLastError() }
        ));
    }
    if dacl_present == 0 || dacl.is_null() || descriptor_dacl.is_null() {
        return Ok(false);
    }

    let mut acl_size = ACL_SIZE_INFORMATION::default();
    if unsafe {
        GetAclInformation(
            dacl,
            (&mut acl_size as *mut ACL_SIZE_INFORMATION).cast(),
            std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    } == 0
    {
        return Err(format!(
            "could not inspect the Guardian stop signal ACL (Windows error {})",
            unsafe { GetLastError() }
        ));
    }
    if acl_size.AceCount != 1 {
        return Ok(false);
    }

    let mut ace = std::ptr::null_mut();
    if unsafe { GetAce(dacl, 0, &mut ace) } == 0 || ace.is_null() {
        return Err(format!(
            "could not inspect the Guardian stop signal ACL (Windows error {})",
            unsafe { GetLastError() }
        ));
    }
    let header = unsafe { &*(ace.cast::<ACE_HEADER>()) };
    if header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8
        || header.AceFlags != 0
        || usize::from(header.AceSize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>()
    {
        return Ok(false);
    }
    let allowed = unsafe { &*(ace.cast::<ACCESS_ALLOWED_ACE>()) };
    if allowed.Mask != STOP_EVENT_ACCESS {
        return Ok(false);
    }
    let ace_sid = std::ptr::addr_of!(allowed.SidStart).cast_mut().cast();
    Ok(unsafe { EqualSid(ace_sid, expected_sid.0) } != 0)
}

struct StopEvent(Handle);

impl StopEvent {
    fn create() -> Result<Self, String> {
        let (name, sid) = stop_event_identity()?;
        Self::create_named(&name, &sid)
    }

    fn create_named(name: &[u16], sid: &str) -> Result<Self, String> {
        let descriptor = SecurityDescriptor::for_user(sid)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0.0,
            bInheritHandle: 0,
        };
        unsafe { SetLastError(0) };
        let handle = unsafe {
            CreateEventExW(
                &attributes,
                name.as_ptr(),
                CREATE_EVENT_MANUAL_RESET,
                STOP_EVENT_ACCESS,
            )
        };
        if handle.is_null() || handle as isize == -1 {
            return Err(format!(
                "could not create the Guardian stop signal (Windows error {})",
                unsafe { GetLastError() }
            ));
        }
        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        if !matches!(stop_event_security_matches(handle, sid), Ok(true)) {
            unsafe { CloseHandle(handle) };
            return Err(
                "Guardian stop signal has unexpected ownership or access rules; refusing to use it"
                    .into(),
            );
        }
        if already_exists {
            match unsafe { WaitForSingleObject(handle, 0) } {
                WAIT_TIMEOUT => {}
                WAIT_OBJECT_0 => {
                    unsafe { CloseHandle(handle) };
                    return Err(
                        "a Guardian stop is already in progress for this user session".into(),
                    );
                }
                _ => {
                    let error = unsafe { GetLastError() };
                    unsafe { CloseHandle(handle) };
                    return Err(format!(
                        "could not inspect the Guardian stop signal (Windows error {error})"
                    ));
                }
            }
        }
        Ok(Self(handle))
    }

    fn request_stop(json: bool) -> Result<(), String> {
        let (name, sid) = stop_event_identity()?;
        let requested = Self::request_stop_named(&name, &sid)?;
        if json {
            println!("{{\"stop_requested\":{requested}}}");
        } else if requested {
            println!(
                "Stop request sent to active Guardian power lease(s) in this user session; release is not confirmed."
            );
        } else {
            println!("No active Guardian power lease was found in this user session.");
        }
        Ok(())
    }

    fn request_stop_named(name: &[u16], sid: &str) -> Result<bool, String> {
        let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE | READ_CONTROL, 0, name.as_ptr()) };
        if handle.is_null() || handle as isize == -1 {
            let error = unsafe { GetLastError() };
            if error == ERROR_FILE_NOT_FOUND {
                Ok(false)
            } else {
                Err(format!(
                    "could not access the Guardian stop signal (Windows error {error})"
                ))
            }
        } else {
            let event = Self(handle);
            if !matches!(stop_event_security_matches(event.0, sid), Ok(true)) {
                return Err(
                    "Guardian stop signal has unexpected ownership or access rules; refusing to use it"
                        .into(),
                );
            }
            if unsafe { SetEvent(event.0) } == 0 {
                return Err(format!(
                    "could not signal Guardian to stop (Windows error {})",
                    unsafe { GetLastError() }
                ));
            }
            Ok(true)
        }
    }
}

impl Drop for StopEvent {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeaseWait {
    TargetExited,
    StopRequested,
    Timeout,
}

fn wait_for_target_or_stop(
    process: &ProcessHandle,
    stop_event: &StopEvent,
    timeout_ms: u32,
) -> Result<LeaseWait, String> {
    let handles = [process.0, stop_event.0];
    match unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, timeout_ms) } {
        WAIT_OBJECT_0 => Ok(LeaseWait::TargetExited),
        value if value == WAIT_OBJECT_0 + 1 => Ok(LeaseWait::StopRequested),
        WAIT_TIMEOUT => Ok(LeaseWait::Timeout),
        WAIT_FAILED => Err(format!(
            "could not wait for the target process or stop request (Windows error {})",
            unsafe { GetLastError() }
        )),
        _ => Err("Windows returned an unexpected lease wait result".into()),
    }
}

#[cfg(test)]
mod stop_event_tests {
    use super::*;

    fn unique_name() -> Vec<u16> {
        format!("Local\\AgentGuardian.StopTest.{}", LeaseId::new_v4())
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect()
    }

    #[test]
    fn event_name_is_stable_per_user_and_session_without_exposing_sid() {
        let sid = "S-1-5-21-123-456-789-1001";
        let first = stop_event_name_for(7, sid);
        assert_eq!(first, stop_event_name_for(7, sid));
        assert_ne!(first, stop_event_name_for(8, sid));
        assert_ne!(first, stop_event_name_for(7, "S-1-5-21-987-654-321-1002"));
        assert!(first.starts_with("Local\\AgentGuardian.Stop.7."));
        assert!(!first.contains(sid));
    }

    #[test]
    fn user_stop_signal_wakes_lease_wait_without_power_api() {
        let (_, sid) = stop_event_identity().unwrap();
        let name = unique_name();
        let event = StopEvent::create_named(&name, &sid).unwrap();
        let second_handle = StopEvent::create_named(&name, &sid).unwrap();
        assert!(StopEvent::request_stop_named(&name, &sid).unwrap());
        let current_process = ProcessHandle::open(unsafe { GetCurrentProcessId() })
            .unwrap()
            .0;
        let wait_handles = [current_process.0, event.0];
        assert_eq!(
            unsafe { WaitForMultipleObjects(2, wait_handles.as_ptr(), 0, 0) },
            WAIT_OBJECT_0 + 1
        );
        assert!(StopEvent::request_stop_named(&name, &sid).unwrap());
        drop(second_handle);
        drop(event);
        assert!(!StopEvent::request_stop_named(&name, &sid).unwrap());
    }

    #[test]
    fn precreated_stop_event_with_broad_acl_is_rejected() {
        let (_, sid) = stop_event_identity().unwrap();
        let name = unique_name();
        let descriptor = SecurityDescriptor::from_sddl("D:P(A;;GA;;;WD)").unwrap();
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0.0,
            bInheritHandle: 0,
        };
        let handle = unsafe {
            CreateEventExW(
                &attributes,
                name.as_ptr(),
                CREATE_EVENT_MANUAL_RESET,
                STOP_EVENT_ACCESS,
            )
        };
        assert!(!handle.is_null() && handle as isize != -1);
        let event = StopEvent(handle);

        assert!(StopEvent::create_named(&name, &sid).is_err());
        assert!(StopEvent::request_stop_named(&name, &sid).is_err());
        assert_eq!(unsafe { WaitForSingleObject(event.0, 0) }, WAIT_TIMEOUT);
    }
}

struct PowerLease {
    handle: Handle,
    _reason_storage: Vec<u16>,
}

impl PowerLease {
    fn acquire(reason: &str) -> Result<Self, String> {
        let mut reason_storage: Vec<u16> =
            reason.encode_utf16().chain(std::iter::once(0)).collect();
        let context = ReasonContext {
            version: 0,
            flags: 1,
            reason: Reason {
                simple_reason: reason_storage.as_mut_ptr(),
            },
        };
        let handle = unsafe { PowerCreateRequest(&context) };
        if handle.is_null() || handle as isize == -1 {
            return Err(format!(
                "PowerCreateRequest failed (Windows error {})",
                unsafe { GetLastError() }
            ));
        }
        if unsafe { PowerSetRequest(handle, POWER_REQUEST_SYSTEM_REQUIRED) } == 0 {
            let error = unsafe { GetLastError() };
            unsafe {
                CloseHandle(handle);
            }
            return Err(format!("PowerSetRequest failed (Windows error {error})"));
        }
        Ok(Self {
            handle,
            _reason_storage: reason_storage,
        })
    }
}

impl Drop for PowerLease {
    fn drop(&mut self) {
        unsafe {
            let _ = PowerClearRequest(self.handle, POWER_REQUEST_SYSTEM_REQUIRED);
            let _ = CloseHandle(self.handle);
        }
    }
}

struct ActiveLease {
    power: Option<PowerLease>,
    event_store: EventStore,
    session_id: LeaseId,
    finished: bool,
}

impl ActiveLease {
    fn new(power: PowerLease, event_store: EventStore, session_id: LeaseId) -> Self {
        Self {
            power: Some(power),
            event_store,
            session_id,
            finished: false,
        }
    }

    fn finish(mut self, reason: ReleaseReason) -> Result<(), String> {
        self.power.take();
        self.finished = true;
        self.event_store.append(&SessionEvent::lease_released(
            self.session_id,
            unix_time_ms(),
            reason,
        ))
    }
}

impl Drop for ActiveLease {
    fn drop(&mut self) {
        self.power.take();
        if !self.finished {
            let _ = self.event_store.append(&SessionEvent::lease_released(
                self.session_id,
                unix_time_ms(),
                ReleaseReason::RuntimeExit,
            ));
            self.finished = true;
        }
    }
}

pub fn start(
    pid: Option<u32>,
    ttl_minutes: u32,
    allow_battery: bool,
    config: Config,
) -> Result<(), String> {
    let process_id = pid.unwrap_or_else(|| unsafe { GetCurrentProcessId() });
    let (process, creation_time) = ProcessHandle::open(process_id)?;
    if process.wait(0)? {
        return Err(format!("process {process_id} has already exited"));
    }

    let power = power_status()?;
    let on_battery_or_unknown = power.ac_line_status != 1;
    let requested = Duration::from_secs(u64::from(ttl_minutes) * 60);
    let source = if on_battery_or_unknown {
        PowerSourceClass::BatteryOrUnknown
    } else {
        PowerSourceClass::Ac
    };
    let mut policy = PowerLeasePolicy::new(
        0,
        requested.as_millis() as u64,
        BATTERY_LEASE_LIMIT.as_millis() as u64,
        source,
        allow_battery,
    )
    .map_err(|error| match error {
        PowerLeasePolicyError::BatteryNotAuthorized => "AC power could not be confirmed. Connect AC or pass --allow-battery for a bounded lease.".to_string(),
        other => other.to_string(),
    })?;
    let initial_duration = Duration::from_millis(policy.deadline_ms());
    let started = Instant::now();
    let started_at = unix_time_ms();
    let expires_at = started_at.saturating_add(requested.as_millis() as u64);
    let mut leases = LeaseBook::new(32, 24 * 60 * 60 * 1000);
    let identity = ProcessIdentity {
        pid: process_id,
        creation_time_100ns: creation_time,
    };
    let lease_id = leases
        .acquire(identity, started_at, expires_at)
        .map_err(|error| error.to_string())?;

    let stop_event = StopEvent::create()?;
    let event_store = event_store(&config)?;
    event_store.prepare()?;
    let reason = format!("Agent Guardian lease for PID {process_id}");
    let power_lease = PowerLease::acquire(&reason)?;
    let session_id = LeaseId::new_v4();
    event_store.append(&SessionEvent::lease_started(session_id, unix_time_ms()))?;
    let active_lease = ActiveLease::new(power_lease, event_store, session_id);
    println!("Power request active for PID {process_id} (lease {lease_id}).");
    println!(
        "The display remains eligible to turn off. Explicit Sleep, Shutdown, Restart, or critical battery behavior remains under Windows control."
    );
    if on_battery_or_unknown {
        println!(
            "Battery or unknown power source: lease expires in {} minute(s), capped at five minutes; Modern Standby may limit it further.",
            initial_duration.as_secs() / 60
        );
    } else {
        println!(
            "AC power confirmed. Lease expires in {} minute(s). Use guardian stop, Ctrl+C, or wait for the target process to exit.",
            initial_duration.as_secs() / 60
        );
    }

    while (started.elapsed().as_millis() as u64) < policy.deadline_ms() {
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let remaining = Duration::from_millis(policy.deadline_ms().saturating_sub(elapsed_ms));
        let wait_for = remaining.min(POWER_POLL_INTERVAL);
        match wait_for_target_or_stop(&process, &stop_event, wait_for.as_millis().max(1) as u32)? {
            LeaseWait::TargetExited => {
                let _ = leases.reap(unix_time_ms(), |_| false);
                println!("Target process exited; releasing the power request.");
                active_lease.finish(ReleaseReason::TargetExited)?;
                return Ok(());
            }
            LeaseWait::StopRequested => {
                let _ = leases.reap(unix_time_ms(), |_| false);
                println!("Stop requested; releasing the power request.");
                active_lease.finish(ReleaseReason::UserRequestedStop)?;
                return Ok(());
            }
            LeaseWait::Timeout => {}
        }

        let power_source = if power_status()?.ac_line_status == 1 {
            PowerSourceClass::Ac
        } else {
            PowerSourceClass::BatteryOrUnknown
        };
        match policy.observe_source(started.elapsed().as_millis() as u64, power_source) {
            PowerLeaseDecision::Release => {
                println!(
                    "Power changed to battery; releasing the power request because --allow-battery was not set."
                );
                active_lease.finish(ReleaseReason::BatteryTransition)?;
                return Ok(());
            }
            PowerLeaseDecision::DeadlineChanged { .. }
                if power_source == PowerSourceClass::BatteryOrUnknown =>
            {
                println!(
                    "Power changed to battery; the remaining lease is capped at five minutes from this transition."
                );
            }
            PowerLeaseDecision::DeadlineChanged { .. } => {
                println!("AC power restored; the original lease duration resumes.");
            }
            PowerLeaseDecision::Unchanged => {}
        }
    }
    let _ = leases.reap(unix_time_ms(), |_| true);
    println!("Lease expired; releasing the power request.");
    active_lease.finish(ReleaseReason::TtlExpired)?;
    Ok(())
}

pub fn install(json: bool, expected_sha256: &str) -> Result<(), String> {
    install::install(json, expected_sha256)
}

pub fn uninstall(json: bool) -> Result<(), String> {
    install::uninstall(json)
}

pub fn stop(json: bool) -> Result<(), String> {
    StopEvent::request_stop(json)
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn event_store(config: &Config) -> Result<EventStore, String> {
    event_store_at(config, "events.jsonl")
}

fn network_event_store(config: &Config) -> Result<EventStore, String> {
    event_store_at(config, "network-events.jsonl")
}

fn event_store_at(config: &Config, filename: &str) -> Result<EventStore, String> {
    let local_app_data = std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
        "LOCALAPPDATA is unavailable; cannot open the local event log".to_string()
    })?;
    Ok(EventStore::from_logging_config(
        PathBuf::from(local_app_data)
            .join("AgentGuardian")
            .join(filename),
        &config.logging,
    ))
}

pub fn powercfg(arguments: &[&str]) -> Result<String, String> {
    let executable = system_directory()?.join("powercfg.exe");
    if arguments == ["/requests"] {
        let status = Command::new(&executable)
            .args(arguments)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|error| format!("could not run {}: {error}", executable.display()))?;
        if !status.success() {
            return Err(format!(
                "powercfg {} exited with {} (command output suppressed)",
                arguments.join(" "),
                status
            ));
        }
        return Ok(redacted_power_request_output().into());
    }

    let output = Command::new(&executable)
        .args(arguments)
        .output()
        .map_err(|error| format!("could not run {}: {error}", executable.display()))?;
    if !output.status.success() {
        return Err(format!(
            "powercfg {} exited with {} (command output suppressed)",
            arguments.join(" "),
            output.status
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn redacted_power_request_output() -> &'static str {
    "Power request query completed; requestor details were redacted.\n"
}

fn system_directory() -> Result<PathBuf, String> {
    let mut buffer = vec![0_u16; 32_768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 || length as usize >= buffer.len() {
        return Err(format!(
            "GetSystemDirectoryW failed or returned an overlong path (Windows error {})",
            unsafe { GetLastError() }
        ));
    }
    let directory = OsString::from_wide(&buffer[..length as usize]);
    Ok(PathBuf::from(directory))
}

pub fn doctor(json: bool) -> Result<(), String> {
    let available_sleep_states = powercfg(&["/a"]);
    let active_requests = powercfg(&["/requests"]);
    let power = power_status()?;
    let power_source = match power.ac_line_status {
        1 => "ac",
        0 => "battery",
        _ => "unknown",
    };
    let warnings: Vec<_> = [&available_sleep_states, &active_requests]
        .into_iter()
        .filter_map(|result| result.as_ref().err().cloned())
        .collect();
    if json {
        let result = serde_json::json!({
            "guardian_version": env!("CARGO_PKG_VERSION"),
            "os": "windows",
            "architecture": std::env::consts::ARCH,
            "power_source": power_source,
            "battery_percent": (power.battery_life_percent != 255).then_some(power.battery_life_percent),
            "available_sleep_states": available_sleep_states.as_ref().ok(),
            "power_request_query_available": active_requests.is_ok(),
            "power_request_details_redacted": true,
            "diagnostic_warnings": warnings,
            "locked_session_verified": false,
            "network_recovery_enabled": false
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&result).map_err(|error| error.to_string())?
        );
    } else {
        println!("Guardian {}", env!("CARGO_PKG_VERSION"));
        println!("OS: Windows ({})", std::env::consts::ARCH);
        println!("Power source: {power_source}");
        if power.battery_life_percent != 255 {
            println!("Battery: {}%", power.battery_life_percent);
        }
        println!();
        println!("Available sleep states (powercfg /a):");
        match &available_sleep_states {
            Ok(output) => print!("{output}"),
            Err(error) => println!("Unavailable: {error}"),
        }
        println!();
        println!("Active power request diagnostics (powercfg /requests):");
        match &active_requests {
            Ok(output) => print!("{output}"),
            Err(error) => println!("Unavailable: {error}"),
        }
        println!("Locked-session behavior: not verified on this device.");
        println!("Automatic network recovery: disabled.");
        println!("A one-shot saved-profile reconnect requires an explicit network command option.");
    }
    Ok(())
}

pub fn network(
    json: bool,
    probe: bool,
    reconnect_saved_wifi: bool,
    config: Config,
) -> Result<(), String> {
    let mut snapshot = local_network_snapshot()?;
    if reconnect_saved_wifi {
        let outcome = recovery::attempt(&snapshot.summary, true);
        let recovery_event_logged =
            record_recovery_event(network_event_store(&config), &outcome, unix_time_ms());
        let network_change_requested = matches!(
            outcome.disposition,
            "connected" | "connection_not_confirmed" | "connection_confirmation_failed"
        );
        match local_network_snapshot() {
            Ok(mut refreshed) => {
                refreshed.network_change_requested = network_change_requested;
                refreshed.recovery_event_logged = Some(recovery_event_logged);
                refreshed.post_recovery_snapshot_refreshed = Some(true);
                refreshed.recovery = Some(outcome);
                snapshot = refreshed;
            }
            Err(_) => {
                snapshot.network_change_requested = network_change_requested;
                snapshot.recovery_event_logged = Some(recovery_event_logged);
                snapshot.post_recovery_snapshot_refreshed = Some(false);
                snapshot.recovery = Some(outcome);
            }
        }
    }
    if probe {
        let probes = probes::run_bounded_probes();
        snapshot.external_probes_attempted = probes.network_checks_attempted;
        snapshot.internet_connectivity_tested = probes.network_checks_attempted;
        let endpoints = [
            probes
                .microsoft_connect_test
                .attempted
                .then(|| EndpointEvidence {
                    category: "microsoft_ncsi".to_owned(),
                    reachable: probes.microsoft_connect_test.reachable,
                    user_configured_service: false,
                }),
            probes
                .independent_https
                .attempted
                .then(|| EndpointEvidence {
                    category: "independent_https".to_owned(),
                    reachable: probes.independent_https.reachable,
                    user_configured_service: false,
                }),
        ]
        .into_iter()
        .flatten()
        .collect();
        snapshot.connectivity_classification = Some(classify_local_evidence(
            &snapshot.summary,
            &ConnectivityEvidence {
                default_route_present: probes.default_route_present,
                gateway_reachable: probes.gateway_reachable,
                dns_resolves: probes.dns_resolved_for_both,
                captive_portal_suspected: probes.captive_portal_suspected,
                proxy_restricted: probes.proxy_restricted,
                endpoints,
                ..ConnectivityEvidence::default()
            },
        ));
        snapshot.probes = Some(probes);
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&snapshot).map_err(|error| error.to_string())?
        );
    } else {
        println!("Local network snapshot");
        println!(
            "Wi-Fi adapters detected: {}",
            snapshot.summary.wifi_adapters_detected
        );
        println!(
            "Wi-Fi adapter operational: {}",
            snapshot.summary.wifi_adapter_operational
        );
        println!(
            "Operational adapters: {}",
            snapshot.summary.operational_adapter_count
        );
        println!(
            "Gateway address reported: {}",
            snapshot.summary.gateway_address_reported
        );
        println!(
            "Gateway reported on an operational adapter: {}",
            snapshot.summary.operational_gateway_address_reported
        );
        println!(
            "Tunnel-type interface reported: {}",
            snapshot.summary.tunnel_adapter_detected
        );
        println!();
        println!("Adapter metadata is summarized without names or address values.");
        println!("A tunnel-type interface is only a hint; VPN state is not confirmed.");
        if let Some(recovery) = snapshot.recovery {
            println!("Saved-profile reconnect result: {}", recovery.disposition);
            println!(
                "Connection confirmed: {}",
                option_label(recovery.connection_confirmed)
            );
            if let Some(code) = recovery.windows_error {
                println!("Windows error: {code}");
            }
            println!("Profile names and Wi-Fi credentials are not displayed or stored.");
            println!(
                "Post-reconnect snapshot refreshed: {}",
                snapshot.post_recovery_snapshot_refreshed.unwrap_or(false)
            );
            println!(
                "Recovery outcome logged locally: {}",
                snapshot.recovery_event_logged.unwrap_or(false)
            );
        }
        if let Some(probes) = &snapshot.probes {
            println!("One-shot network diagnostics were requested.");
            println!("Web requests attempted: {}", probes.web_requests_attempted);
            println!(
                "Microsoft NCSI name resolved: {}",
                option_label(probes.microsoft_dns_resolved)
            );
            println!(
                "Independent HTTPS name resolved: {}",
                option_label(probes.independent_dns_resolved)
            );
            println!(
                "Default route present: {}",
                option_label(probes.default_route_present)
            );
            println!(
                "Gateway reachability: {}",
                gateway_label(probes.gateway_reachable)
            );
            println!(
                "Microsoft connectivity check: {}",
                probe_label(probes.microsoft_connect_test.reachable)
            );
            println!(
                "Independent HTTPS check: {}",
                probe_label(probes.independent_https.reachable)
            );
            println!(
                "Captive portal suspected: {}",
                probes.captive_portal_suspected
            );
            println!("Proxy authentication required: {}", probes.proxy_restricted);
            if let Some(classification) = snapshot.connectivity_classification {
                println!("Connectivity assessment: {classification:?}");
            }
        } else {
            println!("No external probes were attempted.");
        }
        println!("No adapter or saved-profile configuration was changed.");
        if snapshot.network_change_requested {
            println!("A one-shot request may have changed the active Wi-Fi connection.");
        }
    }
    Ok(())
}

pub fn monitor(json: bool, duration_minutes: u32, interval_seconds: u32) -> Result<(), String> {
    monitor::run(json, duration_minutes, interval_seconds)
}

pub fn logs(json: bool, config: Config) -> Result<(), String> {
    let events = read_all_events(&config)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&events).map_err(|error| error.to_string())?
        );
    } else if events.is_empty() {
        println!("No Guardian events are recorded.");
    } else {
        for event in &events {
            let message = match event.kind {
                SessionEventKind::LeaseStarted => "Power lease started",
                SessionEventKind::LeaseReleased(reason) => release_label(reason),
                SessionEventKind::WifiReconnectOutcome(result) => wifi_reconnect_label(result),
            };
            println!(
                "{}  {}  {}",
                event.occurred_at_unix_ms, event.session_id, message
            );
        }
    }
    Ok(())
}

pub fn report(output: Option<PathBuf>, json: bool, config: Config) -> Result<(), String> {
    let events = read_all_events(&config)?;
    let latest_session = events
        .iter()
        .filter(|event| event.kind == SessionEventKind::LeaseStarted)
        .max_by_key(|event| event.occurred_at_unix_ms)
        .map(|event| event.session_id)
        .ok_or_else(|| "no Guardian session events are recorded".to_string())?;
    let report = SessionReport::from_events(&events, latest_session, unix_time_ms())?;
    if let Some(path) = output {
        write_report(&path, &report)?;
        println!("Wrote report to {}.", path.display());
    } else if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
        );
    } else {
        println!("Guardian session report");
        println!("Session: {}", report.session_id);
        println!("Started (Unix ms): {}", report.started_at_unix_ms);
        match (report.ended_at_unix_ms, report.protected_duration_ms) {
            (Some(ended_at), Some(duration)) => {
                println!("Ended (Unix ms): {ended_at}");
                println!("Protected duration: {}", format_duration(duration));
            }
            _ => println!("Status: incomplete; no release event is recorded."),
        }
        println!("Peak active leases: {}", report.peak_active_leases);
        println!("Incidents:");
        for incident in &report.incidents {
            println!("  {}  {}", incident.occurred_at_unix_ms, incident.summary);
        }
        println!("Unverified behavior:");
        for behavior in &report.unverified_behavior {
            println!("  {behavior}");
        }
    }
    Ok(())
}

fn read_all_events(config: &Config) -> Result<Vec<SessionEvent>, String> {
    let mut events = event_store(config)?.read_events()?;
    let network_store = network_event_store(config)?;
    match std::fs::symlink_metadata(network_store.path()) {
        Ok(_) => events.extend(network_store.read_events()?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "could not inspect local network event log: {error}"
            ));
        }
    }
    events.sort_by_key(|event| event.occurred_at_unix_ms);
    Ok(events)
}

fn recovery_event_result(report: &recovery::RecoveryReport) -> WifiReconnectResult {
    if !report.action_attempted {
        return WifiReconnectResult::NotAttempted;
    }
    match report.disposition {
        "connected" => WifiReconnectResult::AssociationConfirmed,
        "connection_not_confirmed" => WifiReconnectResult::AssociationNotConfirmed,
        "connection_confirmation_failed" => WifiReconnectResult::ConfirmationFailed,
        "connection_request_failed" => WifiReconnectResult::RequestFailed,
        _ => WifiReconnectResult::ConfirmationFailed,
    }
}

fn record_recovery_event(
    store: Result<EventStore, String>,
    report: &recovery::RecoveryReport,
    occurred_at_unix_ms: u64,
) -> bool {
    store
        .and_then(|store| {
            store.append(&SessionEvent::wifi_reconnect_outcome(
                occurred_at_unix_ms,
                recovery_event_result(report),
            ))
        })
        .is_ok()
}

fn wifi_reconnect_label(result: WifiReconnectResult) -> &'static str {
    match result {
        WifiReconnectResult::NotAttempted => "Saved-profile reconnect not attempted",
        WifiReconnectResult::RequestFailed => "Saved-profile reconnect request failed",
        WifiReconnectResult::AssociationConfirmed => "Wi-Fi association confirmed",
        WifiReconnectResult::AssociationNotConfirmed => "Wi-Fi association not confirmed",
        WifiReconnectResult::ConfirmationFailed => "Wi-Fi association confirmation failed",
    }
}

fn write_report(path: &PathBuf, report: &SessionReport) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("could not create report {}: {error}", path.display()))?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(format!(
            "could not write report {}: {error}",
            path.display()
        ));
    }
    Ok(())
}

fn format_duration(duration_ms: u64) -> String {
    let seconds = duration_ms / 1000;
    format!("{}m {}s", seconds / 60, seconds % 60)
}

fn release_label(reason: ReleaseReason) -> &'static str {
    match reason {
        ReleaseReason::TargetExited => "Power lease released when target exited",
        ReleaseReason::TtlExpired => "Power lease expired",
        ReleaseReason::BatteryTransition => "Power lease released after battery transition",
        ReleaseReason::RuntimeError => "Power lease released after runtime error",
        ReleaseReason::RuntimeExit => "Power lease released when runtime exited",
        ReleaseReason::UserRequestedStop => "Power lease released after user stop request",
    }
}

#[derive(Debug, Default, serde::Serialize)]
struct LocalNetworkSnapshot {
    #[serde(flatten)]
    summary: LocalNetworkSummary,
    internet_connectivity_tested: bool,
    external_probes_attempted: bool,
    network_settings_changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    connectivity_classification: Option<ConnectivityState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    probes: Option<probes::ProbeSummary>,
    network_change_requested: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_event_logged: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    post_recovery_snapshot_refreshed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery: Option<recovery::RecoveryReport>,
}

fn local_network_snapshot() -> Result<LocalNetworkSnapshot, String> {
    let snapshot = LocalNetworkSnapshot {
        summary: LocalNetworkSummary::from_adapters(get_adapter_observations()?),
        ..LocalNetworkSnapshot::default()
    };
    Ok(snapshot)
}

fn adapter_observations(
    buffer: &[IP_ADAPTER_ADDRESSES_LH],
) -> Result<Vec<LocalAdapterObservation>, String> {
    if buffer.is_empty() {
        return Ok(Vec::new());
    }
    let base = buffer.as_ptr() as usize;
    let buffer_bytes = buffer
        .len()
        .checked_mul(std::mem::size_of::<IP_ADAPTER_ADDRESSES_LH>())
        .ok_or_else(|| "network adapter buffer size overflowed".to_string())?;
    let end = base
        .checked_add(buffer_bytes)
        .ok_or_else(|| "network adapter buffer address overflowed".to_string())?;
    let mut current = buffer.as_ptr();
    let mut observations = Vec::new();
    let mut visited = 0_usize;
    let minimum_length = std::mem::offset_of!(IP_ADAPTER_ADDRESSES_LH, FirstGatewayAddress)
        + std::mem::size_of::<*mut c_void>();
    let maximum_nodes = buffer_bytes / minimum_length;

    while !current.is_null() {
        let address = current as usize;
        if address < base
            || address
                .checked_add(minimum_length)
                .is_none_or(|minimum_end| minimum_end > end)
            || !(address - base).is_multiple_of(std::mem::align_of::<IP_ADAPTER_ADDRESSES_LH>())
        {
            return Err("Windows returned an invalid network adapter list pointer".into());
        }

        let length =
            unsafe { std::ptr::addr_of!((*current).Anonymous1.Anonymous.Length).read() } as usize;
        if length < minimum_length
            || address
                .checked_add(length)
                .is_none_or(|node_end| node_end > end)
        {
            return Err("Windows returned an incomplete network adapter record".into());
        }
        let if_type = unsafe { std::ptr::addr_of!((*current).IfType).read() };
        let operational_status = unsafe { std::ptr::addr_of!((*current).OperStatus).read() };
        let has_default_gateway =
            !unsafe { std::ptr::addr_of!((*current).FirstGatewayAddress).read() }.is_null();
        observations.push(LocalAdapterObservation {
            is_wifi: if_type == IF_TYPE_IEEE80211,
            is_tunnel: if_type == IF_TYPE_TUNNEL,
            operational: operational_status == IfOperStatusUp,
            default_gateway_reported: has_default_gateway,
        });
        current = unsafe { std::ptr::addr_of!((*current).Next).read() };
        visited += 1;
        if visited > maximum_nodes {
            return Err("Windows returned a cyclic network adapter list".into());
        }
    }

    Ok(observations)
}

fn get_adapter_observations() -> Result<Vec<LocalAdapterObservation>, String> {
    let flags = GAA_FLAG_INCLUDE_GATEWAYS
        | GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_DNS_SERVER
        | GAA_FLAG_SKIP_FRIENDLY_NAME
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_UNICAST;
    let mut capacity_bytes = NETWORK_BUFFER_START_BYTES;
    for _ in 0..3 {
        if capacity_bytes > NETWORK_BUFFER_MAX_BYTES {
            return Err("network adapter metadata exceeds the one MiB safety limit".into());
        }
        let adapter_capacity =
            capacity_bytes.div_ceil(std::mem::size_of::<IP_ADAPTER_ADDRESSES_LH>());
        let mut adapters = vec![IP_ADAPTER_ADDRESSES_LH::default(); adapter_capacity];
        let mut reported_size =
            (adapters.len() * std::mem::size_of::<IP_ADAPTER_ADDRESSES_LH>()) as u32;
        let result = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_UNSPEC),
                flags,
                std::ptr::null(),
                adapters.as_mut_ptr(),
                &mut reported_size,
            )
        };
        if result == 0 {
            return adapter_observations(&adapters);
        }
        if result == ERROR_NO_DATA {
            return Ok(Vec::new());
        }
        if result != ERROR_BUFFER_OVERFLOW {
            return Err(format!(
                "GetAdaptersAddresses failed with Windows error {result}"
            ));
        }
        capacity_bytes = (reported_size as usize).max(capacity_bytes.saturating_mul(2));
    }
    Err("network adapter list changed repeatedly while it was being read".into())
}

fn power_status() -> Result<SystemPowerStatus, String> {
    let mut status = SystemPowerStatus::default();
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
        return Err(format!(
            "GetSystemPowerStatus failed (Windows error {})",
            unsafe { GetLastError() }
        ));
    }
    Ok(status)
}

fn probe_label(reachable: bool) -> &'static str {
    if reachable {
        "reachable"
    } else {
        "not confirmed"
    }
}

fn option_label(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}

fn gateway_label(reachable: Option<bool>) -> &'static str {
    match reachable {
        Some(true) => "confirmed",
        Some(false) => "unreachable",
        None => "not confirmed (ICMP may be filtered)",
    }
}

#[cfg(test)]
mod windows_safety_tests {
    use super::*;

    fn adapter_record() -> IP_ADAPTER_ADDRESSES_LH {
        let mut adapter = IP_ADAPTER_ADDRESSES_LH::default();
        adapter.Anonymous1.Anonymous.Length = std::mem::size_of::<IP_ADAPTER_ADDRESSES_LH>() as u32;
        adapter
    }

    #[test]
    fn power_request_diagnostic_discards_owner_details() {
        let message = redacted_power_request_output();
        assert!(message.contains("redacted"));
        assert!(!message.contains("PID"));
        assert!(!message.contains("C:\\Users\\"));
    }

    #[test]
    fn adapter_summary_follows_only_linked_records() {
        let mut adapters = vec![adapter_record(), adapter_record(), adapter_record()];
        adapters[0].IfType = IF_TYPE_IEEE80211;
        adapters[0].OperStatus = IfOperStatusUp;
        adapters[0].FirstGatewayAddress = std::ptr::NonNull::<
            windows_sys::Win32::NetworkManagement::IpHelper::IP_ADAPTER_GATEWAY_ADDRESS_LH,
        >::dangling()
        .as_ptr();
        adapters[1].IfType = IF_TYPE_IEEE80211;
        adapters[1].OperStatus = IfOperStatusUp;
        unsafe {
            let base = adapters.as_mut_ptr();
            (*base).Next = base.add(2);
            (*base.add(2)).Next = std::ptr::null_mut();
        }

        let observations = adapter_observations(&adapters).unwrap();
        assert_eq!(observations.len(), 2);
        assert!(observations[0].is_wifi);
        assert!(observations[0].operational);
        assert!(observations[0].default_gateway_reported);
        assert!(!observations[1].is_wifi);
    }

    #[test]
    fn adapter_summary_rejects_invalid_and_cyclic_links() {
        let mut adapter = adapter_record();
        let adapter_ptr = &mut adapter as *mut IP_ADAPTER_ADDRESSES_LH;
        unsafe {
            (*adapter_ptr).Next = std::ptr::NonNull::<IP_ADAPTER_ADDRESSES_LH>::dangling().as_ptr();
        }
        assert!(adapter_observations(std::slice::from_ref(&adapter)).is_err());

        let mut adapters = vec![adapter_record()];
        adapters[0].Next = adapters.as_mut_ptr();
        assert!(adapter_observations(&adapters).is_err());
    }

    #[test]
    fn target_process_must_share_current_owner_and_session() {
        assert!(validate_process_context(true, 7, 7).is_ok());
        assert!(validate_process_context(false, 7, 7).is_err());
        assert!(validate_process_context(true, 8, 7).is_err());
    }

    #[test]
    fn current_process_context_can_be_verified() {
        let process_id = unsafe { GetCurrentProcessId() };
        assert!(ProcessHandle::open(process_id).is_ok());
    }
}

#[cfg(test)]
mod recovery_event_tests {
    use super::*;
    use std::fs;

    #[test]
    fn recovery_result_is_logged_redacted_and_survives_log_failure() {
        let directory =
            std::env::temp_dir().join(format!("guardian-network-event-test-{}", LeaseId::new_v4()));
        fs::create_dir(&directory).unwrap();
        let store = EventStore::new(directory.join("network-events.jsonl"), 4096, 14);
        let outcome = recovery::RecoveryReport {
            action_attempted: true,
            connection_confirmed: Some(true),
            disposition: "connected",
            windows_error: None,
        };

        assert!(record_recovery_event(Ok(store.clone()), &outcome, 123));
        let events = store.read_events().unwrap();
        assert!(matches!(
            events[0].kind,
            SessionEventKind::WifiReconnectOutcome(WifiReconnectResult::AssociationConfirmed)
        ));
        let serialized = fs::read_to_string(store.path()).unwrap();
        assert!(!serialized.to_ascii_lowercase().contains("ssid"));
        assert!(!serialized.contains("profile"));

        let blocker = directory.join("not-a-directory");
        fs::write(&blocker, b"existing file").unwrap();
        let unavailable_store = EventStore::new(blocker.join("network-events.jsonl"), 4096, 14);
        assert!(!record_recovery_event(Ok(unavailable_store), &outcome, 456));
        assert_eq!(outcome.disposition, "connected");

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn recovery_dispositions_map_to_typed_outcomes() {
        let cases = [
            (
                false,
                "wifi_disconnect_not_confirmed",
                None,
                WifiReconnectResult::NotAttempted,
            ),
            (
                true,
                "connection_request_failed",
                None,
                WifiReconnectResult::RequestFailed,
            ),
            (
                true,
                "connected",
                Some(true),
                WifiReconnectResult::AssociationConfirmed,
            ),
            (
                true,
                "connection_not_confirmed",
                Some(false),
                WifiReconnectResult::AssociationNotConfirmed,
            ),
            (
                true,
                "connection_confirmation_failed",
                None,
                WifiReconnectResult::ConfirmationFailed,
            ),
        ];

        for (action_attempted, disposition, connection_confirmed, expected) in cases {
            let report = recovery::RecoveryReport {
                action_attempted,
                connection_confirmed,
                disposition,
                windows_error: None,
            };
            assert_eq!(recovery_event_result(&report), expected);
        }
    }
}
