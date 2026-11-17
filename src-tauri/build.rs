// Every command the frontend may call is listed here. tauri-build turns each
// into an `allow-<name>` permission, and capabilities/*.json grant them per
// window: a command that is not granted cannot be called at all.
const COMMANDS: &[&str] = &[
    "app_info",
    "get_settings",
    "set_settings",
    "vault_status",
    "select_database",
    "choose_database",
    "choose_key_file",
    "unlock",
    "lock",
    "forget_recent",
    "list_groups",
    "list_entries",
    "search_entries",
    "get_entry",
    "get_custom_icons",
    "reveal_field",
    "copy_field",
    "get_totp",
    "open_url",
    "show_quick_search",
    "hide_quick_search",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri-build failed");
}
