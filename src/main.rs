use skagit_flats::app::{AppOptions, run};
use skagit_flats::config::load_config;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let opts = AppOptions::from_args(std::env::args().collect());

    let config = load_config(&opts.config_path).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    run(opts, config);
}
