//! The library GameActivity loads (`libbitchord_android.so`): it starts BitChord in
//! the activity it is handed.
#![cfg(target_os = "android")]

use std::sync::atomic::{AtomicBool, Ordering};

use winit::platform::android::activity::input::{ImeOptions, InputType, TextInputAction, TextInputState, TextSpan};
use winit::platform::android::activity::AndroidApp;

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    // The desktop's default filter: our own crate at info, everything else warnings.
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("bitchord")
            .with_filter(android_logger::FilterBuilder::new().parse("warn,bitchord=info,bitchord_android=info").build()),
    );
    // Android drops stderr, where Rust reports a panic; and the app keeps running after
    // one on a worker thread (panic = "unwind"), so log every one.
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic on thread {:?}: {info}", std::thread::current().name());
        report(info);
    }));

    // winit makes one event loop per process, and the glue calls this again for every
    // new activity in the same process: that second run could never start. Ending the
    // process lets the next launch start a fresh one.
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        log::warn!("the activity was created again in a running process: exiting so the next launch starts afresh");
        std::process::exit(0);
    }

    // An app has no home directory, and std's temp dir (/data/local/tmp) isn't writable
    // by it: point both at the app's own storage, `files` and `cache` under its folder.
    if let Some(files) = app.internal_data_path() {
        let cache = files.parent().map_or_else(|| files.join("cache"), |app_dir| app_dir.join("cache"));
        let _ = std::fs::create_dir_all(&files);
        let _ = std::fs::create_dir_all(&cache);
        // SAFETY: the first and only run in this process (STARTED above), before the app
        // starts any thread of its own; nothing else reads the environment yet.
        unsafe {
            std::env::set_var("YTM_DATA_DIR", &files);
            std::env::set_var("TMPDIR", &cache);
        }
        log::info!("data in {}, temporary files in {}", files.display(), cache.display());
    } else {
        log::warn!("Android gave no app storage folder");
    }
    // The keyboard's action key (✓) arrives as an editor action, which winit 0.30 drops
    // too; "no action" makes it an Enter key, which the text fields take as "submit".
    app.set_ime_editor_info(InputType::TYPE_CLASS_TEXT, TextInputAction::None, ImeOptions::empty());
    // winit 0.30 drops what the on-screen keyboard types: the app reads the input
    // method's text and follows its changes (see `platform::take_text_edit`).
    let (reader, clearer) = (app.clone(), app.clone());
    bitchord::platform::set_keyboard(
        move || reader.text_input_state().text,
        move || clearer.set_text_input_state(TextInputState { text: String::new(), selection: TextSpan { start: 0, end: 0 }, compose_region: None }),
    );
    match bitchord::run_with(move |options| options.android_app = Some(app)) {
        Ok(()) => log::info!("closed"),
        Err(e) => log::error!("bitchord stopped: {e}"),
    }
    // The app closed itself (Quit, or "Restart now" in Settings): end the process too,
    // since its event loop can't be made again in it (see STARTED).
    std::process::exit(0);
}

// What MainActivity.java reports (JNI: package io.github.aminuddinihsan20.bitchord). The
// environment and class arguments are never used, so they stay untyped.
type Jni = *mut std::ffi::c_void;

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_MainActivity_nativeInsets(_env: Jni, _class: Jni, left: i32, top: i32, right: i32, bottom: i32) {
    bitchord::platform::set_insets_px(left, top, right, bottom);
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_MainActivity_nativeCanGoBack(_env: Jni, _class: Jni) -> u8 {
    u8::from(bitchord::platform::can_go_back())
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_MainActivity_nativeBack(_env: Jni, _class: Jni) {
    bitchord::platform::request_back();
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_MainActivity_nativeEnter(_env: Jni, _class: Jni) {
    bitchord::platform::press_enter();
}

/// The activity polls for a sign-in the app asked for: 1 Google, 2 Spotify, 0 none.
#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_MainActivity_nativeTakeLoginRequest(_env: Jni, _class: Jni) -> i32 {
    bitchord::platform::take_login_request().map_or(0, bitchord::platform::LoginSite::code)
}

/// LoginActivity's answer: `ok` with the cookie in `text`, or not ok with why (or
/// "cancelled") there.
#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_LoginActivity_nativeLoginResult(mut env: jni::JNIEnv, _class: jni::objects::JClass, ok: u8, text: jni::objects::JString) {
    let text: String = env.get_string(&text).map(Into::into).unwrap_or_default();
    bitchord::platform::finish_login(if ok != 0 { Ok(text) } else { Err(text) });
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_MainActivity_nativeSetBackground(_env: Jni, _class: Jni, away: u8) {
    bitchord::platform::set_background(away != 0);
}

fn now_playing_string(env: &jni::JNIEnv) -> jni::sys::jstring {
    env.new_string(bitchord::platform::now_playing_line()).map_or(std::ptr::null_mut(), |s| s.into_raw())
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_MainActivity_nativeNowPlaying(env: jni::JNIEnv, _class: Jni) -> jni::sys::jstring {
    now_playing_string(&env)
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_PlaybackService_nativeNowPlaying(env: jni::JNIEnv, _class: Jni) -> jni::sys::jstring {
    now_playing_string(&env)
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_PlaybackService_nativeMediaCommand(_env: Jni, _class: Jni, code: i32, arg: i64) {
    bitchord::platform::push_media_command(code, arg);
}

/// What the phone is connected through: 0 nothing, 1 a connection that counts the data,
/// 2 one that doesn't (Connectivity.java).
#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_Connectivity_nativeNetwork(_env: Jni, _class: Jni, state: i32) {
    bitchord::platform::set_network(state);
}

/// Where the music goes and the phone's media volume, whenever either changes
/// (AudioOutput.java).
#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_AudioOutput_nativeOutput(mut env: jni::JNIEnv, _class: jni::objects::JClass, line: jni::objects::JString) {
    let line: String = env.get_string(&line).map(Into::into).unwrap_or_default();
    bitchord::playon::set_output_line(&line);
}

/// A car (Android Auto) connected, or the last one left (AutoBrowserService.java).
#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_AutoBrowserService_nativeConnected(_env: Jni, _class: Jni, on: u8) {
    bitchord::auto::set_wanted(on != 0);
}

/// The nodes of the tree a car browses, each with a count of its changes.
#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_AutoBrowserService_nativeVersions(env: jni::JNIEnv, _class: Jni) -> jni::sys::jstring {
    env.new_string(bitchord::auto::versions()).map_or(std::ptr::null_mut(), |s| s.into_raw())
}

/// The rows of one node of that tree.
#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_AutoBrowserService_nativeRows(mut env: jni::JNIEnv, _class: jni::objects::JClass, node: jni::objects::JString) -> jni::sys::jstring {
    let node: String = env.get_string(&node).map(Into::into).unwrap_or_default();
    env.new_string(bitchord::auto::rows(&node)).map_or(std::ptr::null_mut(), |s| s.into_raw())
}

/// A row tapped in the car (PlayerSession.java).
#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_PlayerSession_nativePlay(mut env: jni::JNIEnv, _class: jni::objects::JClass, id: jni::objects::JString) {
    let id: String = env.get_string(&id).map(Into::into).unwrap_or_default();
    bitchord::auto::push_play(&id);
}

/// Asked for by voice in the car (PlayerSession.java); empty: for anything.
#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_PlayerSession_nativeSearch(mut env: jni::JNIEnv, _class: jni::objects::JClass, query: jni::objects::JString) {
    let query: String = env.get_string(&query).map(Into::into).unwrap_or_default();
    bitchord::auto::push_search(&query);
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_MainActivity_nativeTakeCall(env: jni::JNIEnv, _class: Jni) -> jni::sys::jstring {
    let line = bitchord::platform::take_call().map(|c| c.to_line()).unwrap_or_default();
    env.new_string(line).map_or(std::ptr::null_mut(), |s| s.into_raw())
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_aminuddinihsan20_bitchord_MainActivity_nativeAnswer(mut env: jni::JNIEnv, _class: jni::objects::JClass, id: i64, ok: u8, text: jni::objects::JString) {
    let text: String = env.get_string(&text).map(Into::into).unwrap_or_default();
    bitchord::platform::answer_call(id as u64, ok != 0, &text);
}
