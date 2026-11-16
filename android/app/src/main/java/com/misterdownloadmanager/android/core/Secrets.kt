package com.misterdownloadmanager.android.core

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The core's keychain on Android (crates/app/src/android.rs `keychain_*`): sign-ins encrypted
 * with an AES key that never leaves Android's keystore, kept in the app's private preferences.
 * Entries are by service and account, as on the desktop keychains.
 */
class Secrets(context: Context) {
    private val prefs = context.getSharedPreferences("mdm-secrets", Context.MODE_PRIVATE)

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return gen.generateKey()
    }

    private fun name(service: String, account: String) = "$service\u0000$account"

    fun read(service: String, account: String): String? {
        val stored = prefs.getString(name(service, account), null) ?: return null
        val bytes = Base64.decode(stored, Base64.NO_WRAP)
        if (bytes.size < IV) return null
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes, 0, IV))
        return String(cipher.doFinal(bytes, IV, bytes.size - IV), Charsets.UTF_8)
    }

    fun write(service: String, account: String, secret: String) {
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val sealed = cipher.iv + cipher.doFinal(secret.toByteArray(Charsets.UTF_8))
        prefs.edit().putString(name(service, account), Base64.encodeToString(sealed, Base64.NO_WRAP)).apply()
    }

    fun delete(service: String, account: String) {
        prefs.edit().remove(name(service, account)).apply()
    }

    fun list(service: String): List<String> =
        prefs.all.keys.filter { it.startsWith("$service\u0000") }.map { it.substringAfter('\u0000') }

    private companion object {
        const val ALIAS = "mdm-secrets"
        const val TRANSFORM = "AES/GCM/NoPadding"
        const val IV = 12
    }
}
