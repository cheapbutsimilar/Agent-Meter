//! Local smoke test: cargo run -p agentmeter-antigravity --example probe -- <agy.exe>
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    if agentmeter_antigravity::capture_if_requested() {
        return;
    }
    let exe = std::env::args_os()
        .nth(1)
        .expect("path to official agy.exe");
    let home = std::env::var_os("USERPROFILE").expect("USERPROFILE");
    match agentmeter_antigravity::collect(std::path::Path::new(&exe), std::path::Path::new(&home)) {
        Ok(s) => println!("{}", serde_json::to_string_pretty(&s).unwrap()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
