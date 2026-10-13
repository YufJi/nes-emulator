# NES Emulator Web

**简体中文** · [English](./README.md)

React + Vite 前端，驱动 `crates/nes-wasm` 编译出的 Rust/WASM NES 内核。
把 `.nes` 文件拖到页面任意位置、或在侧栏填入 ROM 链接即可开始游戏。

## 环境要求

- Node.js 18+
- 重新编译 WASM 内核时需要 Rust 工具链与 [`wasm-pack`](https://drager.github.io/wasm-pack/)

## 命令

```bash
npm install         # 安装依赖
npm run dev         # 开发服务器 http://localhost:5173
npm run build       # 类型检查 + 生产构建到 dist/
npm run build:pages # 子路径构建（GitHub Pages），base 取自 PAGES_BASE，默认 /
npm run preview     # 本地预览生产构建
npm run typecheck   # 仅类型检查
npm run build:games # 重新生成 public/games.json（游戏列表）
npm run build:wasm  # 用 wasm-pack 重新生成 src/wasm/ 下的内核绑定
```

`src/wasm/` 由 `npm run build:wasm` 生成，未纳入版本控制。
首次执行该命令时 `wasm-pack` 会下载 `wasm-bindgen` 等工具，需要联网。

部署到 GitHub Pages 由根目录的 `.github/workflows/deploy-pages.yml` 负责：它会重新
生成 `src/wasm/`、用 `npm run build:pages` 按 `<repo>` 子路径构建并发布 `dist/`。
`games.json` 与 AudioWorklet 都通过 `import.meta.env.BASE_URL` 取路径，所以子路径
部署无需改代码。详见根 [`README.zh-CN.md`](../README.zh-CN.md) 的「部署（GitHub Pages）」。

## 界面

界面由 [animal-island-ui](https://github.com/guokaigdg/animal-island-ui) 的动森风格组件搭
建，样式表在 `src/main.tsx` 里统一引入一次（`import 'animal-island-ui/style'`）。
`src/styles.css` 只负责本项目自己的布局：它不覆盖库组件的颜色、圆角与阴影，而是读取库
在 `:root` 上声明的 `--animal-*` token，让自定义部分与组件保持同一套色板。

- 组件：`Card` / `Title` / `Button` / `Input` / `Switch` / `Tag` / `Modal` / `Skeleton` /
  `Collapse` / `Divider` / `Tooltip` / `Notification` / `Background` /
  `Cursor` / `Footer`。
- 图标一律取自 `naive-icons`（库的运行时依赖），不使用 emoji 或自制 SVG。
- 加载失败提示走 `Notification` 命令式接口，「清空存档」的二次确认走 `Modal`。
- 品牌图标直接就是 `public/favicon.svg`，标签页与页面不会各画一套。

## 多语言

界面支持中文 / English / 日本語，默认中文。

- 所有文案集中在 `src/i18n/messages.ts`：中文表定义 key，另外两份以 `Dictionary` 类型标注，
  因此「某个 key 加了中文却忘了英文/日文」会直接变成编译错误，而不是在译文页面里冒出一句中文。
  占位符写成 `{name}`，由 `t(key, params)` 填充。
- `src/i18n/index.ts` 把当前语言放在模块级而不是 React context：卡带加载（`src/emulator/rom.ts`）
  与模拟器内核也要产出页面要显示的报错文案，而它们不是组件。组件用 `useI18n()` 订阅，切换语言时
  自动重渲染。
- 切换器（`src/components/LanguageSwitcher.tsx`）出现在宽屏页顶栏、手机列表页头部、手机播放页的
  工具栏抽屉里；选择记在 `localStorage`，`<html lang>` 同步跟随。
- 不翻译的内容：`A` / `B` / `Start` / `Select`、`NES Emulator`、游戏列表里的游戏名，以及写进浏览器
  控制台的诊断日志。

## 产物体积

`vite.config.ts` 里的 `trimIslandAssets` 插件会从库的样式表中删掉两类本项目用不上的声明，
把 `dist/` 从约 4.6 MB 压到约 730 KB：

- **Noto Sans SC 的 6 个 `@font-face`**（3 个简体中文字面各约 1.15 MB，且没有 `unicode-range`，
  浏览器可能全部下载）。库里每个 `font-family` 列表都紧接着写了系统字体
  （`PingFang SC` / `Microsoft YaHei`），所以删掉字面只会让中文改用系统字体，其余排版不变；
  拉丁字母仍用 Nunito（3 个拉丁子集共约 48 KB）。
- **4 张装饰性场景插画**（约 316 KB）。它们只服务于 `Background type="sweet-corner" |
  "coffee-break"` 与 `Progress` 的 `variant`，本项目都没用到；以后要用就删掉插件里对应的
  那一行正则。

库一旦换字体或换图片，插件会在构建时输出警告而不是静默失效。

## 目录结构

```
src/
  App.tsx            # 路由表：/m 开头走手机布局，其余是宽屏页面
  router.ts          # 极简路由（读地址、navigate、页面刷新时的 404 回跳还原）
  games.ts           # 游戏目录：拉取 games.json、游戏 id 与查找
  emulator/
    NesEmulator.ts   # 帧时钟、画面渲染、输入与音频的总调度
    AudioOutput.ts   # AudioWorklet（回退 ScriptProcessorNode）音频输出
    rom.ts           # ROM 校验与链接下载（iNES 文件头、Mapper 支持情况）
    constants.ts     # 分辨率、帧率、按键映射等常量
  hooks/
    useNesEmulator.ts  # 把模拟器接到 React 状态上
    useIntegerScale.ts # 画布整数倍缩放
    useGameList.ts     # 游戏目录的加载状态
    usePagedList.ts    # 滚动分片渲染（弹窗与手机列表共用）
  components/          # 宽屏页面：画面、工具栏、链接输入、游戏列表弹窗、虚拟手柄、键位表、拖拽覆盖层
  mobile/              # 手机页面：列表 dashboard、游玩页、触控手柄、工具栏抽屉
public/
  games.json          # 游戏列表（名称 + 直链），由 npm run build:games 生成
  nes-audio-processor.js  # 运行在音频线程上的 AudioWorklet
  favicon.svg         # 标签页图标：island 绿底上的像素风卡带
  apple-touch-icon.png    # 同一张卡带的满幅版本，供「添加到主屏幕」使用
scripts/
  build-game-list.mjs # 从 GitHub 仓库读取文件树，生成 public/games.json
  build-wasm.mjs      # wasm-pack 构建 + 生成物拷贝
```

## 手机页面（/m）

宽屏页面在手机上把九个面板挤成一列，游戏画面反而很小，因此手机版是另开的两条路由：

```
/                      宽屏页面（原样保留）
/m                     游戏列表 dashboard：搜索、本地 ROM、随机开一个
/m/play?game=<游戏名>   游玩页：画面在上、触控手柄在下，其余收进右上角工具栏
```

- `?game=` 取自 `games.json` 里**唯一的游戏名**（列表来自同一个扁平目录，名称不重复），
  所以在聊天里分享出去仍然是可读的 `/m/play?game=1943`。名字在列表里查不到时，游玩页会
  提示并给出回到列表的入口。
- 访问站点根地址时，明显是手机的 UA 会自动进入 `/m`（`src/mobile/entry.ts`，判定在首帧渲染前
  完成，因此不会闪过宽屏页面；用 `replaceState` 改写地址，不会留下「后退又跳回来」的历史记录）。
  只匹配确定是手机的平台（`Android…Mobile` / `iPhone` / `iPod` / Windows Phone…），平板与桌面
  UA 保持宽屏页面。界面上**没有**桌面 / 手机互切的入口：本轮跳转在 `sessionStorage` 里记一次，
  同一标签页里再次打开 `/`（手动输入地址、旧书签）就不再跳转，这是从手机进入宽屏页面的唯一
  途径，避免被反复弹回无处可去。
- 游玩页是 `100dvh` 的网格：顶栏、画面、手柄三行，互不滚动；**暂停、重置、静音、全屏、
  存档槽、换 ROM、从链接加载**都在右上角按钮打开的 `Drawer` 里（`vite.config.ts` 里关掉了
  抽屉的背景下沉景深——它会把画布一起缩放，画面会糊）。暂停时点画面即可继续。
- 横屏（`orientation: landscape` 且高度 ≤ 540px）时手柄改到手柄两侧：方向键与 SELECT 贴左、
  B/A 与 START 贴右，画面居中，拇指落在握持手机时本来就在的位置。手柄容器本身
  `pointer-events: none`，只有按键可点，画面中间仍然属于画面（暂停后点画面照常继续）。
- 页面用 `viewport-fit=cover` 占满整屏，因此系统占用的每条边都用 `env(safe-area-inset-*)`
  还回去：手机页面的左右与底部加安全区内边距（带刘海的手机横屏时，不安全区在**左右两侧**，
  正好是手柄所在的边），游玩页是不滚动的固定网格、顶部也一并加上；列表页顶部只留 14px——
  这一页整页就是滚动容器，顶部再加一次状态栏高度会和主屏打开时系统自己留出的那条叠成双份，
  而且它属于滚动内容，一划就跟着滚走。列表页的吸顶搜索框仍贴在状态栏之下而不是被压住，portal
  出来的浮层同样加内边距，宽屏页面也自带安全区。
- 但这些数值得是窗口**现在**所在的那块，而 `env()` 答的不是：从主屏图标启动 web app 时，
  页面是在 iOS 的加载条还压在上方的时候完成首次排版的，这条样式不会被重新解析，加载条消失后
  顶部就留下一块属于加载条的边距。`src/mobile/useViewport.ts` 因此自己重新测量：现场新建一个
  探针元素，让它的 padding 按当下这一刻的 inset 解析（新元素没有旧结果可捡），窗口 `resize`、
  `orientationchange`、`visualViewport` 尺寸变化时都重测，App 打开后的几秒里也反复重测（加载条
  自己消失不触发任何事件）。结果写到 `:root` 的 `--safe-area-top/right/bottom/left` 与
  `--viewport-height` 上，手机样式表优先读它们、用 `env()` / `100dvh` 兜底，因此首屏帧与没有
  该机制的设备都不受影响。浮层是相对物理屏幕 `fixed` 的，**高度**和留白一样重要：工具栏抽屉用
  同一次测量得到的高度（手机上 `100vh` 是「地址栏收起」的高视口，用它会发现最后一行存档被
  浏览器栏压住），并把刘海、圆角、Home 指示条都用内边距让开；对话框走 `maskStyle`、通知容器
  按类名前缀匹配（库没有为它提供 prop）做同样处理。所有 inset 都以 0 兜底，没有该机制的设备
  不受影响。
- 全屏请求的是整个页面而不是画面：抽屉、弹窗、通知都挂在 `body` 上，元素全屏会把它们藏起来。
- 触控手柄见 `src/mobile/TouchPad.tsx`：整个面板由一个容器处理指针事件，每次移动都按坐标
  重新判断按到了什么，因此十字键是「一根摇杆」而不是四个独立按钮（斜向 = 同时按住两个方向，
  拇指滑到别的键上会立即切换），多点触控互不干扰，`touch-action: none` 保证滑动不会变成页面滚动，
  按下时有轻微振动反馈。
- 方向区因此画成一个连续的触点盘而不是四个按键：整块圆面都是触摸区（元素比绘制的圆面再多留
  8px 隐形边距，拇指伸向角落也能搓出斜向），四个箭头只是印在上面的提示，中间的摇杆头会朝按下
  方向偏移、对应箭头变色——不需要瞄准任何东西，方向由落点决定。A/B/SELECT/START 仍是独立按键：
  NES 读的是 8 向数字量，没有模拟值可以交给真正的摇杆。
- 每个指针归属于它按下的控件，因此按住不会被中途丢掉：方向区上方向始终按坐标算——手指滑出圆盘
  很远也保持住，只有回到死区或抬手才松开；按钮上按下则能在相邻按键之间移交（B→A 滚动就是这么
  做的），拇指漂到面板内的空白处仍保持按住，滑出整个手柄面板才松开。
- 动作区在 B/A 上方多了一个 **A+B** 键：单拇指不可能一边按住 B（跑）一边点 A（跳），这个键一次
  上报两个按键——正是「跑跳」。它是一次真正的双键按下而不是连发：抬手前主机看到的就是 A、B 同时
  按下，也可以和方向区配合（按住方向，再点 A+B）。
- 能震动的平台会给触感反馈：`src/mobile/haptics.ts` 包了 `navigator.vibrate`（Android 浏览器；
  WebKit 至今没有实现，iPhone 上静默失败、开关也呈禁用态），面键 12ms、方向区每次离开死区 6ms，
  方向之间滚动不重复震。默认开启，可在工具栏「震动」（「静音」下面）关掉。
- 列表行的选择挂在「轻点」而不是 `click` 上（`src/mobile/useTap.ts`）：列表还在惯性滑动时（或
  刚停下时）的第一下轻点，WebKit 会把它用在刹车而不派发 `click`，于是变成「要点两次才能进游戏」。
  这里在 `pointerup` 上识别轻点（按下到抬起位移 ≤ 10px，滚动会被 `pointercancel` 打断），并且
  优先让随后到达的 `click` 触发，只有 `click` 确实没来时由兜底定时器补上——若在 `pointerup` 里
  直接改 DOM，随后那个 `click` 会落在新出现的遮罩上，打开的工具栏抽屉会被立刻关掉。
- 手机页面整体禁用文本选择与长按 callout（`body.phone-ui`，由 `src/App.tsx` 按路由挂到 `body`
   上，所以 portal 出来的抽屉与弹窗同样生效），行、按键、画面都不会拖出选择放大镜或 iOS 菜单；
   **输入框除外**，搜索框与 ROM 链接框仍可选中文字、移动光标。宽屏页面保持原样。
- 画面在手机页面上允许非整数倍缩放：手机上「1× 清晰但只占三分之一屏」不如「铺满屏」。
  窗口本身就是画面的等比盒，比例取自内核导出的帧尺寸（`MobileStage` 按 `screen` 写成
  `aspect-ratio`）：竖屏按宽度撑满，横屏由可用高度推导宽度并居中，所以不会出现铺满整屏的
  黑色边框，也不会把画面拉伸变形。
- 深链（`/m/play?game=…`）在 GitHub Pages 上没有服务端 rewrite，构建时会额外产出
  `404.html`：它把原始地址塞进 `?route=` 交回应用，`router.ts` 在首帧渲染前用
  `history.replaceState` 还原，因此刷新和分享链接都能直接打开对应页面。

## 载入 ROM

- **本地文件**：拖到页面任意位置，或用工具栏的「选择 ROM」。
- **游戏列表**：工具栏的「游戏列表」打开弹窗，可按名称搜索并直接开玩，当前卡带会
  标记为「当前」。
- **链接**：在侧栏「从链接加载」中粘贴 ROM 直链后回车（省略 `http(s)://` 时按
  `https` 处理）。下载的镜像会经过与本地文件完全相同的 iNES / Mapper 校验。
  GitHub 的 `blob`（文件页面）与 `raw` 链接会自动改写为 `raw.githubusercontent.com`
  的直链——`github.com/…/raw/…` 的 302 跳转不带 `Access-Control-Allow-Origin`，
  浏览器会直接丢弃该响应。其他站点的链接需要服务器允许跨域（CORS）。
- **存档归属**：电池存档与即时存档按「来源」区分，本地文件用文件名、链接用规范化
  后的 URL，因此同一份 ROM 的本地副本与链接副本互不覆盖。

### 游戏列表数据

`public/games.json` 是形如 `[{ "name": "90坦克", "url": "https://raw.githubusercontent.com/…" }]`
的静态文件，默认收录 [mengfei523060/nesrom](https://github.com/mengfei523060/nesrom)
中的 932 个 ROM（只存名字与直链，**不含 ROM 数据**，仓库里的 ROM 一律不入版本控制）。
弹窗首次打开时才请求该文件，因此不会增加首屏体积；列表按名称排序，先渲染 20 行，
滚动到接近底部时自动追加下一批 20 行（`IntersectionObserver` 监听列表末尾的哨兵），
因此没有分页器，也不会一次渲染近千行；搜索框吸顶，滚到列表深处也能随手换关键词。

换一个仓库或更新列表：

```bash
npm run build:games                      # 默认 mengfei523060/nesrom 的 master 分支
npm run build:games -- owner/repo main   # 指定仓库与分支
GITHUB_TOKEN=… npm run build:games       # 提高 GitHub API 额度（未认证每小时 60 次）
```

脚本优先调用 GitHub API；被限流时自动改为遍历仓库页面（无额度限制）。

## 操作方式

| 动作 | 按键 |
| --- | --- |
| 方向 | `W` `A` `S` `D` |
| A | `J` / `I` |
| B | `K` / `O` |
| Start | `Enter` / `Space` |
| Select | `Shift` |
| 暂停 / 继续 | `P` |

触屏设备可直接使用右侧的虚拟手柄。

### 关于 Select

《超级马里奥兄弟》的标题画面只在**每 16 帧中的 1 帧**上采样 Select（游戏内部用一个
`$0780` 计数器做闸门），并且它的读手柄程序（`$8E5C`）会把 Select 变成一个**只有单帧
有效**的「刚按下」标志。因此一次约 100 ms 的普通点按只有约 1/16 的概率正好落在采样帧
上，而按住不放更是完全无效——标志在下一个采样窗口打开前就已经过期了。此外游戏还有一个
次级闸门（`$07A2`）会周期性屏蔽 Select，所以即使时机正确也可能被丢掉。

这是 ROM 自身的逻辑，不是模拟器或前端的缺陷。**本项目刻意不做任何连发 / 重试处理**，
按键怎么按下就怎么上报给手柄，保持与原机一致的输入时序；代价是切换 P1 / P2 需要像在
原机上一样多点几次（或在正确的那一帧上快速点按）。若以后想加输入辅助，只需在
`src/emulator/NesEmulator.ts` 的 `setButton` 外面包一层脉冲逻辑即可，核心与前端渲染
都不必改动。

## 实现要点

- **帧时钟**：按 NES 的 60.0988 Hz 而非屏幕刷新率推进帧，用 `requestAnimationFrame`
  累积时间片并限制单次追帧数量；标签页切回时丢弃积压，避免快进。
- **整数倍缩放**：画布后备缓冲固定为 256×240，用 `ResizeObserver` 计算整数缩放比后
  只改 CSS 尺寸，配合 `image-rendering: pixelated` 得到锐利像素。
- **音频**：内核每个模拟帧产出一批交错立体声样本，交给 AudioWorklet 队列按声卡节奏
  播放；不支持 AudioWorklet 的浏览器自动回退到 `ScriptProcessorNode`。
- **ROM 校验**：无论是本地文件还是链接下载，交给内核前都先检查 iNES 文件头与 Mapper
  是否已被内核实现，给出可读的错误提示；加载失败时保留当前正在运行的游戏。
- **电池存档**：带电池的卡带其存档按 ROM 文件名写入 `localStorage`，再次拖入同一
  ROM 时自动恢复；工具栏的「清空存档」在二次确认后删除该存档并重置卡带，方便重开。
- **游戏列表**：`scripts/build-game-list.mjs` 生成静态的 `public/games.json`，前端在
  打开弹窗时才拉取并按名称过滤，避免把近千条数据打进首屏 bundle。
- **即时存档**：`save_state` / `load_state` 把 CPU、PPU、APU、总线以及卡带的可变状态
  （bank 选择、IRQ 计数器、工作 RAM、扩展音频…）编码成一段字节流，PRG-ROM 不重复存储；
  侧栏提供 4 个槽位，保存在 IndexedDB，可随时保存、读取或清除。

## 免责声明

本站是纯个人学习研究项目：只为学习，不提供任何 ROM / 游戏文件，与任天堂没有任何关联。
页面底部会重复这条提示，完整声明（覆盖仓库、本前端、iOS 应用等）见
[`../DISCLAIMER.zh-CN.md`](../DISCLAIMER.zh-CN.md)。
