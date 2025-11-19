# 中文字体说明

## 如何添加中文字体支持

为了在游戏中正确显示中文，您需要将中文字体文件放在此目录（`assets/fonts/`）下。

### 推荐的中文字体

1. **Noto Sans CJK**（推荐）
   - 下载地址: https://www.google.com/get/noto/
   - 选择 "Noto Sans CJK SC" (简体中文)
   - 将字体文件重命名为 `NotoSansCJK-Regular.ttf` 并放在此目录

2. **Source Han Sans（思源黑体）**
   - 下载地址: https://github.com/adobe-fonts/source-han-sans
   - 选择 "SourceHanSansCN-Regular.otf"
   - 将字体文件放在此目录

3. **文泉驿正黑体**
   - 下载地址: http://wenq.org/wqy2/
   - 适合 Linux 系统

### 使用方法

1. 下载中文字体文件（TTF 或 OTF 格式）
2. 将字体文件放在 `assets/fonts/` 目录下
3. 确保文件名与代码中尝试加载的名称匹配：
   - `NotoSansCJK-Regular.ttf`（优先）
   - `SourceHanSansCN-Regular.otf`
   - `NotoSansSC-Regular.otf`
   - `simsun.ttf`
   - `msyh.ttf`

4. 运行游戏，字体将自动加载

### 注意

- 如果字体文件不存在，游戏会使用默认字体（不支持中文）
- 中文文本可能会显示为方块或问号
- 字体文件通常较大（几MB到几十MB），这是正常的

