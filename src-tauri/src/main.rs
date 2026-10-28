#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Codex launches this executable as the Canvas MCP server.
    if args.iter().any(|a| a == "--mcp-canvas") {
        match canvas_assistant_lib::mcp::data_dir_from_args(&args) {
            Some(dir) => canvas_assistant_lib::mcp::run_stdio(dir),
            None => std::process::exit(2),
        }
        return;
    }
    canvas_assistant_lib::run();
}
