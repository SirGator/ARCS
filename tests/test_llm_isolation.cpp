#include <cassert>
#include <iostream>
#include <string>

namespace arcs {

// Minimaler Modell-Output für den Test
struct ModelGeneratedOptionCandidate {
  std::string source_class;   // "model"
  std::string trust_level;    // "low"
  std::string schema_id;      // absichtlich falsch
  bool schema_valid;
};

// ARCS-Regel:
// LLM-Output darf nur als Vorschlag rein.
// Wenn Schema ungültig ist, darf daraus nichts Executables entstehen.
bool should_reject_llm_candidate(const ModelGeneratedOptionCandidate& candidate) {
  if (candidate.source_class != "model") {
    return false;
  }

  if (candidate.trust_level != "low") {
    return true;
  }

  if (!candidate.schema_valid) {
    return true;
  }

  if (candidate.schema_id != "arcs.option.v1") {
    return true;
  }

  return false;
}

} // namespace arcs

int main() {
  using namespace arcs;

  ModelGeneratedOptionCandidate invalid_llm_output{
      .source_class = "model",
      .trust_level = "low",
      .schema_id = "arcs.option.v999",   // absichtlich falsch
      .schema_valid = false              // absichtlich ungültig
  };

  const bool rejected = should_reject_llm_candidate(invalid_llm_output);

  assert(rejected);

  std::cout << "[PASS] LLM isolation: invalid model output rejected before execution\n";
  return 0;
}