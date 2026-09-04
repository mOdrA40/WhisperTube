# WhisperTube 0.1

WhisperTube 是一款 local-first 桌面应用，使用 `yt-dlp`、FFmpeg 和 whisper.cpp 将受支持平台的视频转换为转录文本。音频和转录过程都保留在用户设备上。

主文档：[README.md](README.md)（English）。印度尼西亚语文档：[README_ID.md](README_ID.md)。

## 当前范围

- Windows 10/11 x64 是主要的开发和测试目标。
- 默认界面语言为 English，也可以在 Settings 中选择 Bahasa Indonesia 或 中文（普通话）。
- 开发环境包含 CPU 推理引擎。
- NVIDIA CUDA 为可选组件，单独下载，以保持基础安装包较小。
- 应用会检测操作系统、架构、GPU、浏览器和配置文件，再显示可用的可选组件。

支持的来源包括 YouTube、TikTok、X/Twitter、Facebook、Instagram、Reddit、Twitch、Vimeo、Dailymotion、Pinterest、LinkedIn、Tumblr、Bilibili 和 VK。支持范围取决于当前 yt-dlp extractor，并可能随着平台变化而改变。

## 功能

- 粘贴受支持的视频 URL，在下载前检查视频元数据。
- 支持公开视频，以及通过导入 Netscape 格式 `cookies.txt` 来处理需要登录的视频。
- 不要求网站密码，也不会把 cookies 复制到应用数据库。
- 自动检测受支持的 Chrome、Edge、Firefox、Brave、Chromium、Opera、Vivaldi、Whale；macOS 还支持 Safari。Settings 只显示设备上实际检测到的浏览器和配置文件。
- 下载 `bestaudio/best`，使用 FFmpeg 转换为 16-bit PCM WAV、单声道、16 kHz，再通过本地 whisper.cpp 推理。
- 模型：Fast（`base`，约 142 MB）、Balanced（`large-v3-turbo-q5_0`，约 547 MB）、Accurate（`large-v3-q5_0`，约 1.1 GB）。
- 模型 checksum 校验、进度显示、取消、时间戳、TXT/SRT/VTT 导出和本地 SQLite 历史记录。
- 模型和视频下载会显示已传输的字节数，模型下载可以取消。
- 转录期间会在平台支持时显示 CPU/GPU 使用率，下载时会显示网络速度。
- 可以清除当前 URL、预览和转录文本，也可以永久删除单条、多条或当前列表中的全部历史记录。
- 除非打开保留处理后音频的选项，否则处理结束后会删除临时音频和 WAV 文件。

## Windows 开发设置

安装 Node.js 22 或更高版本、Rust，以及带有 **Desktop development with C++** 工作负载的 Visual Studio Build Tools 2022，并保留 MSVC 和 Windows SDK。

```powershell
cd D:\Projects\WhisperTube
.\scripts\setup-windows.ps1
.\scripts\run-dev.ps1
```

如果 Vite 报告端口 `1420` 已被占用，请先关闭之前的开发进程。

模型从应用的模型面板下载，并存储在用户 app data 中，而不是 source tree 中。

## GPU 加速

打开 **Settings → Hardware**。应用只显示与当前操作系统、架构和已检测 GPU 匹配的 accelerator：

- Windows x64 + 检测到 NVIDIA：可从固定版本的官方 whisper.cpp release 安装 CUDA。
- macOS：可提供匹配的 Apple Metal pack。
- Windows/Linux x64 + 检测到 GPU：可提供匹配的 Vulkan pack 作为替代加速器。
- 只有 CPU 或不受支持的设备不会看到无关的 accelerator 下载按钮。

CUDA 不包含在基础安装中。应用会将其下载到用户 app storage，并在启用前验证 SHA-256。

## 转录流程

1. 粘贴受支持的视频链接。
2. 点击 **检查视频**。
3. 确认视频元数据显示出来。
4. 下载并选择模型。
5. 除非需要指定后端，否则保持 **计算后端** 为 **自动**。
6. 选择转录语言或 **自动检测**。
7. 点击 **立即转录**。

```text
视频来源 → yt-dlp → FFmpeg WAV 16 kHz 单声道 → whisper.cpp
       → 片段/时间戳 → JSON + TXT + SRT + VTT + 历史记录
```

## 需要登录的视频

先在浏览器中登录相关平台并导出 Netscape 格式的 `cookies.txt`。在 WhisperTube 的 Settings 中导入该文件，然后返回转录页面点击 **检查视频**。

WhisperTube 只在本地将该文件传递给 yt-dlp，不会上传或写入历史记录。平台的 extractor、登录流程或反爬策略发生变化时，受保护视频支持仍可能受到影响。

在 Windows 上，基于 Chromium 的浏览器加密可能导致 yt-dlp 无法解密 Brave/Chrome/Edge 配置文件。此时可在 **Settings → 手动 cookies 文件** 中导入 Netscape 格式的 `cookies.txt`。该文件只由本机 WhisperTube 进程使用。

## 构建 Windows 安装程序

```powershell
.\scripts\build-windows.ps1
```

安装程序通常位于 `src-tauri\target\release\bundle\`。`npm run tauri:build` 是完整的 release 构建：编译 release 模式 Rust、构建前端、打包 runtime 资源并创建安装程序。

## Accelerator release（维护者）

`.github/workflows/build-accelerator-packs.yml` 使用固定版本的官方 whisper.cpp source 构建 Metal/Vulkan pack。开发和 CI 阶段 repository 可以保持 private，但发布 EXE 前 accelerator release 必须公开，因为应用不会内置 GitHub token。

## 项目结构和本地数据

- `src/components/`：UI 组件。
- `src/i18n.tsx`：English/Indonesia/中文界面翻译。
- `src/assets/fonts/`：随应用打包的界面字体。
- `src/hooks/`：应用状态和操作。
- `src/services/`：Tauri IPC 边界。
- `src-tauri/src/`：Rust command 和领域模块。

模型、任务、导出文件和 `whispertube.db` 存储在操作系统对应的 app-local-data 目录中。

## v0.1 限制

- 当前固定版本 CUDA bootstrap 面向 Windows x64 和已检测到 NVIDIA 驱动的设备。
- Metal/Vulkan 需要匹配的公开 GitHub Release asset。
- 需要登录的视频依赖 yt-dlp 兼容性以及浏览器/平台的安全策略变化。
- 目前没有 playlist/batch job、说话人分离、逐词字幕编辑或 runtime 自动更新。
- macOS/Linux 的 installer 流程还没有 Windows 路径成熟。

## 许可证

WhisperTube source code 使用 MIT 许可证。Runtime 和字体遵循各自 upstream 的许可证；请查看 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
