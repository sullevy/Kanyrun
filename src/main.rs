mod actions;
mod app;
mod cli;
mod config;
mod context;
mod daemon;
mod detect;
mod menu;
mod rules;
mod ui;

fn main() {
    if let Err(error) = app::run() {
        eprintln!("kanyrun: {error}");
        std::process::exit(1);
    }
}
