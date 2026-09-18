//! Fixture starts run as this UID; inspect only that owner, with exact activation.
use crate::{process, types::*};
use std::{fs, os::unix::fs::MetadataExt};

pub(super) fn resolve(d: &Demand) -> Result<Option<Process>, String> {
    if let Some(saved) = &d.process {
        if saved.unit.is_some() || saved.container.is_some() {
            return Err("recovery_fixture_identity_ambiguous".into());
        }
        if process::gone(saved)? {
            return match process::capture(saved.pid, &d.generation) {
                Ok(_) => Err("recovery_fixture_process_changed".into()),
                Err(error) if error == "process_is_zombie" => Ok(None),
                Err(error) => {
                    if std::path::Path::new(&format!("/proc/{}", saved.pid))
                        .try_exists()
                        .map_err(|e| e.to_string())?
                    {
                        Err(error)
                    } else {
                        Ok(None)
                    }
                }
            };
        }
        process::verify(saved, &d.profile)?;
        return Ok(Some(saved.clone()));
    }
    if !d.effect_started {
        return Ok(None);
    }
    let executable = d
        .profile
        .executable
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let uid = fs::metadata("/proc/self").map_err(|e| e.to_string())?.uid();
    let expected = format!("RACK_RUNTIME_ACTIVATION={}", d.generation);
    let mut found = None;
    for entry in fs::read_dir("/proc").map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let metadata = match entry.metadata() {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        if metadata.uid() != uid {
            continue;
        }
        let observed_executable = match fs::read_link(entry.path().join("exe")) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("recovery_fixture_executable_unreadable: {error}")),
        };
        if observed_executable != executable {
            continue;
        }
        let env = match fs::read(entry.path().join("environ")) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("recovery_fixture_environment_unreadable: {error}")),
        };
        if !env
            .split(|byte| *byte == 0)
            .any(|value| value == expected.as_bytes())
        {
            continue;
        }
        let p = process::capture(pid, &d.generation)?;
        process::verify(&p, &d.profile)?;
        if found.is_some() {
            return Err("recovery_fixture_process_ambiguous".into());
        }
        found = Some(p);
    }
    Ok(found)
}
