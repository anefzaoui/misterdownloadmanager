# rustls-platform-verifier's Kotlin side, called from Rust.
-keep, includedescriptorclasses class org.rustls.platformverifier.** { *; }
# GameActivity's Java side, called from libmdm.so (android-activity).
-keep class com.google.androidgamesdk.** { *; }
-keep class com.misterdownloadmanager.android.MainActivity { *; }
-keepclasseswithmembernames class * { native <methods>; }
