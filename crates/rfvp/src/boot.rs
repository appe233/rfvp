use crate::{
    app::App,
    config::{
        app_config::{AppConfig, AppConfigBuilder},
        logger_config::LoggerConfig,
        window_config::WindowConfigBuilder,
    },
    utils::file::app_base_path,
};
use anyhow::Result;
use log::LevelFilter;

use crate::script::parser::{Nls, Parser};
use crate::subsystem::{anzu_scene::AnzuScene, resources::thread_manager::ThreadManager};

pub fn app_config(title: &str, size: (u32, u32)) -> AppConfig {
    AppConfigBuilder::new()
        .with_app_name(title.to_string())
        .with_logger_config(LoggerConfig {
            app_level_filter: LevelFilter::Info,
            level_filter: LevelFilter::Debug,
        })
        .with_window_config(
            WindowConfigBuilder::new()
                .with_dimensions(size)
                .with_resizable(false)
                .get(),
        )
        .get()
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub fn prepare_app(
    root: &std::path::Path,
    nls: Nls,
    options: &crate::preparation::LaunchOptions,
) -> Result<crate::app::AppBuilder> {
    let prepared = crate::preparation::prepare_game(
        root,
        nls,
        crate::preparation::ScriptDiscovery::PreferBch,
        options,
    )?;
    let title = prepared.parser.get_title();
    let size = prepared.parser.get_screen_size();
    Ok(App::app_with_prepared(app_config(&title, size), prepared)?
        .with_scene::<AnzuScene>()
        .with_script_engine(ThreadManager::new())
        .with_window_title(&title)
        .with_window_size(size))
}

pub fn load_script(nls: Nls) -> Result<Parser> {
    let base_path = app_base_path();
    let opcode_path = App::find_hcb(base_path.get_path())?;
    if let Some(parent) = opcode_path.parent() {
        if let Some(parent) = parent.to_str() {
            crate::utils::file::set_base_path(parent);
            crate::utils::file::set_hcb_root_path(parent);
        }
    }

    Parser::new(opcode_path, nls)
}
