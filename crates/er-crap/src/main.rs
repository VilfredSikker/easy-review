use clap::Parser;

use er_crap::cli::Cli;

fn main() {
    let cli = Cli::parse();
    let opts = er_crap::Opts::from(cli);
    match er_crap::run(&opts) {
        Ok(outcome) => {
            print!("{}", outcome.report);
            std::process::exit(outcome.exit_code);
        }
        Err(err) => {
            eprintln!("er-crap: {err:#}");
            std::process::exit(2);
        }
    }
}
