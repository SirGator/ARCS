use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::core::artifact::SchemaId;

use super::{SchemaDefinition, SchemaViolation};

// Die Verträge werden in die Binary eingebettet. Ihre Verfügbarkeit hängt
// damit zur Laufzeit nicht von einem veränderbaren Arbeitsverzeichnis ab.
const BUNDLED_SCHEMAS: &[&str] = &[
    include_str!("../../../schemas/v1/input.schema.json"),

];

/// Fehler beim Einlesen eines Schemas in die Registry.
#[derive(Debug)]
pub enum RegistryError {
    /// Das Schema-Dokument enthält kein gültiges JSON.
    InvalidJson(serde_json::Error),
    /// Dem Dokument fehlt die als Identität verwendete `$id`.
    MissingId,
    /// Die `$id` folgt nicht dem ARCS-Schema `arcs.<typ>[.<variante>].v<n>`.
    InvalidId(String),
    /// Ein Vertrag mit derselben `$id` ist bereits registriert.
    DuplicateId(String),
}

/// Kontrollierte Sammlung aller bekannten Payload-Verträge.
///
/// Unbekannte Schemas werden nicht akzeptiert. Das verhindert, dass ungeprüfte
/// Nutzdaten unter einer erfundenen Schema-ID in den Core gelangen.
pub struct SchemaRegistry {
    schemas: HashMap<SchemaId, SchemaDefinition>,
}

impl SchemaRegistry {
    /// Erzeugt eine leere Registry.
    pub fn new() -> Self {
        Self {
            schemas: HashMap::new(),
        }
    }

    /// Lädt alle mit ARCS ausgelieferten V1-Verträge.
    pub fn with_bundled_schemas() -> Result<Self, RegistryError> {
        let mut registry = Self::new();
        for source in BUNDLED_SCHEMAS {
            registry.register_json(source)?;
        }
        Ok(registry)
    }

    /// Registriert ein JSON-Schema unter seiner eigenen `$id`.
    pub fn register_json(&mut self, source: &str) -> Result<(), RegistryError> {
        let document: Value = serde_json::from_str(source).map_err(RegistryError::InvalidJson)?;
        let id = document
            .get("$id")
            .and_then(Value::as_str)
            .ok_or(RegistryError::MissingId)?;
        let schema_id = SchemaId(id.to_owned());
        if self.schemas.contains_key(&schema_id) {
            return Err(RegistryError::DuplicateId(id.to_owned()));
        }
        let (artifact_type, version) = parse_schema_id(id)?;
        self.schemas.insert(
            schema_id.clone(),
            SchemaDefinition {
                id: schema_id,
                artifact_type,
                version,
                document,
            },
        );
        Ok(())
    }

    /// Liefert einen registrierten Vertrag anhand seiner ID.
    pub fn get(&self, id: &SchemaId) -> Option<&SchemaDefinition> {
        self.schemas.get(id)
    }

    /// Prüft einen JSON-Wert gegen den bezeichneten Vertrag.
    ///
    /// Alle Verstöße werden gesammelt. Auch eine unbekannte Schema-ID ist ein
    /// Verstoß, sodass der Aufrufer stets fail-closed reagieren kann.
    pub fn validate(&self, id: &SchemaId, instance: &Value) -> Result<(), Vec<SchemaViolation>> {
        let Some(schema) = self.get(id) else {
            return Err(vec![SchemaViolation::new(
                "$",
                format!("schema '{}' is not registered", id.0),
            )]);
        };
        let mut violations = Vec::new();
        validate_value(&schema.document, instance, "$", &mut violations);
        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }
}

/// Zerlegt eine ARCS-Schema-ID in fachlichen Typ und Version.
fn parse_schema_id(id: &str) -> Result<(String, u64), RegistryError> {
    let body = id
        .strip_prefix("arcs.")
        .ok_or_else(|| RegistryError::InvalidId(id.to_owned()))?;
    let (name, version) = body
        .rsplit_once(".v")
        .ok_or_else(|| RegistryError::InvalidId(id.to_owned()))?;
    let artifact_type = name
        .split('.')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| RegistryError::InvalidId(id.to_owned()))?;
    let version = version
        .parse::<u64>()
        .ok()
        .filter(|value| *value >= 1)
        .ok_or_else(|| RegistryError::InvalidId(id.to_owned()))?;
    Ok((artifact_type.to_owned(), version))
}

impl Default for SchemaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_value(
    schema: &Value,
    value: &Value,
    path: &str,
    violations: &mut Vec<SchemaViolation>,
) {
    // Bei einem falschen Grundtyp sind nachfolgende Objekt- oder Arrayregeln
    // nicht sinnvoll anwendbar; deshalb endet dieser Zweig sofort.
    if let Some(expected) = schema.get("type").and_then(Value::as_str) {
        let matches = match expected {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => true,
        };
        if !matches {
            violations.push(SchemaViolation::new(path, format!("expected {expected}")));
            return;
        }
    }

    // Strukturelle JSON-Gleichheit unterstützt Enums aller Werttypen.
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
        && !allowed.contains(value)
    {
        violations.push(SchemaViolation::new(path, "value is not in enum"));
    }

    if let Some(expected) = schema.get("const")
        && expected != value
    {
        violations.push(SchemaViolation::new(
            path,
            format!("must equal {expected}"),
        ));
    }

    if let Some(text) = value.as_str()
        && let Some(minimum) = schema.get("minLength").and_then(Value::as_u64)
        && text.chars().count() < minimum as usize
    {
        violations.push(SchemaViolation::new(
            path,
            format!("must contain at least {minimum} character(s)"),
        ));
    }

    if let Some(number) = value.as_f64() {
        if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64)
            && number < minimum
        {
            violations.push(SchemaViolation::new(
                path,
                format!("must be at least {minimum}"),
            ));
        }
        if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64)
            && number > maximum
        {
            violations.push(SchemaViolation::new(
                path,
                format!("must be at most {maximum}"),
            ));
        }
    }

    if schema.get("format").and_then(Value::as_str) == Some("date-time")
        && let Some(text) = value.as_str()
        && !is_rfc3339(text)
    {
        violations.push(SchemaViolation::new(path, "must be a valid RFC 3339 date-time"));
    }

    if let Some(object) = value.as_object() {
        validate_object(schema, object, path, violations);
    }

    if let Some(items) = value.as_array()
        && let Some(item_schema) = schema.get("items")
    {
        for (index, item) in items.iter().enumerate() {
            validate_value(item_schema, item, &format!("{path}[{index}]"), violations);
        }
    }

    if let Some(items) = value.as_array()
        && let Some(minimum) = schema.get("minItems").and_then(Value::as_u64)
        && items.len() < minimum as usize
    {
        violations.push(SchemaViolation::new(
            path,
            format!("must contain at least {minimum} item(s)"),
        ));
    }
}

/// Strikte, abhängigkeitfreie Prüfung des von den Verträgen verwendeten
/// RFC-3339-Profils.
fn is_rfc3339(value: &str) -> bool {
    let Some((date, time_and_offset)) = value.split_once('T') else {
        return false;
    };
    let mut date_parts = date.split('-');
    let (Some(year), Some(month), Some(day), None) = (
        date_parts.next(),
        date_parts.next(),
        date_parts.next(),
        date_parts.next(),
    ) else {
        return false;
    };
    let (Ok(year), Ok(month), Ok(day)) = (
        year.parse::<i32>(),
        month.parse::<u32>(),
        day.parse::<u32>(),
    ) else {
        return false;
    };
    if year < 0 || !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return false;
    }

    let (time, offset) = if let Some(time) = time_and_offset.strip_suffix('Z') {
        (time, "Z")
    } else if let Some(index) = time_and_offset
        .char_indices()
        .skip(1)
        .find_map(|(index, character)| matches!(character, '+' | '-').then_some(index))
    {
        (&time_and_offset[..index], &time_and_offset[index..])
    } else {
        return false;
    };

    let mut time_parts = time.split(':');
    let (Some(hour), Some(minute), Some(second), None) = (
        time_parts.next(),
        time_parts.next(),
        time_parts.next(),
        time_parts.next(),
    ) else {
        return false;
    };
    let second = second.split('.').next().unwrap_or(second);
    let (Ok(hour), Ok(minute), Ok(second)) = (
        hour.parse::<u32>(),
        minute.parse::<u32>(),
        second.parse::<u32>(),
    ) else {
        return false;
    };
    if hour > 23 || minute > 59 || second > 59 {
        return false;
    }

    if offset == "Z" {
        return true;
    }
    let offset = &offset[1..];
    let Some((hour, minute)) = offset.split_once(':') else {
        return false;
    };
    matches!(
        (hour.parse::<u32>(), minute.parse::<u32>()),
        (Ok(0..=23), Ok(0..=59))
    )
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => 0,
    }
}

fn validate_object(
    schema: &Value,
    object: &Map<String, Value>,
    path: &str,
    violations: &mut Vec<SchemaViolation>,
) {
    // Jedes fehlende Pflichtfeld erhält seinen exakten erwarteten JSON-Pfad.
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(name) {
                violations.push(SchemaViolation::new(
                    format!("{path}.{name}"),
                    "required property is missing",
                ));
            }
        }
    }

    let properties = schema.get("properties").and_then(Value::as_object);
    if let Some(properties) = properties {
        for (name, value) in object {
            if let Some(property_schema) = properties.get(name) {
                validate_value(
                    property_schema,
                    value,
                    &format!("{path}.{name}"),
                    violations,
                );
            } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                // Geschlossene Verträge blockieren Tippfehler und ungeprüfte
                // zusätzliche Daten.
                violations.push(SchemaViolation::new(
                    format!("{path}.{name}"),
                    "additional property is not allowed",
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    // Ein unvollständiger Payload muss blockiert werden.
    fn bundled_schema_rejects_missing_required_payload_field() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let result = registry.validate(
            &SchemaId("arcs.task.v1".into()),
            &json!({"description": "missing title"}),
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|violation| violation.path == "$.title")
        );
    }
}
