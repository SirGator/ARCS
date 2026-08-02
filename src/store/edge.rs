use serde::{Deserialize, Serialize};

use crate::core::VersionId;

/// Persistierte, gerichtete Beziehung zwischen zwei Artefaktversionen.
///
/// Der Kantenvertrag liegt bewusst zwischen Datenbank- und Network-Slice,
/// damit beide voneinander unabhängig auf denselben Typ zugreifen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkEdge {
    pub from: VersionId,
    pub to: VersionId,
    pub weight: f64,
}
