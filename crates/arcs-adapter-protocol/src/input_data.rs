//! Begrenzte Binäreingaben für explizit ausgewählten Reasoning-Kontext.
use crate::{CapabilityRef, DataReference, VersionId};
use serde::{Deserialize, Serialize};
pub const MAX_INPUT_DATA_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_INPUT_DATA_OBJECTS: usize = 8;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputDataLimits {
    pub max_total_bytes: u64,
    pub max_objects: usize,
}
impl InputDataLimits {
    pub fn is_valid(&self) -> bool {
        (1..=MAX_INPUT_DATA_BYTES).contains(&self.max_total_bytes)
            && (1..=MAX_INPUT_DATA_OBJECTS).contains(&self.max_objects)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputDataSelection {
    pub name: String,
    pub source: CapabilityRef,
    pub source_version_id: VersionId,
    /// JSON-Pointer zur Referenz innerhalb des ausdrücklich ausgewählten Kontextes.
    pub reference_pointer: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputDataDescriptor {
    pub selection: InputDataSelection,
    pub reference: DataReference,
}
impl InputDataDescriptor {
    pub fn is_valid(&self) -> bool {
        let selection = &self.selection;
        !selection.name.is_empty()
            && selection.name.len() <= 64
            && selection
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            && !selection.source_version_id.0.trim().is_empty()
            && selection.reference_pointer.starts_with('/')
            && selection.reference_pointer.len() <= 512
            && !selection.reference_pointer.chars().any(char::is_control)
            && self.reference.is_valid()
            && self.reference.size_bytes <= MAX_INPUT_DATA_BYTES
    }
}
/// Rein transportierte Daten; Grants und Herkunft setzt weiterhin der Core.
#[derive(Debug, Clone)]
pub struct ReceivedInputData {
    pub descriptor: InputDataDescriptor,
    pub bytes: Vec<u8>,
}
