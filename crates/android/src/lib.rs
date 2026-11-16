//! `libmdm.so`: the JNI side of Mister Download Manager for Android. The Android app (android/,
//! `NativeCore.kt`) starts the core with its folders and a `NativeHost`, sends it JSON requests,
//! and gets snapshots back through the host. Everything else is `mdm_app::android`.
//!
//! Java side (package `com.misterdownloadmanager.android.core`):
//!
//! ```kotlin
//! object NativeCore {
//!     @JvmStatic external fun start(context: Context, config: String, host: NativeHost): String?
//!     @JvmStatic external fun request(json: String): String
//!     @JvmStatic external fun stop()
//! }
//! interface NativeHost {
//!     fun changed(snapshot: String)
//!     fun call(json: String): String
//! }
//! ```

#![cfg(target_os = "android")]
// The JNI entry points are exported C functions: this crate is the Android FFI, as win32.rs is
// Windows' (D-008).
#![allow(unsafe_code)]

use std::sync::{Arc, Mutex};

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JObject, JString, JValue};
use jni::refs::Global;
use jni::{Env, EnvUnowned, JavaVM, jni_sig, jni_str};
use mdm_app::android::{Config, Core, Host};
use serde_json::{Value, json};

/// The running core (one per process).
static CORE: Mutex<Option<Core>> = Mutex::new(None);

/// The Kotlin `NativeHost`, called from the core's threads.
struct JavaHost {
    vm: JavaVM,
    host: Global<JObject<'static>>,
}

impl JavaHost {
    /// `host.call(json)`: the platform calls, answered in JSON.
    fn call(&self, request: &Value) -> Result<Value, String> {
        let text = request.to_string();
        let answer = self
            .vm
            .attach_current_thread(|env| -> jni::errors::Result<String> {
                let arg = env.new_string(&text)?;
                let out = env
                    .call_method(
                        &self.host,
                        jni_str!("call"),
                        jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                        &[JValue::Object(&arg)],
                    )?
                    .l()?;
                let out = env.cast_local::<JString>(out)?;
                out.try_to_string(env)
            })
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&answer).map_err(|e| e.to_string())
    }
}

/// `{"ok": …, "error": …}` → the value or the error.
fn result(v: Result<Value, String>) -> Result<Value, String> {
    let v = v?;
    match v.get("error").and_then(Value::as_str) {
        Some(e) => Err(e.to_owned()),
        None => Ok(v.get("value").cloned().unwrap_or(Value::Null)),
    }
}

impl Host for JavaHost {
    fn changed(&self, snapshot: &str) {
        let _ = self
            .vm
            .attach_current_thread(|env| -> jni::errors::Result<()> {
                let arg = env.new_string(snapshot)?;
                env.call_method(
                    &self.host,
                    jni_str!("changed"),
                    jni_sig!("(Ljava/lang/String;)V"),
                    &[JValue::Object(&arg)],
                )?;
                Ok(())
            });
    }

    fn keychain(
        &self,
        op: &str,
        service: &str,
        account: &str,
        secret: Option<&str>,
    ) -> Result<Option<String>, String> {
        let v = result(self.call(&json!({
            "fn": "keychain",
            "op": op,
            "service": service,
            "account": account,
            "secret": secret,
        })))?;
        Ok(v.as_str().map(str::to_owned))
    }

    fn wake_at(&self, at: Option<i64>) -> Result<(), String> {
        result(self.call(&json!({ "fn": "wake_at", "at": at }))).map(|_| ())
    }

    fn keep_awake(&self, reason: Option<&str>) -> bool {
        result(self.call(&json!({ "fn": "keep_awake", "reason": reason })))
            .is_ok_and(|v| v.as_bool().unwrap_or(true))
    }
}

fn start(env: &mut Env, context: &JObject, config: &JString, host: &JObject) -> Result<(), String> {
    let mut core = CORE.lock().unwrap_or_else(|e| e.into_inner());
    if core.is_some() {
        return Ok(());
    }
    // HTTPS through Android's own certificate checks (and the user's CAs).
    let ctx = env.new_local_ref(context).map_err(|e| e.to_string())?;
    rustls_platform_verifier::android::init_with_env(env, ctx)
        .map_err(|e| format!("certificate checks: {e}"))?;
    let config = config.try_to_string(env).map_err(|e| e.to_string())?;
    let config: Config = serde_json::from_str(&config).map_err(|e| e.to_string())?;
    let host = JavaHost {
        vm: env.get_java_vm().map_err(|e| e.to_string())?,
        host: env.new_global_ref(host).map_err(|e| e.to_string())?,
    };
    *core = Some(Core::start(config, Arc::new(host))?);
    Ok(())
}

/// `NativeCore.start(context, config, host)`: null once started, else why it couldn't.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_misterdownloadmanager_android_core_NativeCore_start<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    context: JObject<'local>,
    config: JString<'local>,
    host: JObject<'local>,
) -> JString<'local> {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("mdm"),
    );
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        match start(env, &context, &config, &host) {
            Ok(()) => Ok(JString::default()),
            Err(e) => {
                log::error!("start: {e}");
                env.new_string(e)
            }
        }
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// `NativeCore.request(json)`: the core's JSON answer.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_misterdownloadmanager_android_core_NativeCore_request<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    json: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let json = json.try_to_string(env)?;
        let answer = match CORE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            Some(core) => core.request(&json),
            None => mdm_app::android::error("not-started", "the core isn't running"),
        };
        env.new_string(answer)
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// `NativeCore.stop()`: downloads stop where they are and the list is saved.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_misterdownloadmanager_android_core_NativeCore_stop<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
) {
    env.with_env(|_env| -> jni::errors::Result<()> {
        let core = CORE.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(core) = core {
            core.stop();
        }
        Ok(())
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}
