# Third-Party Notices

WhisperTube source code is licensed under the MIT License. The runtime
components, model files, and bundled fonts are separate works and retain their
own licenses and notices.

## Components

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
- React: MIT License.
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
