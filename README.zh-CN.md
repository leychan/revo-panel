# RevoPanel

[English](README.md)

RevoPanel 是面向机械革命笔记本的 Linux 桌面控制台。首台验证机为耀世 `X6AR55xY`，使用 Manjaro 和 KDE Plasma。

## 安装

- **Arch Linux / Manjaro** 可从 AUR 安装 [`revo-panel-bin`](https://aur.archlinux.org/packages/revo-panel-bin)，也可以下载 `.pkg.tar.zst` 后用 `sudo pacman -U ./package.pkg.tar.zst` 安装，文件名以实际下载的为准。
- **Ubuntu 24.04 及兼容发行版** 下载 `.deb` 后用 `sudo apt install ./package.deb` 安装，文件名以实际下载的为准。
- **其他 x86_64 Linux 发行版** 可下载 AppImage，添加执行权限后运行。

安装包见 [GitHub Releases](https://github.com/leychan/revo-panel/releases)。

## 主要功能

- **硬件监测** 查看 CPU 温度和负载、电池健康度及放电功率、USB-C 供电协商参数和 NVIDIA 独显休眠状态。独显详细指标按需读取。
- **灯光控制** 支持键盘亮度、颜色和灯效，以及设备提供的机身灯光分区。
- **外置水冷** 通过 BlueZ 自动查找并重连兼容水冷箱，如 LCT22002。支持水流监测、水泵、风扇和灯光调节。
- **系统设置** 支持 Linux 电源档位、KDE 内屏亮度与刷新率、触控板开关，以及电池供电时限制 Intel CPU 睿频。电源、屏幕、独显和触控板卡片支持拖拽。
- **桌面体验** 支持系统托盘、明暗主题、简体中文和英文。首次启动跟随系统语言，可在「系统设置 → 外观 → 语言」切换。

## 兼容性

键盘和机身灯光已在 ITE `048d:600b`、`048d:7001` 设备上测试，验证机支持前灯条。A 面徽标、内置风扇控制和电池限充取决于机型与 Linux 驱动提供的接口。

KDE 功能使用 KScreen 和 KWin。水冷箱连接依赖 BlueZ。灯光和睿频控制在会话内首次使用时可能需要系统授权。

## 源码构建

安装 [Tauri Linux 构建依赖](https://v2.tauri.app/start/prerequisites/#linux)、Rust、Node.js 和 pnpm。

```sh
pnpm install --frozen-lockfile
pnpm tauri dev
```

`pnpm dev` 可在浏览器中查看使用演示数据的界面。运行 `revo-panel --help` 可查看命令行诊断选项。

## GitHub 发布与 AUR

[打包工作流](.github/workflows/package-linux.yml) 生成 x86_64 的 AppImage、Debian 和 Arch 安装包。推送 `v0.1.1` 这样的版本标签后，安装包会发布到 GitHub Releases。公开仓库配置 `AUR_SSH_PRIVATE_KEY` 且对应的 AUR 账号已验证时，稳定版本也会更新 AUR。补发 AUR 时，在默认分支手动运行工作流，并将已有 Release 的标签填入 `aur_release_tag`。

## 许可证与致谢

[GPL-3.0-only](LICENSE)。硬件协议参考了 [OpenRevo](https://github.com/faintonce/open-revo)、[iteglow](https://github.com/JeanElsner/iteglow) 和 [TUXEDO Control Center](https://github.com/tuxedocomputers/tuxedo-control-center)。
