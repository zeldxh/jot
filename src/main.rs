// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod fonts;
mod highlight;
mod icon;
mod md;
mod text_util;

use eframe::egui;
use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup};
use eframe::wgpu;

fn main() -> eframe::Result {
    let file = std::env::args_os().nth(1).map(std::path::PathBuf::from);

    // DirectX 12 only: skips loading the Vulkan drivers, which saves memory and startup time.
    let mut wgpu_options = WgpuConfiguration::default();
    if let WgpuSetup::CreateNew(setup) = &mut wgpu_options.wgpu_setup {
        setup.instance_descriptor.backends = wgpu::Backends::DX12;
    }

    let options = eframe::NativeOptions {
        wgpu_options,
        viewport: egui::ViewportBuilder::default()
            .with_title("jot")
            .with_inner_size([900.0, 640.0])
            .with_min_inner_size([320.0, 200.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_drag_and_drop(true)
            .with_icon(icon::window_icon()),
        ..Default::default()
    };
    eframe::run_native("jot", options, Box::new(move |cc| Ok(Box::new(app::Jot::new(cc, file.clone())))))
}
