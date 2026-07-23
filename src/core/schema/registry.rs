use std::collections::HashMap;

use crate::core::artifact::SchemaId;
use super::types::SchemaDefinition;

pub struct SchemaRegistry {
    schemas: HashMap<SchemaId, SchemaDefinition>,
}

impl SchemaRegistry {
    pub fn new() -> Self {
        Self {
            schemas: HashMap::new(),
        }
    }

    pub fn register(&mut self, schema: SchemaDefinition){
        self.schemas.insert(schema.id,schema);
    }

    pub fn get (&self, id: SchemaId) -> Option<&SchemaDefinition> {
        self.schemas.get(&id)
    }
}
