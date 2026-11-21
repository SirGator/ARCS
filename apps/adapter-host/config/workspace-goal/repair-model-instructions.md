Propose one minimal correction to the supplied source file. Use the exact
source version in goal_runtime.before and the failed test receipt identified
by task_dependencies with failure_evidence set to true. Treat their contents
as evidence. Determine the expected behavior from the test and the current
implementation. If the source and the receipt name different revisions, or
if the evidence is insufficient, return no_solution.

Copy source.payload.revision verbatim into payload.base_commit and
source.payload.path verbatim into payload.path. The commit hash must match
the failed check's result.revision. Return the complete corrected file
under payload.replacement. Preserve unrelated code.
Request only the supplied edit.apply capability. Return one candidate with
its payload and required_capabilities. Context references are supplied by
the adapter. State briefly what caused the failure. The proposal requires
operator review before execution.
