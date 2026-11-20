//! BitChord: the app, run by the desktop binary (`main.rs`) or the Android activity
//! (`android/rust`).


mod appicon;
mod apple;
mod artist_signature;
mod fullscreen;
mod proxy;
mod renderer;
mod art;
mod audio;
mod backend;
mod bidi;
mod deeplink;
mod discord;
mod dash;
mod download_queue;
mod downloads;
mod eq;
mod hires;
mod images;
mod jam;
mod innertube;
mod jsc;
mod lastfm;
mod local;
mod login;
mod lyrics;
mod media;
mod model;
mod motion;
pub mod platform;
mod opus;
mod preset;
mod shader;
mod sleep;
mod stats;
mod spatial;
mod sponsorblock;
mod stream;
mod theme;
mod transfer;
mod translate;
// Android has no tray icon nor webview windows: stand-ins with the same API.
#[cfg_attr(target_os = "android", path = "tray_android.rs")]
mod tray;
#[cfg_attr(target_os = "android", path = "turnstile_android.rs")]
mod turnstile;
mod ui;
mod update;
mod webmusic;
mod ytdlp;
mod ytm;
mod vault;
mod ytmapi;

use std::sync::{Arc, OnceLock};

use crossbeam_channel::unbounded;

/// Window size on first launch, and the smallest it may be (points).
pub const DEFAULT_SIZE: [f32; 2] = [1200.0, 780.0];
pub const MIN_SIZE: [f32; 2] = [760.0, 480.0];

/// Desktop: when this process was started as a sign-in or Turnstile window
/// (`--login <dir>`, see login.rs / turnstile.rs), run that window; true once it has.
#[cfg(not(target_os = "android"))]
pub fn run_child_window() -> bool {
    let mut args = std::env::args_os().skip(1);
    if let (Some(flag), Some(dir)) = (args.next(), args.next()) {
        // The parent hands its proxy setting down through the environment.
        proxy::init(std::path::Path::new(""));
        if flag == login::FLAG {
            login::child_main(login::Site::Google, dir.into());
            return true;
        }
        if flag == login::SPOTIFY_FLAG {
            login::child_main(login::Site::Spotify, dir.into());
            return true;
        }
        if flag == turnstile::FLAG {
            turnstile::child_main(dir.into());
            return true;
        }
    }
    false
}

/// Open the app's window and run until it closes. `configure` adjusts the window
/// options first (Android hands over its activity there).
pub fn run_with(configure: impl FnOnce(&mut eframe::NativeOptions)) -> eframe::Result {
    // Everything below keeps its state in the data folder: without one, say so and stop.
    if let Err(e) = try_data_dir() {
        return Err(eframe::Error::AppCreation(e.into()));
    }
    // A party link (`bitchord://party/CODE` or the https invite page): hand it to the running
    // instance if there is one, else join once this one is up. Before `init_logging`,
    // which would truncate the running instance's log file.
    if let Some(link) = deeplink::from_args() {
        if deeplink::forward_to_running(&link.url()) {
            return Ok(());
        }
        deeplink::push(link);
    } else if deeplink::forward_show() {
        return Ok(());
    }
    // Android logs to logcat, set up by the activity's crate.
    #[cfg(not(target_os = "android"))]
    init_logging();
    update::clean_portable();
    stream::init_dev_hooks();
    proxy::init(&data_dir());

    // Dev hook: YTM_BACKGROUND opens the window off-screen, unfocused and off the
    // taskbar, so automated test runs don't pop up over whatever the user is doing.
    let background = std::env::var_os("YTM_BACKGROUND").is_some();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("BitChord")
        .with_app_id("bitchord")
        .with_inner_size(DEFAULT_SIZE)
        // The phone layout (forced on a desktop to preview it) fits narrow windows.
        .with_min_inner_size(if platform::phone() { [240.0, 320.0] } else { MIN_SIZE })
        // Windows: the app draws its own title bar (see `ui::titlebar`).
        .with_decorations(!ui::custom_titlebar())
        .with_icon(appicon::get(appicon::load(&data_dir())).icon_data().expect("bundled icons are valid PNGs"));
    if background {
        viewport = viewport.with_position([-4000.0, 0.0]).with_inner_size([1920.0, 1057.0]).with_active(false).with_taskbar(false);
    }
    let renderer = renderer::init(&data_dir());
    let mut options = eframe::NativeOptions {
        viewport,
        renderer: match renderer {
            renderer::Renderer::OpenGl => eframe::Renderer::Glow,
            renderer::Renderer::Wgpu => eframe::Renderer::Wgpu,
        },
        persist_window: !background,
        // Settings live with the rest of the data: beside the portable exe, or a
        // YTM_DATA_DIR profile (e.g. a clean signed-out one for screenshots).
        persistence_path: Some(data_dir().join("app.ron")),
        // No MSAA/depth/stencil: egui doesn't need them and they cost VRAM.
        multisampling: 0,
        depth_buffer: 0,
        stencil_buffer: 0,
        // Dev hook: YTM_FPS paces frames itself (see `dev_pace`), so vsync is off.
        glow_options: eframe::egui_glow::GlowConfiguration { vsync: std::env::var_os("YTM_FPS").is_none(), ..Default::default() },
        wgpu_options: renderer::wgpu_options(),
        ..Default::default()
    };
    configure(&mut options);

    let run = eframe::run_native(
        "BitChord",
        options,
        Box::new(|cc| {
            let ctx = cc.egui_ctx.clone();
            theme::install_style(&ctx);
            theme::install_fonts(&ctx, "inter");

            // Background threads wake the UI through this; set once the context exists.
            static CTX: OnceLock<egui::Context> = OnceLock::new();
            let _ = CTX.set(ctx.clone());
            let wake = || {
                if let Some(c) = CTX.get() {
                    c.request_repaint();
                }
            };

            // Party links from later launches arrive over a pipe (first instance only).
            deeplink::listen(wake);
            // Keep `bitchord:` links pointing at this copy; dev builds leave it alone
            // (even with YTM_UPDATE_FORCE).
            if update::installed() && !cfg!(debug_assertions) {
                deeplink::register_protocol();
            }

            let volume = cc
                .storage
                .and_then(|s| s.get_string("volume"))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.8f32);
            // The slider is logarithmic unless turned off. A volume saved before that
            // existed was linear: move it to where it sounds the same.
            let log_setting = cc.storage.and_then(|s| s.get_string("log_volume"));
            let log_volume = log_setting.as_deref().is_none_or(|v| v == "1");
            let volume = if log_setting.is_none() { volume.cbrt() } else { volume };
            // Dev hook: YTM_MUTE starts silent and never persists the volume.
            let volume = if std::env::var_os("YTM_MUTE").is_some() { 0.0 } else { volume };

            let device = cc.storage.and_then(|s| s.get_string("output_device")).filter(|d| !d.is_empty());

            let (audio_tx, audio_rx) = unbounded();
            let audio = audio::spawn(volume, log_volume, device, audio_tx, wake);

            let data_dir = data_dir();
            let (resp_tx, resp_rx) = unbounded();
            let backend = backend::Backend::new(data_dir, resp_tx, audio.clone(), Arc::new(wake))?;

            egui_extras::install_image_loaders(&ctx);
            ctx.add_bytes_loader(Arc::new(images::HttpImages::new(backend.clone())));

            let (media_tx, media_rx) = unbounded();
            if let Some(h) = hwnd(cc) {
                appicon::set_hwnd(h);
                fullscreen::set_hwnd(h);
                ui::set_titlebar_hwnd(h);
                // eframe sets only the small icon; the taskbar button wants the big one.
                appicon::set_window_icon(appicon::get(appicon::load(backend.data_dir())));
            }
            let media = media::Media::new(hwnd(cc), media_tx, wake);

            // Which GL driver this is, for reports of drawing problems (Windows on ARM runs
            // GL through Mesa's D3D12 layer).
            if let Some(gl) = &cc.gl {
                use eframe::glow::HasContext;
                // SAFETY: plain string queries on the current context.
                let (renderer, version) = unsafe { (gl.get_parameter_string(eframe::glow::RENDERER), gl.get_parameter_string(eframe::glow::VERSION)) };
                log::info!("gl: {renderer} ({version})");
            }
            if let Some(rs) = &cc.wgpu_render_state {
                let info = rs.adapter.get_info();
                log::info!("wgpu: {} ({:?}, driver {} {})", info.name, info.backend, info.driver, info.driver_info);
            }
            // The animated background needs the GPU context; without it the UI falls back
            // to a flat gradient.
            let shader = std::env::var_os("YTM_NO_SHADER").is_none();
            let bg = match (&cc.gl, &cc.wgpu_render_state) {
                (Some(gl), _) if shader => shader::Background::new(gl).map_err(|e| log::warn!("background shader unavailable: {e:#}")).ok(),
                (_, Some(rs)) if shader => Some(shader::Background::new_wgpu(rs)),
                _ => None,
            };

            let app = ui::App::new(backend, audio, ui::Channels { resp_rx, audio_rx, media_rx }, media, bg, cc.storage);
            // Android: the music is kept going from a second thread while the app is away.
            #[cfg(target_os = "android")]
            return Ok(Box::new(ui::Shared::new(app)));
            #[cfg(not(target_os = "android"))]
            Ok(Box::new(app))
        }),
    );
    // A wgpu window that won't open would fail the same way at every launch: go back
    // to OpenGL for the next one.
    if run.is_err() && renderer == renderer::Renderer::Wgpu {
        log::error!("renderer: {} failed to start; switching back to OpenGL", renderer::api_name());
        let _ = renderer::save(&data_dir(), renderer::Renderer::OpenGl);
    }
    run
}

#[cfg(windows)]
fn hwnd(cc: &eframe::CreationContext) -> Option<*mut std::ffi::c_void> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as *mut std::ffi::c_void),
        _ => None,
    }
}

#[cfg(not(windows))]
fn hwnd(_: &eframe::CreationContext) -> Option<*mut std::ffi::c_void> {
    None
}

/// Log to stderr in debug builds; release builds have no console, so log to a file
/// next to the settings (overwritten each launch).
#[cfg(not(target_os = "android"))]
fn init_logging() {
    let mut builder = env_logger::Builder::from_env(
        // A test build can be made more talkative: YTM_DEFAULT_LOG=warn,bitchord=debug cargo build.
        env_logger::Env::default().default_filter_or(option_env!("YTM_DEFAULT_LOG").unwrap_or("warn,bitchord=info")),
    );
    // Dev hook: YTM_LOG=<path> logs there (test runs beside a running copy).
    if let Some(path) = std::env::var_os("YTM_LOG") {
        if let Ok(file) = std::fs::File::create(path) {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
    } else if !cfg!(debug_assertions) {
        let dir = data_dir();
        let _ = std::fs::create_dir_all(&dir);
        // The last run's log too: reopening the app after a problem would otherwise wipe it.
        let _ = std::fs::rename(dir.join("ytm-rs.log"), dir.join("ytm-rs.prev.log"));
        if let Ok(file) = std::fs::File::create(dir.join("ytm-rs.log")) {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
    }
    builder.init();
}

/// Settings, caches and the sign-in: `data` beside the portable build's exe, else the
/// user's app data. YTM_DATA_DIR comes first: a dev profile, or on Android the app's
/// own storage (which has no home directory to find it from).
fn try_data_dir() -> anyhow::Result<std::path::PathBuf> {
    resolve_data_dir(
        std::env::var_os("YTM_DATA_DIR").map(Into::into),
        update::portable_dir().map(|dir| dir.join("data")),
        directories::ProjectDirs::from("", "", "ytm-rs").map(|d| d.data_dir().to_path_buf()),
    )
}

/// The first data folder there is, from the most specific source to the default.
fn resolve_data_dir(env: Option<std::path::PathBuf>, portable: Option<std::path::PathBuf>, default: Option<std::path::PathBuf>) -> anyhow::Result<std::path::PathBuf> {
    env.or(portable).or(default).ok_or_else(|| anyhow::anyhow!("no data folder: there's no home directory and YTM_DATA_DIR isn't set"))
}

/// The data folder; `run_with` has checked there is one before anything asks.
fn data_dir() -> std::path::PathBuf {
    try_data_dir().expect("the data folder was found at startup")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_without_any_source_is_an_error_not_a_panic() {
        assert!(resolve_data_dir(None, None, None).is_err());
    }

    #[test]
    fn data_dir_env_wins() {
        let d = resolve_data_dir(Some("/x".into()), Some("/p".into()), Some("/d".into())).unwrap();
        assert_eq!(d, std::path::PathBuf::from("/x"));
    }
}
