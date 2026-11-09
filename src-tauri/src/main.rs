// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if ai_route_manager_lib::run_managed_repair_if_requested() {
        return;
    }
    if ai_route_manager_lib::run_background_if_requested() {
        return;
    }
    ai_route_manager_lib::run()
}
