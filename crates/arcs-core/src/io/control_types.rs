/// Diese Artefakte dürfen nur durch die dafür zuständigen Core-Dienste
/// entstehen, niemals als frei gelieferte Transform- oder Output-Rückgabe.
pub(crate) fn is_control_type(artifact_type: &str) -> bool {
    matches!(
        artifact_type,
        "action"
            | "approval"
            | "verification_report"
            | "invocation"
            | "execution_result"
            | "outcome"
            | "reasoning_request"
            | "reasoning_result"
            | "output_request"
    )
}
