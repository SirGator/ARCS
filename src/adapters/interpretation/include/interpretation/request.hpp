#pragma once

#include <optional>
#include <string>

#include <nlohmann/json.hpp>

namespace arcs::interpretation {

// Vollstaendiger Auftrag an die externe Interpretation.
// Alles, was der Parser braucht, steht in einem Request-Objekt.
struct InterpretationRequest {
    // Vom Aufrufer vergebene Korrelations-ID fuer Logs und Rueckfragen.
    std::string request_id;
    // Der ungeparste Nutztext, der interpretiert werden soll.
    std::string raw_input;
    // Ziel-Schema, das die Antwort einhalten soll.
    std::string schema_id;
    // Das Schema selbst wird inline mitgesendet, damit der Parser ohne extra Lookup arbeiten kann.
    nlohmann::json schema;
    // Laufzeitkontext fuer die Interpretation, z.B. Sprache, Zeitzone und aktuelle Zeit.
    nlohmann::json context;
    // Steuerung des Prompt-/Parser-Verhaltens, z.B. Modus oder Temperatur.
    nlohmann::json prompt_config;
};

} // namespace arcs::interpretation
