#pragma once

#include <optional>
#include <string>

#include <nlohmann/json.hpp>

namespace arcs::interpretation {

// Einheitliche Antwort des Parsers.
// `payload` enthaelt das strukturierte Proposal, `error` nur bei Problemen.
struct InterpretationResponse {
    // Signalisiert, ob die Anfrage technisch und fachlich akzeptiert wurde.
    bool ok{false};
    // Rueckgabe der Anfrage-ID, falls der Parser sie mitsendet.
    std::string request_id;
    // Echo des verwendeten Schemas, damit Aufrufer Response und Request koppeln koennen.
    std::string schema_id;
    // Strukturierte Interpretation als JSON-Objekt.
    nlohmann::json payload;
    // Fehlermeldung bei nicht erfolgreicher Verarbeitung.
    std::optional<std::string> error;
};

} // namespace arcs::interpretation
