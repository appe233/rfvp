mod script;
mod subsystem;
mod app;
mod utils;
mod rendering;
mod config;
mod window;
mod audio_player;
mod debug_ui;
mod vm_worker;
mod rfvp_render;
mod rfvp_audio;
mod vm_runner;
mod trace;
mod font;
mod boot;
mod legacy_save_load_ui;
mod exit_confirm_ui;

pub(crate) mod platform_time;

use std::path::PathBuf;
use std::sync::Arc;

use script::parser::{Nls, Parser};
use script::string_patch::StringPatchTable;
use subsystem::{anzu_scene::AnzuScene, resources::thread_manager::ThreadManager};

use crate::app::App;
use crate::utils::file::{app_base_path, set_base_path};
use anyhow::{Context, Result};
use boot::{app_config, load_script};
use log::LevelFilter;


/// Parse `--project-dir <path>` or `--project-dir=<path>` from argv.
fn parse_project_dir_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if let Some(val) = a.strip_prefix("--project-dir=") {
            if !val.is_empty() {
                return Some(val.to_string());
            }
        } else if a == "--project-dir" {
            if let Some(val) = args.get(i + 1) {
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
        i += 1;
    }
    None
}

/// Parse `--nls <value>` or `--nls=<value>` from argv, default to ShiftJIS.
fn parse_nls_arg() -> Nls {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if let Some(val) = a.strip_prefix("--nls=") {
            return val.parse().unwrap_or_else(|e| {
                eprintln!("rfvp: {e}");
                std::process::exit(1);
            });
        } else if a == "--nls" {
            if let Some(val) = args.get(i + 1) {
                return val.parse().unwrap_or_else(|e| {
                    eprintln!("rfvp: {e}");
                    std::process::exit(1);
                });
            } else {
                eprintln!("rfvp: --nls requires a value (sjis, gbk, utf8)");
                std::process::exit(1);
            }
        }
        i += 1;
    }
    Nls::ShiftJIS
}

/// Parse `--system-font`; when present, system-wide CJK fallback fonts are scanned.
fn parse_system_font_arg() -> bool {
    std::env::args().skip(1).any(|a| a == "--system-font")
}

/// Parse `--sakura-moyu-chs-patch`; this explicitly enables the Chinese patch.
fn parse_sakura_moyu_chs_patch_arg() -> bool {
    std::env::args()
        .skip(1)
        .any(|a| a == "--sakura-moyu-chs-patch")
}

fn require_sakura_moyu_chs_patch_files() -> Result<(PathBuf, PathBuf)> {
    let base_path = app_base_path().get_path().clone();
    let patch_dat_path = base_path.join("patch.dat");
    let patch_bin_path = base_path.join("patch.bin");

    if !patch_dat_path.is_file() {
        anyhow::bail!(
            "--sakura-moyu-chs-patch requires patch.dat under {}",
            base_path.display()
        );
    }
    if !patch_bin_path.is_file() {
        anyhow::bail!(
            "--sakura-moyu-chs-patch requires patch.bin under {}",
            base_path.display()
        );
    }

    Ok((patch_dat_path, patch_bin_path))
}

// use dhat;

// #[global_allocator]
// static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() -> Result<()> {
    // let _profiler = dhat::Profiler::new_heap();
    // env_logger::init();
    if let Some(project_dir) = parse_project_dir_arg() {
        set_base_path(&project_dir);
    }
    let nls = parse_nls_arg();
    let system_font = parse_system_font_arg();
    let sakura_moyu_chs_patch = parse_sakura_moyu_chs_patch_arg();
    let patch_paths = if sakura_moyu_chs_patch {
        Some(require_sakura_moyu_chs_patch_files()?)
    } else {
        None
    };
    let parser = if let Some((patch_dat_path, _)) = &patch_paths {
        let string_patch = Arc::new(
            StringPatchTable::from_path(patch_dat_path)
                .with_context(|| format!("load {}", patch_dat_path.display()))?,
        );
        log::info!(
            "loaded Sakura Moyu Chinese string patch: {} entries",
            string_patch.len()
        );
        load_script(nls)?.with_string_patch(string_patch)
    } else {
        load_script(nls)?
    };
    let title  = parser.get_title();
    let size = parser.get_screen_size();
    let script_engine = ThreadManager::new();

    let app = App::app_with_config(app_config(&title, size))
        .with_scene::<AnzuScene>()
        .with_script_engine(script_engine)
        .with_window_title(&title)
        .with_window_size(size)
        .with_parser(parser);
    let app = if let Some((_, patch_bin_path)) = &patch_paths {
        app.with_sakura_moyu_chs_patch_vfs(nls, patch_bin_path)?
    } else {
        app.with_vfs(nls)?
    };
    let app = if system_font {
        app.with_system_font(true)
    } else {
        app
    };
    app.run();

    // handle.shutdown();
    
    Ok(())
}


// test
mod tests {
    use std::{thread::sleep, time::Duration};
    use super::*;
    use crate::subsystem::world::GameData;

    #[test]
    fn test_audio_system() {
        std::env::set_var("FVP_TEST", "1");
        let mut world = GameData::default();
        let vfs=  crate::subsystem::resources::vfs::Vfs::new(Nls::ShiftJIS).unwrap();
        let buff = vfs.read_file("bgm/001").unwrap();
        // is oggs?
        assert!(&buff[0..4] == [0x4fu8, 0x67u8, 0x67u8, 0x53u8].as_slice(), "BGM file is not OGG format");
        crate::trace::vm(format_args!("BGM data size: {}", buff.len()));
        world.bgm_player_mut().load(0, buff).unwrap();
        let mut fade_in = kira::Tween {
            duration: Duration::from_secs(0),
            ..Default::default()
        };
        fade_in.duration = Duration::from_secs(0);
        world.bgm_player_mut().play(0, true, 1.0, 0.5, fade_in, &vfs).unwrap();
        sleep(Duration::from_secs(20));
    }

    #[test]
    fn test_audio_system_mix() {
        std::env::set_var("FVP_TEST", "1");
        let mut world = GameData::default();
        let vfs=  crate::subsystem::resources::vfs::Vfs::new(Nls::ShiftJIS).unwrap();
        let buff = vfs.read_file("bgm/001").unwrap();
        let buff2 = vfs.read_file("bgm/002").unwrap();
        // is oggs?
        assert!(&buff[0..4] == [0x4fu8, 0x67u8, 0x67u8, 0x53u8].as_slice(), "BGM file is not OGG format");
        assert!(&buff2[0..4] == [0x4fu8, 0x67u8, 0x67u8, 0x53u8].as_slice(), "BGM file is not OGG format");
        crate::trace::vm(format_args!("BGM data size: {}", buff.len()));
        world.bgm_player_mut().load(0, buff).unwrap();
        let mut fade_in = kira::Tween {
            duration: Duration::from_secs(0),
            ..Default::default()
        };
        world.bgm_player_mut().load(1, buff2).unwrap();
        fade_in.duration = Duration::from_secs(0);
        world.bgm_player_mut().play(0, true, 1.0, 0.5, fade_in, &vfs).unwrap();
        world.bgm_player_mut().play(1, true, 1.0, 0.5, fade_in, &vfs).unwrap();
        sleep(Duration::from_secs(20));
    }
}
