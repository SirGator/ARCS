//! Auflösung der wirksamen Schutzstufe beim Commit.
//!
//! Der Store ist die einzige Stelle, an dem aus Deklaration und Herkunft eine
//! wirksame Schutzstufe entsteht. Damit gilt für alle Erzeuger dieselbe
//! Regel: ohne Deklaration wird geerbt, eine Herabstufung braucht einen
//! belegten Freigabevermerk, und das gespeicherte Artefakt trägt immer seine
//! wirksame Stufe. Leser müssen die Vererbung nicht erneut nachrechnen.

use rusqlite::{Connection, OptionalExtension, params};

use crate::core::security::{
    Classification, ClassificationError, ClassificationSource, resolve_classification,
};
use crate::core::{Artifact, VersionId};

use super::StoreError;

/// Löst die wirksame Schutzstufe eines Artefakts auf.
///
/// Das Ergebnis trägt die aufgelöste Stufe in `security.classification`; die
/// Deklaration des Erzeugers bleibt unverändert erhalten.
pub(super) fn resolve(
    connection: &Connection,
    artifact: &Artifact,
) -> Result<Artifact, StoreError> {
    let mut inherited: Vec<(VersionId, Classification)> = Vec::new();
    if let Some(provenance) = &artifact.provenance {
        for parent in &provenance.parents {
            if let Some(classification) = effective_classification(connection, parent)? {
                inherited.push((VersionId(parent.clone()), classification));
            }
        }
    }
    if let Some(previous) = previous_classification(connection, artifact)? {
        inherited.push(previous);
    }
    let sources = inherited
        .iter()
        .map(|(version_id, classification)| ClassificationSource {
            version_id,
            classification: *classification,
        })
        .collect::<Vec<_>>();

    // Die Beanspruchung des Erzeugers wirkt wie eine Deklaration: Sie kann
    // die Stufe nur erhöhen. Eine mitgeführte, bereits aufgelöste Stufe darf
    // eine geerbte Schutzstufe nicht wegdеklarieren.
    let claimed = artifact.security.claimed();
    let effective = resolve_classification(
        claimed,
        &sources,
        artifact.security.declassified_from.as_ref(),
        artifact.created_by.actor_type,
        artifact.security.declassification_reason.is_some(),
    )
    .map_err(classification_error)?;

    let mut resolved = artifact.clone();
    resolved.security.classification =
        stored_classification(artifact.security.classification, claimed, effective);
    Ok(resolved)
}

/// Entscheidet, welche Stufe das gespeicherte Artefakt trägt.
///
/// Die Auflösung schreibt nur dann eine Stufe in das Artefakt, wenn sie von
/// der Beanspruchung des Erzeugers abweicht. Ein erzeugtes Artefakt bleibt
/// damit bytegleich, solange die Herkunft nichts strengeres verlangt. Fehlt
/// jede Angabe und verlangt die Herkunft nichts als die Standardstufe, bleibt
/// das Feld leer; die wirksame Stufe ist dann die Standardstufe.
fn stored_classification(
    carried: Option<Classification>,
    claimed: Option<Classification>,
    effective: Classification,
) -> Option<Classification> {
    match (claimed, effective) {
        (None, effective) if effective == Classification::DEFAULT => None,
        (claimed, effective) if claimed == Some(effective) => carried,
        (_, effective) => Some(effective),
    }
}

/// Liest die wirksame Stufe einer bereits gespeicherten Version.
///
/// Fehlt die Version, gibt es keine erbbare Stufe. Ein fehlender Vorgänger
/// senkt die Stufe nicht ab: Die Auflösung bleibt bei der strengsten
/// tatsächlich vorhandenen Quelle.
fn effective_classification(
    connection: &Connection,
    version_id: &str,
) -> Result<Option<Classification>, StoreError> {
    let json: Option<String> = connection
        .query_row(
            "SELECT artifact_json FROM artifact_versions WHERE version_id = ?1",
            params![version_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(json) = json else {
        return Ok(None);
    };
    let artifact: Artifact = serde_json::from_str(&json)?;
    Ok(Some(artifact.security.effective()))
}

/// Liest die Stufe der Vorgängerversion desselben Artefakts.
///
/// Eine neue Version darf nie weniger sensibel sein als die Version, die sie
/// ersetzt. Deshalb ist die Vorgängerversion eine erbbare Quelle.
fn previous_classification(
    connection: &Connection,
    artifact: &Artifact,
) -> Result<Option<(VersionId, Classification)>, StoreError> {
    if artifact.version <= 1 {
        return Ok(None);
    }
    let row: Option<(String, String)> = connection
        .query_row(
            "SELECT version_id, artifact_json FROM artifact_versions
             WHERE artifact_id = ?1 AND version = ?2",
            params![artifact.artifact_id.0, (artifact.version - 1) as i64],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((version_id, json)) = row else {
        return Ok(None);
    };
    let previous: Artifact = serde_json::from_str(&json)?;
    Ok(Some((VersionId(version_id), previous.security.effective())))
}

/// Übersetzt einen Auflösungsfehler in einen Speicherfehler.
fn classification_error(error: ClassificationError) -> StoreError {
    match error {
        ClassificationError::UnauthorizedDowngrade {
            declared,
            inherited,
        } => StoreError::UnauthorizedDowngrade {
            declared,
            inherited,
        },
        ClassificationError::IncompleteDeclassification => StoreError::IncompleteDeclassification,
    }
}
