//! Gemeinsame, ausschließlich Core-seitig gesetzte Envelope-Klassifikation.

use crate::adapters::registration::ProducerClass;
use crate::core::{ActorType, SourceClass, Trust, TrustLevel};

/// Übersetzt die Betreiberklassifikation in den auditierbaren Artifact-Actor.
pub(super) fn actor_type_for(producer: ProducerClass) -> ActorType {
    match producer {
        ProducerClass::Adapter => ActorType::Adapter,
        ProducerClass::Model => ActorType::Model,
        ProducerClass::System => ActorType::System,
        ProducerClass::Executor => ActorType::Executor,
    }
}

/// Setzt Trust aus dem Grant, ohne einem Modell höhere Autorität zu erlauben.
pub(super) fn trust_for(producer: ProducerClass, assigned: TrustLevel) -> Trust {
    match producer {
        ProducerClass::Model => Trust {
            level: TrustLevel::Low,
            source_class: SourceClass::Model,
        },
        ProducerClass::System => Trust {
            level: assigned,
            source_class: SourceClass::System,
        },
        ProducerClass::Adapter | ProducerClass::Executor => Trust {
            level: assigned,
            source_class: SourceClass::External,
        },
    }
}
