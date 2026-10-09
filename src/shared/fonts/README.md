# 本地界面字体子集

三个派生 WOFF2 仅用于配置、托盘菜单和模式提示。内部字体名使用 `TranslayInterface`、`TranslayDisplay`、`TranslayReading`，避免使用上游保留名称；界面不新增字体选择设置。

| 角色 | 上游 | 固定来源 | 字形与体积 |
| --- | --- | --- | --- |
| Interface | Noto Sans SC 变量字体，版本 2.004-H2 | Google Fonts 提交 `5e8a3ba899557829a76cfdac30fa512bda91d7ca` | 305 字形；110,688 字节 |
| Display | 得意黑／Smiley Sans Oblique 2.0.1 | 作者发布 `v2.0.1` | 8 字形；4,092 字节 |
| Reading | 霞鹜文楷屏幕阅读版 1.522 | 作者发布 `v1.522` | 33 字形；11,176 字节 |

固定源文件地址：

- [Noto Sans SC](https://raw.githubusercontent.com/google/fonts/5e8a3ba899557829a76cfdac30fa512bda91d7ca/ofl/notosanssc/NotoSansSC%5Bwght%5D.ttf)
- [得意黑发布包](https://github.com/atelier-anchor/smiley-sans/releases/download/v2.0.1/smiley-sans-v2.0.1.zip)，其中 `SmileySans-Oblique.ttf`
- [霞鹜文楷屏幕阅读版](https://github.com/lxgw/LxgwWenKai-Screen/releases/download/v1.522/LXGWWenKaiScreen.ttf)

均保留 SIL OFL 1.1 许可，分别见 `OFL-interface.txt`、`OFL-display.txt`、`OFL-reading.txt`。字体二进制中保留原版权并嵌入完整许可。原设计与作者归属见各上游许可；这些是 Translay 的裁剪子集，不代表作者对本应用背书。

## 重建

不安装系统字体，不修改项目运行依赖。将上述源文件放在隔离工作目录中：Noto 文件命名 `interface.ttf`，文楷文件命名 `reading.ttf`，得意黑解压到 `smiley/`。裁剪工具会验证固定 SHA-256，拒绝无意使用其他源版本。源哈希与输出哈希见 `manifest.json`。

在隔离工具目录准备 `fonttools==4.66.1` 和 `brotli==1.2.0` 后执行：

```powershell
python tools/subset-ui-fonts.py <源文件目录> --python-deps <隔离 Python 包目录>
```

工具从配置页源码与托盘菜单源码提取固定界面字形，包含 ASCII；个性字体使用固定短标题／学习引导字集。只读取源码，不读取任何运行配置、用户数据或凭据。新增相关中文标签时重新生成，并运行三个既有浏览器回归及原生托盘验收。完成后清理完整源字体和隔离依赖目录，保留本目录的子集、许可与清单。

保存确认文字使用同一文楷子集，新增「已保存」三个字形。只更新一个角色时可使用 `--role reading`（也支持 `interface`、`display`），只需准备对应源文件；其他字体二进制、字体声明和清单条目保持不变。默认仍重建全部三个角色。
