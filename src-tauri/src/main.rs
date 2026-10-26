#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if whispertube_lib::run_qwen_worker() {
        return;
    }
    whispertube_lib::run();
}
