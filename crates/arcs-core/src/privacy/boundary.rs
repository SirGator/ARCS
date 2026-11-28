//! Egress-Grenze einer Adapterinstallation.
//!
//! Die Grenze beschreibt, wohin Daten aus einer Installation herausgelangen
//! dürfen. Sie ist eine Betreiberentscheidung und steht deshalb im Grant, nicht
//! im Manifest: Das Manifest sagt, was ein Adapter technisch kann, der Grant
//! sagt, was er darf.
//!
//! Ohne Grenze gilt Default-Deny. Ein Auftrag, der ein Ziel nennt, das die
//! Grenze nicht freigibt, wird nicht gesendet.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Schlüssel der Payload-Markierung, die ein externes Ziel benennt.
pub const DESTINATION_MARKER: &str = "$destination";

/// Grenze der externen Umgebung einer Adapterinstallation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryPolicy {
    /// Darf die Installation das Netzwerk erreichen?
    ///
    /// Ohne Netzwerk bleiben nur lokale Ziele ohne Hostnamen zulässig.
    #[serde(default)]
    pub network: bool,
    /// Erlaubte Ziele.
    ///
    /// Ein Eintrag ist ein Hostname (`api.example.com`) oder ein Suffix
    /// (`.example.com`), der alle Subdomains einschließt. Ein Schema, ein
    /// Port oder ein Pfad ist nicht erlaubt.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destinations: Vec<String>,
    /// Erlaubte Dateisystemwurzel für die Arbeitsverzeichnisse der Installation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filesystem_roots: Vec<String>,
}

/// Verletzung einer Egress-Grenze.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryViolation {
    /// Die Installation darf das Netzwerk nicht erreichen.
    NetworkNotAllowed,
    /// Das Ziel ist nicht freigegeben.
    DestinationNotAllowed { destination: String },
    /// Der Pfad liegt außerhalb der freigegebenen Wurzeln.
    PathNotAllowed { path: String },
}

impl BoundaryPolicy {
    /// Prüft die strukturelle Gültigkeit der Grenze.
    pub fn is_valid(&self) -> bool {
        let destinations_valid = self.destinations.iter().all(|entry| {
            let host = entry.trim();
            if host.is_empty() || host.len() > 253 {
                return false;
            }
            let body = host.strip_prefix('.').unwrap_or(host);
            !body.is_empty()
                && body
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        });
        destinations_valid
            && self.filesystem_roots.iter().all(|root| {
                root.starts_with('/')
                    && root.len() > 1
                    && !root.chars().any(char::is_control)
                    && !root.ends_with('/')
            })
    }

    /// Prüft, ob ein Ziel von der Grenze freigegeben ist.
    pub fn allows_destination(&self, destination: &str) -> bool {
        if !self.network {
            return false;
        }
        let host = normalize_host(destination);
        if host.is_empty() {
            return false;
        }
        self.destinations.iter().any(|allowed| {
            let allowed = allowed.trim();
            if let Some(suffix) = allowed.strip_prefix('.') {
                host == suffix || host.ends_with(&format!(".{suffix}"))
            } else {
                host == allowed
            }
        })
    }

    /// Prüft, ob ein Pfad unter einer freigegebenen Wurzel liegt.
    pub fn allows_path(&self, path: &str) -> bool {
        if !path.starts_with('/') {
            return false;
        }
        self.filesystem_roots.iter().any(|root| {
            let root = root.trim_end_matches('/');
            path == root || path.starts_with(&format!("{root}/"))
        })
    }

    /// Prüft ein Ziel und benennt die Verletzung.
    pub fn check_destination(&self, destination: &str) -> Result<(), BoundaryViolation> {
        if self.allows_destination(destination) {
            return Ok(());
        }
        if !self.network {
            return Err(BoundaryViolation::NetworkNotAllowed);
        }
        Err(BoundaryViolation::DestinationNotAllowed {
            destination: destination.to_string(),
        })
    }

    /// Prüft einen Pfad und benennt die Verletzung.
    pub fn check_path(&self, path: &str) -> Result<(), BoundaryViolation> {
        if self.allows_path(path) {
            return Ok(());
        }
        Err(BoundaryViolation::PathNotAllowed {
            path: path.to_string(),
        })
    }
}

/// Bringt ein Ziel auf seinen Hostnamen zurück.
///
/// Schema, Port und Pfad werden entfernt, damit `https://api.example.com/v1`
/// und `api.example.com` dasselbe Ziel beschreiben.
fn normalize_host(destination: &str) -> String {
    let without_scheme = match destination.split_once("://") {
        Some((scheme, rest))
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") =>
        {
            rest
        }
        _ => destination,
    };
    let host = without_scheme
        .split('/')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default();
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Sammelt alle Ziele, die ein Auftrag als Payload-Markierung benennt.
///
/// Die Markierung ist ein Objekt mit genau einem Schlüssel `$destination` und
/// einem Textwert. Alles andere bleibt unberührt: Die Grenze liest nur
/// Markierungen, sie verändert keinen Inhalt.
pub fn collect_destinations(payload: &Value) -> Vec<String> {
    let mut destinations = Vec::new();
    walk(payload, &mut |value| {
        if let Some(destination) = marker_text(value, DESTINATION_MARKER) {
            destinations.push(destination);
        }
    });
    destinations
}

/// Besucht alle Knoten eines JSON-Baums in Tiefensuche.
fn walk(value: &Value, visit: &mut impl FnMut(&Value)) {
    visit(value);
    match value {
        Value::Array(items) => items.iter().for_each(|item| walk(item, visit)),
        Value::Object(fields) => fields.values().for_each(|item| walk(item, visit)),
        _ => {}
    }
}

/// Liest eine Markierung mit genau einem Schlüssel und einem Textwert.
fn marker_text(value: &Value, key: &str) -> Option<String> {
    let Value::Object(fields) = value else {
        return None;
    };
    if fields.len() != 1 {
        return None;
    }
    let text = fields.get(key)?.as_str()?.trim();
    if text.is_empty() || text.len() > 253 {
        return None;
    }
    Some(text.to_string())
}
