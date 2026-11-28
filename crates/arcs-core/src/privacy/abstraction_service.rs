//! Persistenz und Rekonstruktion der Abstraktion.
//!
//! Die Abstraktion erzeugt zwei Artefakte: ein abstraktes Problem, das an
//! einen externen Reasoner gehen darf, und eine lokale Bindungstabelle, die
//! im Core bleibt. Das abstrakte Problem ist eine ausdrücklich freigegebene
//! Herabstufung; die Bindungstabelle erbt die Schutzstufe der Quelle.
//!
//! Die Rekonstruktion ersetzt die Platzhalter lokal. Das Ergebnis erbt die
//! Schutzstufe der Quelle und wird gegen den Zielvertrag geprüft, bevor es
//! gespeichert wird. Eine unvollständige Lösung wird nicht geraten: Sie
//! bricht ab, damit die Verarbeitung lokal fortgesetzt werden kann.

use serde_json::{Value, json};

use super::abstraction::{
    ABSTRACT_BINDING_SCHEMA, ABSTRACT_PROBLEM_SCHEMA, AbstractionError, AbstractionPolicy,
    BindingTable, VariableDescriptor, abstract_payload, reconstruct,
};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactIdGenerator, Classification, Clock, Provenance,
    SchemaDefinition, SchemaId, SchemaRegistry, SecurityProfile, Source, SourceClass, SourceKind,
    Trust, TrustLevel, VersionId,
};
use crate::store::SqliteArtifactStore;

/// Ergebnis einer vorbereiteten Abstraktion.
#[derive(Debug, Clone)]
pub struct PreparedAbstraction {
    /// Version des abstrakten Problems.
    pub problem_version: VersionId,
    /// Version der lokalen Bindungstabelle.
    pub binding_version: VersionId,
    /// Version der Quelle, aus der abstrahiert wurde.
    pub source_version: VersionId,
    /// Die Nutzlast, die an den externen Reasoner geht.
    pub problem: Value,
    /// Die belegten Variablen ohne Werte.
    pub variables: Vec<VariableDescriptor>,
}

/// Core-Dienst für Abstraktion, Freigabe und lokale Rekonstruktion.
pub struct AbstractionService<'a> {
    store: &'a SqliteArtifactStore,
    schemas: &'a SchemaRegistry,
    ids: &'a mut dyn ArtifactIdGenerator,
    clock: &'a dyn Clock,
}

impl<'a> AbstractionService<'a> {
    /// Erzeugt den Dienst über den bestehenden Store und die Schema-Registry.
    pub fn new(
        store: &'a SqliteArtifactStore,
        schemas: &'a SchemaRegistry,
        ids: &'a mut dyn ArtifactIdGenerator,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            store,
            schemas,
            ids,
            clock,
        }
    }

    /// Bereitet eine Abstraktion vor.
    ///
    /// Der Dienst prüft die Freigabe, zerlegt die Quelle lokal, weist eine
    /// verbliebene Offenlegung zurück und speichert Problem und Bindungen.
    /// `ceiling` ist die Schutzstufe, die der Empfänger erreichen darf.
    pub fn prepare(
        &mut self,
        source: &VersionId,
        policy: &AbstractionPolicy,
        ceiling: Classification,
    ) -> Result<PreparedAbstraction, AbstractionError> {
        policy.validate()?;
        let artifact = self
            .store
            .get(source)?
            .ok_or_else(|| AbstractionError::MissingSource {
                version: source.clone(),
            })?;
        let source_class = artifact.security.effective();
        if policy.target_classification.rank() > source_class.rank() {
            return Err(AbstractionError::TargetAboveSource {
                target: policy.target_classification,
                source: source_class,
            });
        }
        if !policy.target_classification.allows(ceiling) {
            return Err(AbstractionError::TargetClassExceedsGrant {
                target: policy.target_classification,
                permitted: ceiling,
            });
        }
        let abstraction = abstract_payload(&artifact.payload, policy)?;

        let binding =
            self.build_binding_artifact(source, policy, &abstraction.bindings, source_class)?;
        let binding = self.store.append(&binding, self.schemas)?;
        let problem = self.build_problem_artifact(
            source,
            policy,
            &abstraction.problem,
            &abstraction.variables,
            source_class,
        )?;
        let problem = self.store.append(&problem, self.schemas)?;
        Ok(PreparedAbstraction {
            problem_version: problem.version_id,
            binding_version: binding.version_id,
            source_version: source.clone(),
            problem: abstraction.problem,
            variables: abstraction.variables,
        })
    }

    /// Rekonstruiert eine abstrakte Lösung lokal.
    ///
    /// Das Ergebnis wird gegen den Zielvertrag geprüft und als Artefakt mit
    /// der Schutzstufe der Quelle gespeichert.
    pub fn reconstruct(
        &mut self,
        prepared: &PreparedAbstraction,
        solution: &Value,
        target_schema_id: &SchemaId,
    ) -> Result<Artifact, AbstractionError> {
        let binding = self.store.get(&prepared.binding_version)?.ok_or_else(|| {
            AbstractionError::MissingBinding {
                version: prepared.binding_version.clone(),
            }
        })?;
        let table = binding_table(&binding)?;
        let payload = reconstruct(solution, &table)?;
        let definition = self
            .schemas
            .get(target_schema_id)
            .ok_or_else(|| {
                AbstractionError::InvalidResult(format!(
                    "unknown target schema '{}'",
                    target_schema_id.0
                ))
            })?
            .clone();
        self.schemas
            .validate(target_schema_id, &payload)
            .map_err(|violations| {
                AbstractionError::InvalidResult(
                    violations
                        .iter()
                        .map(|violation| violation.path.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                )
            })?;
        let source = self.store.get(&prepared.source_version)?.ok_or_else(|| {
            AbstractionError::MissingSource {
                version: prepared.source_version.clone(),
            }
        })?;
        let artifact = self.build_result_artifact(
            &definition,
            payload,
            &prepared.source_version,
            &prepared.problem_version,
            &source.security,
        );
        Ok(self.store.append(&artifact, self.schemas)?)
    }

    /// Baut das Artefakt der lokalen Bindungstabelle.
    fn build_binding_artifact(
        &mut self,
        source: &VersionId,
        policy: &AbstractionPolicy,
        bindings: &BindingTable,
        source_class: Classification,
    ) -> Result<Artifact, AbstractionError> {
        let definition = self.definition(ABSTRACT_BINDING_SCHEMA)?;
        let generated = self.ids.next(&definition.artifact_type);
        let variables = bindings
            .entries
            .iter()
            .map(|entry| {
                json!({
                    "name": entry.name,
                    "kind": entry.kind,
                    "field": entry.field,
                    "value": entry.value,
                })
            })
            .collect::<Vec<_>>();
        Ok(Artifact {
            artifact_id: generated.artifact_id,
            version_id: generated.version_id,
            version: 1,
            artifact_type: definition.artifact_type,
            schema_id: definition.id,
            schema_version: definition.version,
            created_at: self.clock.now_rfc3339(),
            created_by: Actor {
                actor_type: ActorType::System,
                id: super::abstraction::PUBLISHER.into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: format!("abstraction-binding:{}", source.0),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: format!("abstraction-binding:{}", source.0),
            subject: None,
            tags: vec![format!("reasoner:{}", policy.reasoner.capability_id.0)],
            payload: json!({
                "purpose": policy.purpose,
                "reasoner": policy.reasoner,
                "source": source.0,
                "variables": variables,
            }),
            provenance: Some(Provenance {
                parents: vec![source.0.clone()],
                rules_applied: vec!["privacy.binding_local".into()],
                models_used: vec![],
                transform: Some("privacy.bind".into()),
            }),
            // Die Bindung trägt die Werte und erbt deshalb die Stufe der
            // Quelle. Sie ist nie für einen externen Empfänger bestimmt.
            security: SecurityProfile {
                declared: Some(source_class),
                classification: None,
                owner: None,
                purpose: Some(policy.purpose.clone()),
                declassified_from: None,
                declassification_reason: None,
            },
        })
    }

    /// Baut das Artefakt des abstrakten Problems.
    fn build_problem_artifact(
        &mut self,
        source: &VersionId,
        policy: &AbstractionPolicy,
        problem: &Value,
        variables: &[VariableDescriptor],
        source_class: Classification,
    ) -> Result<Artifact, AbstractionError> {
        let definition = self.definition(ABSTRACT_PROBLEM_SCHEMA)?;
        let generated = self.ids.next(&definition.artifact_type);
        let declared = policy.target_classification;
        // Die Herabstufung ist nur gültig, wenn sie belegt ist: Sie braucht
        // die Quelle, eine Begründung und einen deterministischen Erzeuger.
        let security = if declared.rank() < source_class.rank() {
            SecurityProfile::declassified(
                source.clone(),
                declared,
                format!(
                    "authorized abstraction for {}",
                    policy.reasoner.capability_id.0
                ),
            )
        } else {
            SecurityProfile::declared(declared)
        };
        let security = SecurityProfile {
            purpose: Some(policy.purpose.clone()),
            owner: None,
            ..security
        };
        Ok(Artifact {
            artifact_id: generated.artifact_id,
            version_id: generated.version_id,
            version: 1,
            artifact_type: definition.artifact_type,
            schema_id: definition.id,
            schema_version: definition.version,
            created_at: self.clock.now_rfc3339(),
            created_by: Actor {
                actor_type: ActorType::System,
                id: super::abstraction::PUBLISHER.into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: format!("abstract-problem:{}", source.0),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: format!("abstract-problem:{}", source.0),
            subject: None,
            tags: vec![
                format!("reasoner:{}", policy.reasoner.capability_id.0),
                format!("adapter:{}", policy.reasoner.adapter_id.0),
            ],
            payload: json!({
                "purpose": policy.purpose,
                "reasoner": policy.reasoner,
                "problem": problem,
                "variables": variables,
                "residual_risk": "pseudonymization_is_not_anonymization",
            }),
            provenance: Some(Provenance {
                parents: vec![source.0.clone()],
                rules_applied: vec![
                    "privacy.abstracted".into(),
                    "privacy.disclosure_checked".into(),
                ],
                models_used: vec![],
                transform: Some("privacy.abstract".into()),
            }),
            security,
        })
    }

    /// Baut das Artefakt des rekonstruierten Ergebnisses.
    fn build_result_artifact(
        &mut self,
        definition: &SchemaDefinition,
        payload: Value,
        source: &VersionId,
        problem: &VersionId,
        source_security: &SecurityProfile,
    ) -> Artifact {
        let generated = self.ids.next(&definition.artifact_type);
        Artifact {
            artifact_id: generated.artifact_id,
            version_id: generated.version_id,
            version: 1,
            artifact_type: definition.artifact_type.clone(),
            schema_id: definition.id.clone(),
            schema_version: definition.version,
            created_at: self.clock.now_rfc3339(),
            created_by: Actor {
                actor_type: ActorType::System,
                id: super::abstraction::PUBLISHER.into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: format!("abstract-result:{}", source.0),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: format!("abstract-result:{}", source.0),
            subject: None,
            tags: vec!["abstract:reconstructed".into()],
            payload,
            provenance: Some(Provenance {
                // Das Ergebnis erbt von der Quelle und vom abstrakten Problem.
                // Damit bleibt die konkrete Lösung so sensibel wie ihr Anlass.
                parents: vec![source.0.clone(), problem.0.clone()],
                rules_applied: vec!["privacy.reconstructed_locally".into()],
                models_used: vec![],
                transform: Some("privacy.reconstruct".into()),
            }),
            security: SecurityProfile {
                declared: None,
                classification: None,
                owner: source_security.owner.clone(),
                purpose: source_security.purpose.clone(),
                declassified_from: None,
                declassification_reason: None,
            },
        }
    }

    /// Lädt eine Schemadefinition.
    fn definition(&self, schema_id: &str) -> Result<SchemaDefinition, AbstractionError> {
        self.schemas
            .get(&SchemaId(schema_id.into()))
            .cloned()
            .ok_or_else(|| AbstractionError::InvalidResult(format!("unknown schema '{schema_id}'")))
    }
}

/// Liest die Bindungstabelle aus einem gespeicherten Artefakt.
fn binding_table(artifact: &Artifact) -> Result<BindingTable, AbstractionError> {
    let variables = artifact
        .payload
        .get("variables")
        .and_then(Value::as_array)
        .ok_or_else(|| AbstractionError::InvalidResult("binding table is malformed".into()))?;
    let mut entries = Vec::with_capacity(variables.len());
    for variable in variables {
        let name = variable
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| AbstractionError::InvalidResult("binding name is missing".into()))?;
        let kind = variable
            .get("kind")
            .and_then(Value::as_str)
            .and_then(|kind| serde_json::from_value(json!(kind)).ok())
            .ok_or_else(|| AbstractionError::InvalidResult("binding kind is missing".into()))?;
        let field = variable
            .get("field")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let value = variable.get("value").cloned().unwrap_or(Value::Null);
        entries.push(super::abstraction::Binding {
            name: name.to_string(),
            value,
            kind,
            field,
        });
    }
    Ok(BindingTable { entries })
}
