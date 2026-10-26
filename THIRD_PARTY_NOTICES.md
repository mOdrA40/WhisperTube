# Third-Party Notices

WhisperTube source code is licensed under the MIT License. The runtime
components, model files, and bundled fonts are separate works and retain their
own licenses and notices.

## Components

- Qwen3-ASR-1.7B model: Apache License 2.0. Official source:
  https://huggingface.co/Qwen/Qwen3-ASR-1.7B. The optional Q8 GGUF is downloaded
  from a pinned handy-computer conversion revision, retaining the original model
  license: https://huggingface.co/handy-computer/Qwen3-ASR-1.7B-gguf.
- transcribe.cpp 0.4.1: MIT License. Source:
  https://github.com/handy-computer/transcribe.cpp/tree/v0.4.1. The optional native
  runtime is downloaded separately; its `licenses` directory is retained in full.
- Qwen native loading uses libloading (ISC); runtime archive decoding uses tar
  and flate2 (MIT OR Apache-2.0). Sources: https://github.com/nagisa/rust_libloading,
  https://github.com/alexcrichton/tar-rs, https://github.com/rust-lang/flate2-rs.

- whisper.cpp: MIT License. Source: https://github.com/ggml-org/whisper.cpp
- Whisper model files: downloaded from the ggerganov/whisper.cpp model
  repository on Hugging Face: https://huggingface.co/ggerganov/whisper.cpp.
  Review the model repository and the applicable terms for the exact model
  files distributed with a build.
- yt-dlp: Unlicense, together with notices and licenses for bundled
  third-party components where applicable. Source and notices:
  https://github.com/yt-dlp/yt-dlp
- FFmpeg: licensing depends on the exact configuration and enabled components.
  Windows uses the pinned Gyan.dev essentials build. macOS and Linux build
  FFmpeg 9.0.1 from the pinned source archive with static linking,
  autodetection disabled, and network support disabled. Review the exact
  configuration and provide the required notices and source offers before
  redistributing binaries.
- Tauri and its plugins: a multi-license ecosystem that includes MIT and
  Apache-2.0 components. Review the exact crates and bundled notices for the
  release being distributed.
- The system tray uses Tauri's optional tray-icon feature and tray-icon
  (MIT OR Apache-2.0). Source: https://github.com/tauri-apps/tray-icon.
- Windows completion notification identity uses tauri-winrt-notification
  (MIT OR Apache-2.0) and windows-registry (MIT OR Apache-2.0), already present
  in the notification dependency graph. Sources:
  https://github.com/tauri-apps/tauri-winrt-notification and
  https://github.com/microsoft/windows-rs/tree/master/crates/libs/registry.
- Non-Windows completion notifications use tauri-plugin-notification (MIT OR
  Apache-2.0) and its platform notification dependencies. Source:
  https://github.com/tauri-apps/plugins-workspace/tree/v2/plugins/notification.
- React: MIT License.
- Phosphor Icons (@phosphor-icons/react): MIT License. Source and license:
  https://github.com/phosphor-icons/react. Icons are bundled locally.
- Plus Jakarta Sans: SIL Open Font License 1.1. The license text is included
  in src/assets/fonts/OFL-PlusJakartaSans.txt.
- JetBrains Mono: SIL Open Font License 1.1. The license text is included in
  src/assets/fonts/OFL-JetBrainsMono.txt.

## Distribution responsibilities

The Tauri bundle includes the project license and this notice file. The
development bootstrap downloads runtime binaries into
src-tauri/runtime. Before distributing an installer, perform a license review
of the exact versions and build configurations included in that installer.

For a commercial or proprietary distribution, verify in particular:

1. the exact yt-dlp executable and its bundled notices;
2. the exact FFmpeg binary, configuration, and corresponding source-offer
   obligations;
3. the whisper.cpp binary and model files;
4. the Tauri crates and other transitive runtime components;
5. the notices and license texts shipped with the installer.

## Phosphor Icons license

MIT License

Copyright (c) 2020 Phosphor Icons

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
