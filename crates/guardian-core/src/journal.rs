use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

use crate::config::LoggingConfig;

const EVENT_SCHEMA_VERSION: u16 = 1;
const ROTATED_FILE_COUNT: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseReason {
    TargetExited,
    TtlExpired,
    BatteryTransition,
    RuntimeError,
    RuntimeExit,
    UserRequestedStop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiReconnectResult {
    NotAttempted,
    RequestFailed,
    AssociationConfirmed,
    AssociationNotConfirmed,
    ConfirmationFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", content = "reason", rename_all = "snake_case")]
pub enum SessionEventKind {
    LeaseStarted,
    LeaseReleased(ReleaseReason),
    WifiReconnectOutcome(WifiReconnectResult),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionEvent {
    pub schema_version: u16,
    pub session_id: Uuid,
    pub occurred_at_unix_ms: u64,
    pub kind: SessionEventKind,
}

impl SessionEvent {
    pub fn lease_started(session_id: Uuid, occurred_at_unix_ms: u64) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id,
            occurred_at_unix_ms,
            kind: SessionEventKind::LeaseStarted,
        }
    }

    pub fn lease_released(
        session_id: Uuid,
        occurred_at_unix_ms: u64,
        reason: ReleaseReason,
    ) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id,
            occurred_at_unix_ms,
            kind: SessionEventKind::LeaseReleased(reason),
        }
    }

    pub fn wifi_reconnect_outcome(occurred_at_unix_ms: u64, result: WifiReconnectResult) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            // Network outcomes are standalone records, not members of a power lease session.
            session_id: Uuid::new_v4(),
            occurred_at_unix_ms,
            kind: SessionEventKind::WifiReconnectOutcome(result),
        }
    }
}

/// Bounded JSONL event storage for typed lease lifecycle and Wi-Fi reconnect result events.
#[derive(Debug, Clone)]
pub struct EventStore {
    path: PathBuf,
    maximum_file_bytes: u64,
    retention: Duration,
    rotated_file_count: u8,
}

impl EventStore {
    pub fn from_logging_config(path: impl Into<PathBuf>, config: &LoggingConfig) -> Self {
        Self::new(
            path,
            u64::from(config.maximum_file_size_mib) * 1024 * 1024,
            config.retention_days,
        )
    }

    pub fn new(path: impl Into<PathBuf>, maximum_file_bytes: u64, retention_days: u16) -> Self {
        Self {
            path: path.into(),
            maximum_file_bytes: maximum_file_bytes.max(256),
            retention: Duration::from_secs(u64::from(retention_days.max(1)) * 24 * 60 * 60),
            rotated_file_count: ROTATED_FILE_COUNT,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Create the log directory and reject symlinked log paths before a runtime begins.
    pub fn prepare(&self) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "event log path has no parent directory".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create local event directory: {error}"))?;
        let parent_metadata = fs::symlink_metadata(parent)
            .map_err(|error| format!("could not inspect local event directory: {error}"))?;
        if parent_metadata.file_type().is_symlink() {
            return Err("local event directory must not be a symbolic link".into());
        }
        self.ensure_regular_or_missing(&self.path)?;
        self.ensure_regular_or_missing(&self.lock_path())?;
        for index in 1..self.rotated_file_count {
            self.ensure_regular_or_missing(&self.rotated_path(index))?;
        }
        Ok(())
    }

    pub fn append(&self, event: &SessionEvent) -> Result<(), String> {
        self.prepare()?;
        let _lock = self.acquire_lock()?;
        let mut encoded = serde_json::to_vec(event)
            .map_err(|error| format!("could not encode local event: {error}"))?;
        encoded.push(b'\n');
        if encoded.len() as u64 > self.maximum_file_bytes {
            return Err("local event exceeds the configured file-size limit".into());
        }
        self.remove_expired_files()?;
        let current_size = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(format!("could not inspect local event log: {error}")),
        };
        if current_size.saturating_add(encoded.len() as u64) > self.maximum_file_bytes {
            self.rotate()?;
        }
        self.ensure_regular_or_missing(&self.path)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| format!("could not append local event log: {error}"))?;
        file.write_all(&encoded)
            .and_then(|()| file.flush())
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("could not write local event log: {error}"))
    }

    pub fn read_events(&self) -> Result<Vec<SessionEvent>, String> {
        self.prepare()?;
        let _lock = self.acquire_lock()?;
        let mut events = Vec::new();
        for path in self.all_paths() {
            self.ensure_regular_or_missing(&path)?;
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(format!("could not read local event log: {error}")),
            };
            let metadata = file
                .metadata()
                .map_err(|error| format!("could not inspect local event log: {error}"))?;
            if metadata.len() > self.maximum_file_bytes {
                return Err("local event log exceeds the configured file-size limit".into());
            }
            for (line_index, line) in BufReader::new(file).lines().enumerate() {
                let line = line.map_err(|error| format!("could not read event line: {error}"))?;
                if line.trim().is_empty() {
                    continue;
                }
                let event: SessionEvent = serde_json::from_str(&line).map_err(|error| {
                    format!(
                        "invalid event log record at {}:{}: {error}",
                        path.display(),
                        line_index + 1
                    )
                })?;
                if event.schema_version != EVENT_SCHEMA_VERSION {
                    return Err(format!(
                        "unsupported local event schema version {}",
                        event.schema_version
                    ));
                }
                events.push(event);
            }
        }
        events.sort_by_key(|event| event.occurred_at_unix_ms);
        Ok(events)
    }

    fn rotate(&self) -> Result<(), String> {
        for index in (1..self.rotated_file_count).rev() {
            let source = if index == 1 {
                self.path.clone()
            } else {
                self.rotated_path(index - 1)
            };
            let destination = self.rotated_path(index);
            self.ensure_regular_or_missing(&source)?;
            self.ensure_regular_or_missing(&destination)?;
            if !source.exists() {
                continue;
            }
            if destination.exists() {
                fs::remove_file(&destination)
                    .map_err(|error| format!("could not rotate local event log: {error}"))?;
            }
            fs::rename(&source, &destination)
                .map_err(|error| format!("could not rotate local event log: {error}"))?;
        }
        Ok(())
    }

    fn remove_expired_files(&self) -> Result<(), String> {
        let now = SystemTime::now();
        for path in self.all_paths() {
            self.ensure_regular_or_missing(&path)?;
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(format!("could not inspect local event log: {error}")),
            };
            let expired = metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > self.retention);
            if expired {
                fs::remove_file(&path).map_err(|error| {
                    format!("could not remove expired local event log: {error}")
                })?;
            }
        }
        Ok(())
    }

    fn ensure_regular_or_missing(&self, path: &Path) -> Result<(), String> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
                "event log path must not be a symbolic link: {}",
                path.display()
            )),
            Ok(metadata) if !metadata.is_file() => Err(format!(
                "event log path is not a regular file: {}",
                path.display()
            )),
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("could not inspect event log path: {error}")),
        }
    }

    fn rotated_path(&self, index: u8) -> PathBuf {
        let mut name = self.path.file_stem().unwrap_or_default().to_os_string();
        name.push(format!(".{index}"));
        if let Some(extension) = self.path.extension() {
            name.push(".");
            name.push(extension);
        }
        self.path.with_file_name(name)
    }

    fn lock_path(&self) -> PathBuf {
        self.path.with_extension("lock")
    }

    fn acquire_lock(&self) -> Result<File, String> {
        let path = self.lock_path();
        self.ensure_regular_or_missing(&path)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| format!("could not open event log lock: {error}"))?;
        file.lock()
            .map_err(|error| format!("could not lock local event log: {error}"))?;
        Ok(file)
    }

    fn all_paths(&self) -> Vec<PathBuf> {
        let mut paths = vec![self.path.clone()];
        paths.extend((1..self.rotated_file_count).map(|index| self.rotated_path(index)));
        paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::SessionReport;

    fn store() -> EventStore {
        let directory =
            std::env::temp_dir().join(format!("agent-guardian-test-{}", Uuid::new_v4()));
        EventStore::new(directory.join("events.jsonl"), 256, 14)
    }

    fn cleanup(store: &EventStore) {
        if let Some(parent) = store.path().parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn event_store_round_trips_typed_power_and_privacy_limited_network_events() {
        let store = store();
        let network_store = EventStore::new(
            store.path().parent().unwrap().join("network-events.jsonl"),
            256,
            14,
        );
        let session = Uuid::new_v4();
        store
            .append(&SessionEvent::lease_started(session, 100))
            .unwrap();
        network_store
            .append(&SessionEvent::wifi_reconnect_outcome(
                50,
                WifiReconnectResult::NotAttempted,
            ))
            .unwrap();
        network_store
            .append(&SessionEvent::wifi_reconnect_outcome(
                150,
                WifiReconnectResult::AssociationConfirmed,
            ))
            .unwrap();
        network_store
            .append(&SessionEvent::wifi_reconnect_outcome(
                250,
                WifiReconnectResult::RequestFailed,
            ))
            .unwrap();
        store
            .append(&SessionEvent::lease_released(
                session,
                200,
                ReleaseReason::TargetExited,
            ))
            .unwrap();
        let stored_text = fs::read_to_string(store.path()).unwrap();
        assert!(!stored_text.contains("process_id"));
        assert!(!stored_text.contains("wifi_reconnect_outcome"));
        let network_text = fs::read_to_string(network_store.path()).unwrap();
        assert!(!network_text.to_ascii_lowercase().contains("ssid"));
        assert!(!network_text.contains("home-wifi"));
        assert!(!network_text.contains("192.168.1.42"));
        let mut events = store.read_events().unwrap();
        events.extend(network_store.read_events().unwrap());
        events.sort_by_key(|event| event.occurred_at_unix_ms);
        let report = SessionReport::from_events(&events, session, 250).unwrap();
        assert_eq!(report.started_at_unix_ms, 100);
        assert_eq!(report.ended_at_unix_ms, Some(200));
        assert_eq!(report.protected_duration_ms, Some(100));
        assert_eq!(report.incidents.len(), 3);
        assert_eq!(
            report.incidents[1].kind,
            crate::report::IncidentKind::WifiAssociationConfirmed
        );
        assert!(
            report.incidents[1]
                .summary
                .contains("Internet access was not tested")
        );
        assert!(report.validate());
        cleanup(&store);
    }

    #[test]
    fn concurrent_sessions_are_serialized_in_the_shared_event_store() {
        let store = store();
        let directory = store.path().parent().unwrap().to_path_buf();
        let store = EventStore::new(directory.join("events.jsonl"), 16_384, 14);
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || {
                    for timestamp in 0..10 {
                        store
                            .append(&SessionEvent::lease_started(Uuid::new_v4(), timestamp))
                            .unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(store.read_events().unwrap().len(), 40);
        cleanup(&store);
    }

    #[test]
    fn event_store_rotates_and_keeps_a_fixed_number_of_bounded_files() {
        let store = store();
        let session = Uuid::new_v4();
        for timestamp in 0..12 {
            store
                .append(&SessionEvent::lease_started(session, timestamp))
                .unwrap();
        }
        for path in store.all_paths() {
            if path.exists() {
                assert!(fs::metadata(path).unwrap().len() <= 256);
            }
        }
        assert!(store.rotated_path(2).exists());
        cleanup(&store);
    }

    #[test]
    fn configured_log_size_rotates_and_retention_removes_expired_files() {
        let directory =
            std::env::temp_dir().join(format!("agent-guardian-config-{}", Uuid::new_v4()));
        let config = LoggingConfig {
            maximum_file_size_mib: 1,
            retention_days: 1,
        };
        let store = EventStore::from_logging_config(directory.join("events.jsonl"), &config);
        store.prepare().unwrap();

        let session_id = Uuid::new_v4();
        let seed_event = SessionEvent::lease_started(session_id, 100);
        let next_event = SessionEvent::lease_released(session_id, 200, ReleaseReason::TargetExited);
        let mut seed_line = serde_json::to_vec(&seed_event).unwrap();
        seed_line.push(b'\n');
        let mut next_line = serde_json::to_vec(&next_event).unwrap();
        next_line.push(b'\n');
        assert!(next_line.len() > seed_line.len());
        let repetitions = (1024 * 1024) / seed_line.len();
        let seeded_log = seed_line.repeat(repetitions);
        assert!(seeded_log.len() as u64 <= store.maximum_file_bytes);
        assert!(seeded_log.len() + next_line.len() > store.maximum_file_bytes as usize);
        fs::write(store.path(), &seeded_log).unwrap();

        store.append(&next_event).unwrap();
        let rotated = store.rotated_path(1);
        assert_eq!(
            fs::metadata(&rotated).unwrap().len(),
            seeded_log.len() as u64
        );
        assert_eq!(store.read_events().unwrap().len(), repetitions + 1);

        let old_time = SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60);
        OpenOptions::new()
            .write(true)
            .open(&rotated)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(old_time))
            .unwrap();
        store
            .append(&SessionEvent::lease_started(Uuid::new_v4(), 300))
            .unwrap();
        assert!(!rotated.exists());
        assert_eq!(store.read_events().unwrap().len(), 2);
        cleanup(&store);
    }

    #[test]
    fn incomplete_sessions_do_not_fabricate_protected_duration() {
        let store = store();
        let session = Uuid::new_v4();
        store
            .append(&SessionEvent::lease_started(session, 100))
            .unwrap();
        let events = store.read_events().unwrap();
        let report = SessionReport::from_events(&events, session, 300).unwrap();
        assert_eq!(report.ended_at_unix_ms, None);
        assert_eq!(report.protected_duration_ms, None);
        assert!(report.validate());
        cleanup(&store);
    }

    #[test]
    fn event_store_rejects_symbolic_log_paths() {
        let store = store();
        let parent = store.path().parent().unwrap();
        store.prepare().unwrap();
        let link_path = parent.join("linked.jsonl");
        #[cfg(windows)]
        {
            let target = parent.join("target.jsonl");
            fs::write(&target, "").unwrap();
            if std::os::windows::fs::symlink_file(&target, &link_path).is_ok() {
                let linked_store = EventStore::new(link_path, 256, 14);
                assert!(linked_store.prepare().is_err());
            }
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(parent.join("target.jsonl"), &link_path).unwrap();
            let linked_store = EventStore::new(link_path, 256, 14);
            assert!(linked_store.prepare().is_err());
        }
        cleanup(&store);
    }
}
