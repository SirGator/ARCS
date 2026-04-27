#include <iostream>
#include <string>

#include "core/flow.hpp"
#include "input/input_adapter.hpp"
#include "execution/cli_output_adapter.hpp"

int main()
{
    std::cout << "ARCS CLI\n";
    std::cout << "Enter input like: approval=yes permission=yes\n";
    std::cout << "> " << std::flush;

    arcs::input::CliTextInputAdapter input_adapter;
    const auto input_artifact = input_adapter.read_artifact(std::cin);
    const auto output = arcs::core::run_text_flow(input_artifact);

    arcs::execution::CliTextOutputAdapter output_adapter;
    output_adapter.write(std::cout, output);
    return 0;
}
