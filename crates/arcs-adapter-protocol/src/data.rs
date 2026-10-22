//! Referenzen auf unveränderliche Binärdaten und begrenzter blockweiser Abruf.

use serde::{Deserialize, Serialize};

use crate::CapabilityRef;

pub const MAX_DATA_CHUNK_BYTES: u32 = 8192;
pub const MAX_DATA_OBJECT_BYTES: u64 = 64 * 1024 * 1024;
pub const DATA_REFERENCE_SCHEMA: &str = "arcs.data.reference.v1";
pub const DATA_REFERENCE_SCHEMA_JSON: &str = include_str!("../schemas/data-reference.schema.json");

/// Die Identität gilt innerhalb genau eines Adapters und einer Capability.
/// Gleiche ID und Metadaten müssen dauerhaft dieselben Bytes bezeichnen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataReference {
    /// Opaker Schlüssel; kein Dateipfad und keine URL.
    pub id: String,
    pub media_type: String,
    pub size_bytes: u64,
}

impl DataReference {
    pub fn is_valid(&self) -> bool {
        let Some((kind, subtype)) = self.media_type.split_once('/') else {
            return false;
        };
        let token = |value: &str| {
            !value.is_empty()
                && value.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || b"!#$&^_.+-".contains(&byte)
                })
        };
        !self.id.is_empty()
            && self.id.len() <= 96
            && self
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            && self.id != "."
            && self.id != ".."
            && self.media_type.len() <= 128
            && token(kind)
            && token(subtype)
            && self.size_bytes <= MAX_DATA_OBJECT_BYTES
    }
}

/// Vom Betreiber gesetzte, zusätzliche Grenzen für den Datenabruf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataLimits {
    pub max_object_bytes: u64,
    pub max_chunk_bytes: u32,
}

impl DataLimits {
    pub fn is_valid(&self) -> bool {
        (1..=MAX_DATA_OBJECT_BYTES).contains(&self.max_object_bytes)
            && (1..=MAX_DATA_CHUNK_BYTES).contains(&self.max_chunk_bytes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataRead {
    pub capability: CapabilityRef,
    pub reference: DataReference,
    pub offset: u64,
    pub max_bytes: u32,
}

impl DataRead {
    pub fn is_valid(&self) -> bool {
        self.reference.is_valid()
            && (self.offset < self.reference.size_bytes
                || (self.reference.size_bytes == 0 && self.offset == 0))
            && (1..=MAX_DATA_CHUNK_BYTES).contains(&self.max_bytes)
    }

    pub fn expected_bytes(&self) -> Option<usize> {
        self.is_valid().then(|| {
            (self.reference.size_bytes - self.offset).min(u64::from(self.max_bytes)) as usize
        })
    }
}

/// Bytes werden als JSON-Zahlen übertragen; die Blockgrenze hält Frames klein.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataChunk {
    pub reference_id: String,
    pub offset: u64,
    pub bytes: Vec<u8>,
}

impl DataChunk {
    pub fn matches(&self, request: &DataRead) -> bool {
        self.reference_id == request.reference.id
            && self.offset == request.offset
            && Some(self.bytes.len()) == request.expected_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference() -> DataReference {
        DataReference {
            id: "sample-v1".into(),
            media_type: "application/octet-stream".into(),
            size_bytes: 10,
        }
    }

    #[test]
    fn references_are_bounded_opaque_identifiers() {
        assert!(reference().is_valid());
        for id in [
            "",
            ".",
            "..",
            "../secret",
            "/etc/passwd",
            "https://example.org",
            "a b",
        ] {
            let mut value = reference();
            value.id = id.into();
            assert!(!value.is_valid());
        }
        for media_type in [
            "",
            "image",
            "image/",
            "/png",
            "image/png/extra",
            "image/png\n",
            "text/plain; charset=utf-8",
        ] {
            let mut value = reference();
            value.media_type = media_type.into();
            assert!(!value.is_valid());
        }
        let mut value = reference();
        value.size_bytes = MAX_DATA_OBJECT_BYTES + 1;
        assert!(!value.is_valid());
        value.size_bytes = 0;
        assert!(value.is_valid());
        assert!(
            serde_json::from_str::<DataReference>(
                r#"{"id":"x","media_type":"text/plain","size_bytes":1,"path":"/etc/passwd"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn chunks_require_exact_identity_offset_and_length_including_empty_objects() {
        let mut request = DataRead {
            capability: CapabilityRef::new("test", "read"),
            reference: reference(),
            offset: 8,
            max_bytes: 8,
        };
        let mut chunk = DataChunk {
            reference_id: "sample-v1".into(),
            offset: 8,
            bytes: vec![1, 2],
        };
        assert!(chunk.matches(&request));
        chunk.offset = 7;
        assert!(!chunk.matches(&request));
        chunk.offset = 8;
        chunk.bytes.pop();
        assert!(!chunk.matches(&request));
        chunk.bytes = vec![1, 2, 3];
        assert!(!chunk.matches(&request));
        chunk.bytes = vec![1, 2];
        chunk.reference_id = "other".into();
        assert!(!chunk.matches(&request));
        request.offset = 10;
        assert!(!request.is_valid());
        request.reference.size_bytes = 0;
        request.offset = 0;
        assert_eq!(request.expected_bytes(), Some(0));
        request.offset = 1;
        assert!(!request.is_valid());
        request.offset = 0;
        request.max_bytes = 0;
        assert!(!request.is_valid());
        request.max_bytes = MAX_DATA_CHUNK_BYTES + 1;
        assert!(!request.is_valid());
    }

    #[test]
    fn largest_chunk_fits_the_existing_json_frame_limit() {
        use crate::stdio::{AdapterMessage, Envelope, MAX_FRAME_BYTES, WIRE_VERSION, write_frame};
        let mut frame = Vec::new();
        write_frame(
            &mut frame,
            &Envelope {
                protocol_version: WIRE_VERSION,
                request_id: "x".repeat(128),
                message: AdapterMessage::DataChunk {
                    chunk: DataChunk {
                        reference_id: "x".repeat(96),
                        offset: MAX_DATA_OBJECT_BYTES,
                        bytes: vec![255; MAX_DATA_CHUNK_BYTES as usize],
                    },
                },
            },
        )
        .unwrap();
        assert!(frame.len() < MAX_FRAME_BYTES);
    }
}
