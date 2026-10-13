/**
 * Every string the interface shows, in the three languages it speaks.
 *
 * The Chinese table is the source of the keys: the other two are typed as
 * `Dictionary`, so a key added here and forgotten there is a compile error
 * rather than a Chinese sentence in the middle of a translated page.
 *
 * Placeholders are `{name}` and are filled by `t(key, params)`; the same
 * placeholder may be used in any order, which is what keeps word order free
 * between the languages.
 */

export const LOCALES = ['zh', 'en', 'ja'] as const;
export type Locale = (typeof LOCALES)[number];

export const LOCALE_LABELS: Record<Locale, string> = {
  zh: '中文',
  en: 'English',
  ja: '日本語',
};

/**
 * What the switcher prints. Three languages have to fit the width of a phone's
 * header, and a visitor looking for their own language recognises it in one
 * character; the full name goes in the button's tooltip and accessible name.
 */
export const LOCALE_SHORT: Record<Locale, string> = {
  zh: '中',
  en: 'EN',
  ja: '日',
};

/** Shown in the tab and on the brand line; a product name, so never translated. */
export const APP_NAME = 'NES Emulator';

export const ZH = {
  'brand.tagline': 'Rust + WebAssembly 内核 · 拖入 ROM 即可开玩',
  'brand.mobileTitle': 'NES Emulator 手机版',
  'language.label': '语言',

  'stage.canvasLabel': 'NES 画面',
  'stage.dropTitle': '把 .nes 文件拖到这里',
  'stage.loadingCore': '正在加载 WASM 内核…',
  'stage.dropHint': '或者选择本地文件 / 填入链接 / 打开游戏列表',
  'stage.chooseRom': '选择 ROM 文件',
  'stage.paused': '已暂停',
  'stage.pausedHint': '按 P 或点击继续按钮恢复游戏',
  'stage.resume': '继续游戏',
  'stage.mobilePausedHint': '点按画面继续；其他操作在右上角工具栏里。',

  'drop.title': '松手即可加载 ROM',
  'drop.hint': '支持 .nes（iNES 格式）文件',

  'status.idle': '待机',
  'status.running': '运行中',
  'status.paused': '已暂停',
  'status.loading': '加载中',
  'status.noCart': '还没有选择卡带',
  'status.noRom': '未加载 ROM',

  'toolbar.title': '控制台',
  'toolbar.swapRom': '换一个 ROM',
  'toolbar.chooseRom': '选择 ROM',
  'toolbar.gameList': '游戏列表',
  'toolbar.resume': '继续',
  'toolbar.pause': '暂停',
  'toolbar.reset': '重置',
  'toolbar.clearSave': '清空存档',
  'toolbar.fullscreen': '全屏',
  'toolbar.mute': '静音',
  'toolbar.haptics': '震动',
  'toolbar.hapticsHint': '按键与方向都会有轻微振动；觉得吵可以关掉。',
  'toolbar.hapticsUnsupported': '当前浏览器不提供震动接口（iOS Safari 没有该 API），此处无法开启。',
  'toolbar.clearSaveHint': '清空存档会删除该卡带保存的电池存档，并让游戏从头开始。',
  'toolbar.noBatteryHint': '这张卡带没有电池存档，无需清空。',

  'keyGuide.title': '键位',
  'keyGuide.dpad': '方向',
  'keyGuide.turboA': '连发 A',
  'keyGuide.turboB': '连发 B',
  'keyGuide.note':
    'P 键暂停 / 继续；方向键不再绑定，移动请用 WASD。按住连发键即自动连打 A / B（约 15 次 / 秒），普通按键不受影响。',
  'keyGuide.selectQuestion': 'Select 为什么要按好几次？',
  'keyGuide.selectAnswer':
    'Select 未做任何连发处理，完全按原机时序：初代标题画面每 16 帧才采样一次 Select，单次轻点常被游戏丢弃，来回切换 P1 / P2 需要多点几次。',

  'gamepad.title': '蓝牙手柄',
  'gamepad.none': '未连接',
  'gamepad.note':
    '有线 / 蓝牙手柄都能直接开玩：先配对（iPad 在「设置 → 蓝牙」里），再按一下手柄上的任意键，它就会出现在这里。十字键与左摇杆 = 方向。四个面键和小霸王手柄一样分成一对单发、一对连发，连发就贴在它连打的那个键旁边：下方 = A、右侧 = 连发 A、左侧 = B、上方 = 连发 B。肩键 / 扳机也是连发（左 = 连发 B、右 = 连发 A），这样按住连发时拇指还能留在 A / B 上；+ / − = Start / Select。按住的键会实时显示在手柄名旁边——某个键没反应时，先看这里有没有亮；显示成 #编号 的，是前端没有对应按键的输入。',
  'gamepad.unsupported':
    '当前浏览器未开放 Gamepad 接口，读不到手柄，请改用键盘或虚拟手柄。',

  'romUrl.title': '从链接加载',
  'romUrl.label': 'ROM 链接',
  'romUrl.submit': '加载',
  'romUrl.note':
    '支持 iNES (.nes) 直链；GitHub 的 Raw / 文件页面链接会自动转换为直链，其他站点需允许跨域（CORS）。',

  'saveState.title': '即时存档',
  'saveState.slot': '槽位 {slot}',
  'saveState.filled': '已保存',
  'saveState.empty': '空',
  'saveState.saveTo': '保存到槽位 {slot}',
  'saveState.loadFrom': '读取槽位 {slot}',
  'saveState.clearSlot': '清除槽位 {slot}',
  'saveState.note': '保存整个机器状态，可随时读取。',

  'gameList.title': '游戏列表',
  'gameList.searchPlaceholder': '搜索游戏名…',
  'gameList.searchLabel': '搜索游戏',
  'gameList.noMatch': '没有匹配 “{query}” 的游戏。',
  'gameList.current': '当前',
  'gameList.footCount': '共 {count} 个游戏',
  'gameList.footList': '游戏列表',
  'gameList.footSource': ' · 资料来自 ',

  'clearSave.title': '清空卡带存档？',
  'clearSave.cancel': '取消',
  'clearSave.confirm': '清空并重新开始',
  'clearSave.body': '删除该卡带的电池存档并重新开始，此操作无法撤销。',

  'pad.group': '虚拟手柄',
  'pad.up': '上',
  'pad.down': '下',
  'pad.left': '左',
  'pad.right': '右',
  'pad.a': 'A 键',
  'pad.b': 'B 键',
  'pad.ab': 'A+B 合并键',
  'pad.turboA': '连A',
  'pad.turboB': '连B',

  'play.back': '返回游戏列表',
  'play.openTools': '打开工具栏',
  'play.noCartName': '没有卡带',
  'play.missingTitle': '找不到这个游戏',
  'play.preparingTitle': '正在准备内核…',
  'play.noCartTitle': '还没有选择卡带',
  'play.missingHint': '游戏列表里没有 “{game}”，可能列表已经更新过了。',
  'play.noCartHint': '从游戏列表挑一个，或载入自己的 .nes 文件。',

  'dashboard.sub': '手机版 · 点一个游戏开玩',
  'dashboard.localRom': '本地 ROM',
  'dashboard.random': '随便玩一个',

  'tools.title': '工具栏',
  'tools.load': '载入',

  'disclaimer.line': '纯个人学习研究项目 · 无任何商业行为 · 不提供任何 ROM / 游戏文件。',
  'disclaimer.link': '免责声明',

  'notification.failed': '出错了',
  'error.gamesUnavailable': '游戏列表载入失败：{reason}',
  'error.emptyFile': '“{name}” 是空文件。',
  'error.fileTooLarge': '“{name}” 有 {size} MB，远超 NES ROM 的正常体积。',
  'error.linkMissing': '请先填入 ROM 链接。',
  'error.linkInvalid': '“{url}” 不是有效的链接。',
  'error.linkScheme': '只能加载 http(s) 直链，暂不支持 {protocol}// 链接。',
  'error.downloadFailed': '下载失败：服务器返回 {status}。',
  'error.notARom': '“{url}” 返回的是网页而不是 ROM 文件，请改用文件的直链。',
  'error.timeout': '下载超时（{seconds} 秒无响应）：{url}',
  'error.fetchFailed': '无法下载“{url}”：{reason}',
  'error.linkTooLarge': '该链接有 {size} MB，远超 NES ROM 的正常体积。',
  'error.linkUnavailable': '{detail}链接可能已失效，或该服务器未允许跨域访问（CORS）。',
  'error.tooSmall': '“{name}” 太小（{bytes} 字节），不是 NES ROM 文件。',
  'error.badHeader': '文件头缺少 NES\\x1a 标识，文件可能已损坏。',
  'error.badExtension': '扩展名不是 .nes，如果是压缩包请先解压。',
  'error.notInes': '“{name}” 不是 iNES 格式文件：{hint}',
  'error.unif': '“{name}” 使用了 UNIF/NES 2.0 容器，当前仅支持 iNES 格式。',
  'error.unsupportedMapper':
    '“{name}” 使用了 Mapper {mapper}，当前内核尚未实现该 Mapper（已支持 {supported}）。',
  'error.coreRejected': '模拟器内核拒绝了该 ROM：{reason}',
  'error.wasmFailed': 'WASM 模块加载失败：{reason}',
  'error.coreStopped':
    '模拟器内核异常，游戏已停止（ROM 可能已损坏）。换一个 ROM 即可继续；详情见浏览器控制台。',
  'error.canvasUnavailable': '无法创建 Canvas 2D 上下文。',
  'error.disposed': '模拟器实例已销毁。',
  'error.coreNotReady': 'WASM 内核尚未初始化，无法读取其内存。',
  'error.coreSpecUnavailable': 'WASM 内核尚未初始化，无法读取画面规格。',
  'error.pixelFormat': '内核返回了无法识别的像素格式：{format}',
};

export type MessageKey = keyof typeof ZH;
export type Dictionary = Record<MessageKey, string>;

export const EN: Dictionary = {
  'brand.tagline': 'Rust + WebAssembly core · drop in a ROM and play',
  'brand.mobileTitle': 'NES Emulator for phones',
  'language.label': 'Language',

  'stage.canvasLabel': 'NES screen',
  'stage.dropTitle': 'Drop a .nes file here',
  'stage.loadingCore': 'Loading the WASM core…',
  'stage.dropHint': 'or choose a local file / paste a link / open the game list',
  'stage.chooseRom': 'Choose a ROM file',
  'stage.paused': 'Paused',
  'stage.pausedHint': 'Press P or use the resume button to carry on',
  'stage.resume': 'Resume',
  'stage.mobilePausedHint': 'Tap the picture to carry on; everything else is in the toolbar, top right.',

  'drop.title': 'Release to load the ROM',
  'drop.hint': '.nes (iNES) files are supported',

  'status.idle': 'Idle',
  'status.running': 'Running',
  'status.paused': 'Paused',
  'status.loading': 'Loading',
  'status.noCart': 'No cartridge picked yet',
  'status.noRom': 'No ROM loaded',

  'toolbar.title': 'Console',
  'toolbar.swapRom': 'Swap ROM',
  'toolbar.chooseRom': 'Choose ROM',
  'toolbar.gameList': 'Game list',
  'toolbar.resume': 'Resume',
  'toolbar.pause': 'Pause',
  'toolbar.reset': 'Reset',
  'toolbar.clearSave': 'Clear save',
  'toolbar.fullscreen': 'Fullscreen',
  'toolbar.mute': 'Mute',
  'toolbar.haptics': 'Vibration',
  'toolbar.hapticsHint': 'Buttons and the stick give a short buzz; turn it off if it gets noisy.',
  'toolbar.hapticsUnsupported':
    'This browser has no vibration API (iOS Safari does not provide one), so it cannot be turned on.',
  'toolbar.clearSaveHint':
    "Clearing the save deletes the cartridge's battery save and starts the game over.",
  'toolbar.noBatteryHint': 'This cartridge has no battery save, so there is nothing to clear.',

  'keyGuide.title': 'Keys',
  'keyGuide.dpad': 'D-pad',
  'keyGuide.turboA': 'Turbo A',
  'keyGuide.turboB': 'Turbo B',
  'keyGuide.note':
    'P pauses and resumes; the arrow keys are deliberately unbound — steer with WASD. Holding a turbo key fires that button about 15 times a second; an ordinary press is unaffected.',
  'keyGuide.selectQuestion': 'Why does Select need several presses?',
  'keyGuide.selectAnswer':
    "Select is never repeated or re-tried: the console's own timing is kept. The title screen samples Select once every 16 frames, so a quick tap is usually dropped — switching between 1P and 2P takes a few taps.",

  'gamepad.title': 'Bluetooth gamepad',
  'gamepad.none': 'Not connected',
  'gamepad.note':
    'Wired and Bluetooth controllers play as they are: pair the pad first (on an iPad, in Settings → Bluetooth), then press any of its buttons and it shows up here. D-pad and left stick steer. The four face buttons are a plain pair and a turbo pair, as on a clone pad or a NES Advantage, with each turbo switch right beside the button it fires: bottom is A, right is turbo A, left is B, top is turbo B. The shoulders and triggers turbo too (left fires B, right fires A), so a turbo can be held with the thumb still on A / B; + / − are Start and Select. Whatever the pad holds appears beside its name as you press it — the place to look when a button seems to do nothing. Anything shown as #n is a button this front end has no use for.',
  'gamepad.unsupported':
    'This browser does not expose the Gamepad API, so it cannot see a controller; use the keyboard or the virtual pad instead.',

  'romUrl.title': 'Load from a link',
  'romUrl.label': 'ROM URL',
  'romUrl.submit': 'Load',
  'romUrl.note':
    'Direct iNES (.nes) links; GitHub Raw and file-page links are rewritten to direct links. Other sites have to allow cross-origin requests (CORS).',

  'saveState.title': 'Save states',
  'saveState.slot': 'Slot {slot}',
  'saveState.filled': 'Saved',
  'saveState.empty': 'Empty',
  'saveState.saveTo': 'Save to slot {slot}',
  'saveState.loadFrom': 'Load slot {slot}',
  'saveState.clearSlot': 'Clear slot {slot}',
  'saveState.note': 'Snapshots the whole machine, ready to load at any time.',

  'gameList.title': 'Game list',
  'gameList.searchPlaceholder': 'Search games…',
  'gameList.searchLabel': 'Search games',
  'gameList.noMatch': 'No game matches “{query}”.',
  'gameList.current': 'Playing',
  'gameList.footCount': '{count} games',
  'gameList.footList': 'Game list',
  'gameList.footSource': ' · catalogue from ',

  'clearSave.title': 'Clear the cartridge save?',
  'clearSave.cancel': 'Cancel',
  'clearSave.confirm': 'Clear and start over',
  'clearSave.body':
    "Deletes this cartridge's battery save and starts over. This cannot be undone.",

  'pad.group': 'Virtual controller',
  'pad.up': 'Up',
  'pad.down': 'Down',
  'pad.left': 'Left',
  'pad.right': 'Right',
  'pad.a': 'A button',
  'pad.b': 'B button',
  'pad.ab': 'A and B together',
  'pad.turboA': 'Turbo A',
  'pad.turboB': 'Turbo B',

  'play.back': 'Back to the game list',
  'play.openTools': 'Open the toolbar',
  'play.noCartName': 'No cartridge',
  'play.missingTitle': 'Game not found',
  'play.preparingTitle': 'Preparing the core…',
  'play.noCartTitle': 'No cartridge yet',
  'play.missingHint': 'The game list has no “{game}”; it may have been updated since.',
  'play.noCartHint': 'Pick one from the game list, or load your own .nes file.',

  'dashboard.sub': 'Phone layout · tap a game to play',
  'dashboard.localRom': 'Local ROM',
  'dashboard.random': 'Surprise me',

  'tools.title': 'Toolbar',
  'tools.load': 'Load',

  'disclaimer.line':
    'A personal study project · non-commercial · no ROMs or game files are provided.',
  'disclaimer.link': 'Disclaimer',

  'notification.failed': 'Something went wrong',
  'error.gamesUnavailable': 'Could not load the game list: {reason}',
  'error.emptyFile': '“{name}” is an empty file.',
  'error.fileTooLarge': '“{name}” is {size} MB, far larger than a NES ROM should be.',
  'error.linkMissing': 'Paste a ROM URL first.',
  'error.linkInvalid': '“{url}” is not a valid link.',
  'error.linkScheme': 'Only http(s) direct links can be loaded; {protocol}// is not supported.',
  'error.downloadFailed': 'Download failed: the server answered {status}.',
  'error.notARom': '“{url}” returned a web page rather than a ROM; use the file’s direct link.',
  'error.timeout': 'Download timed out ({seconds} s without a response): {url}',
  'error.fetchFailed': 'Could not download “{url}”: {reason}',
  'error.linkTooLarge': 'That link is {size} MB, far larger than a NES ROM should be.',
  'error.linkUnavailable':
    '{detail}The link may be dead, or the server may not allow cross-origin requests (CORS).',
  'error.tooSmall': '“{name}” is too small ({bytes} bytes) to be a NES ROM.',
  'error.badHeader': 'The header has no NES\\x1a signature; the file is probably damaged.',
  'error.badExtension': 'The extension is not .nes — if this is an archive, unpack it first.',
  'error.notInes': '“{name}” is not an iNES file: {hint}',
  'error.unif': '“{name}” uses a UNIF/NES 2.0 container; only iNES is supported.',
  'error.unsupportedMapper':
    '“{name}” uses mapper {mapper}, which the core does not implement yet (supported: {supported}).',
  'error.coreRejected': 'The emulator core rejected this ROM: {reason}',
  'error.wasmFailed': 'Could not load the WASM module: {reason}',
  'error.coreStopped':
    'The emulator core faulted and the game stopped (the ROM may be damaged). Load another ROM to carry on; the browser console has the details.',
  'error.canvasUnavailable': 'Could not create a Canvas 2D context.',
  'error.disposed': 'The emulator instance has been disposed.',
  'error.coreNotReady': 'The WASM core is not initialized yet, so its memory cannot be read.',
  'error.coreSpecUnavailable': 'The WASM core is not initialized yet, so its frame size is unknown.',
  'error.pixelFormat': 'The core reported an unknown pixel format: {format}',
};

export const JA: Dictionary = {
  'brand.tagline': 'Rust + WebAssembly コア · ROM をドロップして遊ぶ',
  'brand.mobileTitle': 'NES Emulator スマホ版',
  'language.label': '言語',

  'stage.canvasLabel': 'NES 画面',
  'stage.dropTitle': '.nes ファイルをここにドロップ',
  'stage.loadingCore': 'WASM コアを読み込み中…',
  'stage.dropHint': 'またはローカルファイルを選ぶ / リンクを貼る / ゲーム一覧を開く',
  'stage.chooseRom': 'ROM ファイルを選ぶ',
  'stage.paused': '一時停止中',
  'stage.pausedHint': 'P キーか再開ボタンでゲームに戻ります',
  'stage.resume': 'ゲームを再開',
  'stage.mobilePausedHint': '画面をタップで再開。ほかの操作は右上のツールバーにあります。',

  'drop.title': '離すと ROM を読み込みます',
  'drop.hint': '.nes（iNES 形式）に対応',

  'status.idle': '待機',
  'status.running': '実行中',
  'status.paused': '一時停止中',
  'status.loading': '読み込み中',
  'status.noCart': 'カートリッジ未選択',
  'status.noRom': 'ROM 未読み込み',

  'toolbar.title': 'コンソール',
  'toolbar.swapRom': 'ROM を変更',
  'toolbar.chooseRom': 'ROM を選ぶ',
  'toolbar.gameList': 'ゲーム一覧',
  'toolbar.resume': '再開',
  'toolbar.pause': '一時停止',
  'toolbar.reset': 'リセット',
  'toolbar.clearSave': 'セーブを消去',
  'toolbar.fullscreen': '全画面',
  'toolbar.mute': 'ミュート',
  'toolbar.haptics': '振動',
  'toolbar.hapticsHint': 'ボタンと十字キーが軽く振動します。うるさければオフにできます。',
  'toolbar.hapticsUnsupported':
    'このブラウザは振動 API に対応していません（iOS Safari にはありません）。ここではオンにできません。',
  'toolbar.clearSaveHint':
    'セーブを消去すると、このカートリッジのバッテリーセーブが削除され、最初から始まります。',
  'toolbar.noBatteryHint': 'このカートリッジにはバッテリーセーブがありません。',

  'keyGuide.title': 'キー配置',
  'keyGuide.dpad': '十字キー',
  'keyGuide.turboA': '連射 A',
  'keyGuide.turboB': '連射 B',
  'keyGuide.note':
    'P で一時停止 / 再開。矢印キーは割り当てていません。移動は WASD で。連射キーを押している間はそのボタンが毎秒約 15 回連射されます（通常の押下はそのまま）。',
  'keyGuide.selectQuestion': 'Select はなぜ何度も押す必要がある？',
  'keyGuide.selectAnswer':
    'Select は連打も再試行もせず、実機どおりのタイミングを保っています。タイトル画面は 16 フレームに 1 回しか Select を読まないため、軽いタップは捨てられがちで、1P / 2P の切り替えには数回押す必要があります。',

  'gamepad.title': 'Bluetooth コントローラー',
  'gamepad.none': '未接続',
  'gamepad.note':
    '有線・Bluetooth どちらのコントローラーもそのまま使えます。先にペアリングし（iPad なら「設定 → Bluetooth」）、どれかのボタンを押すとここに表示されます。十字キーと左スティック＝方向。4 つの面ボタンは当時の連射パッドと同じく「通常の 1 対＋連射の 1 対」で、連射はそれが連射するボタンのすぐ隣にあります：下＝A、右＝連射 A、左＝B、上＝連射 B。肩ボタン／トリガーも連射（左＝連射 B、右＝連射 A）なので、親指を A / B に置いたまま連射できます。＋ / − が Start / Select です。押しているボタンは名前の横にそのまま出るので、反応しないときはここを見てください。#番号 はこのフロントエンドに対応するボタンがない入力です。',
  'gamepad.unsupported':
    'このブラウザは Gamepad API を公開しておらず、コントローラーを認識できません。キーボードか仮想パッドをお使いください。',

  'romUrl.title': 'リンクから読み込む',
  'romUrl.label': 'ROM の URL',
  'romUrl.submit': '読み込む',
  'romUrl.note':
    'iNES (.nes) の直リンクに対応。GitHub の Raw / ファイルページのリンクは直リンクに書き換えます。ほかのサイトは CORS が必要です。',

  'saveState.title': 'ステートセーブ',
  'saveState.slot': 'スロット {slot}',
  'saveState.filled': '保存済み',
  'saveState.empty': '空き',
  'saveState.saveTo': 'スロット {slot} に保存',
  'saveState.loadFrom': 'スロット {slot} を読み込む',
  'saveState.clearSlot': 'スロット {slot} を消去',
  'saveState.note': 'マシン全体の状態を保存し、いつでも読み込めます。',

  'gameList.title': 'ゲーム一覧',
  'gameList.searchPlaceholder': 'ゲーム名で検索…',
  'gameList.searchLabel': 'ゲームを検索',
  'gameList.noMatch': '「{query}」に一致するゲームはありません。',
  'gameList.current': 'プレイ中',
  'gameList.footCount': '全 {count} タイトル',
  'gameList.footList': 'ゲーム一覧',
  'gameList.footSource': ' · データ提供: ',

  'clearSave.title': 'カートリッジのセーブを消去しますか？',
  'clearSave.cancel': 'キャンセル',
  'clearSave.confirm': '消去して最初から',
  'clearSave.body':
    'このカートリッジのバッテリーセーブを削除して最初から始めます。元に戻せません。',

  'pad.group': 'バーチャルコントローラ',
  'pad.up': '上',
  'pad.down': '下',
  'pad.left': '左',
  'pad.right': '右',
  'pad.a': 'A ボタン',
  'pad.b': 'B ボタン',
  'pad.ab': 'A と B を同時に',
  'pad.turboA': '連射A',
  'pad.turboB': '連射B',

  'play.back': 'ゲーム一覧に戻る',
  'play.openTools': 'ツールバーを開く',
  'play.noCartName': 'カートリッジなし',
  'play.missingTitle': 'ゲームが見つかりません',
  'play.preparingTitle': 'コアを準備しています…',
  'play.noCartTitle': 'カートリッジ未選択',
  'play.missingHint': '一覧に「{game}」はありません。一覧が更新された可能性があります。',
  'play.noCartHint': '一覧から選ぶか、自分の .nes ファイルを読み込んでください。',

  'dashboard.sub': 'スマホ版 · ゲームを選んで開始',
  'dashboard.localRom': 'ローカル ROM',
  'dashboard.random': 'おまかせで 1 本',

  'tools.title': 'ツールバー',
  'tools.load': '読み込み',

  'disclaimer.line':
    '個人の学習・研究用プロジェクトです · 商用利用なし · ROM / ゲームファイルは提供していません。',
  'disclaimer.link': '免責事項',

  'notification.failed': 'エラーが発生しました',
  'error.gamesUnavailable': 'ゲーム一覧を読み込めませんでした: {reason}',
  'error.emptyFile': '「{name}」は空のファイルです。',
  'error.fileTooLarge': '「{name}」は {size} MB もあり、NES ROM としては大きすぎます。',
  'error.linkMissing': '先に ROM の URL を入力してください。',
  'error.linkInvalid': '「{url}」は有効なリンクではありません。',
  'error.linkScheme': '読み込めるのは http(s) の直リンクのみです（{protocol}// には未対応）。',
  'error.downloadFailed': 'ダウンロードに失敗しました: サーバーが {status} を返しました。',
  'error.notARom': '「{url}」は ROM ではなく Web ページを返しました。ファイルの直リンクをお使いください。',
  'error.timeout': 'ダウンロードがタイムアウトしました（{seconds} 秒応答なし）: {url}',
  'error.fetchFailed': '「{url}」をダウンロードできません: {reason}',
  'error.linkTooLarge': 'そのリンクは {size} MB もあり、NES ROM としては大きすぎます。',
  'error.linkUnavailable':
    '{detail}リンクが無効か、サーバーが CORS を許可していない可能性があります。',
  'error.tooSmall': '「{name}」は小さすぎます（{bytes} バイト）。NES ROM ではありません。',
  'error.badHeader': 'ヘッダーに NES\\x1a の署名がありません。ファイルが壊れている可能性があります。',
  'error.badExtension': '拡張子が .nes ではありません。圧縮ファイルなら展開してください。',
  'error.notInes': '「{name}」は iNES 形式ではありません: {hint}',
  'error.unif': '「{name}」は UNIF/NES 2.0 コンテナです。現在は iNES 形式のみ対応しています。',
  'error.unsupportedMapper':
    '「{name}」は Mapper {mapper} を使用していますが、コアは未実装です（対応: {supported}）。',
  'error.coreRejected': 'エミュレータのコアがこの ROM を拒否しました: {reason}',
  'error.wasmFailed': 'WASM モジュールの読み込みに失敗しました: {reason}',
  'error.coreStopped':
    'コアで例外が発生し、ゲームが停止しました（ROM が壊れている可能性があります）。別の ROM を読み込めば続けられます。詳細はブラウザのコンソールへ。',
  'error.canvasUnavailable': 'Canvas 2D コンテキストを作成できません。',
  'error.disposed': 'エミュレータのインスタンスは破棄されました。',
  'error.coreNotReady': 'WASM コアが未初期化のため、メモリを読み取れません。',
  'error.coreSpecUnavailable': 'WASM コアが未初期化のため、画面仕様を取得できません。',
  'error.pixelFormat': 'コアが未知のピクセル形式を返しました: {format}',
};

export const DICTIONARIES: Record<Locale, Dictionary> = { zh: ZH, en: EN, ja: JA };
