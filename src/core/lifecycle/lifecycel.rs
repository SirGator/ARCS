use super::types::{ArtifactBase, ArtifactKind, ArtifactState};

#[derive(Debug, Clone)]
pub enum LifecycleError {
    InvalidTransition {
        kind: ArtifactKind,
        from: ArtifactState,
        to: ArtifactState,
    },
}

pub fn can_transition(
    kind: ArtifactKind,
    from: ArtifactState,
    to: ArtifactState,
) -> bool {
    match (kind, from, to) {
        // Input
        (ArtifactKind::Input, ArtifactState::Raw, ArtifactState::Validated) => true,
        (ArtifactKind::Input, ArtifactState::Raw, ArtifactState::Rejected) => true,

        // Intent
        (ArtifactKind::Intent, ArtifactState::Raw, ArtifactState::Validated) => true,
        (ArtifactKind::Intent, ArtifactState::Raw, ArtifactState::Rejected) => true,

        // Action
        (ArtifactKind::Action, ArtifactState::Raw, ArtifactState::Validated) => true,
        (ArtifactKind::Action, ArtifactState::Raw, ArtifactState::Rejected) => true,
        (ArtifactKind::Action, ArtifactState::Validated, ArtifactState::Approved) => true,
        (ArtifactKind::Action, ArtifactState::Validated, ArtifactState::Rejected) => true,
        (ArtifactKind::Action, ArtifactState::Approved, ArtifactState::Executed) => true,
        (ArtifactKind::Action, ArtifactState::Approved, ArtifactState::Failed) => true,

        // Result
        (ArtifactKind::Result, ArtifactState::Raw, ArtifactState::Validated) => true,
        (ArtifactKind::Result, ArtifactState::Raw, ArtifactState::Failed) => true,

        // Error
        (ArtifactKind::Error, ArtifactState::Raw, ArtifactState::Validated) => true,

        _ => false,
    }
}

fn transition_state(
    artifact: &mut ArtifactBase,
    to: ArtifactState,
) -> Result<(), LifecycleError> {
    let from = artifact.state;
    let kind = artifact.kind;

    if can_transition(kind, from, to) {
        artifact.state = to;
        Ok(())
    } else {
        Err(LifecycleError::InvalidTransition {
            kind,
            from,
            to,
        })
    }
}

pub fn validate_artifact(
    artifact: &mut ArtifactBase,
) -> Result<(), LifecycleError> {
    transition_state(artifact, ArtifactState::Validated)
}

pub fn reject_artifact(
    artifact: &mut ArtifactBase,
) -> Result<(), LifecycleError> {
    transition_state(artifact, ArtifactState::Rejected)
}

pub fn approve_artifact(
    artifact: &mut ArtifactBase,
) -> Result<(), LifecycleError> {
    transition_state(artifact, ArtifactState::Approved)
}

pub fn execute_artifact(
    artifact: &mut ArtifactBase,
) -> Result<(), LifecycleError> {
    transition_state(artifact, ArtifactState::Executed)
}

pub fn fail_artifact(
    artifact: &mut ArtifactBase,
) -> Result<(), LifecycleError> {
    transition_state(artifact, ArtifactState::Failed)
}