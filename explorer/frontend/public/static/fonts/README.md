To add new glyphs to the Inter fallback font:

1. Download and install [FontForge](https://fontforge.org/en-US/downloads/mac-dl/).
2. Download the [Inter](https://rsms.me/inter/) typefaces.
3. In FontForge, open the Inter Regular typeface from the web directory.
4. Find the desired glyph (e.g., a missing symbol or character).
5. Copy the glyph (Edit > Copy or Cmd+C).
6. Open the `Inter-fallback.sfd` file in FontForge.
7. Go to the correct Unicode slot for the glyph (use Encoding > Goto or right-click > Goto, then enter the Unicode value).
8. Paste the glyph into the slot (Edit > Paste or Cmd+V).
9. Generate a new .ttf file
10. To use [pyftsubset](https://fonttools.readthedocs.io/en/latest/subset/index.html) tool run `brew install fonttools`
11. Adjust the range of included glyphs and run the script 
    ```bash
    pyftsubset <path-to-ttf-file> \
    --output-file=./public/static/fonts/Inter-fallback.woff2 \
    --flavor=woff2 \
    --unicodes=U+2192
    ```