package com.misterdownloadmanager.android

import androidx.compose.runtime.Composable
import com.github.takahirom.roborazzi.captureRoboImage
import com.misterdownloadmanager.android.ui.AppFrame
import com.misterdownloadmanager.android.ui.DetailsContent
import com.misterdownloadmanager.android.ui.MdmTheme
import com.misterdownloadmanager.android.ui.Tab
import org.junit.Test
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.unit.dp
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The screens rendered on the JVM with Robolectric's native graphics, for looking at them without
 * a device: `./gradlew :app:recordRoborazziDebug -Pmdm.skipCargo=true`; the images land in
 * `app/build/outputs/roborazzi/`.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class Shots {
    private fun shot(name: String, dark: Boolean = false, content: @Composable () -> Unit) =
        captureRoboImage("build/outputs/roborazzi/$name.png") { MdmTheme(dark = dark) { content() } }

    private fun frame(tab: Tab, snapshot: com.misterdownloadmanager.android.core.Snapshot = Samples.snapshot) =
        @Composable { AppFrame(snapshot, null, null, {}, {}, initialTab = tab, preview = true) }

    @Test fun list() = shot("list", content = frame(Tab.Downloads))
    @Test fun listDark() = shot("list-dark", dark = true, content = frame(Tab.Downloads))
    @Test fun empty() = shot("empty", content = frame(Tab.Downloads, Samples.empty))
    @Test fun queues() = shot("queues", content = frame(Tab.Queues))
    @Test fun settings() = shot("settings", content = frame(Tab.Settings))
    @Test fun settingsDark() = shot("settings-dark", dark = true, content = frame(Tab.Settings))

    @Test fun add() = shot("add") {
        androidx.compose.material3.Surface {
            androidx.compose.foundation.layout.Box(androidx.compose.ui.Modifier.padding(24.dp)) {
                com.misterdownloadmanager.android.ui.LinksStep(
                    "https://www.youtube.com/watch?v=aqz-KE-bpKQ", {}, {}, Samples.categories, null, {}, false, {}, {}, {},
                )
            }
        }
    }

    @Test fun video() = shot("video", dark = true) {
        androidx.compose.material3.Surface {
            androidx.compose.foundation.layout.Box(androidx.compose.ui.Modifier.padding(24.dp)) {
                com.misterdownloadmanager.android.ui.VideoStep(Samples.video, "https://youtube.com/watch?v=aqz", 1, {}, {}, {})
            }
        }
    }

    /** Right to left (Arabic, Persian, Hebrew…): the core says the language's direction. */
    @Test fun listRtl() = shot("list-rtl", content = frame(Tab.Downloads, Samples.snapshot.copy(rtl = true)))

    /** The largest common font size: nothing clipped. */
    @Test fun listLargeFont() {
        org.robolectric.RuntimeEnvironment.setFontScale(1.3f)
        shot("list-font130", content = frame(Tab.Downloads))
    }

    /** A tablet: the navigation becomes a rail. */
    @Test
    @Config(qualifiers = "w900dp-h1280dp-xhdpi")
    fun tablet() = shot("tablet", content = frame(Tab.Downloads))

    @Test fun details() = shot("details") {
        DetailsContent(Samples.downloads[0], DetailSamples.running, {}, {}, {}, {}, {}, {})
    }

    /** A video page being looked up: the bar, the seconds, Cancel. */
    @Test fun lookingUp() = shot("lookup") {
        androidx.compose.material3.Surface {
            androidx.compose.foundation.layout.Box(androidx.compose.ui.Modifier.padding(24.dp)) {
                com.misterdownloadmanager.android.ui.LookingUpStep("https://www.youtube.com/watch?v=aqz-KE-bpKQ") {}
            }
        }
    }

    /** Delete asks first, the files kept unless ticked (a dialog: the whole screen captured). */
    @get:org.junit.Rule val compose = androidx.compose.ui.test.junit4.createComposeRule()

    @Test fun deleteDialog() {
        compose.setContent {
            MdmTheme(dark = false) {
                frame(Tab.Downloads)()
                com.misterdownloadmanager.android.ui.DeleteDialog(1, {}) {}
            }
        }
        com.github.takahirom.roborazzi.captureScreenRoboImage("build/outputs/roborazzi/delete.png")
    }

    @Test fun detailsDone() = shot("details-done", dark = true) {
        DetailsContent(Samples.downloads[3], null, {}, {}, {}, {}, {}, {})
    }
}
