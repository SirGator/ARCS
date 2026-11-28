//! Geheimnisse außerhalb des Artefaktnetzes.
//!
//! Ein Geheimnis verlässt den Tresor nicht. Der Core vermittelt es über
//! eine Freigabe, die an Zweck, Ressource, Ablaufzeit und Nutzungszahl
//! gebunden ist. Die Freigabe selbst ist ein Artefakt und damit auditierbar;
//! sie enthält nie den Wert.
//!
//! Damit ein Geheimnis weder in einen Modellkontext noch in ein Log noch in
//! das Artefaktnetz gelangen kann, wird es erst beim Versand eines Auftrags
//! eingesetzt. Der gespeicherte Auftrag behält nur die Referenz.

use std::fmt;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::core::artifact_factory::{format_rfc3339, unix_seconds_from_rfc3339};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, Classification, Clock, Provenance, SchemaId,
    SchemaRegistry, SecurityProfile, Source, SourceClass, SourceKind, SubjectId, Trust, TrustLevel,
    VersionId,
};
use crate::store::{SqliteArtifactStore, StoreError};

/// Schema der auditierbaren Freigabe.
pub const GRANT_SCHEMA: &str = "arcs.secret_grant.v1";
/// Herausgeber der Freigabe.
pub const PUBLISHER: &str = "arcs.privacy.secrets";
/// Schlüssel der Payload-Markierung, die auf eine Freigabe verweist.
pub const SECRET_MARKER: &str = "$secret";

/// Maximale Länge eines Geheimnisses.
pub const MAX_SECRET_BYTES: usize = 4096;

/// Opake Kennung eines Geheimnisses.
///
/// Die Kennung wird vergeben, nicht aus dem Wert abgeleitet: Ein ableitbarer
/// Wert würde selbst eine Offenlegung sein.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretId(String);

impl SecretId {
    /// Stabile Kennung eines Geheimnisses.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Zustand einer Freigabe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantState {
    /// Die Freigabe kann genutzt werden.
    Active,
    /// Die Freigabe ist abgelaufen.
    Expired,
    /// Die Freigabe wurde ausdrücklich widerrufen.
    Revoked,
    /// Die Freigabe ist aufgebraucht.
    Spent,
}

impl GrantState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
            Self::Spent => "spent",
        }
    }
}

/// Auditierbare Freigabe eines Geheimnisses.
///
/// Die Freigabe nennt Zweck, Ressource und Ablauf. Sie trägt keinen Wert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretGrant {
    /// Stabile Kennung der Freigabe.
    pub grant_id: String,
    /// Geheimnis, das freigegeben wird.
    pub secret_id: SecretId,
    /// Name des Parameters, den der Wert im Auftrag ersetzt.
    pub parameter: String,
    /// Zweck der Freigabe, etwa `act`.
    pub purpose: String,
    /// Ressource der Freigabe, etwa `fan:act`.
    pub resource: String,
    /// Ablaufzeitpunkt der Freigabe.
    pub expires_at: String,
    /// Höchstzahl der Nutzungen.
    pub max_uses: u32,
    /// Bisherige Nutzungen.
    pub uses: u32,
    /// Zustand der Freigabe.
    pub state: GrantState,
}

/// Entwurf einer neuen Freigabe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretGrantSpec {
    pub secret_id: SecretId,
    pub parameter: String,
    pub purpose: String,
    pub resource: String,
    /// Gültigkeitsdauer in Sekunden.
    pub ttl_seconds: u64,
    pub max_uses: u32,
}

/// Nutzung einer Freigabe.
///
/// Die Nutzung nennt die tatsächlich geltende Ressource. Eine Freigabe gilt
/// nur für die Ressource, für die sie ausgestellt wurde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretUse {
    pub purpose: String,
    pub resource: String,
}

/// Fehler der Geheimnisgrenze.
#[derive(Debug)]
pub enum SecretError {
    /// Der Store hat die Änderung nicht angenommen.
    Store(StoreError),
    /// Der Tresor hat die Änderung nicht angenommen.
    Vault(rusqlite::Error),
    /// Das Geheimnis ist unbekannt.
    UnknownSecret(String),
    /// Die Freigabe ist unbekannt.
    UnknownGrant(String),
    /// Der Zweck der Nutzung passt nicht zur Freigabe.
    PurposeMismatch { expected: String, actual: String },
    /// Die Ressource der Nutzung passt nicht zur Freigabe.
    ResourceMismatch { expected: String, actual: String },
    /// Die Freigabe ist abgelaufen.
    Expired(String),
    /// Die Freigabe ist widerrufen.
    Revoked(String),
    /// Die Freigabe ist aufgebraucht.
    Exhausted(String),
    /// Die Angabe ist strukturell unzulässig.
    Invalid(String),
}

impl fmt::Display for SecretError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "store rejected the secret grant: {error:?}"),
            Self::Vault(error) => write!(formatter, "vault rejected the secret: {error}"),
            Self::UnknownSecret(id) => write!(formatter, "unknown secret '{id}'"),
            Self::UnknownGrant(id) => write!(formatter, "unknown secret grant '{id}'"),
            Self::PurposeMismatch { expected, actual } => {
                write!(
                    formatter,
                    "secret grant purpose '{expected}' does not match '{actual}'"
                )
            }
            Self::ResourceMismatch { expected, actual } => {
                write!(
                    formatter,
                    "secret grant resource '{expected}' does not match '{actual}'"
                )
            }
            Self::Expired(id) => write!(formatter, "secret grant '{id}' expired"),
            Self::Revoked(id) => write!(formatter, "secret grant '{id}' was revoked"),
            Self::Exhausted(id) => write!(formatter, "secret grant '{id}' is exhausted"),
            Self::Invalid(message) => write!(formatter, "invalid secret request: {message}"),
        }
    }
}

impl From<StoreError> for SecretError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<rusqlite::Error> for SecretError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Vault(value)
    }
}

fn invalid(message: impl Into<String>) -> SecretError {
    SecretError::Invalid(message.into())
}

/// Tresor der Geheimnisse.
///
/// Der Tresor steht außerhalb des Artefaktnetzes: Seine Tabelle ist kein
/// Artefakt, und die Retrieval- und Memory-Pfade erreichen sie nicht.
pub struct SecretVault {
    connection: Connection,
}

impl fmt::Debug for SecretVault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretVault(<redacted>)")
    }
}

impl SecretVault {
    /// Öffnet einen Tresor in einer eigenen Datei.
    pub fn open(path: &Path) -> Result<Self, SecretError> {
        Ok(Self::prepare(Connection::open(path)?))
    }

    /// Öffnet einen flüchtigen Tresor für Tests und Probeaufbauten.
    pub fn in_memory() -> Result<Self, SecretError> {
        Ok(Self::prepare(Connection::open_in_memory()?))
    }

    fn prepare(connection: Connection) -> Self {
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS secret_values (
                     secret_id TEXT PRIMARY KEY,
                     value TEXT NOT NULL
                 );",
            )
            .expect("secret vault table");
        Self { connection }
    }

    fn put(&self, secret_id: &str, value: &str) -> Result<(), SecretError> {
        self.connection.execute(
            "INSERT INTO secret_values (secret_id, value) VALUES (?1, ?2)",
            params![secret_id, value],
        )?;
        Ok(())
    }

    fn get(&self, secret_id: &str) -> Result<Option<String>, SecretError> {
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM secret_values WHERE secret_id = ?1",
                params![secret_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?)
    }

    fn values(&self) -> Result<Vec<String>, SecretError> {
        let mut statement = self.connection.prepare("SELECT value FROM secret_values")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut values = Vec::new();
        for row in rows {
            values.push(row?);
        }
        Ok(values)
    }

    fn next_id(&self) -> Result<String, SecretError> {
        let count: i64 =
            self.connection
                .query_row("SELECT COUNT(*) FROM secret_values", [], |row| row.get(0))?;
        Ok(format!("secret:{}", count + 1))
    }
}

/// Dienst für Geheimnisse und ihre Freigaben.
///
/// Der Dienst verbindet den Tresor mit dem auditierbaren Artefaktnetz: Der
/// Wert bleibt im Tresor, die Freigabe wird als Artefakt festgehalten.
pub struct SecretBroker<'a> {
    store: &'a SqliteArtifactStore,
    schemas: &'a SchemaRegistry,
    vault: &'a SecretVault,
    clock: &'a dyn Clock,
}

impl<'a> SecretBroker<'a> {
    pub fn new(
        store: &'a SqliteArtifactStore,
        schemas: &'a SchemaRegistry,
        vault: &'a SecretVault,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            store,
            schemas,
            vault,
            clock,
        }
    }

    /// Nimmt ein Geheimnis in den Tresor auf.
    ///
    /// Der Wert verlässt den Tresor nicht. Die Rückgabe ist eine opake
    /// Kennung, die weder Rückschlüsse auf den Wert noch eine Suche im
    /// Artefaktnetz erlaubt.
    pub fn register(&self, value: &str) -> Result<SecretId, SecretError> {
        if value.is_empty() || value.len() > MAX_SECRET_BYTES {
            return Err(invalid("a secret must hold 1..4096 bytes"));
        }
        if value.chars().any(char::is_control) {
            return Err(invalid("a secret must not contain control characters"));
        }
        let secret_id = self.vault.next_id()?;
        self.vault.put(&secret_id, value)?;
        Ok(SecretId(secret_id))
    }

    /// Stellt eine Freigabe für ein Geheimnis aus.
    pub fn issue(&self, spec: &SecretGrantSpec) -> Result<SecretGrant, SecretError> {
        check_identifier("parameter", &spec.parameter)?;
        check_identifier("purpose", &spec.purpose)?;
        check_identifier("resource", &spec.resource)?;
        if spec.max_uses == 0 {
            return Err(invalid("a grant needs at least one use"));
        }
        if spec.ttl_seconds == 0 || spec.ttl_seconds > 86_400 {
            return Err(invalid("a grant is valid for at most one day"));
        }
        if self.vault.get(spec.secret_id.as_str())?.is_none() {
            return Err(SecretError::UnknownSecret(spec.secret_id.as_str().into()));
        }
        let now = self.clock.now_rfc3339();
        let now_seconds = unix_seconds_from_rfc3339(&now)
            .ok_or_else(|| invalid("the clock returned an unusable timestamp"))?;
        let grant = SecretGrant {
            grant_id: grant_id(&spec.secret_id, &spec.purpose, &spec.resource),
            secret_id: spec.secret_id.clone(),
            parameter: spec.parameter.clone(),
            purpose: spec.purpose.clone(),
            resource: spec.resource.clone(),
            expires_at: format_rfc3339(now_seconds.saturating_add(spec.ttl_seconds)),
            max_uses: spec.max_uses,
            uses: 0,
            state: GrantState::Active,
        };
        self.write(&grant, None, 1)
    }

    /// Liest den aktuellen Stand einer Freigabe.
    pub fn lookup(&self, grant_id: &str) -> Result<Option<SecretGrant>, SecretError> {
        Ok(self.load(grant_id)?.map(|stored| stored.grant))
    }

    /// Nutzt eine Freigabe und gibt ihren Wert zurück.
    ///
    /// Die Nutzung zählt, ist an Zweck und Ressource gebunden und endet mit
    /// dem Ablauf. Eine fremde Ressource erhält den Wert nicht.
    pub fn resolve(&self, grant_id: &str, use_: &SecretUse) -> Result<String, SecretError> {
        let grant = self.authorize(grant_id, use_)?;
        self.vault
            .get(grant.secret_id.as_str())?
            .ok_or_else(|| SecretError::UnknownSecret(grant.secret_id.as_str().into()))
    }

    /// Prüft eine Freigabe, zählt die Nutzung und hält sie fest.
    pub fn authorize(&self, grant_id: &str, use_: &SecretUse) -> Result<SecretGrant, SecretError> {
        let stored = self
            .load(grant_id)?
            .ok_or_else(|| SecretError::UnknownGrant(grant_id.to_string()))?;
        let grant = stored.grant;
        if grant.purpose != use_.purpose {
            return Err(SecretError::PurposeMismatch {
                expected: grant.purpose,
                actual: use_.purpose.clone(),
            });
        }
        if grant.resource != use_.resource {
            return Err(SecretError::ResourceMismatch {
                expected: grant.resource,
                actual: use_.resource.clone(),
            });
        }
        match grant.state {
            GrantState::Revoked => return Err(SecretError::Revoked(grant_id.to_string())),
            GrantState::Spent => return Err(SecretError::Exhausted(grant_id.to_string())),
            GrantState::Expired => return Err(SecretError::Expired(grant_id.to_string())),
            GrantState::Active => {}
        }
        let now = self.clock.now_rfc3339();
        let now_seconds = unix_seconds_from_rfc3339(&now)
            .ok_or_else(|| invalid("the clock returned an unusable timestamp"))?;
        let expires = unix_seconds_from_rfc3339(&grant.expires_at)
            .ok_or_else(|| invalid("the grant carries an unusable expiry"))?;
        if now_seconds >= expires {
            let expired = SecretGrant {
                state: GrantState::Expired,
                ..grant
            };
            self.write(
                &expired,
                Some(&stored.version_id),
                stored.version.saturating_add(1),
            )?;
            return Err(SecretError::Expired(grant_id.to_string()));
        }
        if grant.uses >= grant.max_uses {
            let spent = SecretGrant {
                state: GrantState::Spent,
                ..grant
            };
            self.write(
                &spent,
                Some(&stored.version_id),
                stored.version.saturating_add(1),
            )?;
            return Err(SecretError::Exhausted(grant_id.to_string()));
        }
        let uses = grant.uses.saturating_add(1);
        let next = SecretGrant {
            uses,
            state: if uses >= grant.max_uses {
                GrantState::Spent
            } else {
                GrantState::Active
            },
            ..grant
        };
        self.write(
            &next,
            Some(&stored.version_id),
            stored.version.saturating_add(1),
        )
    }

    /// Widerruft eine Freigabe ausdrücklich.
    pub fn revoke(&self, grant_id: &str) -> Result<SecretGrant, SecretError> {
        let stored = self
            .load(grant_id)?
            .ok_or_else(|| SecretError::UnknownGrant(grant_id.to_string()))?;
        let revoked = SecretGrant {
            state: GrantState::Revoked,
            ..stored.grant
        };
        self.write(
            &revoked,
            Some(&stored.version_id),
            stored.version.saturating_add(1),
        )
    }

    /// Setzt im Auftrag die Referenzen auf Geheimnisse ein.
    ///
    /// Der gespeicherte Auftrag behält die Referenz. Erst der ausgehende
    /// Auftrag trägt den Wert, und nur für die Nutzung, für die die Freigabe
    /// ausgestellt wurde.
    pub fn materialize(&self, payload: &Value, use_: &SecretUse) -> Result<Value, SecretError> {
        let references = collect_secret_references(payload);
        let mut values = std::collections::HashMap::new();
        for reference in references {
            let value = self.resolve(&reference, use_)?;
            values.insert(reference, value);
        }
        Ok(substitute(payload, &values))
    }

    /// Entfernt bekannte Geheimniswerte aus einem JSON-Baum.
    ///
    /// Antworten externer Adapter können Werte zurückspiegeln. Bevor eine
    /// solche Antwort gespeichert wird, ersetzt der Dienst sie durch die
    /// Referenz, damit kein Wert ins Artefaktnetz gelangt.
    pub fn redact_value(&self, value: &Value) -> Value {
        let values = self.vault.values().unwrap_or_default();
        redact_value(value, &values)
    }

    /// Entfernt bekannte Geheimniswerte aus einem Text.
    pub fn redact_text(&self, text: &str) -> String {
        let values = self.vault.values().unwrap_or_default();
        redact_text(text, &values)
    }

    fn load(&self, grant_id: &str) -> Result<Option<StoredGrant>, SecretError> {
        let Some(artifact) = self.store.current(
            &SubjectId(subject(grant_id)),
            &SchemaId(GRANT_SCHEMA.into()),
        )?
        else {
            return Ok(None);
        };
        Ok(Some(StoredGrant {
            version: artifact.version,
            version_id: artifact.version_id.clone(),
            grant: parse(artifact)?,
        }))
    }

    fn write(
        &self,
        grant: &SecretGrant,
        previous: Option<&VersionId>,
        version: u64,
    ) -> Result<SecretGrant, SecretError> {
        let schema = self
            .schemas
            .get(&SchemaId(GRANT_SCHEMA.into()))
            .ok_or_else(|| invalid("missing secret grant schema"))?;
        let id = artifact_id(&grant.grant_id);
        let artifact = Artifact {
            artifact_id: ArtifactId(id.clone()),
            version_id: VersionId(format!("{id}:v{version}")),
            version,
            artifact_type: schema.artifact_type.clone(),
            schema_id: schema.id.clone(),
            schema_version: schema.version,
            created_at: self.clock.now_rfc3339(),
            created_by: Actor {
                actor_type: ActorType::System,
                id: PUBLISHER.into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: grant.grant_id.clone(),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: format!("secret_grant:{}", grant.grant_id),
            subject: Some(SubjectId(subject(&grant.grant_id))),
            tags: vec![
                format!("state:{}", grant.state.as_str()),
                format!("purpose:{}", grant.purpose),
            ],
            payload: serde_json::to_value(grant).map_err(|error| invalid(error.to_string()))?,
            provenance: Some(Provenance {
                parents: previous
                    .map(|version| vec![version.0.clone()])
                    .unwrap_or_default(),
                rules_applied: vec!["privacy.secret_grant".into()],
                models_used: vec![],
                transform: Some("privacy.secret_grant".into()),
            }),
            security: SecurityProfile::declared(Classification::Restricted),
        };
        let stored = self
            .store
            .append_current_resolved(&artifact, self.schemas)?;
        parse(stored)
    }
}

/// Gespeicherte Freigabe mit ihrer Fassung.
struct StoredGrant {
    grant: SecretGrant,
    version: u64,
    version_id: VersionId,
}

/// Sammelt alle Referenzen auf Geheimnisse in einem Auftrag.
///
/// Eine Referenz ist ein Objekt mit genau einem Schlüssel `$secret` und einem
/// Textwert. Andere Objekte bleiben unberührt.
pub fn collect_secret_references(payload: &Value) -> Vec<String> {
    let mut references = Vec::new();
    walk(payload, &mut |value| {
        if let Some(reference) = marker_text(value, SECRET_MARKER) {
            references.push(reference);
        }
    });
    references
}

/// Ersetzt die Referenzen eines Auftrags durch ihre Werte.
fn substitute(payload: &Value, values: &std::collections::HashMap<String, String>) -> Value {
    match payload {
        Value::Object(fields) => {
            if fields.len() == 1
                && let Some(Value::String(reference)) = fields.get(SECRET_MARKER)
                && let Some(value) = values.get(reference)
            {
                return Value::String(value.clone());
            }
            let mut result = Map::new();
            for (key, item) in fields {
                result.insert(key.clone(), substitute(item, values));
            }
            Value::Object(result)
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| substitute(item, values)).collect())
        }
        other => other.clone(),
    }
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
    if text.is_empty() || text.len() > 256 {
        return None;
    }
    Some(text.to_string())
}

/// Ersetzt bekannte Geheimniswerte in einem JSON-Baum.
fn redact_value(value: &Value, secrets: &[String]) -> Value {
    match value {
        Value::String(text) => Value::String(redact_text(text, secrets)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_value(item, secrets))
                .collect(),
        ),
        Value::Object(fields) => {
            let mut result = Map::new();
            for (key, item) in fields {
                result.insert(key.clone(), redact_value(item, secrets));
            }
            Value::Object(result)
        }
        other => other.clone(),
    }
}

/// Ersetzt bekannte Geheimniswerte in einem Text.
fn redact_text(text: &str, secrets: &[String]) -> String {
    let mut result = text.to_string();
    for secret in secrets {
        if secret.is_empty() {
            continue;
        }
        if result.contains(secret.as_str()) {
            result = result.replace(secret.as_str(), "«secret»");
        }
    }
    result
}

/// Prüft eine Kennung im Freitextfeld einer Freigabe.
fn check_identifier(field: &str, value: &str) -> Result<(), SecretError> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "'{field}' must hold 1..128 characters without control characters"
        )));
    }
    Ok(())
}

/// Kennung des Artefakts einer Freigabe.
fn artifact_id(grant_id: &str) -> String {
    format!("secret_grant:{}", grant_id.replace(':', "-"))
}

/// Betreff des Artefakts einer Freigabe.
fn subject(grant_id: &str) -> String {
    format!("secret_grant:{grant_id}")
}

/// Stabile Kennung einer Freigabe.
fn grant_id(secret_id: &SecretId, purpose: &str, resource: &str) -> String {
    format!(
        "grant:{}-{}-{}",
        secret_id.as_str().replace(':', "-"),
        purpose.replace(':', "-"),
        resource.replace(':', "-")
    )
}

/// Liest die Freigabe aus einem Artefakt.
fn parse(artifact: Artifact) -> Result<SecretGrant, SecretError> {
    serde_json::from_value(artifact.payload)
        .map_err(|error| invalid(format!("malformed secret grant: {error}")))
}
