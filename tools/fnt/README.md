# TRUEOS font assets

TRUEOS does not bundle JuliaMono. To enable the preferred Shell2 terminal
face, download the unmodified `JuliaMono-Regular.ttf` from the
[`t4ce/juliamono`](https://github.com/t4ce/juliamono) repository and place it
on the mounted TrueOSFS root at:

```text
fonts/JuliaMono-Regular.ttf
```

The background font worker validates the TTF with Skrifa and publishes it as
face 4 (`julia-mono`). Until that publication completes, Shell2 resolves face
4 to the embedded Inconsolata face 3. The fallback is selected before glyph
recipes and OceanCache keys are made, so the two faces never share cached
outlines.

Lucida Sans Unicode remains the embedded general typography face, and Noto
Sans SC remains the optional CJK face at `fonts/NotoSansSC[wght].ttf`.
