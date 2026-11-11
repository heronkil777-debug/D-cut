// SPDX-License-Identifier: AGPL-3.0-or-later

fn main() {
    if let Err(error) = editor_ui::run() {
        eprintln!("D-cut failed to start: {error:#}");
        std::process::exit(1);
    }
}
