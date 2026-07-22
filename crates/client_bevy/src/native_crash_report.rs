//! Privacy-safe next-launch native crash collection for `GB-M03-09`.
//!
//! The canonical GDD (`TECH-123`, `TEL-001`-`005`), Content Production Specification
//! (`CONT-002`), and Development Roadmap (`ADR-005`, `GB-M03-09`) permit only bounded typed
//! crash facts. Panic text, stack traces, paths, account data, tickets, and network identifiers
//! are never written. One marker is retained until the authenticated server durably accepts it.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Once,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use protocol::{
    NATIVE_CRASH_SCHEMA_VERSION, NativeCrashKindV1, NativeCrashReportFrameV1,
    NativeCrashReportResultCodeV1, NativeCrashReportResultV1,
};

const MARKER_FILE: &str = "pending-native-crash-v1.json";
const TEMP_FILE: &str = "pending-native-crash-v1.tmp";
const SIGNATURE_DOMAIN: &[u8] = b"gravebound.native-crash-signature.v1";

static INSTALL: Once = Once::new();

pub(crate) fn install() {
    INSTALL.call_once(|| {
        let started = Instant::now();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Some(path) = marker_path() {
                let _ = persist_panic_marker(&path, started, info.location());
            }
            previous(info);
        }));
    });
}

pub(crate) fn pending() -> Option<NativeCrashReportFrameV1> {
    let path = marker_path()?;
    let marker = load_marker(&path);
    if marker.is_none() {
        // A partial/tampered marker is never transmitted and must not permanently prevent a
        // future valid crash from claiming the single bounded slot.
        let _ = fs::remove_file(path);
    }
    marker
}

pub(crate) fn acknowledge(result: &NativeCrashReportResultV1) {
    let Some(path) = marker_path() else { return };
    acknowledge_path(&path, result);
}

fn acknowledge_path(path: &Path, result: &NativeCrashReportResultV1) {
    if result.code != NativeCrashReportResultCodeV1::Accepted {
        return;
    }
    let Some(marker) = load_marker(path) else {
        return;
    };
    if marker.crash_id == result.crash_id {
        let _ = fs::remove_file(path);
    }
}

fn marker_path() -> Option<PathBuf> {
    let root = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(root)
            .join("Gravebound")
            .join("Telemetry")
            .join(MARKER_FILE),
    )
}

fn persist_panic_marker(
    path: &Path,
    started: Instant,
    location: Option<&std::panic::Location<'_>>,
) -> Result<(), ()> {
    if path.exists() {
        return Ok(());
    }
    let occurred_at_utc_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ())?
        .as_millis()
        .try_into()
        .map_err(|_| ())?;
    let mut signature = blake3::Hasher::new();
    signature.update(SIGNATURE_DOMAIN);
    if let Some(location) = location {
        signature.update(location.file().as_bytes());
        signature.update(&location.line().to_le_bytes());
        signature.update(&location.column().to_le_bytes());
    } else {
        signature.update(b"unknown-location");
    }
    let marker = NativeCrashReportFrameV1 {
        schema_version: NATIVE_CRASH_SCHEMA_VERSION,
        crash_id: *uuid::Uuid::now_v7().as_bytes(),
        kind: NativeCrashKindV1::Panic,
        signature: *signature.finalize().as_bytes(),
        uptime_millis: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        occurred_at_utc_millis,
    };
    write_marker(path, &marker)
}

fn write_marker(path: &Path, marker: &NativeCrashReportFrameV1) -> Result<(), ()> {
    marker.validate().map_err(|_| ())?;
    let parent = path.parent().ok_or(())?;
    fs::create_dir_all(parent).map_err(|_| ())?;
    let encoded = serde_json::to_vec(marker).map_err(|_| ())?;
    let temporary = parent.join(TEMP_FILE);
    fs::write(&temporary, encoded).map_err(|_| ())?;
    if path.exists() {
        let _ = fs::remove_file(temporary);
        return Ok(());
    }
    fs::rename(&temporary, path).map_err(|_| ())
}

fn load_marker(path: &Path) -> Option<NativeCrashReportFrameV1> {
    let bytes = fs::read(path).ok()?;
    let marker: NativeCrashReportFrameV1 = serde_json::from_slice(&bytes).ok()?;
    marker.validate().ok()?;
    Some(marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_contains_only_the_typed_redacted_report_and_survives_nonacceptance() {
        let root =
            std::env::temp_dir().join(format!("gravebound-crash-marker-{}", uuid::Uuid::now_v7()));
        let path = root.join(MARKER_FILE);
        let marker = NativeCrashReportFrameV1 {
            schema_version: NATIVE_CRASH_SCHEMA_VERSION,
            crash_id: *uuid::Uuid::now_v7().as_bytes(),
            kind: NativeCrashKindV1::Panic,
            signature: [4; 32],
            uptime_millis: 10,
            occurred_at_utc_millis: 20,
        };
        write_marker(&path, &marker).unwrap();
        assert_eq!(load_marker(&path), Some(marker.clone()));
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 6);

        let unavailable = NativeCrashReportResultV1 {
            schema_version: NATIVE_CRASH_SCHEMA_VERSION,
            crash_id: marker.crash_id,
            code: NativeCrashReportResultCodeV1::Unavailable,
        };
        acknowledge_path(&path, &unavailable);
        assert!(path.exists());

        let accepted = NativeCrashReportResultV1 {
            code: NativeCrashReportResultCodeV1::Accepted,
            ..unavailable
        };
        acknowledge_path(&path, &accepted);
        assert!(!path.exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_marker_is_not_overwritten_by_a_later_crash() {
        let root =
            std::env::temp_dir().join(format!("gravebound-crash-marker-{}", uuid::Uuid::now_v7()));
        let path = root.join(MARKER_FILE);
        let first = NativeCrashReportFrameV1 {
            schema_version: NATIVE_CRASH_SCHEMA_VERSION,
            crash_id: *uuid::Uuid::now_v7().as_bytes(),
            kind: NativeCrashKindV1::Panic,
            signature: [5; 32],
            uptime_millis: 10,
            occurred_at_utc_millis: 20,
        };
        let mut second = first.clone();
        second.crash_id = *uuid::Uuid::now_v7().as_bytes();
        write_marker(&path, &first).unwrap();
        write_marker(&path, &second).unwrap();
        assert_eq!(load_marker(&path), Some(first));
        fs::remove_dir_all(root).unwrap();
    }
}
