//! Schutzstufen und Herkunftsbindung für Artefakte.
//!
//! Die Schutzstufe beantwortet eine andere Frage als das Vertrauensniveau des
//! Ursprungs: `Trust` beschreibt, wie belastbar eine Information ist, die
//! Klassifikation beschreibt, wie sensibel sie ist. Beide Dimensionen bleiben
//! getrennt.
//!
//! Eine wirksame Klassifikation entsteht aus drei Quellen:
//!
//! 1. Die Deklaration des Erzeugers (`declared`).
//! 2. Die geerbten Stufen der Quellartefakte (`provenance.parents`).
//! 3. Die Stufe der Vorgängerversion desselben Artefakts.
//!
//! Fehlt eine Deklaration, gilt die geerbte Stufe. Eine Deklaration unter der
//! geerbten Stufe ist eine Herabstufung und nur mit ausdrücklicher Freigabe
//! zulässig. Unklarheit führt zur Ablehnung.
//!
//! Deklaration und wirksame Stufe werden getrennt gespeichert: Nur der Store
//! löst die wirksame Stufe auf und schreibt sie in das gespeicherte Artefakt.
//! Dadurch kann ein Erzeuger eine Deklaration nicht versehentlich als bereits
//! geerbte Stufe weiterreichen, und Leser entscheiden ohne erneute Vererbung.

use serde::{Deserialize, Serialize};

use super::{ActorType, VersionId};

/// Schutzstufe eines Artefakts.
///
/// Die Stufen sind geordnet. `Public` ist für allgemein freigegebene
/// Informationen, `Secret` für Werte, die weder in Modelle noch in Logs
/// gelangen dürfen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Public,
    Internal,
    Confidential,
    Restricted,
    Secret,
}

impl Classification {
    /// Stufe, die ein Artefakt ohne Deklaration und ohne geerbte Quelle trägt.
    pub const DEFAULT: Classification = Classification::Internal;

    /// Numerischer Rang der Stufe. Höher bedeutet sensibler.
    pub fn rank(self) -> u8 {
        match self {
            Classification::Public => 0,
            Classification::Internal => 1,
            Classification::Confidential => 2,
            Classification::Restricted => 3,
            Classification::Secret => 4,
        }
    }

    /// Stabile Kurzbezeichnung für Protokolle und Berichte.
    pub fn as_str(self) -> &'static str {
        match self {
            Classification::Public => "public",
            Classification::Internal => "internal",
            Classification::Confidential => "confidential",
            Classification::Restricted => "restricted",
            Classification::Secret => "secret",
        }
    }

    /// Prüft, ob die Stufe die vorgegebene Obergrenze einhält.
    pub fn allows(self, ceiling: Classification) -> bool {
        self.rank() <= ceiling.rank()
    }

    /// Die strengere der beiden Stufen.
    pub fn strongest(self, other: Classification) -> Classification {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

/// Herkunfts- und Schutzangaben eines Artefakts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityProfile {
    /// Vom Erzeuger deklarierte Schutzstufe.
    ///
    /// `None` bedeutet "nicht deklariert". Die wirksame Stufe wird dann aus den
    /// Quellartefakten geerbt. Ein deklarierter Wert unterhalb der geerbten
    /// Stufe ist eine Herabstufung und wird ohne Freigabe abgelehnt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared: Option<Classification>,
    /// Wirksame Schutzstufe dieser Version.
    ///
    /// Sie wird vom Store beim Commit aus Deklaration und geerbten Quellen
    /// aufgelöst und gespeichert. Ohne diesen Wert ist das Artefakt noch nicht
    /// committed; Leser müssen dann die Deklaration als vorläufig behandeln.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
    /// Verantwortliche Organisation oder Person für den Inhalt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Zweck, zu dem der Inhalt verarbeitet werden darf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    /// Quellversion, aus der eine ausdrücklich freigegebene Herabstufung stammt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declassified_from: Option<VersionId>,
    /// Nachvollziehbare Begründung einer freigegebenen Herabstufung.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declassification_reason: Option<String>,
}

/// Grenzen für die Freitextfelder des Sicherheitsprofils.
pub const MAX_SECURITY_TEXT_BYTES: usize = 512;

/// Fehler bei der Prüfung eines Sicherheitsprofils.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityProfileError {
    /// Ein Freitextfeld ist leer, zu lang oder enthält Steuerzeichen.
    InvalidText(&'static str),
    /// Herabstufungsbeleg und Begründung gehören zusammen.
    IncompleteDeclassification,
}

impl SecurityProfile {
    /// Erzeugt ein deklariertes Profil ohne Herkunftsbeleg.
    pub fn declared(classification: Classification) -> Self {
        Self {
            declared: Some(classification),
            ..Default::default()
        }
    }

    /// Erzeugt ein Profil mit dokumentierter Herabstufung.
    pub fn declassified(
        source: VersionId,
        classification: Classification,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            declared: Some(classification),
            declassified_from: Some(source),
            declassification_reason: Some(reason.into()),
            ..Default::default()
        }
    }

    /// Wirksame Stufe des Artefakts.
    ///
    /// Vor dem Commit ist das die strengere von Deklaration und mitgeführter
    /// Stufe, danach die vom Store aufgelöste Stufe. Ohne beide gilt die
    /// Standardstufe.
    pub fn effective(&self) -> Classification {
        self.classification
            .max(self.declared)
            .unwrap_or(Classification::DEFAULT)
    }

    /// Die Stufe, die der Erzeuger selbst beansprucht.
    ///
    /// Eine mitgeführte, bereits aufgelöste Stufe wirkt wie eine Deklaration:
    /// Sie kann die Stufe nur erhöhen, nie senken. Damit kann ein Erzeuger
    /// keine ererbte Schutzstufe wegdeklarieren.
    pub fn claimed(&self) -> Option<Classification> {
        self.classification.max(self.declared)
    }

    /// Prüft die Belegkonsistenz und die Freitextgrenzen des Profils.
    pub fn validate(&self) -> Result<(), SecurityProfileError> {
        for (field, value) in [
            ("owner", self.owner.as_deref()),
            ("purpose", self.purpose.as_deref()),
            (
                "declassification_reason",
                self.declassification_reason.as_deref(),
            ),
        ] {
            if let Some(value) = value
                && (value.trim().is_empty()
                    || value.len() > MAX_SECURITY_TEXT_BYTES
                    || value.chars().any(char::is_control))
            {
                return Err(SecurityProfileError::InvalidText(field));
            }
        }
        match (&self.declassified_from, &self.declassification_reason) {
            (None, None) => Ok(()),
            (Some(_), Some(_)) => Ok(()),
            _ => Err(SecurityProfileError::IncompleteDeclassification),
        }
    }

    /// Prüft, ob eine Herabstufung ausdrücklich freigegeben wurde.
    ///
    /// Eine Freigabe verlangt einen Beleg auf eine echte Quellversion, eine
    /// Begründung und einen deterministischen Erzeuger. Ein Modell darf nie
    /// herabstufen.
    pub fn authorizes_declassification(&self, source: &VersionId, actor_type: ActorType) -> bool {
        self.declassified_from.as_ref() == Some(source)
            && self.declassification_reason.is_some()
            && actor_type == ActorType::System
    }
}

/// Fehler bei der Auflösung der wirksamen Schutzstufe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassificationError {
    /// Eine Herabstufung wurde ohne ausdrückliche Freigabe erklärt.
    UnauthorizedDowngrade {
        /// Vom Erzeuger deklarierte Stufe.
        declared: Classification,
        /// Aus den Quellen geerbte Stufe.
        inherited: Classification,
    },
    /// Der Herkunftsbeleg einer Herabstufung ist unvollständig.
    IncompleteDeclassification,
}

/// Quelle, aus der die wirksame Schutzstufe geerbt wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassificationSource<'a> {
    /// Version des Quellartefakts.
    pub version_id: &'a VersionId,
    /// Wirksame Stufe des Quellartefakts.
    pub classification: Classification,
}

/// Löst die wirksame Schutzstufe eines Artefakts auf.
///
/// Ohne Deklaration gilt die strengste geerbte Stufe. Eine Deklaration unter
/// der geerbten Stufe ist eine Herabstufung: Sie ist nur zulässig, wenn ein
/// Herkunftsbeleg auf eine tatsächliche Quelle zeigt, der Erzeuger
/// deterministisch ist und eine Begründung vorliegt. Alle übrigen Quellen
/// begrenzen die Stufe weiterhin.
pub fn resolve_classification(
    declared: Option<Classification>,
    sources: &[ClassificationSource<'_>],
    declassified_from: Option<&VersionId>,
    actor_type: ActorType,
    has_reason: bool,
) -> Result<Classification, ClassificationError> {
    // Ein Herkunftsbeleg ohne Deklaration ist widersprüchlich: Es gibt nichts,
    // wofür freigegeben werden könnte.
    if declassified_from.is_some() && declared.is_none() {
        return Err(ClassificationError::IncompleteDeclassification);
    }
    let mut ceiling = Classification::Public;
    let mut source_found = false;
    for source in sources {
        if declassified_from == Some(source.version_id) {
            let target = declared.unwrap_or(Classification::Public);
            if actor_type != ActorType::System
                || !has_reason
                || source.classification.rank() <= target.rank()
            {
                return Err(ClassificationError::IncompleteDeclassification);
            }
            source_found = true;
            continue;
        }
        ceiling = ceiling.strongest(source.classification);
    }
    match declared {
        None => Ok(ceiling.strongest(Classification::DEFAULT)),
        Some(declared) => {
            if source_found {
                if ceiling.rank() > declared.rank() {
                    return Err(ClassificationError::UnauthorizedDowngrade {
                        declared,
                        inherited: ceiling,
                    });
                }
                Ok(declared)
            } else if declared.rank() < ceiling.rank() {
                Err(ClassificationError::UnauthorizedDowngrade {
                    declared,
                    inherited: ceiling,
                })
            } else {
                Ok(declared)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_classifications_by_sensitivity() {
        assert!(Classification::Public < Classification::Internal);
        assert!(Classification::Internal < Classification::Confidential);
        assert!(Classification::Confidential < Classification::Restricted);
        assert!(Classification::Restricted < Classification::Secret);
        assert!(Classification::Secret.allows(Classification::Secret));
        assert!(!Classification::Secret.allows(Classification::Restricted));
    }

    #[test]
    fn keeps_the_stricter_classification() {
        assert_eq!(
            Classification::Public.strongest(Classification::Confidential),
            Classification::Confidential
        );
        assert_eq!(
            Classification::Restricted.strongest(Classification::Internal),
            Classification::Restricted
        );
    }

    #[test]
    fn defaults_to_internal_without_declaration() {
        assert_eq!(
            SecurityProfile::default().effective(),
            Classification::Internal
        );
    }

    #[test]
    fn prefers_the_resolved_classification_over_the_declaration() {
        let profile = SecurityProfile {
            declared: Some(Classification::Confidential),
            classification: Some(Classification::Restricted),
            ..Default::default()
        };
        assert_eq!(profile.effective(), Classification::Restricted);
    }

    #[test]
    fn inherits_the_strictest_source_without_declaration() {
        let confidential = VersionId("confidential-v1".into());
        let public = VersionId("public-v1".into());
        let sources = [
            ClassificationSource {
                version_id: &confidential,
                classification: Classification::Confidential,
            },
            ClassificationSource {
                version_id: &public,
                classification: Classification::Public,
            },
        ];
        let effective = resolve_classification(None, &sources, None, ActorType::Model, false)
            .expect("inheritance resolves");
        assert_eq!(effective, Classification::Confidential);
    }

    #[test]
    fn rejects_an_unauthorized_downgrade() {
        let confidential = VersionId("confidential-v1".into());
        let sources = [ClassificationSource {
            version_id: &confidential,
            classification: Classification::Confidential,
        }];
        let error = resolve_classification(
            Some(Classification::Public),
            &sources,
            None,
            ActorType::System,
            false,
        )
        .expect_err("downgrade needs evidence");
        assert_eq!(
            error,
            ClassificationError::UnauthorizedDowngrade {
                declared: Classification::Public,
                inherited: Classification::Confidential,
            }
        );
    }

    #[test]
    fn accepts_a_documented_declassification() {
        let confidential = VersionId("confidential-v1".into());
        let sources = [ClassificationSource {
            version_id: &confidential,
            classification: Classification::Confidential,
        }];
        let effective = resolve_classification(
            Some(Classification::Internal),
            &sources,
            Some(&confidential),
            ActorType::System,
            true,
        )
        .expect("declassification resolves");
        assert_eq!(effective, Classification::Internal);
    }

    #[test]
    fn rejects_a_declassification_by_a_model() {
        let confidential = VersionId("confidential-v1".into());
        let sources = [ClassificationSource {
            version_id: &confidential,
            classification: Classification::Confidential,
        }];
        let error = resolve_classification(
            Some(Classification::Internal),
            &sources,
            Some(&confidential),
            ActorType::Model,
            true,
        )
        .expect_err("models may not declassify");
        assert_eq!(error, ClassificationError::IncompleteDeclassification);
    }

    #[test]
    fn rejects_a_declassification_without_a_reason() {
        let confidential = VersionId("confidential-v1".into());
        let sources = [ClassificationSource {
            version_id: &confidential,
            classification: Classification::Confidential,
        }];
        let error = resolve_classification(
            Some(Classification::Internal),
            &sources,
            Some(&confidential),
            ActorType::System,
            false,
        )
        .expect_err("a reason is required");
        assert_eq!(error, ClassificationError::IncompleteDeclassification);
    }

    #[test]
    fn rejects_a_declassification_from_an_unknown_source() {
        let confidential = VersionId("confidential-v1".into());
        let unknown = VersionId("unknown-v1".into());
        let sources = [ClassificationSource {
            version_id: &confidential,
            classification: Classification::Confidential,
        }];
        let error = resolve_classification(
            Some(Classification::Internal),
            &sources,
            Some(&unknown),
            ActorType::System,
            true,
        )
        .expect_err("an unknown source grants nothing");
        assert_eq!(
            error,
            ClassificationError::UnauthorizedDowngrade {
                declared: Classification::Internal,
                inherited: Classification::Confidential,
            }
        );
    }

    #[test]
    fn keeps_other_sources_binding_after_a_declassification() {
        let confidential = VersionId("confidential-v1".into());
        let restricted = VersionId("restricted-v1".into());
        let sources = [
            ClassificationSource {
                version_id: &confidential,
                classification: Classification::Confidential,
            },
            ClassificationSource {
                version_id: &restricted,
                classification: Classification::Restricted,
            },
        ];
        let error = resolve_classification(
            Some(Classification::Internal),
            &sources,
            Some(&confidential),
            ActorType::System,
            true,
        )
        .expect_err("the other source still binds");
        assert_eq!(
            error,
            ClassificationError::UnauthorizedDowngrade {
                declared: Classification::Internal,
                inherited: Classification::Restricted,
            }
        );
    }

    #[test]
    fn rejects_incomplete_declassification_evidence() {
        let profile = SecurityProfile {
            declared: Some(Classification::Public),
            declassified_from: Some(VersionId("source-v1".into())),
            ..Default::default()
        };
        assert_eq!(
            profile.validate(),
            Err(SecurityProfileError::IncompleteDeclassification)
        );
    }

    #[test]
    fn rejects_a_declaration_without_a_declaration_to_downgrade() {
        let error = resolve_classification(
            None,
            &[],
            Some(&VersionId("source-v1".into())),
            ActorType::System,
            true,
        )
        .expect_err("evidence without a target is contradictory");
        assert_eq!(error, ClassificationError::IncompleteDeclassification);
    }

    #[test]
    fn models_may_never_declassify() {
        let profile = SecurityProfile::declassified(
            VersionId("source-v1".into()),
            Classification::Public,
            "operator approved",
        );
        assert!(
            profile.authorizes_declassification(&VersionId("source-v1".into()), ActorType::System)
        );
        assert!(
            !profile.authorizes_declassification(&VersionId("source-v1".into()), ActorType::Model)
        );
        assert!(
            !profile.authorizes_declassification(&VersionId("other-v1".into()), ActorType::System)
        );
    }
}
