# RevoPanel

[简体中文](README.zh-CN.md)

RevoPanel is a Linux desktop control panel for MECHREVO laptops. It was first tested on the Yaoshi `X6AR55xY` with Manjaro and KDE Plasma.

## Install

- **Arch Linux / Manjaro** Install [`revo-panel-bin`](https://aur.archlinux.org/packages/revo-panel-bin) from the AUR, or install the downloaded `.pkg.tar.zst` with `sudo pacman -U ./package.pkg.tar.zst`, using its actual filename.
- **Ubuntu 24.04 and compatible distributions** Install the downloaded `.deb` with `sudo apt install ./package.deb`, using its actual filename.
- **Other x86_64 Linux distributions** Download the AppImage, make it executable, and run it.

All installers are available on the [GitHub Releases page](https://github.com/leychan/revo-panel/releases).

## Features

- **Monitoring** CPU temperature and load, battery health and discharge power, USB-C power negotiation, and NVIDIA GPU sleep status. Detailed NVIDIA metrics are read on demand.
- **Lighting** Keyboard brightness, colors and effects, plus supported chassis lighting zones.
- **External cooling** Automatic discovery and reconnection, flow monitoring, pump and fan controls, and lighting for compatible BlueZ coolers such as the LCT22002.
- **System settings** Linux power profiles, KDE display brightness and refresh rate, touchpad control, and an Intel turbo limit while on battery. The power, display, GPU, and touchpad cards can be rearranged.
- **Desktop** System tray, light and dark themes, and English or Simplified Chinese. The language follows the system on first launch and can be changed under **System settings → Appearance → Language**.

## Compatibility

Keyboard and chassis lighting were tested with ITE devices `048d:600b` and `048d:7001`. The front light bar is supported on the tested machine. Availability of the lid logo, internal fan controls, and battery charge limits depends on the model and the interfaces exposed by its Linux drivers.

KDE-specific controls use KScreen and KWin. Bluetooth cooling uses BlueZ. RevoPanel remembers the battery turbo preference. Lighting and turbo control share one authorization per app session when required.

## Build from source

Install the [Tauri Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux), Rust, Node.js, and pnpm.

```sh
pnpm install --frozen-lockfile
pnpm tauri dev
```

Use `pnpm dev` for a browser preview with sample data. Use `revo-panel --help` to see the desktop binary's diagnostics.

## Releases and AUR

The [Linux packaging workflow](.github/workflows/package-linux.yml) builds x86_64 AppImage, Debian, and Arch packages. A version tag such as `v0.1.2` publishes them to GitHub Releases. Stable releases can also update the AUR package when the repository has the `AUR_SSH_PRIVATE_KEY` secret and the associated AUR account is verified. To retry an AUR publication, run the workflow on the default branch with `aur_release_tag` set to an existing release tag.

## License and credits

[GPL-3.0-only](LICENSE). Hardware protocol references include [OpenRevo](https://github.com/faintonce/open-revo), [iteglow](https://github.com/JeanElsner/iteglow), and [TUXEDO Control Center](https://github.com/tuxedocomputers/tuxedo-control-center).
