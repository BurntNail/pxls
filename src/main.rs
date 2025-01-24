use std::env::args;
use crate::cli::cli_main;
use crate::gui::gui_main;

mod cli;
mod logic;
mod gui;

fn main () {
    let args: Vec<String> = args().skip(1).collect();
    if args.is_empty() {
        gui_main();
    } else {
        let mut should_ask = false;
        if args.len() == 1 {
            let first = args[0].to_lowercase();
            if ["--help", "-help", "-h", "--h", "help", "h", "?", "-?"].contains(&first.as_str()) {
                eprintln!("usage: pxls [input_file] [chunks_per_dimension] [closeness_threshold] [distance_algo] [output_file] [output_virtual_pixel_size] [dithering_factor]\nor usage: pxls ask");
                std::process::exit(1);
            } else if ["a", "-a", "--a", "ask", "-ask", "--ask"].contains(&first.as_str()) {
                should_ask = true;
            }
        }

        if let Err(e) = cli_main(should_ask) {
            eprintln!("Error w/ CLI: {e:?}");
        }
    }

}

