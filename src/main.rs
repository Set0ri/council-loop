//! `council`: the command line, the MCP server and the lock hook of the Council Loop.

fn main() {
    std::process::exit(council_loop::cli::main());
}
