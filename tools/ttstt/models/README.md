# Local Kokoro AOT inputs

This ignored directory is the canonical local home of the two large inputs
shared by the TRUEOS Kokoro AOT tools and the preserved `trueos-ttstt` host
utility:

| Relative path | Bytes | SHA-256 |
| --- | ---: | --- |
| `kokoro/kokoro-rten.onnx` | 124,604,222 | `239d9f4df112a375bea52146570b97eb5c5af727c007761ee121ed123fd1ab29` |
| `kokoro/voices-v1.0.bin` | 28,214,398 | `bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d` |

The prepared RTen graph is generated from the Kokoro ConvInteger model by
`tools/trueos-ttstt/tools/prepare_kokoro_rten.py`. The voice archive comes
from the Kokoro ONNX model-files v1.0 release documented in
`tools/trueos-ttstt/README.md`. Model assets retain their upstream licenses and
are deliberately not committed to the TRUEOS source repository.

Verify the local files before AOT compilation:

```sh
sha256sum tools/ttstt/models/kokoro/kokoro-rten.onnx \
  tools/ttstt/models/kokoro/voices-v1.0.bin
```
