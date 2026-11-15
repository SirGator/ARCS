//! Host-seitige Prozessgrenze; keine Anbieter- oder Gerätekenntnis.
use std::collections::HashSet;
use std::process::Command;

use crate::HostError;

const BASE: &str = include_str!("../config/environment-base.json");

pub(crate) fn validate(names: &[String]) -> Result<(), HostError> {
    let mut seen = HashSet::new();
    if names.len() > 64
        || names.iter().any(|name| {
            let mut bytes = name.bytes();
            name.len() > 128
                || !bytes
                    .next()
                    .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
                || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || !seen.insert(name)
        })
    {
        return Err(HostError::InvalidConfig("inherit_env requires at most 64 unique environment names (1..128 ASCII letters, digits or '_'; first character a letter or '_')".into()));
    }
    Ok(())
}

pub(crate) fn configure(command: &mut Command, inherit: &[String]) {
    let base: Vec<String> = serde_json::from_str(BASE).expect("bundled process environment policy");
    command.env_clear();
    for name in base.iter().chain(inherit) {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}
