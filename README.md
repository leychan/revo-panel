# RevoPanel · 机械革命 Linux 控制台

面向机械革命笔记本的 Linux 桌面仪表盘，首台验证机是耀世 `X6AR55xY`。

## 启动桌面仪表盘

RevoPanel 使用 Tauri 2、Rust 和 React/TypeScript。在图形会话中进入仓库后运行：

```sh
pnpm install
pnpm tauri dev
```

桌面窗口、Linux 遥测、HID 灯控、BlueZ 水冷箱通信及系统设置使用 Rust。桌面程序以普通用户权限运行，必要时为灯控会话授权一次，打开 HID 设备后即恢复普通用户身份。 [Tauri Linux 开发环境](https://v2.tauri.app/start/prerequisites/#linux)

`pnpm dev` 可在浏览器查看标有「界面预览」的演示界面。

需要本地直接启动的构建产物时，可运行 `pnpm tauri build --debug --no-bundle`，再运行 `src-tauri/target/debug/revo-panel`。同一个可执行文件提供桌面程序和硬件会话。

在 KDE Plasma 等提供系统托盘的桌面上，关闭窗口会收进托盘，水冷箱监测和灯控会话继续运行。通过托盘菜单的「打开控制台」或再次启动程序可以恢复窗口。选择「退出程序」会先结束硬件操作，再退出应用。桌面没有可用托盘时，关闭窗口会正常退出。在 Manjaro 上托盘依赖 `libappindicator`。

「系统设置 → 外观」支持浅色、深色和跟随系统，以及预设或自定义主题色。选择会随应用保存，下次启动自动恢复。

系统设置下方的电源、屏幕、独显和触控板卡片可拖到任意一列，松手后自动保存所在列和顺序。也可聚焦把手，用左右方向键换列，用上下方向键在列内排序。

- **总览和温度传感器**每两秒更新一次。总览显示 CPU 负载、封装温度、最近 60 秒的温度曲线，以及电池电量、充电状态、容量健康度、循环次数和外接电源。「温度传感器」列出内核当前公开的温度读数。
- **省电读数**在总览显示电池放电瓦数和近 3 分钟样本均值，切换供电来源后重新取样。总览和托盘也显示 NVIDIA 独显的 PCI 运行状态，后台跳过独显传感器。温度、显存和独显功耗可在系统设置里按需读取。
- **USB-C 供电**在总览的「电池与供电」显示各端口连接状态、供电角色和内核报告的协商电压、电流。电池放电功率独立显示，便于判断接入电源时电池是否仍在放电。 [Linux USB Type-C 接口](https://docs.kernel.org/driver-api/usb/typec.html)
- **键盘灯光**支持读取亮度、调节 0–50 级亮度、单色常亮、呼吸、波浪流光、彩虹循环及其他板载灯效。支持速度选择，波浪流光可选方向。改动后自动发送，拖动时合并连续改动。切换到亮灯模式时，如果亮度读回为零，会使用 `20/50`。仪表盘启动时连接灯控，必要时通过 `pkexec` 认证一次。键盘和机身灯光随后复用同一个灯控会话。 [键盘灯效协议](https://github.com/JeanElsner/iteglow/blob/main/docs/protocol.md#keyboard-048d600b-ite-8291)
- **机身灯光**使用 `048d:7001` 控制器，提供前灯条及控制器报告支持的 A 面徽标、出风口灯通道。每个通道的亮度、颜色和灯效独立调节。A 面徽标卡片始终显示，通道不可用时会说明原因。 [7001 灯效协议](https://github.com/JeanElsner/iteglow/blob/main/docs/protocol.md#lightbar-048d7001) · [通道能力映射](https://github.com/faintonce/open-revo/blob/b7d2b52/src-tauri/src/driver/lighting/mod.rs#L161)
- **水冷箱**随仪表盘启动，通过 BlueZ 查找并自动连接已识别的设备，意外断线后自动重连。多台设备同时可用且没有已选水冷箱时，可以在页面选择目标。点「停止监测」会暂停本次运行的自动连接。连接后可读取水流及固件版本，`7/8/11/12 V` 四个电压档默认 PWM 为 `45/60/80/100%`，也可手动调整或停泵。改动自动发送，`LCT22002` 的水箱灯和冷排灯可分开调节。 [LCT22002 水泵和灯光实现](https://github.com/tuxedocomputers/tuxedo-control-center/blob/master/src/e-app/LCT21001.ts)
- **系统设置**支持 Linux 电源档位、KDE 内屏亮度和当前分辨率下的刷新率，以及 KWin 内置触控板开关。NVIDIA 独显的温度、负载、显存和功耗通过 `nvidia-smi` 按需读取。
- **CPU 睿频限制**位于「系统设置 → 系统电源档位」。启用后，RevoPanel 在电池供电时限制 Intel CPU 睿频，接回电源、关闭开关或退出时恢复原睿频状态。系统要求时，本次运行只需授权一次，收进托盘仍会继续监测供电变化。 [Intel P-state 睿频接口](https://docs.kernel.org/admin-guide/pm/intel_pstate.html)

在 Manjaro 上，水冷箱通过 D-Bus 使用 `bluez`。桌面设置对应 `libkscreen`、`power-profiles-daemon`、`nvidia-utils` 和 `polkit`，各功能根据本机服务的可用性启用。

## GitHub 自动打包与发布

普通推送会生成 x86_64 的 `.AppImage`、`.deb` 和 `.pkg.tar.zst`，可在 **Actions → Linux 打包与发布 → Artifacts** 下载。推送版本标签会自动创建 GitHub Release，并将安装包放到该版本的 **Assets**。标签中的版本号会用于安装包名称。 [下载 GitHub Actions 产物](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/download-workflow-artifacts)

```sh
git tag v0.1.0
git push origin v0.1.0
```

Arch Linux 和 Manjaro 可下载 `.pkg.tar.zst`，用 `sudo pacman -U ./revo-panel-bin-0.1.0-1-x86_64.pkg.tar.zst` 安装。Ubuntu 24.04 等兼容系统可用 `sudo apt install ./文件名.deb` 安装，也可为 `.AppImage` 添加执行权限后运行。

### AUR 自动发布

公开仓库的稳定版本成功发布到 GitHub Releases 后，工作流会为 `revo-panel-bin` 更新 AUR。AUR 以 Release 里的 `.deb` 为来源，使用 SHA-256 校验，安装后通过 Manjaro 或 Arch 的系统依赖运行。

1. 在仓库根目录添加 `LICENSE`，并在 `package.json` 的 `license` 填写对应的 SPDX 标识。
2. 为 AUR 生成单独的 SSH 密钥，把公钥加到有权维护该包的 AUR 账号。首次发布时可用该账号提交尚未被占用的包名。 [AUR 提交流程](https://wiki.archlinux.org/title/AUR_submission_guidelines)
3. 将可供 CI 使用的无口令私钥保存到 GitHub 仓库 **Settings → Secrets and variables → Actions**，命名为 `AUR_SSH_PRIVATE_KEY`。推送稳定版本标签，或重新运行已有标签的工作流，再到 **Actions** 查看 AUR 发布结果。

## 命令行诊断

```sh
src-tauri/target/debug/revo-panel --telemetry
src-tauri/target/debug/revo-panel --system-status
src-tauri/target/debug/revo-panel --gpu-status
src-tauri/target/debug/revo-panel --cooler-list
```

`--telemetry` 读取机型与 BIOS、温度、供电和电池状态，并查找驱动公开的风扇转速、平台档位、充电上限和键盘背光入口。命令行诊断无需启动桌面窗口。

键盘诊断定位 `X6AR55xY` 的 ITE 8291 灯控接口。本机访问该 HID 节点需要管理员权限。`--keyboard-status` 只查询亮度和模式，`--verify-brightness` 会读原亮度、短暂试调、读回确认并恢复原值。

```sh
sudo src-tauri/target/debug/revo-panel --keyboard-status
sudo src-tauri/target/debug/revo-panel --verify-brightness
```

`sudo src-tauri/target/debug/revo-panel --chassis-status` 查询机身 USB 灯控通道。本机实际返回 `00 a2 01 00 00 00 00 00 00`，能力码 `1` 对应前灯条。OpenRevo 另有一条经主板 EC 控制徽标的路径，本机 Linux 当前缺少对应接口。 [主板徽标控制路径](https://github.com/faintonce/open-revo/blob/b7d2b52/src-tauri/src/driver/lighting/mod.rs#L732)

## 电池限充与笔记本风扇曲线

- 本机 Manjaro 内核当前没有公布 `charge_control_end_threshold` 或 `charge_types`。Uniwill 驱动在部分机型上公开百分比阈值，在另一些机型上公开充电模式，两者需要分别验证。 [Linux Uniwill 充电接口](https://docs.kernel.org/admin-guide/laptops/uniwill-laptop.html#battery-charging-control)
- 本机没有公布 `fan*_input`、`pwm*` 或曲线节点。OpenRevo 的笔记本风扇曲线通过机型专用 EC 表和提交协议生效，Linux 侧需要先打通适配此机型的驱动。 [OpenRevo 风扇曲线实现](https://github.com/faintonce/open-revo/blob/b7d2b52/src-tauri/src/system/power/fan_table.rs#L207)

## 耀世首台验证机

- 主板 `YAOSHI Series-X6AR55xY`，BIOS `N.1.33MRO63`，RTX 5070 Ti Laptop，Manjaro `7.2.9-1-MANJARO`。
- 已读到 CPU 等温度传感器、电池百分比与充电状态、适配器接入状态。USB 枚举发现 ITE 8291（`048d:600b`）和 ITE 8233（`048d:7001`）。
- 风扇验证涵盖 `hwmon` 和 `acpi-fan`。`hwmon` 无风扇转速通道，五个 ACPI 风扇入口只公开 `0/1` 散热状态，没有实时 RPM。系统有 `uniwill-laptop` 模块文件，本机未匹配其 DMI 自动加载别名。 [ACPI 风扇转速接口](https://docs.kernel.org/admin-guide/acpi/fan_performance_states.html)
- ITE 8291（`048d:600b`）已绑定 `hid-generic`。亮度实测 `36/50 → 12/50 → 36/50`。RGB 试色下发 `#32D4FF` 后键盘实际亮起，芯片读回 RGB 模式和亮度。试色后的完整灯效参数未恢复到原值。新增板载动画仍待逐项实机确认。 [ITE 8291 协议](https://github.com/JeanElsner/iteglow/blob/main/docs/protocol.md#keyboard-048d600b-ite-8291)
- 水冷箱 `CoolingSystem LCT22002` 已实测蓝牙连接、固件 `2.0.0.4` 和正常水流。按 TUXEDO 的同型号实现修正了水泵指令中字节顺序，依次发送使能、PWM 占空比和电压码。水泵各档与箱灯的实体效果仍待确认。当前回包用于判读水流状态。 [OpenRevo 水流协议实现](https://github.com/faintonce/open-revo/blob/b7d2b52/src-tauri/src/driver/ble_cooler.rs#L1183)
- 系统档位、内屏亮度、`2560 × 1600` 下的 `60/240 Hz` 和触控板开关都已实机试调、读回并恢复原设置。已读到 RTX 5070 Ti 的温度、负载、显存和功耗。 [系统电源档位接口](https://upower.pages.freedesktop.org/power-profiles-daemon/gdbus-org.freedesktop.UPower.PowerProfiles.html)
- 同主板已有 Linux 适配案例。其验证组合使用 BIOS `N.1.13MRO16` 和 RTX 5080，适配补丁依赖精确机型匹配。其键盘灯效适配可作参考，前灯条状态需要单独判断。 [耀世 Linux 适配项目](https://github.com/noteMASTER11/Mechrevo-Yaoshi-Linux)

## 源码评估

OpenRevo 提供了对应的 `X6AR55` 机型配置、硬件抽象接口和 HID 灯控实现。React/Tauri 界面组件、机型数据结构与灯效算法可择需复用。桌面壳和核心 crate 均引入 Windows API，EC 通信使用 Win32 IOCTL 或 Windows WMI，灯控使用 Windows HID API。Linux 版本需要自己的硬件后端。 [OpenRevo 对应机型配置](https://github.com/faintonce/open-revo/blob/b7d2b52/src-tauri/resources/builtin_models.json#L399) · [EC 驱动](https://github.com/faintonce/open-revo/blob/b7d2b52/src-tauri/src/driver/wmi_ec.rs#L87) · [HID 绑定](https://github.com/faintonce/open-revo/blob/b7d2b52/src-tauri/src/driver/hid_ffi.rs#L97)

风扇转速适配可从 `uniwill-laptop` 的机型匹配和 `hwmon` 输出入手。性能档位需要验证是否有驱动能切换固件档位。充电阈值和充电模式分别显示实机公开的接口。自定义风扇曲线、显卡直连和功耗调节各自需要按主板、BIOS 和硬件组合验证。 [Uniwill 内核文档](https://docs.kernel.org/admin-guide/laptops/uniwill-laptop.html) · [Linux 平台档位接口](https://docs.kernel.org/userspace-api/sysfs-platform_profile.html)
