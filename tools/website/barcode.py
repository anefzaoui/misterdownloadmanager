"""Code 128 (set B) as inline SVG: the release's barcode on the site's waybill labels. A real
barcode, so a phone's scanner reads what it says (the version), not a decoration drawn to look
like one."""
from __future__ import annotations

# The 107 symbols: bar, space, bar, space, bar, space widths in modules (the stop symbol has a
# seventh, a final bar). ISO/IEC 15417.
PATTERNS = (
    "212222 222122 222221 121223 121322 131222 122213 122312 132212 221213 221312 231212 112232 "
    "122132 122231 113222 123122 123221 223211 221132 221231 213212 223112 312131 311222 321122 "
    "321221 312212 322112 322211 212123 212321 232121 111323 131123 131321 112313 132113 132311 "
    "211313 231113 231311 112133 112331 132131 113123 113321 133121 313121 211331 231131 213113 "
    "213311 213131 311123 311321 331121 312113 312311 332111 314111 221411 431111 111224 111422 "
    "121124 121421 141122 141221 112214 112412 122114 122411 142112 142211 241211 221114 413111 "
    "241112 134111 111242 121142 121241 114212 124112 124211 411212 421112 421211 212141 214121 "
    "412121 111143 111341 131141 114113 114311 411113 411311 113141 114131 311141 411131 211412 "
    "211214 211232 2331112"
).split()
START_B, STOP = 104, 106
assert len(PATTERNS) == 107 and all(sum(map(int, p)) == 11 for p in PATTERNS[:106])


def symbols(text: str) -> list[int]:
    """Start B, the characters, the checksum, stop."""
    if any(not 32 <= ord(c) <= 126 for c in text):
        raise ValueError(f"Code 128 B takes printable ASCII only: {text!r}")
    values = [ord(c) - 32 for c in text]
    check = (START_B + sum((i + 1) * v for i, v in enumerate(values))) % 103
    return [START_B, *values, check, STOP]


def svg(text: str, height: int = 48, css_class: str = "barcode") -> str:
    """The barcode for `text`, one unit per module, with the quiet zones the standard asks for
    (10 modules each side); it scales with its width. Bars take currentColor."""
    x, bars = 10, []
    for s in symbols(text):
        for k, w in enumerate(map(int, PATTERNS[s])):
            if k % 2 == 0:
                bars.append(f"M{x} 0h{w}v{height}h-{w}z")
            x += w
    width = x + 10
    return (f'<svg class="{css_class}" viewBox="0 0 {width} {height}" preserveAspectRatio="none" '
            f'role="img" aria-label="{text}"><path fill="currentColor" d="{"".join(bars)}"/></svg>')
