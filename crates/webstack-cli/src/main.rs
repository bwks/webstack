use std::process::ExitCode;

const HELP: &str = "Webstack application framework

Usage: webstack <COMMAND>

Commands:
  new       Generate an independent Webstack application
  dev       Run the application and asset watcher
  assets    Set up or build frontend assets
  generate  Generate application source files
  doctor    Check the development and runtime environment
  help      Print this message
";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None | Some("help" | "--help" | "-h") => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Some(command) => {
            eprintln!("webstack {command}: not implemented yet");
            ExitCode::from(2)
        }
    }
}
