use std::{fs, path::Path};

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VersionFormat {
    Calendar,
    Cargo,
}

pub fn validate_version(version: &str, format: VersionFormat) -> Result<()> {
    match format {
        VersionFormat::Calendar => {
            cargo_version(version)?;
            Ok(())
        }
        VersionFormat::Cargo => {
            if is_cargo_version(version) {
                Ok(())
            } else {
                Err(Error::InvalidInput(format!(
                    "version {version:?} is not a supported Cargo version"
                )))
            }
        }
    }
}

pub fn cargo_version(public_version: &str) -> Result<String> {
    let parts = public_version.split('.').collect::<Vec<_>>();
    if parts.len() != 4
        || parts[0].len() != 4
        || parts[1].len() != 2
        || parts[2].len() != 2
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(Error::InvalidInput(
            "calendar versions must use YYYY.MM.DD.N".into(),
        ));
    }
    let year = parts[0].parse::<u16>().map_err(invalid_number)?;
    let month = parts[1].parse::<u8>().map_err(invalid_number)?;
    let day = parts[2].parse::<u8>().map_err(invalid_number)?;
    let release = parts[3].parse::<u32>().map_err(invalid_number)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || release == 0 {
        return Err(Error::InvalidInput(
            "calendar version contains an invalid date or release number".into(),
        ));
    }
    Ok(format!("{year}.{month}.{day}-{release}"))
}

pub fn emit_version_file(
    path: impl AsRef<Path>,
    environment_name: &str,
    format: VersionFormat,
) -> Result<String> {
    if environment_name.is_empty()
        || !environment_name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(Error::InvalidInput(
            "build environment name must use uppercase ASCII, digits, and underscores".into(),
        ));
    }
    let path = path.as_ref();
    let version = fs::read_to_string(path)?;
    let version = version.trim();
    validate_version(version, format)?;
    println!("cargo:rerun-if-changed={}", path.display());
    println!("cargo:rustc-env={environment_name}={version}");
    Ok(version.to_string())
}

fn invalid_number(error: impl std::fmt::Display) -> Error {
    Error::InvalidInput(format!("invalid numeric version component: {error}"))
}

fn is_cargo_version(version: &str) -> bool {
    let core = version
        .split_once('+')
        .map_or(version, |(value, _)| value)
        .split_once('-')
        .map_or(version, |(value, _)| value);
    let parts = core.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_versions_map_to_cargo() {
        assert_eq!(cargo_version("2026.07.21.12").unwrap(), "2026.7.21-12");
        assert!(cargo_version("2026.13.21.1").is_err());
        assert!(cargo_version("2026.07.21.0").is_err());
        assert!(cargo_version("1.2.3").is_err());
    }

    #[test]
    fn cargo_versions_accept_release_and_build_metadata() {
        assert!(validate_version("1.2.3", VersionFormat::Cargo).is_ok());
        assert!(validate_version("1.2.3-rc.1", VersionFormat::Cargo).is_ok());
        assert!(validate_version("1.2", VersionFormat::Cargo).is_err());
    }
}
