# Third-party components

WhisperTube source code is MIT-licensed. Runtime components are separate upstream projects and retain their own licenses.

- whisper.cpp — MIT: https://github.com/ggml-org/whisper.cpp
- OpenAI Whisper model architecture/weights — see upstream model repository and license terms.
- yt-dlp — Unlicense plus third-party components; official packaged executables may include additional licenses. See its bundled notices/repository: https://github.com/yt-dlp/yt-dlp
- FFmpeg — LGPL/GPL depending on the exact build configuration. The Windows bootstrap currently downloads the Gyan.dev "essentials" build. Review that distribution's license/build configuration before redistributing binaries commercially.
- Tauri — Apache-2.0 / MIT ecosystem components.
- React — MIT.

## Important distribution note

The development bootstrap downloads runtime binaries into `src-tauri/runtime`. Before distributing a commercial/proprietary installer, perform a formal license review of the exact yt-dlp and FFmpeg binaries you bundle and ship all required license notices/source offers where applicable.
