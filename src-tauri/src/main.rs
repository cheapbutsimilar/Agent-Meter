// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if agentmeter_antigravity::capture_if_requested() {
        return;
    }
    agentmeter_lib::run();
}
