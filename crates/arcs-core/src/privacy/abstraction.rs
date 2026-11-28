//! Abstraktion vor der Offenlegung: symbolische Aufgaben statt Werten.
//!
//! Ein vertraulicher Sachverhalt darf extern bearbeitet werden, wenn der
//! Betreiber die Verarbeitung freigibt und der Core den Sachverhalt lokal in
//! Variablen zerlegt. Der externe Reasoner sieht dann Struktur, Regeln und
//! Schnittstellen, aber keine Werte. Die konkrete Lösung wird lokal
//! rekonstruiert und erbt die Schutzstufe der Quelle.
//!
//! Die Abstraktion ist damit die einzige dokumentierte Form einer
//! Herabstufung: Sie braucht eine ausdrückliche Freigabe, benennt ihren
//! Zweck und ihren Empfänger und wird vor dem Versand auf Offenlegung
//! geprüft. Pseudonymisierung ist keine Anonymisierung: Über mehrere
//! Abfragen bleibt ein Rückschluss möglich. Das ist im Ergebnis vermerkt.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::adapters::CapabilityRef;
use crate::core::{Classification, VersionId};

/// Schema der abstrakten Aufgabe, die ein externer Reasoner sehen darf.
pub const ABSTRACT_PROBLEM_SCHEMA: &str = "arcs.abstract_problem.v1";
/// Schema der lokalen Bindungstabelle.
pub const ABSTRACT_BINDING_SCHEMA: &str = "arcs.abstract_binding.v1";
/// Herausgeber der Abstraktionsartefakte.
pub const PUBLISHER: &str = "arcs.privacy.abstraction";
/// Schlüssel der Markierung, die einen ganzen Wert ersetzt.
pub const VARIABLE_MARKER: &str = "$var";
/// Präfix einer Variablen in einem Text.
pub const VARIABLE_PREFIX: &str = "$v";
/// Mindestlänge eines Wertes, dessen Wiederholung als Offenlegung gilt.
///
/// Kürzere Werte tragen zuwenig Information, um eine Offenlegung zu belegen,
/// und würden zufällige Treffer in sichtbaren Feldern auslösen.
pub const MIN_DISCLOSURE_LENGTH: usize = 4;
/// Höchstzahl der Variablen einer Abstraktion.
pub const MAX_VARIABLES: usize = 64;
/// Höchstlänge eines Zwecktextes.
pub const MAX_PURPOSE_BYTES: usize = 512;

/// Art einer abstrahierten Variable.
///
/// Die Art wird vom Betreiber für das Feld festgelegt, nicht aus dem Wert
/// abgeleitet: Eine Ableitung würde selbst eine Offenlegung sein.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableKind {
    /// Freier Text, der in Texten eingesetzt werden darf.
    Text,
    /// Bezeichner, der in Texten eingesetzt werden darf.
    Identifier,
    /// Zahl ohne Einheit.
    Number,
    /// Zahl mit Einheit.
    Quantity,
    /// Wert ohne weitergehende Bedeutung; nur als Ganzes einsetzbar.
    Opaque,
}

impl VariableKind {
    /// Darf der Wert in einem Text eingesetzt werden?
    pub fn is_textual(self) -> bool {
        matches!(self, Self::Text | Self::Identifier)
    }

    /// Erwartet die Art einen Textwert?
    fn expects_text(self) -> bool {
        matches!(self, Self::Text | Self::Identifier)
    }

    /// Erwartet die Art einen Zahlenwert?
    fn expects_number(self) -> bool {
        matches!(self, Self::Number | Self::Quantity)
    }
}

/// Ein abstrahiertes Feld einer Nutzlast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbstractedField {
    /// Name des Payload-Feldes.
    pub field: String,
    /// Art der Werte in diesem Feld.
    pub kind: VariableKind,
}

/// Freigabe des Betreibers für eine Abstraktion.
///
/// Die Freigabe nennt Zweck, Zielstufe, Empfänger und die Felder, die als
/// Variablen erscheinen dürfen. Alles andere bleibt draußen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbstractionPolicy {
    /// Zweck der Verarbeitung.
    pub purpose: String,
    /// Schutzstufe, auf die herabgestuft wird.
    pub target_classification: Classification,
    /// Reasoner, dem die Abstraktion erlaubt ist.
    pub reasoner: CapabilityRef,
    /// Felder, die als Variablen erscheinen dürfen.
    pub abstract_fields: Vec<AbstractedField>,
    /// Felder, die wörtlich gesendet werden dürfen.
    pub visible_fields: Vec<String>,
    /// Höchstzahl der Variablen.
    pub max_variables: usize,
}

impl AbstractionPolicy {
    /// Prüft die strukturelle Gültigkeit der Freigabe.
    pub fn validate(&self) -> Result<(), AbstractionError> {
        if self.purpose.trim().is_empty()
            || self.purpose.len() > MAX_PURPOSE_BYTES
            || self.purpose.chars().any(char::is_control)
        {
            return Err(AbstractionError::InvalidPolicy(
                "purpose must be non-empty, control-free and at most 512 bytes".into(),
            ));
        }
        if self.max_variables == 0 || self.max_variables > MAX_VARIABLES {
            return Err(AbstractionError::InvalidPolicy(
                "max_variables must be within 1..=64".into(),
            ));
        }
        if self.abstract_fields.is_empty() && self.visible_fields.is_empty() {
            return Err(AbstractionError::InvalidPolicy(
                "an abstraction without any field shares nothing".into(),
            ));
        }
        let mut seen = HashSet::new();
        for field in &self.abstract_fields {
            if !valid_field_name(&field.field) {
                return Err(AbstractionError::InvalidPolicy(format!(
                    "invalid abstracted field '{}'",
                    field.field
                )));
            }
            if !seen.insert(field.field.clone()) {
                return Err(AbstractionError::InvalidPolicy(format!(
                    "field '{}' is listed twice",
                    field.field
                )));
            }
        }
        for field in &self.visible_fields {
            if !valid_field_name(field) {
                return Err(AbstractionError::InvalidPolicy(format!(
                    "invalid visible field '{field}'"
                )));
            }
            if !seen.insert(field.clone()) {
                return Err(AbstractionError::InvalidPolicy(format!(
                    "field '{field}' is listed twice"
                )));
            }
        }
        Ok(())
    }
}

/// Beschreibung einer Variablen ohne ihren Wert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableDescriptor {
    /// Name der Variablen, etwa `v1`.
    pub name: String,
    /// Art der Variable.
    pub kind: VariableKind,
    /// Feld, aus dem die Variable stammt.
    pub field: String,
}

/// Eine lokale Bindung: Platzhalter und Wert.
///
/// Die Bindung verlässt den Core nicht. Sie wird nur zur Rekonstruktion
/// gebraucht und bleibt in einem Artefakt der Schutzstufe der Quelle.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    /// Name der Variablen.
    pub name: String,
    /// Konkreter Wert.
    pub value: Value,
    /// Art der Variable.
    pub kind: VariableKind,
    /// Feld, aus dem die Variable stammt.
    pub field: String,
}

/// Lokale Bindungstabelle einer Abstraktion.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BindingTable {
    /// Die Bindungen in stabiler Reihenfolge.
    pub entries: Vec<Binding>,
}

impl BindingTable {
    /// Sucht eine Bindung nach Namen.
    pub fn get(&self, name: &str) -> Option<&Binding> {
        self.entries.iter().find(|entry| entry.name == name)
    }
}

/// Ergebnis einer Abstraktion.
#[derive(Debug, Clone, PartialEq)]
pub struct Abstraction {
    /// Die abstrakte Nutzlast ohne konkrete Werte.
    pub problem: Value,
    /// Die belegten Variablen ohne Werte.
    pub variables: Vec<VariableDescriptor>,
    /// Die lokale Bindungstabelle.
    pub bindings: BindingTable,
}

/// Fehler der Abstraktion.
#[derive(Debug)]
pub enum AbstractionError {
    /// Die Freigabe ist strukturell unzulässig.
    InvalidPolicy(String),
    /// Die Nutzlast ist kein Objekt.
    InvalidPayload(String),
    /// Ein Feld der Freigabe fehlt in der Nutzlast.
    MissingField { field: String },
    /// Der Wert eines Feldes passt nicht zur deklarierten Art.
    FieldKindMismatch { field: String },
    /// Es wurden mehr Variablen gebildet als erlaubt.
    TooManyVariables { maximum: usize },
    /// Die abstrakte Nutzlast enthält noch einen konkreten Wert.
    DisclosureDetected { field: String },
    /// Die Lösung verweist auf eine unbekannte Variable.
    UnknownVariable { name: String },
    /// Das abstrakte Problem ist für diesen Reasoner nicht freigegeben.
    NotAuthorizedReasoner {
        expected: CapabilityRef,
        actual: CapabilityRef,
    },
    /// Die Lösung enthält eine Variable, die nicht eingesetzt werden kann.
    UnresolvedVariable { name: String },
    /// Die Zielstufe liegt über der Grenze des Betreibers.
    TargetClassExceedsGrant {
        target: Classification,
        permitted: Classification,
    },
    /// Die Zielstufe liegt über der Stufe der Quelle.
    TargetAboveSource {
        target: Classification,
        source: Classification,
    },
    /// Der Grund darf die Herabstufung nicht tragen.
    UnauthorizedDeclassification(String),
    /// Die Quelle der Abstraktion ist unbekannt.
    MissingSource { version: VersionId },
    /// Die Bindungstabelle der Abstraktion ist unbekannt.
    MissingBinding { version: VersionId },
    /// Die rekonstruierte Nutzlast verletzt den Zielvertrag.
    InvalidResult(String),
    /// Fehler des Speichers.
    Store(crate::store::StoreError),
    /// Fehler der Serialisierung.
    Serialization(serde_json::Error),
}

impl std::fmt::Display for AbstractionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPolicy(message) => {
                write!(formatter, "invalid abstraction policy: {message}")
            }
            Self::InvalidPayload(message) => {
                write!(formatter, "invalid payload for abstraction: {message}")
            }
            Self::MissingField { field } => {
                write!(formatter, "field '{field}' is missing in the payload")
            }
            Self::FieldKindMismatch { field } => write!(
                formatter,
                "field '{field}' does not match its declared variable kind"
            ),
            Self::TooManyVariables { maximum } => {
                write!(formatter, "abstraction exceeds {maximum} variables")
            }
            Self::DisclosureDetected { field } => write!(
                formatter,
                "abstract problem still contains a concrete value of field '{field}'"
            ),
            Self::UnknownVariable { name } => {
                write!(formatter, "solution refers to unknown variable '{name}'")
            }
            Self::NotAuthorizedReasoner { expected, actual } => write!(
                formatter,
                "abstract problem is released to '{}' but not to '{}'",
                expected.capability_id.0, actual.capability_id.0
            ),
            Self::UnresolvedVariable { name } => {
                write!(formatter, "solution leaves variable '{name}' unresolved")
            }
            Self::TargetClassExceedsGrant { target, permitted } => write!(
                formatter,
                "target classification '{target:?}' exceeds the granted ceiling '{permitted:?}'"
            ),
            Self::TargetAboveSource { target, source } => write!(
                formatter,
                "target classification '{target:?}' is above the source classification '{source:?}'"
            ),
            Self::UnauthorizedDeclassification(message) => {
                write!(formatter, "declassification rejected: {message}")
            }
            Self::MissingSource { version } => {
                write!(formatter, "source '{}' is unknown", version.0)
            }
            Self::MissingBinding { version } => {
                write!(formatter, "binding '{}' is unknown", version.0)
            }
            Self::InvalidResult(message) => {
                write!(formatter, "reconstructed result is invalid: {message}")
            }
            Self::Store(error) => write!(formatter, "store rejected the abstraction: {error:?}"),
            Self::Serialization(error) => {
                write!(formatter, "abstraction could not be serialized: {error}")
            }
        }
    }
}

impl From<crate::store::StoreError> for AbstractionError {
    fn from(value: crate::store::StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<serde_json::Error> for AbstractionError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

/// Prüft einen Feldnamen der Freigabe.
fn valid_field_name(field: &str) -> bool {
    !field.trim().is_empty()
        && field.len() <= 256
        && !field.chars().any(char::is_control)
        && field.is_ascii()
}

/// Zerlegt eine Nutzlast in Variablen und sichtbare Felder.
///
/// Die Sichtbarkeit folgt der Freigabe: Ein Feld, das weder abstrahiert noch
/// sichtbar ist, wird nicht gesendet. Damit gilt auch hier Default-Deny.
pub fn abstract_payload(
    payload: &Value,
    policy: &AbstractionPolicy,
) -> Result<Abstraction, AbstractionError> {
    policy.validate()?;
    let object = payload
        .as_object()
        .ok_or_else(|| AbstractionError::InvalidPayload("the payload must be an object".into()))?;
    let mut problem = Map::new();
    for field in &policy.visible_fields {
        let value = object
            .get(field)
            .ok_or_else(|| AbstractionError::MissingField {
                field: field.clone(),
            })?;
        problem.insert(field.clone(), value.clone());
    }
    let mut variables = Vec::new();
    let mut bindings = Vec::new();
    for field in &policy.abstract_fields {
        let value = object
            .get(&field.field)
            .ok_or_else(|| AbstractionError::MissingField {
                field: field.field.clone(),
            })?;
        let abstracted = abstract_leaves(
            value,
            &field.field,
            &field.kind,
            policy.max_variables,
            &mut variables,
            &mut bindings,
        )?;
        problem.insert(field.field.clone(), abstracted);
    }
    let abstraction = Abstraction {
        problem: Value::Object(problem),
        variables,
        bindings: BindingTable { entries: bindings },
    };
    verify_no_disclosure(&abstraction)?;
    Ok(abstraction)
}

/// Ersetzt alle skalaren Blätter eines Feldes durch Variablen.
fn abstract_leaves(
    value: &Value,
    field: &str,
    kind: &VariableKind,
    maximum: usize,
    variables: &mut Vec<VariableDescriptor>,
    bindings: &mut Vec<Binding>,
) -> Result<Value, AbstractionError> {
    match value {
        Value::Object(fields) => {
            let mut result = Map::new();
            for (key, item) in fields {
                result.insert(
                    key.clone(),
                    abstract_leaves(item, field, kind, maximum, variables, bindings)?,
                );
            }
            Ok(Value::Object(result))
        }
        Value::Array(items) => {
            let mut result = Vec::with_capacity(items.len());
            for item in items {
                result.push(abstract_leaves(
                    item, field, kind, maximum, variables, bindings,
                )?);
            }
            Ok(Value::Array(result))
        }
        Value::Null => Ok(Value::Null),
        Value::String(text) => {
            if !kind.expects_text() && *kind != VariableKind::Opaque {
                return Err(AbstractionError::FieldKindMismatch {
                    field: field.to_string(),
                });
            }
            Ok(bind_value(
                Value::String(text.clone()),
                *kind,
                field,
                maximum,
                variables,
                bindings,
            )?)
        }
        Value::Number(number) => {
            if !kind.expects_number() && *kind != VariableKind::Opaque {
                return Err(AbstractionError::FieldKindMismatch {
                    field: field.to_string(),
                });
            }
            Ok(bind_value(
                Value::Number(number.clone()),
                *kind,
                field,
                maximum,
                variables,
                bindings,
            )?)
        }
        Value::Bool(flag) => {
            if *kind != VariableKind::Opaque {
                return Err(AbstractionError::FieldKindMismatch {
                    field: field.to_string(),
                });
            }
            Ok(bind_value(
                Value::Bool(*flag),
                *kind,
                field,
                maximum,
                variables,
                bindings,
            )?)
        }
    }
}

/// Bindet einen Wert an eine neue Variable.
fn bind_value(
    value: Value,
    kind: VariableKind,
    field: &str,
    maximum: usize,
    variables: &mut Vec<VariableDescriptor>,
    bindings: &mut Vec<Binding>,
) -> Result<Value, AbstractionError> {
    if variables.len() >= maximum {
        return Err(AbstractionError::TooManyVariables { maximum });
    }
    let name = format!("v{}", variables.len() + 1);
    variables.push(VariableDescriptor {
        name: name.clone(),
        kind,
        field: field.to_string(),
    });
    bindings.push(Binding {
        name: name.clone(),
        value,
        kind,
        field: field.to_string(),
    });
    Ok(serde_json::json!({ VARIABLE_MARKER: name }))
}

/// Prüft, dass die abstrakte Nutzlast keinen konkreten Wert enthält.
///
/// Die Prüfung ist der Nachweis der Transformation: Sie belegt, dass der
/// herabgestufte Inhalt keine Werte der Quelle mehr trägt. Findet sie einen
/// Wert, etwa weil ein sichtbares Feld ihn wiederholt, wird die Abstraktion
/// verworfen.
pub fn verify_no_disclosure(abstraction: &Abstraction) -> Result<(), AbstractionError> {
    let mut leaves = Vec::new();
    collect_leaves(&abstraction.problem, &mut leaves);
    for binding in &abstraction.bindings.entries {
        let candidate = match &binding.value {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            _ => continue,
        };
        if candidate.len() < MIN_DISCLOSURE_LENGTH {
            continue;
        }
        for leaf in &leaves {
            let repeated = match leaf {
                Leaf::Text(text) => text.contains(&candidate),
                Leaf::Number(text) => text == &candidate,
            };
            if repeated {
                return Err(AbstractionError::DisclosureDetected {
                    field: binding.field.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Ein skalares Blatt einer Nutzlast.
enum Leaf {
    Text(String),
    Number(String),
}

/// Sammelt die skalaren Blätter einer Nutzlast.
fn collect_leaves(value: &Value, leaves: &mut Vec<Leaf>) {
    match value {
        Value::Object(fields) => {
            for item in fields.values() {
                collect_leaves(item, leaves);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_leaves(item, leaves);
            }
        }
        Value::String(text) => leaves.push(Leaf::Text(text.clone())),
        Value::Number(number) => leaves.push(Leaf::Number(number.to_string())),
        _ => {}
    }
}

/// Setzt eine abstrakte Lösung lokal in konkrete Werte um.
///
/// Die Lösung darf nur gebundene Variablen verwenden. Ein unbekannter oder
/// nicht einsetzbarer Platzhalter bricht die Rekonstruktion ab: Eine
/// unvollständige Lösung wird nicht geraten, sondern lokal nachgerechnet.
pub fn reconstruct(solution: &Value, bindings: &BindingTable) -> Result<Value, AbstractionError> {
    let result = substitute(solution, bindings)?;
    ensure_resolved(&result)?;
    Ok(result)
}

/// Ersetzt Platzhalter in einer Lösung.
fn substitute(value: &Value, bindings: &BindingTable) -> Result<Value, AbstractionError> {
    match value {
        Value::Object(fields) => {
            if fields.len() == 1
                && let Some(Value::String(name)) = fields.get(VARIABLE_MARKER)
            {
                let binding = bindings
                    .get(name)
                    .ok_or_else(|| AbstractionError::UnknownVariable { name: name.clone() })?;
                return Ok(binding.value.clone());
            }
            let mut result = Map::new();
            for (key, item) in fields {
                result.insert(key.clone(), substitute(item, bindings)?);
            }
            Ok(Value::Object(result))
        }
        Value::Array(items) => {
            let mut result = Vec::with_capacity(items.len());
            for item in items {
                result.push(substitute(item, bindings)?);
            }
            Ok(Value::Array(result))
        }
        Value::String(text) => substitute_text(text, bindings),
        _ => Ok(value.clone()),
    }
}

/// Setzt Platzhalter in einem Text ein.
fn substitute_text(text: &str, bindings: &BindingTable) -> Result<Value, AbstractionError> {
    if !text.contains(VARIABLE_PREFIX) {
        return Ok(Value::String(text.to_string()));
    }
    let mut result = String::new();
    let mut rest = text;
    while let Some(position) = rest.find(VARIABLE_PREFIX) {
        result.push_str(&rest[..position]);
        let tail = &rest[position + VARIABLE_PREFIX.len()..];
        let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            // Keine Variable: Der Text bleibt unverändert.
            result.push_str(VARIABLE_PREFIX);
            rest = tail;
            continue;
        }
        let name = format!("v{digits}");
        let binding = bindings
            .get(&name)
            .ok_or_else(|| AbstractionError::UnknownVariable { name: name.clone() })?;
        if !binding.kind.is_textual() {
            return Err(AbstractionError::UnresolvedVariable { name });
        }
        let Value::String(value) = &binding.value else {
            return Err(AbstractionError::UnresolvedVariable { name });
        };
        result.push_str(value);
        rest = &tail[digits.len()..];
    }
    result.push_str(rest);
    Ok(Value::String(result))
}

/// Prüft, dass in der rekonstruierten Lösung kein Platzhalter blieb.
fn ensure_resolved(value: &Value) -> Result<(), AbstractionError> {
    match value {
        Value::Object(fields) => {
            if fields.contains_key(VARIABLE_MARKER) {
                return Err(AbstractionError::UnresolvedVariable {
                    name: fields
                        .get(VARIABLE_MARKER)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                });
            }
            for item in fields.values() {
                ensure_resolved(item)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                ensure_resolved(item)?;
            }
            Ok(())
        }
        Value::String(text) => {
            if let Some(name) = find_variable(text) {
                return Err(AbstractionError::UnresolvedVariable { name });
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Findet den ersten Platzhalter in einem Text.
fn find_variable(text: &str) -> Option<String> {
    let mut search = text;
    while let Some(position) = search.find(VARIABLE_PREFIX) {
        let tail = &search[position + VARIABLE_PREFIX.len()..];
        let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            search = tail;
            continue;
        }
        return Some(format!("v{digits}"));
    }
    None
}

/// Liest den freigegebenen Reasoner eines abstrakten Problems.
pub fn authorized_reasoner(payload: &Value) -> Option<CapabilityRef> {
    serde_json::from_value(payload.get("reasoner")?.clone()).ok()
}

/// Prüft, ob ein abstraktes Problem für einen Reasoner freigegeben ist.
///
/// Die Prüfung gehört an die Offenlegungsgrenze: Eine Abstraktion darf nur
/// den Reasoner sehen, dem der Betreiber sie zugedacht hat.
pub fn ensure_authorized_reasoner(
    payload: &Value,
    capability: &CapabilityRef,
) -> Result<(), AbstractionError> {
    match authorized_reasoner(payload) {
        Some(expected) if &expected == capability => Ok(()),
        Some(expected) => Err(AbstractionError::NotAuthorizedReasoner {
            expected,
            actual: capability.clone(),
        }),
        None => Err(AbstractionError::NotAuthorizedReasoner {
            expected: CapabilityRef::new("", ""),
            actual: capability.clone(),
        }),
    }
}
