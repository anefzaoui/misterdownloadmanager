# JNI: libmdm.so calls these by name.
-keep class com.misterdownloadmanager.android.core.NativeCore { *; }
-keep interface com.misterdownloadmanager.android.core.NativeHost { *; }
-keep class * implements com.misterdownloadmanager.android.core.NativeHost { *; }
# rustls-platform-verifier's Kotlin side, called from Rust.
-keep, includedescriptorclasses class org.rustls.platformverifier.** { *; }
# kotlinx.serialization
-keepattributes *Annotation*, InnerClasses
-keepclassmembers class com.misterdownloadmanager.android.** {
    *** Companion;
    kotlinx.serialization.KSerializer serializer(...);
}
