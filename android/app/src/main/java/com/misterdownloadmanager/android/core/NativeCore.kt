package com.misterdownloadmanager.android.core

import android.content.Context

/**
 * libmdm.so (crates/android): the app's core, the same Rust code as the desktop app's, running
 * without a window. Requests and answers are JSON (crates/app/src/android_ui.rs).
 */
object NativeCore {
    init {
        System.loadLibrary("mdm")
    }

    /** Start the core; null once running, else why it couldn't start. */
    @JvmStatic
    external fun start(context: Context, config: String, host: NativeHost): String?

    /** One request (`{"op": …}`), answered in JSON. Blocks until the core answers. */
    @JvmStatic
    external fun request(json: String): String

    /** Stop every download where it is and save the list. */
    @JvmStatic
    external fun stop()
}

/** What the core calls back, from its own threads. */
interface NativeHost {
    /** The list changed: a snapshot (JSON). */
    fun changed(snapshot: String)

    /** A platform call (`{"fn": "keychain" | "wake_at" | "keep_awake", …}`): `{"value": …}` or `{"error": …}`. */
    fun call(json: String): String
}
