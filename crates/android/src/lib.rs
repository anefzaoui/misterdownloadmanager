//! `libmdm.so`: Mister Download Manager on Android. The Android project (android/) runs it in a
//! GameActivity (`MainActivity`), whose native thread starts here, in [`android_main`]: the app's
//! folders are set, HTTPS is given Android's certificate checks, the helper programs the APK
//! carries are unpacked, and then the app runs as on the desktop (`mdm_app::android::run`).

#![cfg(target_os = "android")]
// `android_main` is an exported C function and the activity's folders come through JNI: this
// crate is Android's FFI, as win32.rs is Windows'.
#![allow(unsafe_code)]

use std::path::PathBuf;

use egui_winit::winit::platform::android::activity::AndroidApp;
use jni::objects::{JObject, JString, JValue};
use jni::{Env, JavaVM, jni_sig, jni_str};

/// The activity's native thread (android-activity calls it once the activity is created): the
/// app until the activity ends.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("mdm"),
    );
    let folders = match folders(&app) {
        Ok(f) => f,
        Err(e) => {
            log::error!("the activity's folders: {e}");
            return;
        }
    };
    // The list, settings and logs: the app's private files.
    mdm_ipc::set_data_dir(folders.data.clone());
    // Downloads: the phone's Download folder, MDM's own folder in it.
    if let Some(public) = &folders.downloads {
        mdm_app::android::set_downloads_dir(public.join("MisterDownloadManager"));
    }
    // yt-dlp's Python, ffmpeg, QuickJS and 7-Zip from the APK, unpacked on a thread of their own.
    mdm_tools::android::init(&folders.native_lib, &folders.data, &folders.cache);
    if let Err(e) = mdm_app::android::run(app) {
        log::error!("the app stopped: {e}");
    }
}

/// Where the app keeps and finds things.
struct Folders {
    /// `getFilesDir()/mdm`: the list, settings, logs, the helpers once unpacked.
    data: PathBuf,
    /// `getCacheDir()`: temporary files.
    cache: PathBuf,
    /// `getApplicationInfo().nativeLibraryDir`: the programs the APK carries.
    native_lib: PathBuf,
    /// The phone's Download folder (`Environment.getExternalStoragePublicDirectory`).
    downloads: Option<PathBuf>,
}

/// The activity's folders, and HTTPS given Android's own certificate checks (and the user's
/// CAs) while the activity is at hand.
fn folders(app: &AndroidApp) -> Result<Folders, String> {
    let data = app
        .internal_data_path()
        .ok_or("no internal data folder")?
        .join("mdm");
    // SAFETY: android-activity's JavaVM and its activity, a global reference that outlives this.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread(|env| -> jni::errors::Result<Result<Folders, String>> {
        // SAFETY: the activity's global reference, valid while the activity lives; not deleted.
        let activity = unsafe { JObject::from_raw(env, app.activity_as_ptr().cast()) };
        let native_lib = native_library_dir(env, &activity)?;
        let cache = cache_dir(env, &activity)?;
        let downloads = public_downloads(env).ok();
        if let Err(e) = rustls_platform_verifier::android::init_with_env(env, activity) {
            return Ok(Err(format!("certificate checks: {e}")));
        }
        Ok(Ok(Folders {
            data,
            cache,
            native_lib,
            downloads,
        }))
    })
    .map_err(|e| e.to_string())?
}

/// `context.getApplicationInfo().nativeLibraryDir`.
fn native_library_dir(env: &mut Env, context: &JObject) -> jni::errors::Result<PathBuf> {
    let info = env
        .call_method(
            context,
            jni_str!("getApplicationInfo"),
            jni_sig!("()Landroid/content/pm/ApplicationInfo;"),
            &[],
        )?
        .l()?;
    let dir = env
        .get_field(
            &info,
            jni_str!("nativeLibraryDir"),
            jni_sig!("Ljava/lang/String;"),
        )?
        .l()?;
    let dir = env.cast_local::<JString>(dir)?;
    Ok(PathBuf::from(dir.try_to_string(env)?))
}

/// `Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)`.
fn public_downloads(env: &mut Env) -> jni::errors::Result<PathBuf> {
    let kind = env.new_string("Download")?;
    let dir = env
        .call_static_method(
            jni_str!("android/os/Environment"),
            jni_str!("getExternalStoragePublicDirectory"),
            jni_sig!("(Ljava/lang/String;)Ljava/io/File;"),
            &[JValue::Object(&kind)],
        )?
        .l()?;
    let path = env
        .call_method(
            &dir,
            jni_str!("getAbsolutePath"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    let path = env.cast_local::<JString>(path)?;
    Ok(PathBuf::from(path.try_to_string(env)?))
}

/// `context.getCacheDir().getAbsolutePath()`.
fn cache_dir(env: &mut Env, context: &JObject) -> jni::errors::Result<PathBuf> {
    let dir = env
        .call_method(
            context,
            jni_str!("getCacheDir"),
            jni_sig!("()Ljava/io/File;"),
            &[],
        )?
        .l()?;
    let path = env
        .call_method(
            &dir,
            jni_str!("getAbsolutePath"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    let path = env.cast_local::<JString>(path)?;
    Ok(PathBuf::from(path.try_to_string(env)?))
}

/// `MainActivity.nativeBack()`: the system's back gesture or button reached the activity's back
/// dispatcher (Android 13 and later send no key for it); the app closes the window in front.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_misterdownloadmanager_android_MainActivity_nativeBack(
    _env: *mut jni::sys::JNIEnv,
    _this: jni::sys::jobject,
) {
    mdm_app::android::back_pressed();
}

/// `MainActivity.nativeEnding()`: the activity is being destroyed. Its native side then waits for
/// `android_main` to return, so the app closes first (`mdm_app::android::activity_ending`).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_misterdownloadmanager_android_MainActivity_nativeEnding(
    _env: *mut jni::sys::JNIEnv,
    _this: jni::sys::jobject,
) {
    mdm_app::android::activity_ending();
}

/// `MainActivity.nativeHost(platform)`: Kotlin's `Platform` (the keystore, exact alarms, wake
/// locks and the downloads' foreground service), given to the app as its host once per process.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_misterdownloadmanager_android_MainActivity_nativeHost<'local>(
    mut env: jni::EnvUnowned<'local>,
    _this: JObject<'local>,
    platform: JObject<'local>,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let host = JavaHost {
            vm: env.get_java_vm()?,
            platform: env.new_global_ref(&platform)?,
        };
        mdm_app::android::set_host(std::sync::Arc::new(host));
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// The Kotlin `Platform` (android/…/Platform.kt), called from the app's threads.
struct JavaHost {
    vm: JavaVM,
    platform: jni::refs::Global<JObject<'static>>,
}

impl JavaHost {
    /// `platform.call(what, a, b, c, d)`, Platform's one entry: it answers `ok:<value>`, `none`
    /// (or null), or `err:<why>`.
    fn call(&self, what: &str, args: [Option<&str>; 4]) -> Result<Option<String>, String> {
        let text = self
            .vm
            .attach_current_thread(|env| -> jni::errors::Result<Option<String>> {
                let what = env.new_string(what)?;
                let mut strings = Vec::new();
                for a in args {
                    strings.push(match a {
                        Some(a) => env.new_string(a)?,
                        None => JString::default(),
                    });
                }
                let out = env
                    .call_method(
                        &self.platform,
                        jni_str!("call"),
                        jni_sig!(
                            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"
                        ),
                        &[
                            JValue::Object(&what),
                            JValue::Object(&strings[0]),
                            JValue::Object(&strings[1]),
                            JValue::Object(&strings[2]),
                            JValue::Object(&strings[3]),
                        ],
                    )?
                    .l()?;
                if out.is_null() {
                    return Ok(None);
                }
                let out = env.cast_local::<JString>(out)?;
                Ok(Some(out.try_to_string(env)?))
            })
            .map_err(|e| e.to_string())?;
        match text.as_deref() {
            None | Some("none") => Ok(None),
            Some(t) => match t.split_once(':') {
                Some(("ok", value)) => Ok(Some(value.to_owned())),
                Some(("err", why)) => Err(why.to_owned()),
                _ => Err(format!("unexpected answer: {t}")),
            },
        }
    }
}

impl mdm_app::android::Host for JavaHost {
    fn keychain(
        &self,
        op: &str,
        service: &str,
        account: &str,
        secret: Option<&str>,
    ) -> Result<Option<String>, String> {
        self.call("keychain", [Some(op), Some(service), Some(account), secret])
    }

    fn wake_at(&self, at: Option<i64>) -> Result<(), String> {
        let at = at.map(|t| t.to_string());
        self.call("wakeAt", [at.as_deref(), None, None, None])
            .map(|_| ())
    }

    fn keep_awake(&self, reason: Option<&str>) -> bool {
        self.call("keepAwake", [reason, None, None, None]).is_ok()
    }

    fn to_back(&self) -> bool {
        self.call("toBack", [None; 4]).is_ok()
    }

    fn downloads(&self, json: &str) {
        if let Err(e) = self.call("downloads", [Some(json), None, None, None]) {
            log::warn!("the downloads' notifications: {e}");
        }
    }

    fn pick(&self, kind: &str, dir: Option<&str>) -> Option<String> {
        match self.call("pick", [Some(kind), dir, None, None]) {
            Ok(path) => path,
            Err(e) => {
                log::warn!("the picker: {e}");
                None
            }
        }
    }

    fn clipboard(&self, op: &str, text: Option<&str>) -> Result<Option<String>, String> {
        self.call("clipboard", [Some(op), text, None, None])
    }

    fn status(&self, line: &str, stop: &str) {
        if let Err(e) = self.call("status", [Some(line), Some(stop), None, None]) {
            log::warn!("the downloads' notification: {e}");
        }
    }

    fn open(&self, kind: &str, target: &str) -> Result<(), String> {
        self.call("open", [Some(kind), Some(target), None, None])
            .map(|_| ())
    }
}

/// `MainActivity.nativeEditorAction()`: the soft keyboard's action key (Done, Go), which the
/// app takes as Enter (`mdm_app::android::editor_action`).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_misterdownloadmanager_android_MainActivity_nativeEditorAction(
    _env: *mut jni::sys::JNIEnv,
    _this: jni::sys::jobject,
) {
    mdm_app::android::editor_action();
}

/// `MainActivity.nativeLinks(text)`: text another app handed MDM (Share, or a link opened with
/// it); its links go to the app ([`mdm_app::android::links_given`]).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_misterdownloadmanager_android_MainActivity_nativeLinks<'local>(
    mut env: jni::EnvUnowned<'local>,
    _this: JObject<'local>,
    text: JString<'local>,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let text = text.try_to_string(env)?;
        mdm_app::android::links_given(&text);
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// `DownloadService.nativeStopAll()`: the downloads' notification's Stop All.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_misterdownloadmanager_android_DownloadService_nativeStopAll(
    _env: *mut jni::sys::JNIEnv,
    _this: jni::sys::jobject,
) {
    mdm_app::android::stop_all_asked();
}

/// `DownloadService.nativeStop(id)`: a download's own notification's Stop.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_misterdownloadmanager_android_DownloadService_nativeStop(
    _env: *mut jni::sys::JNIEnv,
    _this: jni::sys::jobject,
    id: jni::sys::jlong,
) {
    mdm_app::android::stop_asked(id as u64);
}

/// `MainActivity.nativeFile(path)`: a `.torrent` another app handed MDM, copied into the app's
/// cache ([`mdm_app::android::file_given`]).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_misterdownloadmanager_android_MainActivity_nativeFile<'local>(
    mut env: jni::EnvUnowned<'local>,
    _this: JObject<'local>,
    path: JString<'local>,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let path = path.try_to_string(env)?;
        mdm_app::android::file_given(&path);
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
