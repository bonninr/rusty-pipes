use anyhow::Result;
use rfd::FileDialog;
use std::path::PathBuf;

/// Shows a native file picker dialog to select an organ file.
/// This runs *before* the main eframe loop.
pub fn run_gui_file_picker_loop() -> Result<Option<PathBuf>> {
    log::info!("No organ file provided. Opening file picker...");
    
    // This creates a native OS file dialog, not an egui window.
    let file = FileDialog::new()
        .set_title("Select an Organ Definition File")
        .add_filter("Organ Files", &["organ", "Organ_Hauptwerk_xml"])
        .set_directory("/")
        .pick_file(); // This is a blocking call

    match file {
        Some(path) => {
            log::info!("File selected: {}", path.display());
            Ok(Some(path))
        }
        None => {
            log::info!("File selection cancelled.");
            Ok(None)
        }
    }
}