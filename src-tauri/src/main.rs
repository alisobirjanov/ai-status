// A release build is a GUI program: no console window behind the panel.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|argument| argument == "--json") {
        pulse_lib::print_json();
        return;
    }
    pulse_lib::run()
}
