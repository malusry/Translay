# Translay Motion Language

Translay 的动作表达系统状态，不承担装饰职责。所有动效遵守四条原则：

1. **Motion follows state**：状态先确定，动画只负责表达状态变化。
2. **Motion borrows attention**：短暂借用注意力，完成后立即归还。
3. **Motion preserves continuity**：变化必须能追溯到原位置和原对象。
4. **Motion ends in silence**：稳定状态没有循环、呼吸或闪烁动画；工作中的 Loading 反馈除外。

## 两条独立状态轴

任务状态与视觉状态相互独立，避免动效改变捕获和翻译逻辑。

```text
任务状态：Capturing → Translating → Success / Failure

视觉状态：Idle → Loading → Preparing → Revealing → Settled → Dismissing
```

## 状态语义

| 状态 | 视觉行为 |
| --- | --- |
| `Loading` | 148×58 Surface 立即出现，圆点提供轻微的工作反馈 |
| `Preparing` | 隐藏渲染并测量内容，稳定原生窗口尺寸 |
| `Revealing` | Surface 从加载锚点扩展，内容和控件分层进入 |
| `Settled` | 完全静止，并从此刻启动 3/5/7 秒阅读计时 |
| `Dismissing` | 内容先归静，Surface 沿出现时的锚点收回并在原位消散 |

## Motion Tokens

| Token | 时长 |
| --- | --- |
| Compact Materialize | 300ms |
| Medium Materialize | 360ms |
| Long Materialize | 420ms |
| Loading Dot | 900ms 往复，仅在 `Loading` 状态运行 |
| Return Controls | 210ms |
| Return Content | 270ms |
| Anchor Return | 630ms：缓慢收回至 148×58 锚点，最终只保留 93% 轻微收束 |
| Air Dissolve | 与 Anchor Return 同步；25% 左右约 60%，50% 左右约 30%，75% 左右接近透明 |
| Transparent Hold | 最后约 76ms 保持完全透明，再交给原生窗口隐藏 |

Materialize 时长由 Surface 的实际展开距离决定，不直接依赖字符数。阅读时长仍由译文有效字符数决定，两套规则互不影响。

## 交互约束

- `Revealing` 期间禁止再次改变原生窗口尺寸。
- 阅读计时必须从 `Settled` 开始。
- 自动退出可被鼠标重新进入取消；手动关闭不可取消。
- 新翻译请求可以接管任何旧视觉状态。
- 同一请求的原生隐藏只能执行一次，安全计时器不得重入关闭过程。
- 前端未完成退出时，Rust 在 1000ms 后执行原生安全隐藏。
- `prefers-reduced-motion` 下保留状态反馈，但取消非必要循环和位移。
