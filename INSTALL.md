# 安装说明

本文档提供在不同平台上安装和运行游戏的详细步骤。

---

## 📋 目录

- [系统要求](#系统要求)
- [安装 Rust 和 Cargo](#安装-rust-和-cargo)
- [安装系统依赖](#安装系统依赖)
- [编译和运行](#编译和运行)
- [常见问题](#常见问题)

---

## 系统要求

### 最低要求

- **操作系统**：Linux、macOS 或 Windows
- **Rust 版本**：1.70.0 或更高版本
- **内存**：至少 2GB RAM
- **显卡**：支持 OpenGL 3.3 或更高版本

### 推荐配置

- **操作系统**：Linux（推荐）或 macOS
- **Rust 版本**：最新稳定版
- **内存**：4GB RAM 或更多
- **显卡**：支持现代 OpenGL/Vulkan

---

## 安装 Rust 和 Cargo

### Linux 和 macOS

使用官方安装脚本：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

安装完成后，重新加载 shell 配置：

```bash
source $HOME/.cargo/env
```

### Windows

1. 下载并运行 [rustup-init.exe](https://rustup.rs/)
2. 按照安装向导完成安装
3. 重启终端或命令提示符

### 验证安装

运行以下命令验证 Rust 和 Cargo 是否已正确安装：

```bash
rustc --version
cargo --version
```

如果看到版本号输出，说明安装成功。

---

## 安装系统依赖

### Linux (Ubuntu/Debian)

#### 基本依赖

```bash
sudo apt-get update
sudo apt-get install -y \
    build-essential \
    pkg-config \
    libasound2-dev \
    libudev-dev \
    libxkbcommon-x11-0 \
    libxkbcommon-x11-dev
```

#### WSL2 特殊配置

如果你在 WSL2 环境中运行，需要额外配置：

1. **安装 X11 相关库**：

```bash
sudo apt-get install -y libxkbcommon-x11-0 libxkbcommon-x11-dev
```

2. **设置 DISPLAY 环境变量**：

```bash
# 方法 1：如果使用 VcXsrv 或 X410
export DISPLAY=$(cat /etc/resolv.conf | grep nameserver | awk '{print $2; exit;}'):0.0

# 方法 2：如果使用其他 X11 服务器
export DISPLAY=:0
```

3. **在 Windows 中运行 X11 服务器**：
   - 安装 [VcXsrv](https://sourceforge.net/projects/vcxsrv/) 或 [X410](https://www.microsoft.com/store/apps/9NLJLZW79SX6)
   - 启动 X11 服务器
   - 确保允许来自网络的连接（用于 WSL2）

### Linux (Fedora/RHEL)

```bash
sudo dnf install -y \
    gcc \
    pkg-config \
    alsa-lib-devel \
    systemd-devel \
    libxkbcommon-x11 \
    libxkbcommon-x11-devel
```

### macOS

使用 Homebrew 安装依赖：

```bash
brew install pkg-config
```

macOS 通常已经包含了其他必要的系统库。

### Windows

Windows 用户通常不需要安装额外的系统依赖，Rust 工具链会自动处理。

---

## 编译和运行

### 克隆项目（如果从 Git 仓库）

```bash
git clone <repository-url>
cd fpsking
```

### 编译项目

```bash
cargo build --release
```

> **注意**：首次编译可能需要较长时间（10-30 分钟），因为需要编译 Bevy 引擎及其依赖。后续编译会快得多。

### 运行游戏

#### 开发模式（带调试信息）

```bash
cargo run
```

#### 发布模式（优化版本）

```bash
cargo run --release
```

### 验证安装

运行以下命令验证系统库是否已正确安装：

```bash
# Linux: 检查 X11 库
ldconfig -p | grep xkbcommon-x11

# 如果看到输出，说明安装成功
```

---

## 常见问题

### 问题 1：`Library libxkbcommon-x11.so could not be loaded`

**原因**：缺少 X11 键盘库。

**解决方案**：

```bash
# Ubuntu/Debian
sudo apt-get install -y libxkbcommon-x11-0 libxkbcommon-x11-dev

# Fedora/RHEL
sudo dnf install -y libxkbcommon-x11 libxkbcommon-x11-devel
```

### 问题 2：WSL2 无法显示图形界面

**原因**：WSL2 需要 X11 转发才能显示图形界面。

**解决方案**：

1. **在 Windows 中安装 X11 服务器**：
   - [VcXsrv](https://sourceforge.net/projects/vcxsrv/)（免费）
   - [X410](https://www.microsoft.com/store/apps/9NLJLZW79SX6)（付费，但更易用）

2. **启动 X11 服务器**：
   - VcXsrv：启动时选择 "Multiple windows" 和 "Disable access control"
   - X410：直接启动即可

3. **在 WSL2 中设置 DISPLAY**：

```bash
# 添加到 ~/.bashrc 或 ~/.zshrc 使其永久生效
export DISPLAY=$(cat /etc/resolv.conf | grep nameserver | awk '{print $2; exit;}'):0.0
```

4. **验证**：

```bash
echo $DISPLAY
# 应该显示类似：172.x.x.x:0.0
```

### 问题 3：编译错误 `error: linker 'cc' not found`

**原因**：缺少 C 编译器。

**解决方案**：

```bash
# Ubuntu/Debian
sudo apt-get install -y build-essential

# Fedora/RHEL
sudo dnf install -y gcc
```

### 问题 4：编译时间过长

**原因**：Bevy 引擎及其依赖需要较长时间编译。

**解决方案**：

1. **使用快速编译配置**（已在 `Cargo.toml` 中配置）：
   ```toml
   [profile.dev]
   opt-level = 1
   [profile.dev.package."*"]
   opt-level = 3
   ```

2. **使用 `cargo build` 而不是 `cargo run`**：
   - 首次编译后，后续运行会更快

3. **使用发布模式**（仅用于最终测试）：
   ```bash
   cargo run --release
   ```

### 问题 5：运行时出现权限错误

**原因**：某些系统库需要特定权限。

**解决方案**：

```bash
# 确保用户有权限访问音频设备（Linux）
sudo usermod -a -G audio $USER
# 然后重新登录
```

### 问题 6：网络联机模式无法连接

**原因**：ZeroTier 未正确安装或配置。

**解决方案**：

1. 查看 [ZeroTier安装指南.md](ZeroTier安装指南.md)
2. 确保 ZeroTier 服务正在运行
3. 确保两台设备都已加入同一个 ZeroTier 网络

---

## 下一步

安装完成后，你可以：

1. 查看 [README.md](README.md) 了解游戏玩法和控制说明
2. 运行 `cargo run` 开始游戏
3. 查看相关文档了解网络联机配置

---

## 获取帮助

如果遇到其他问题：

1. 检查 Rust 和 Cargo 版本是否满足要求
2. 查看 Bevy 官方文档：https://bevyengine.org/
3. 检查系统日志以获取更多错误信息
