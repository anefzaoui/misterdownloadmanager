package com.misterdownloadmanager.android.ui

import java.text.NumberFormat
import java.util.Locale

/**
 * Sizes, speeds and times as the list shows them, each a left-to-right isolate: in a right-to-left
 * language a number and its unit stay in their order ("41 GB", not "GB 41"), as the desktop's
 * `i18n::ltr` keeps them.
 */
object Format {
    private const val LRI = "\u2066"
    private const val PDI = "\u2069"

    /** `text` kept left to right inside right-to-left text. */
    fun ltr(text: String) = "$LRI$text$PDI"

    /** "done / total", kept in that order. */
    fun of(done: String, total: String) = ltr("$done / $total")

    private val units = listOf("B", "KB", "MB", "GB", "TB")

    fun size(bytes: Long): String = ltr(plainSize(bytes))

    private fun plainSize(bytes: Long): String {
        if (bytes < 1024) return "$bytes B"
        var v = bytes.toDouble()
        var u = 0
        while (v >= 1024 && u < units.lastIndex) {
            v /= 1024
            u++
        }
        val nf = NumberFormat.getNumberInstance(Locale.getDefault()).apply {
            maximumFractionDigits = if (v < 10) 2 else if (v < 100) 1 else 0
        }
        return "${nf.format(v)} ${units[u]}"
    }

    fun speed(bps: Long): String = ltr("${plainSize(bps)}/s")

    fun percent(f: Double): String =
        ltr(NumberFormat.getPercentInstance(Locale.getDefault()).apply { maximumFractionDigits = 1 }.format(f))

    fun eta(seconds: Long): String {
        val h = seconds / 3600
        val m = (seconds % 3600) / 60
        val s = seconds % 60
        return ltr(if (h > 0) "%d:%02d:%02d".format(h, m, s) else "%d:%02d".format(m, s))
    }
}
