use crate::cli::cli_main;

mod cli;
mod logic;

fn main () {
    if let Err(e) = cli_main() {
        eprintln!("Error running CLI Main: {e:?}");
    }
}

