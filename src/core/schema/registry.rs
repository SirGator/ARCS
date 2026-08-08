use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use crate::core::artifact::{MAX_ARTIFACT_TYPE_BYTES, SchemaId};

use super::{SchemaDefinition, SchemaViolation};

// Die Verträge werden in die Binary eingebettet. Ihre Verfügbarkeit hängt
// damit zur Laufzeit nicht von einem veränderbaren Arbeitsverzeichnis ab.
const BUNDLED_SCHEMAS: &[&str] = &[
    include_str!("../../../schemas/v1/approval.schema.json"),
    include_str!("../../../schemas/v1/input.schema.json"),
    include_str!("../../../schemas/v1/invocation.schema.json"),
    include_str!("../../../schemas/v1/reasoning_request.schema.json"),
    include_str!("../../../schemas/v1/reasoning_result.schema.json"),
    include_str!("../../../schemas/v1/verification_report.schema.json"),
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
    /// Das Schema verwendet ein Schlüsselwort, das der lokale Validator nicht unterstützt.
    UnsupportedKeyword { path: String, keyword: String },
    /// Das Schema selbst verletzt den bewusst kleinen, unterstützten Vertragsumfang.
    InvalidSchema { path: String, message: String },
}

/// Kontrollierte Sammlung aller bekannten Payload-Verträge.
///
/// Unbekannte Schemas werden nicht akzeptiert. Das verhindert, dass ungeprüfte
/// Nutzdaten unter einer erfundenen Schema-ID in den Core gelangen.
#[derive(Clone)]
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
        validate_schema_document(&document)?;
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

    pub(crate) fn definitions(&self) -> impl Iterator<Item = &SchemaDefinition> {
        self.schemas.values()
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
    if id.len() > 256 {
        return Err(RegistryError::InvalidId(id.to_owned()));
    }
    let body = id
        .strip_prefix("arcs.")
        .ok_or_else(|| RegistryError::InvalidId(id.to_owned()))?;
    let (name, version_text) = body
        .rsplit_once(".v")
        .ok_or_else(|| RegistryError::InvalidId(id.to_owned()))?;
    let mut segments = name.split('.');
    let artifact_type = segments
        .next()
        .filter(|segment| valid_schema_segment(segment))
        .ok_or_else(|| RegistryError::InvalidId(id.to_owned()))?;
    if !segments.all(valid_schema_segment) {
        return Err(RegistryError::InvalidId(id.to_owned()));
    }
    let version = version_text
        .parse::<u64>()
        .ok()
        .filter(|value| *value >= 1)
        .ok_or_else(|| RegistryError::InvalidId(id.to_owned()))?;
    if version.to_string() != version_text {
        return Err(RegistryError::InvalidId(id.to_owned()));
    }
    Ok((artifact_type.to_owned(), version))
}

fn valid_schema_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= MAX_ARTIFACT_TYPE_BYTES
        && segment.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
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
            // Unbekannte Typen werden bereits bei der Registrierung blockiert.
            // Der Validator bleibt trotzdem auch bei intern beschädigtem Zustand
            // fail-closed.
            _ => false,
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
        violations.push(SchemaViolation::new(path, format!("must equal {expected}")));
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
        violations.push(SchemaViolation::new(
            path,
            "must be a valid RFC 3339 date-time",
        ));
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

/// Der eingebaute Validator unterstützt absichtlich nur diesen kleinen,
/// vollständig geprüften JSON-Schema-Teil. Externe Adapter-Schemas mit anderen
/// Schlüsselwörtern werden bei der Registrierung abgelehnt, statt nur
/// teilweise und damit fail-open ausgewertet zu werden.
const SUPPORTED_SCHEMA_KEYWORDS: &[&str] = &[
    "$id",
    "$schema",
    "title",
    "description",
    "type",
    "required",
    "properties",
    "additionalProperties",
    "items",
    "minItems",
    "minLength",
    "minimum",
    "maximum",
    "enum",
    "const",
    "format",
];

fn validate_schema_document(document: &Value) -> Result<(), RegistryError> {
    validate_schema_node(document, "$", true)
}

fn validate_schema_node(schema: &Value, path: &str, is_root: bool) -> Result<(), RegistryError> {
    let object = schema
        .as_object()
        .ok_or_else(|| invalid_schema(path, "schema must be an object"))?;

    for keyword in object.keys() {
        if !SUPPORTED_SCHEMA_KEYWORDS.contains(&keyword.as_str()) {
            return Err(RegistryError::UnsupportedKeyword {
                path: path.to_owned(),
                keyword: keyword.clone(),
            });
        }
    }

    if !is_root && (object.contains_key("$id") || object.contains_key("$schema")) {
        return Err(invalid_schema(
            path,
            "$id and $schema are only allowed at the document root",
        ));
    }

    if let Some(dialect) = object.get("$schema") {
        let dialect = dialect
            .as_str()
            .ok_or_else(|| invalid_schema(&format!("{path}.$schema"), "must be a string"))?;
        if dialect != "https://json-schema.org/draft/2020-12/schema" {
            return Err(invalid_schema(
                &format!("{path}.$schema"),
                "only JSON Schema draft 2020-12 is supported",
            ));
        }
    }

    for keyword in ["title", "description"] {
        if let Some(value) = object.get(keyword)
            && !value.is_string()
        {
            return Err(invalid_schema(
                &format!("{path}.{keyword}"),
                "must be a string",
            ));
        }
    }

    let schema_type = object
        .get("type")
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| invalid_schema(&format!("{path}.type"), "must be a string"))
        })
        .transpose()?;
    if let Some(schema_type) = schema_type
        && !matches!(
            schema_type,
            "object" | "array" | "string" | "integer" | "number" | "boolean" | "null"
        )
    {
        return Err(invalid_schema(
            &format!("{path}.type"),
            "unknown JSON value type",
        ));
    }

    validate_object_schema_keywords(object, path, schema_type)?;
    validate_array_schema_keywords(object, path, schema_type)?;
    validate_string_schema_keywords(object, path, schema_type)?;
    validate_number_schema_keywords(object, path, schema_type)?;

    if let Some(values) = object.get("enum") {
        let values = values
            .as_array()
            .filter(|values| !values.is_empty())
            .ok_or_else(|| invalid_schema(&format!("{path}.enum"), "must be a non-empty array"))?;
        for (index, value) in values.iter().enumerate() {
            if values[..index].contains(value) {
                return Err(invalid_schema(
                    &format!("{path}.enum[{index}]"),
                    "enum values must be unique",
                ));
            }
        }
    }

    Ok(())
}

fn validate_object_schema_keywords(
    object: &Map<String, Value>,
    path: &str,
    schema_type: Option<&str>,
) -> Result<(), RegistryError> {
    let has_object_keywords = ["required", "properties", "additionalProperties"]
        .iter()
        .any(|keyword| object.contains_key(*keyword));
    if has_object_keywords && schema_type != Some("object") {
        return Err(invalid_schema(
            path,
            "object keywords require type 'object'",
        ));
    }

    let properties = object
        .get("properties")
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| invalid_schema(&format!("{path}.properties"), "must be an object"))
        })
        .transpose()?;
    if let Some(properties) = properties {
        for (name, property_schema) in properties {
            validate_schema_node(property_schema, &format!("{path}.properties.{name}"), false)?;
        }
    }

    if let Some(required) = object.get("required") {
        let required = required
            .as_array()
            .ok_or_else(|| invalid_schema(&format!("{path}.required"), "must be an array"))?;
        let mut names = HashSet::new();
        for (index, name) in required.iter().enumerate() {
            let name = name
                .as_str()
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    invalid_schema(
                        &format!("{path}.required[{index}]"),
                        "must be a non-empty string",
                    )
                })?;
            if !names.insert(name) {
                return Err(invalid_schema(
                    &format!("{path}.required[{index}]"),
                    "required property names must be unique",
                ));
            }
            if !properties.is_some_and(|properties| properties.contains_key(name)) {
                return Err(invalid_schema(
                    &format!("{path}.required[{index}]"),
                    "required property must be declared in properties",
                ));
            }
        }
    }

    if let Some(additional) = object.get("additionalProperties")
        && !additional.is_boolean()
    {
        return Err(invalid_schema(
            &format!("{path}.additionalProperties"),
            "must be a boolean",
        ));
    }

    Ok(())
}

fn validate_array_schema_keywords(
    object: &Map<String, Value>,
    path: &str,
    schema_type: Option<&str>,
) -> Result<(), RegistryError> {
    let has_array_keywords = ["items", "minItems"]
        .iter()
        .any(|keyword| object.contains_key(*keyword));
    if has_array_keywords && schema_type != Some("array") {
        return Err(invalid_schema(path, "array keywords require type 'array'"));
    }

    if let Some(items) = object.get("items") {
        validate_schema_node(items, &format!("{path}.items"), false)?;
    }
    if let Some(minimum) = object.get("minItems")
        && minimum.as_u64().is_none()
    {
        return Err(invalid_schema(
            &format!("{path}.minItems"),
            "must be a non-negative integer",
        ));
    }
    Ok(())
}

fn validate_string_schema_keywords(
    object: &Map<String, Value>,
    path: &str,
    schema_type: Option<&str>,
) -> Result<(), RegistryError> {
    let has_string_keywords = ["minLength", "format"]
        .iter()
        .any(|keyword| object.contains_key(*keyword));
    if has_string_keywords && schema_type != Some("string") {
        return Err(invalid_schema(
            path,
            "string keywords require type 'string'",
        ));
    }

    if let Some(minimum) = object.get("minLength")
        && minimum.as_u64().is_none()
    {
        return Err(invalid_schema(
            &format!("{path}.minLength"),
            "must be a non-negative integer",
        ));
    }
    if let Some(format) = object.get("format") {
        let format = format
            .as_str()
            .ok_or_else(|| invalid_schema(&format!("{path}.format"), "must be a string"))?;
        if format != "date-time" {
            return Err(invalid_schema(
                &format!("{path}.format"),
                "unsupported string format",
            ));
        }
    }
    Ok(())
}

fn validate_number_schema_keywords(
    object: &Map<String, Value>,
    path: &str,
    schema_type: Option<&str>,
) -> Result<(), RegistryError> {
    let has_number_keywords = ["minimum", "maximum"]
        .iter()
        .any(|keyword| object.contains_key(*keyword));
    if has_number_keywords && !matches!(schema_type, Some("integer" | "number")) {
        return Err(invalid_schema(
            path,
            "numeric keywords require type 'integer' or 'number'",
        ));
    }

    let minimum = object.get("minimum").map(|value| {
        value
            .as_f64()
            .ok_or_else(|| invalid_schema(&format!("{path}.minimum"), "must be a number"))
    });
    let maximum = object.get("maximum").map(|value| {
        value
            .as_f64()
            .ok_or_else(|| invalid_schema(&format!("{path}.maximum"), "must be a number"))
    });
    let minimum = minimum.transpose()?;
    let maximum = maximum.transpose()?;
    if let (Some(minimum), Some(maximum)) = (minimum, maximum)
        && minimum > maximum
    {
        return Err(invalid_schema(path, "minimum must not exceed maximum"));
    }
    Ok(())
}

fn invalid_schema(path: &str, message: &str) -> RegistryError {
    RegistryError::InvalidSchema {
        path: path.to_owned(),
        message: message.to_owned(),
    }
}

/// Strikte, abhängigkeitfreie Prüfung des von den Verträgen verwendeten
/// RFC-3339-Profils.
pub(crate) fn is_rfc3339(value: &str) -> bool {
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
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return false;
    }
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
    if hour.len() != 2 || minute.len() != 2 {
        return false;
    }
    let second = if let Some((whole, fraction)) = second.split_once('.') {
        if fraction.is_empty()
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
            || fraction.contains('.')
        {
            return false;
        }
        whole
    } else {
        second
    };
    if second.len() != 2 {
        return false;
    }
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
    if offset.len() != 6 || !matches!(offset.as_bytes().first(), Some(b'+' | b'-')) {
        return false;
    }
    let offset = &offset[1..];
    let Some((hour, minute)) = offset.split_once(':') else {
        return false;
    };
    if hour.len() != 2 || minute.len() != 2 {
        return false;
    }
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
    for (name, value) in object {
        if let Some(property_schema) = properties.and_then(|known| known.get(name)) {
            validate_value(
                property_schema,
                value,
                &format!("{path}.{name}"),
                violations,
            );
        } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
            // Auch ein geschlossenes Schema ganz ohne `properties` muss jedes
            // Feld ablehnen. Die Prüfung darf deshalb nicht davon abhängen,
            // ob überhaupt eine Property-Map vorhanden ist.
            violations.push(SchemaViolation::new(
                format!("{path}.{name}"),
                "additional property is not allowed",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    // Ein Input ohne Rohtext muss blockiert werden.
    fn bundled_input_schema_rejects_missing_raw_text() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let result = registry.validate(&SchemaId("arcs.input.v1".into()), &json!({}));

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|violation| violation.path == "$.raw_text")
        );
    }

    #[test]
    // Leerer Text ist kein verwertbarer Input.
    fn bundled_input_schema_rejects_empty_raw_text() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        assert!(
            registry
                .validate(&SchemaId("arcs.input.v1".into()), &json!({"raw_text": ""}),)
                .is_err()
        );
    }

    #[test]
    // Herkunft gehört in den Envelope und darf nicht im Payload dupliziert sein.
    fn bundled_input_schema_rejects_additional_source() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let result = registry.validate(
            &SchemaId("arcs.input.v1".into()),
            &json!({"raw_text": "Hallo ARCS", "source": "duplicate"}),
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|violation| violation.path == "$.source")
        );
    }

    #[test]
    // Ein externer Vertrag darf keine Regeln vortäuschen, die ARCS nicht auswertet.
    fn rejects_unsupported_schema_keywords_instead_of_ignoring_them() {
        let mut registry = SchemaRegistry::new();
        let result = registry.register_json(
            r##"{
                "$id": "arcs.external_input.demo.v1",
                "type": "object",
                "properties": {
                    "payload": {"$ref": "#/$defs/payload"}
                },
                "required": ["payload"],
                "additionalProperties": false
            }"##,
        );

        assert!(matches!(
            result,
            Err(RegistryError::UnsupportedKeyword { keyword, .. }) if keyword == "$ref"
        ));
        assert!(
            registry
                .get(&SchemaId("arcs.external_input.demo.v1".into()))
                .is_none()
        );
    }

    #[test]
    // Auch ein unbekannter Grundtyp muss bereits beim Registrieren scheitern.
    fn rejects_unknown_json_schema_types() {
        let mut registry = SchemaRegistry::new();
        let result = registry.register_json(
            r#"{
                "$id": "arcs.external_input.demo.v1",
                "type": "objectish"
            }"#,
        );

        assert!(matches!(
            result,
            Err(RegistryError::InvalidSchema { path, .. }) if path == "$.type"
        ));
    }

    #[test]
    // Pflichtfelder ohne tatsächlich geprüften Property-Vertrag wären fail-open.
    fn rejects_required_properties_without_definitions() {
        let mut registry = SchemaRegistry::new();
        let result = registry.register_json(
            r#"{
                "$id": "arcs.external_input.demo.v1",
                "type": "object",
                "required": ["payload"],
                "properties": {},
                "additionalProperties": false
            }"#,
        );

        assert!(matches!(
            result,
            Err(RegistryError::InvalidSchema { path, .. })
                if path == "$.required[0]"
        ));
    }

    #[test]
    fn closed_object_without_properties_rejects_every_field() {
        let mut registry = SchemaRegistry::new();
        let schema_id = SchemaId("arcs.empty.demo.v1".into());
        registry
            .register_json(
                r#"{
                    "$id": "arcs.empty.demo.v1",
                    "type": "object",
                    "additionalProperties": false
                }"#,
            )
            .unwrap();

        let result = registry.validate(&schema_id, &json!({"evil": true}));

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .iter()
                .any(|violation| violation.path == "$.evil")
        );
    }

    #[test]
    fn rejects_noncanonical_schema_identifiers() {
        let oversized_type = format!("arcs.{}.v1", "a".repeat(MAX_ARTIFACT_TYPE_BYTES + 1));
        for id in [
            "arcs.Input.demo.v1",
            "arcs.input..demo.v1",
            "arcs.input.demo.v01",
            "https://example.test/input.schema.json",
            &oversized_type,
        ] {
            let mut registry = SchemaRegistry::new();
            let result = registry.register_json(&format!(
                r#"{{
                    "$id": "{id}",
                    "type": "object",
                    "additionalProperties": false
                }}"#
            ));

            assert!(matches!(result, Err(RegistryError::InvalidId(rejected)) if rejected == id));
        }
    }

    #[test]
    fn rfc3339_fraction_must_be_numeric_and_non_empty() {
        assert!(is_rfc3339("2026-07-27T12:34:56.123Z"));
        assert!(!is_rfc3339("2026-07-27T12:34:56.fooZ"));
        assert!(!is_rfc3339("2026-07-27T12:34:56.Z"));
    }
}
