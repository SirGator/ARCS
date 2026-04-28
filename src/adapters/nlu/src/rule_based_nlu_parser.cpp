// src/nlu/rule_based_nlu_parser.cpp

#include "nlu/rule_based_nlu_parser.hpp"

#include <cctype>
#include <algorithm>

namespace arcs::nlu {

static std::string lower(std::string s) {
    std::transform(
        s.begin(),
        s.end(),
        s.begin(),
        [](unsigned char c) { return static_cast<char>(std::tolower(c)); }
    );
    return s;
}

NluResult RuleBasedNluParser::parse(const std::string& text) {
    std::string t = lower(text);

    NluResult result;
    result.raw_text = text;
    result.parser_name = "rule_based_nlu_v1";

    if (t.find("erinnere") != std::string::npos ||
        t.find("erinner") != std::string::npos ||
        t.find("remind") != std::string::npos ||
        t.find("reminder") != std::string::npos) {
        result.status = NluStatus::Parsed;
        result.intent = "create_reminder";
        result.confidence = 0.88;

        if (t.find("morgen") != std::string::npos || t.find("tomorrow") != std::string::npos) {
            result.entities["due_date"] = "tomorrow";
        }

        return result;
    }

    if (t.find("report") != std::string::npos ||
        t.find("bericht") != std::string::npos) {
        result.status = NluStatus::Parsed;
        result.intent = "create_report";
        result.confidence = 0.95;
        result.entities["format"] = "json";

        if (t.find("sensor") != std::string::npos) {
            result.entities["topic"] = "sensor_data";
        }

        return result;
    }

    if (t.find("analysiere") != std::string::npos ||
        t.find("analyse") != std::string::npos) {
        result.status = NluStatus::Parsed;
        result.intent = "analyze";
        result.confidence = 0.80;

        if (t.find("sensor") != std::string::npos) {
            result.entities["topic"] = "sensor_data";
        }

        return result;
    }

    result.status = NluStatus::Unknown;
    result.intent = "unknown";
    result.confidence = 0.0;
    return result;
}

}
