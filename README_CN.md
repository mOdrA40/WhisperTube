# WhisperTube 0.1

WhisperTube 是一款 local-first 桌面应用，使用 `yt-dlp`、FFmpeg 和本地 Whisper/Qwen 引擎 将受支持平台的视频转换为转录文本。音频和转录过程都保留在用户设备上。

主文档：[README.md](README.md)（English）。印度尼西亚语文档：[README_ID.md](README_ID.md)。

## 模型提供方

在下拉框选择 **OpenAI · Whisper** 或 **Alibaba · Qwen**，仅显示对应提供方的模型。Qwen3-ASR-1.7B Q8 通过单独下载的原生运行环境在本地运行，无需 Python 或云端凭据。其字幕时间戳采用音频分块边界，仅供参考；需要语音对齐的字幕请使用 Whisper。详见 [Qwen 安装、硬件与限制](docs/TROUBLESHOOTING.md#qwen-installation-cuda-or-subtitle-timing)。

## 当前范围

- Windows 10/11 x64 是主要的开发和测试目标。
- 默认界面语言为 English，也可以在 Settings 中选择 Bahasa Indonesia 或 中文（普通话）。
- 开发环境包含 CPU 推理引擎。
- NVIDIA CUDA 为可选组件，单独下载，以保持基础安装包较小。
- 应用会检测操作系统、架构和 GPU，再显示匹配的可选组件。需要登录的视频通过手动导入 cookies.txt 获取来源访问权限。

支持的来源包括 YouTube、TikTok、X/Twitter、Facebook、Instagram、Reddit、Twitch、Vimeo、Dailymotion、Pinterest、LinkedIn、Tumblr、Bilibili 和 VK。支持范围取决于当前 yt-dlp extractor，并可能随着平台变化而改变。

为确保磁盘和内存使用可控，转录仅支持时长明确且不超过 8 小时的点播媒体和已结束的直播回放。推理按约五分钟一段运行，并使用经过验证的检查点恢复中断的任务。正在进行的直播和即将开始的直播会在下载前被拒绝。质量和内存限制详见下方的恢复转录说明。

## 功能

公开 Instagram Reels 或 LinkedIn 视频帖子缺少时长时，检查功能会尝试临时下载
音频（最多 32 MiB、45 秒），再用本地音频验证时长（最多 10 秒）。成功时无需
cookies。临时文件会被清理，转录时会再次下载音频；登录、地区和反机器人限制
仍可能需要 cookies。

已安装应用的 YouTube 前提条件：安装 [Node.js 22+](https://nodejs.org/en/download) 或 [Deno 2.3+](https://docs.deno.com/runtime/getting_started/installation/)，将其加入 PATH，然后重启 WhisperTube。安装包包含 yt-dlp 的 EJS 脚本，但不包含 JavaScript 运行环境。应用会在提取 YouTube 信息前检查此前提条件，并在运行环境缺失或版本不受支持时显示明确错误。其他支持的平台不受此前提条件限制。下载 Whisper 模型不会安装 JavaScript 运行环境。

- 粘贴受支持的视频 URL，在下载前检查视频元数据。
- 支持公开视频，以及通过导入 Netscape 格式 `cookies.txt` 来处理需要登录的视频。
- 不要求网站密码，也不会把 cookies 复制到应用数据库。
- 支持从各浏览器系列导入 Netscape 格式的 `cookies.txt`。
- 下载 `bestaudio/best`，使用 FFmpeg 转换为 16-bit PCM WAV、单声道、16 kHz，再通过本地 whisper.cpp 推理。
- 模型：Fast（`base`，约 142 MB）、Balanced（`large-v3-turbo-q5_0`，约 547 MB）、Accurate（`large-v3-q5_0`，约 1.1 GB）。
- 模型 checksum 校验、进度显示、取消、时间戳、TXT/SRT/VTT 导出和本地 SQLite 历史记录。
- 模型和视频下载会显示已传输的字节数，模型下载可以取消。
- 转录期间会在平台支持时显示 CPU/GPU 使用率，下载时会显示网络速度。
- Whisper 和 Qwen 均在主窗口未获得焦点时发送结果通知，区分转录成功、完成但有时长差异、失败和取消。Windows 上取消通知为静音。通知使用界面语言，不包含视频标题或转录内容。系统设置或勿扰模式可能阻止横幅；通知发送失败不会改变任务结果。
- 可以清除当前 URL、预览和转录文本，也可以永久删除单条、多条或当前列表中的全部历史记录。
- 失败或取消的任务会自动保留临时下载、已完成的 WAV 和有效转录进度，以便重试；这不受“完成后保留音频”选项影响。
- 转录成功后会清理临时下载和检查点；仅在启用“完成后保留音频”时保留 WAV。

### 恢复转录与释放空间

使用相同来源和推理设置重试时，应用会复用匹配的音频与有效检查点；中断的片段会重新处理。更改模型、语言或计算设备等设置会开始新的转录，但仍可复用匹配的来源音频。模型按约五分钟一段重新加载，片段边界的识别结果可能与整段推理不同，仍需满足模型内存要求。

未完成任务的缓存计入 20 GiB 任务存储上限。启动时会清理至少 24 小时未更新且未被历史记录引用的任务。也可在设置中查看并删除指定未完成任务的进度以释放空间；删除后该任务需要从头处理。已提交的历史记录和应用存储之外导出的文件不会因此删除。重试不会自动无限循环。

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

如果设备有多个 GPU，计算目标选择器会显示已安装的目标，例如
`CUDA · NVIDIA RTX` 或 `VULKAN · AMD Radeon (iGPU)`。在支持的 Windows NVIDIA
设备上，自动模式优先使用可用的 CUDA；手动目标可以选择具体的 CUDA 或 Vulkan 设备。

CUDA 不包含在基础安装中。应用会将其下载到用户 app storage，并在启用前验证 SHA-256。

如需清理仅用于开发的 Windows CUDA 文件，请运行
`npm run clean:dev-runtime`。该脚本不会触及用户 app data 中的模型或运行时。

在 macOS 或 Linux 上，请运行 `npm run clean:dev-runtime:unix`，删除当前主机
在 `src-tauri/runtime/macos` 或 `src-tauri/runtime/linux` 中生成的开发运行时。
开始开发前请重新运行对应平台的 setup 脚本。

## 卸载与本地数据

WhisperTube 将已安装的应用文件与用户数据分开保存。若要在所有操作系统
上删除模型、历史记录、jobs、保存的音频和已下载的运行时，请先在应用中
使用 **Settings → Reset WhisperTube data**，再删除应用。外部文件（例如用户
选择的 `cookies.txt`）以及 WhisperTube 存储目录之外的导出文件不会被删除。

- **Windows NSIS setup EXE：** 从 Windows Settings 或 `uninstall.exe` 卸载时会
  显示一个删除应用数据的复选框。不勾选会保留数据以便重新安装；勾选只会删除上述
  WhisperTube 管理的数据。WebView、配置和外部文件均会保留。应用更新始终保留用户数据。
- **macOS DMG/app：** 将 `.app` 移到 Trash 只会删除应用，不会删除 app-local-data。
  如果需要完整清理，请先在应用中执行 reset。
- **Linux AppImage：** 删除 AppImage 不会删除 app-local-data。对于 Debian/Ubuntu
  软件包，卸载软件包也会保留用户数据；需要完整清理时请先在应用中执行 reset。
- **开发模式：** `npm run tauri:dev` 没有操作系统卸载程序。开发专用运行时可用
  `npm run clean:dev-runtime` 删除。

默认 app-local-data 路径为：Windows 的 `%LOCALAPPDATA%/app.whispertube.local`，
macOS 的 `~/Library/Application Support/app.whispertube.local`，以及 Linux 的
`${XDG_DATA_HOME:-~/.local/share}/app.whispertube.local`。

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

先登录相关平台并导出最新的 Netscape 格式 `cookies.txt`。在 WhisperTube 的 Settings 中导入该文件，然后返回转录页面点击 **检查视频**。

WhisperTube 只在本地将该文件传递给 yt-dlp，不会上传或写入历史记录。平台的 extractor、登录流程或反爬策略发生变化时，受保护视频支持仍可能受到影响。

WhisperTube 只会在本地任务中将导入的 cookies 文件传递给 yt-dlp。浏览器加密可能导致导出的 cookies 无法使用，请保持文件最新并妥善保密。

## 构建 Windows 安装程序

```powershell
.\scripts\build-windows.ps1
```

安装程序通常位于 `src-tauri\target\release\bundle\`。`npm run tauri:build` 是完整的 release 构建：编译 release 模式 Rust、构建前端、打包 runtime 资源并创建安装程序。

## 从 source 构建 macOS 和 Linux

macOS/Linux 构建必须在对应的 native host 上执行，因为 Tauri 使用系统的
WebView 和打包工具链。仓库现在提供了两个平台的 bootstrap 和 build 脚本。

### macOS

先安装 Xcode Command Line Tools、Homebrew、Rust 和 Node.js，然后在仓库根目录运行：

```bash
xcode-select --install
chmod +x scripts/setup-macos.sh scripts/build-macos.sh
./scripts/setup-macos.sh
./scripts/build-macos.sh
```

setup 会分别生成 CPU 和 Metal whisper runtime。`.app` 和 `.dmg` bundle 位于
`src-tauri/target/release/bundle/`。

### Linux（Ubuntu/Debian）

先安装 Tauri 系统依赖：

```bash
sudo apt-get update
sudo apt-get install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf xdg-utils cmake pkg-config git nasm xz-utils
```

然后运行：

```bash
chmod +x scripts/setup-linux.sh scripts/build-linux.sh
./scripts/setup-linux.sh
./scripts/build-linux.sh
```

Linux 脚本会先构建固定版本的静态 FFmpeg 和 CPU engine，然后在
`src-tauri/target/release/bundle/` 生成 Debian 和 AppImage bundle。
其他发行版需要安装对应的 WebKitGTK、编译器、OpenSSL、FFmpeg 和打包依赖。为了
兼容 glibc，建议在计划支持的最旧基础发行版上构建。

可选 workflow `.github/workflows/build-application-bundles.yml` 会在 GitHub
的 Windows、macOS 和 Linux runner 上重复 native build。手动运行可获得临时
artifact，或者 push 类似 `v0.1.0` 的应用 tag，将 NSIS、DMG、Debian 和 AppImage
原生安装包发布到 GitHub Release。Windows 和 Linux 尚未签名；macOS 使用免费
ad-hoc 签名，但 v0.1 尚未经过 Apple notarize。
每个安装包都会附带 SHA-256 sidecar，应用 bundle 也会包含项目 license 和
third-party notices。

## Accelerator release（维护者）

`.github/workflows/build-accelerator-packs.yml` 使用固定版本的官方 whisper.cpp source 构建 Metal/Vulkan pack。开发和 CI 阶段 repository 可以保持 private，但发布 EXE 前 accelerator release 必须公开，因为应用不会内置 GitHub token。

公开 release 完成后，先读取并同步四个 pack 的 checksum，再重新构建应用：

```powershell
.\scripts\sync-accelerator-hashes.ps1
.\scripts\sync-accelerator-hashes.ps1 -Apply
```

第一条命令只读取并显示 hash；`-Apply` 会将 hash 写入应用目录，使 Metal/Vulkan 下载按钮生效。请检查 diff，并且不要在构建 EXE 后替换 release asset。

## 项目结构和本地数据

- `src/components/`：UI 组件。
- `src/i18n.tsx`：English/Indonesia/中文界面翻译。
- `src/assets/fonts/`：随应用打包的界面字体。
- `src/hooks/`：应用状态和操作。
- `src/services/`：Tauri IPC 边界。
- `src-tauri/src/`：Rust command 和领域模块。

模型、任务、导出文件和 `whispertube.db` 存储在操作系统对应的 app-local-data 目录中。任务存储有 20 GiB 安全上限，当前使用量会显示在设置中。

在设置中可检查未完成的任务，并单独删除重试缓存以释放空间。删除缓存会丢失该任务的重试进度，但会保留已安装的模型和已保存的历史记录。

## v0.1 限制

- 当前固定版本 CUDA bootstrap 面向 Windows x64 和已检测到 NVIDIA 驱动的设备。
- Metal/Vulkan 需要匹配的公开 GitHub Release asset。
- 需要登录的视频依赖 yt-dlp 兼容性以及浏览器/平台的安全策略变化。
- 目前没有 playlist/batch job、说话人分离、逐词字幕编辑或 runtime 自动更新。
- macOS/Linux 已提供 native script 和 CI 构建，但 v0.1 尚未完成签名、公证以及广泛的发行版/硬件 QA。
- macOS/Linux bootstrap 会从固定 source archive 构建静态 FFmpeg；向干净机器分发仍需要完成各平台的依赖和许可证 QA。

## 许可证

WhisperTube source code 使用 MIT 许可证。Runtime 和字体遵循各自 upstream 的许可证；请查看 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
