package com.misterdownloadmanager.android.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import com.misterdownloadmanager.android.R

/*
 * MDM's world (DESIGN.md, "The Waybill on the Parcel") through Material 3's theming: the logo's
 * blue board under the screen's title, thermal-paper sheets with black ink on it, and the logo's
 * green only for the download action and for what has arrived. One face, Archivo, condensed by
 * its width axis for titles and field names, normal width for reading; numbers in tabular figures.
 */

/** The colours outside Material's roles: the board and the green. */
@Immutable
data class Brand(
    /** The header behind a screen's title (the logo's blue; a navy depot in dark). */
    val board: Color,
    val onBoard: Color,
    /** Secondary text on the board. */
    val onBoard2: Color,
    /** The board's own hairline, and a raised control on it. */
    val boardEdge: Color,
    /** The download action (Add): the logo's green, with its dark ink. */
    val go: Color,
    val onGo: Color,
    /** Finished: a delivered green that reads on paper. */
    val stamp: Color,
)

private val LightBrand = Brand(
    board = Color(0xFF1F63D6),
    onBoard = Color.White,
    onBoard2 = Color(0xFFE6EEFD),
    boardEdge = Color(0xFF3B78E0),
    go = Color(0xFF57CC5A),
    onGo = Color(0xFF08200B),
    stamp = Color(0xFF1B7A26),
)

private val DarkBrand = Brand(
    board = Color(0xFF0C1A33),
    onBoard = Color(0xFFF2F5FB),
    onBoard2 = Color(0xFFC3CDE0),
    boardEdge = Color(0xFF1C2E52),
    go = Color(0xFF57CC5A),
    onGo = Color(0xFF08200B),
    stamp = Color(0xFF6BDB6E),
)

val LocalBrand = staticCompositionLocalOf { LightBrand }

/** The brand's colours where Material's roles don't reach. */
val brand: Brand @Composable get() = LocalBrand.current

private val Light: ColorScheme = lightColorScheme(
    primary = Color(0xFF1F63D6),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFDCE7FC),
    onPrimaryContainer = Color(0xFF0A2A66),
    secondary = Color(0xFF434956),
    onSecondary = Color.White,
    secondaryContainer = Color(0xFFE4E8EF),
    onSecondaryContainer = Color(0xFF111318),
    tertiary = Color(0xFF1B7A26),
    onTertiary = Color.White,
    tertiaryContainer = Color(0xFFD9F2DA),
    onTertiaryContainer = Color(0xFF08200B),
    background = Color(0xFFFCFCFB),
    onBackground = Color(0xFF111318),
    surface = Color(0xFFFCFCFB),
    onSurface = Color(0xFF111318),
    surfaceVariant = Color(0xFFF0F3F7),
    onSurfaceVariant = Color(0xFF5F6676),
    surfaceContainerLowest = Color.White,
    surfaceContainerLow = Color(0xFFF7F8FA),
    surfaceContainer = Color(0xFFF0F3F7),
    surfaceContainerHigh = Color(0xFFE9EDF3),
    surfaceContainerHighest = Color(0xFFE2E7EF),
    outline = Color(0xFF8A919F),
    outlineVariant = Color(0xFFDDE1E8),
    inverseSurface = Color(0xFF111318),
    inverseOnSurface = Color(0xFFFCFCFB),
    inversePrimary = Color(0xFF86B6FF),
)

private val Dark: ColorScheme = darkColorScheme(
    primary = Color(0xFF86B6FF),
    onPrimary = Color(0xFF0A2A66),
    primaryContainer = Color(0xFF1C3A73),
    onPrimaryContainer = Color(0xFFDCE7FC),
    secondary = Color(0xFFC3CAD6),
    onSecondary = Color(0xFF11151D),
    secondaryContainer = Color(0xFF2A3242),
    onSecondaryContainer = Color(0xFFEEF1F6),
    tertiary = Color(0xFF6BDB6E),
    onTertiary = Color(0xFF08200B),
    tertiaryContainer = Color(0xFF16401C),
    onTertiaryContainer = Color(0xFFC9F5CA),
    background = Color(0xFF11151D),
    onBackground = Color(0xFFEEF1F6),
    surface = Color(0xFF11151D),
    onSurface = Color(0xFFEEF1F6),
    surfaceVariant = Color(0xFF192030),
    onSurfaceVariant = Color(0xFF98A1B1),
    surfaceContainerLowest = Color(0xFF0C1018),
    surfaceContainerLow = Color(0xFF151A24),
    surfaceContainer = Color(0xFF192030),
    surfaceContainerHigh = Color(0xFF1F2738),
    surfaceContainerHighest = Color(0xFF263042),
    outline = Color(0xFF6E7787),
    outlineVariant = Color(0xFF2A3242),
    inverseSurface = Color(0xFFEEF1F6),
    inverseOnSurface = Color(0xFF11151D),
    inversePrimary = Color(0xFF1F63D6),
)

/** Archivo at a weight and a width (100 normal; the brand condenses titles to 68-80). */
@OptIn(androidx.compose.ui.text.ExperimentalTextApi::class)
private fun archivo(weight: Int, width: Float = 100f) = Font(
    R.font.archivo,
    weight = FontWeight(weight),
    variationSettings = FontVariation.Settings(FontVariation.weight(weight), FontVariation.width(width)),
)

/** Body text: Archivo at its normal width. */
val Archivo = FontFamily(archivo(400), archivo(500), archivo(600), archivo(650), archivo(700))

/** Titles and field names: Archivo condensed, heavy (the label's voice). */
val ArchivoCondensed = FontFamily(
    archivo(700, 76f), archivo(750, 74f), archivo(800, 78f), archivo(850, 72f), archivo(900, 68f),
)

private const val TNUM = "tnum"

private val Type = Typography(
    displayLarge = TextStyle(fontFamily = ArchivoCondensed, fontWeight = FontWeight(900), fontSize = 64.sp, lineHeight = 60.sp, letterSpacing = (-0.01).em, fontFeatureSettings = TNUM),
    displayMedium = TextStyle(fontFamily = ArchivoCondensed, fontWeight = FontWeight(900), fontSize = 52.sp, lineHeight = 50.sp, fontFeatureSettings = TNUM),
    displaySmall = TextStyle(fontFamily = ArchivoCondensed, fontWeight = FontWeight(850), fontSize = 40.sp, lineHeight = 40.sp, fontFeatureSettings = TNUM),
    headlineLarge = TextStyle(fontFamily = ArchivoCondensed, fontWeight = FontWeight(850), fontSize = 36.sp, lineHeight = 38.sp),
    headlineMedium = TextStyle(fontFamily = ArchivoCondensed, fontWeight = FontWeight(850), fontSize = 30.sp, lineHeight = 32.sp),
    headlineSmall = TextStyle(fontFamily = ArchivoCondensed, fontWeight = FontWeight(800), fontSize = 25.sp, lineHeight = 28.sp),
    titleLarge = TextStyle(fontFamily = ArchivoCondensed, fontWeight = FontWeight(800), fontSize = 22.sp, lineHeight = 26.sp),
    titleMedium = TextStyle(fontFamily = Archivo, fontWeight = FontWeight(650), fontSize = 16.sp, lineHeight = 21.sp),
    titleSmall = TextStyle(fontFamily = Archivo, fontWeight = FontWeight(650), fontSize = 14.sp, lineHeight = 19.sp),
    bodyLarge = TextStyle(fontFamily = Archivo, fontWeight = FontWeight(400), fontSize = 16.sp, lineHeight = 24.sp),
    bodyMedium = TextStyle(fontFamily = Archivo, fontWeight = FontWeight(400), fontSize = 14.sp, lineHeight = 20.sp),
    bodySmall = TextStyle(fontFamily = Archivo, fontWeight = FontWeight(400), fontSize = 12.5.sp, lineHeight = 17.sp, fontFeatureSettings = TNUM),
    labelLarge = TextStyle(fontFamily = Archivo, fontWeight = FontWeight(650), fontSize = 14.sp, lineHeight = 20.sp),
    labelMedium = TextStyle(fontFamily = ArchivoCondensed, fontWeight = FontWeight(750), fontSize = 12.sp, lineHeight = 16.sp, letterSpacing = 0.045.em),
    labelSmall = TextStyle(fontFamily = ArchivoCondensed, fontWeight = FontWeight(750), fontSize = 11.sp, lineHeight = 14.sp, letterSpacing = 0.05.em),
)

/** A field's value: tabular figures, semibold (DESIGN.md's field value). */
val FieldValue = TextStyle(fontFamily = Archivo, fontWeight = FontWeight(650), fontSize = 16.sp, lineHeight = 21.sp, fontFeatureSettings = TNUM)

/** Numbers that change in place (speeds, sizes, percentages): tabular, so they don't jitter. */
val Numbers = TextStyle(fontFeatureSettings = TNUM)

/** Die-cut label stock: gently rounded. */
private val MdmShapes = Shapes(
    extraSmall = RoundedCornerShape(4.dp),
    small = RoundedCornerShape(6.dp),
    medium = RoundedCornerShape(10.dp),
    large = RoundedCornerShape(12.dp),
    extraLarge = RoundedCornerShape(20.dp),
)

@Composable
fun MdmTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    CompositionLocalProvider(LocalBrand provides if (dark) DarkBrand else LightBrand) {
        MaterialTheme(colorScheme = if (dark) Dark else Light, typography = Type, shapes = MdmShapes, content = content)
    }
}
