use super::*;
use crate::store::retrieval::{PayloadTextQuery, RetrievalScope};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryTextQuery {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Core-only fields from the first mandatory input artifact, never an
    /// implicit export of those fields to the external reasoning context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_fields: Vec<String>,
}

impl MemoryTextQuery {
    pub(super) fn valid(&self) -> bool {
        let mut fields = HashSet::new();
        self.input_fields.len() <= 16
            && self.input_fields.iter().all(|field| {
                !field.trim().is_empty()
                    && field.len() <= 256
                    && !field.chars().any(char::is_control)
                    && fields.insert(field)
            })
            && if self.text.is_empty() {
                !self.input_fields.is_empty()
            } else {
                PayloadTextQuery {
                    query: self.text.clone(),
                    fields: vec!["body".into()],
                }
                .valid()
            }
    }
}

pub(super) fn resolve_scopes(
    store: &SqliteArtifactStore,
    request: &ReasoningRequestV2,
    policy: &MemoryPolicy,
) -> Result<Vec<RetrievalScope>, ReasoningError> {
    let mut scopes = Vec::new();
    for scope in &policy.scopes {
        let text = if let Some(query) = &scope.text_query {
            let mut text = query.text.clone();
            if !query.input_fields.is_empty() {
                let input = request
                    .context
                    .first()
                    .ok_or_else(|| invalid("memory query requires a mandatory input"))?;
                let artifact = store.get(&input.version_id)?.ok_or_else(|| {
                    ReasoningError::MissingContextArtifact(input.version_id.clone())
                })?;
                for field in &query.input_fields {
                    let value = artifact
                        .payload
                        .get(field)
                        .and_then(serde_json::Value::as_str)
                        .filter(|value| value.chars().any(char::is_alphanumeric))
                        .ok_or_else(|| {
                            invalid("memory query field must be an existing nonempty input string")
                        })?;
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    if text.len().saturating_add(value.len()) > 4096 {
                        return Err(invalid("memory query exceeds 4096 bytes"));
                    }
                    text.push_str(value);
                }
            }
            let resolved = PayloadTextQuery {
                query: text,
                fields: scope.payload_fields.clone(),
            };
            if !resolved.valid() {
                return Err(invalid("invalid resolved memory query"));
            }
            Some(resolved)
        } else {
            None
        };
        scopes.push(RetrievalScope {
            filter: scope.filter.clone(),
            text,
        });
    }
    Ok(scopes)
}
