/**
 * @file cli_output_adapter.hpp
 * @brief Declares a minimal output adapter that writes plain text to a
 *        CLI stream.
 */

#pragma once

#include <ostream>
#include <string>

namespace arcs::adapters::output {

/**
 * @brief Simple output adapter that writes text directly to a stream,
 *        used for delivering output to the command-line interface.
 */
class CliTextOutputAdapter {
public:
    /**
     * @brief Writes the given text to the stream, ensuring it ends with a newline.
     * @param out The output stream to write to.
     * @param text The text to write.
     */
    void write(std::ostream& out, const std::string& text) const;
};

} // namespace arcs::adapters::output
