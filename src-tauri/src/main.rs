//! Binary entry — delegates to the library `run()`.
//!
//! Built-in search/fetch adapters (including PubMed) run in-process; Omiga has
//! no bundled stdio servers in this binary.

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("mcp") {
        eprintln!("No bundled Omiga MCP stdio servers are available.");
        std::process::exit(2);
    }

    omiga_lib::run();
}
