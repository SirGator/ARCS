use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct ArtifactId(i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct SchemaId(i32);

impl SchemaId {
    pub fn new(value: i32) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Origin(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Subject(String);

#[derive(Debug, Clone, Serialize)]
pub struct Content(String);

impl Content {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatedAt(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Input,
    Intent,
    Action,
    Result,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactState {
    Raw,
    Validated,
    Rejected,
}

#[derive(Debug, Clone, Serialize)]
pub struct Relation {
    pub from: ArtifactId,
    pub kind: RelationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    DerivedFrom,
    CausedBy,
    References,
    Replaces,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactBase {
    pub id: ArtifactId,
    pub schema_id: SchemaId,
    pub kind: ArtifactKind,
    pub state: ArtifactState,
    pub origin: Origin,
    pub subject: Subject,
    pub content: Content,
    pub relations: Vec<Relation>,
    pub created_at: CreatedAt,
}

pub fn create_raw_artifact_base(
    id: i32,
    schema_id: i32,
    kind: ArtifactKind,
    origin: String,
    subject: String,
    content: String,
    relations: Vec<Relation>,
    created_at: String,
) -> ArtifactBase {
    ArtifactBase {
        id: ArtifactId(id),
        schema_id: SchemaId(schema_id),
        kind,
        state: ArtifactState::Raw,
        origin: Origin(origin),
        subject: Subject(subject),
        content: Content(content),
        relations,
        created_at: CreatedAt(created_at),
    }
}