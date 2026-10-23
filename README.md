# dsh-pet 🐾
## DSH linux下 faltpak版 桌面宠物，我在原作者基础上采用tauri进行了构建，并生成了flatpak包

## 中文：

  把 dsh-pet 桌宠装进 Tauri：Fedora / GNOME Wayland 原生透明覆盖层，可点击穿透、可拖拽，支持 Flatpak。

## English：
 
  dsh-pet desktop pet in a Tauri shell — a native Wayland transparent overlay for GNOME/Fedora with click-through, drag & drop, and Flatpak packaging.

Topics： tauri rust wayland gnome desktop-pet flatpak webkitgtk fedora linux gtk3


## ✨ 功能特性

- **纯粹的桌宠**：不掺业务功能
- **手绘风透明动画**：待机呼吸、打瞌睡、玩魔方、哼歌、炸毛、吐泡泡、玩水枪、小提琴演奏、蓝鲸现世、吃白饭、照镜子、三支舞、写代码、四季动作（放风筝、堆雪人、吃冰淇淋、放烟花……）全部无缝衔接
- **永不停止的动画链**：每段动画播完立即按权重选下一个（默认 idle 10 / turn 5 / move 5，剩余 80% 归随机动作分类）
- **屏幕漫游**：朝 facing 方向行走，自动检查空间、不走出屏幕
- **点击 / 拖拽（弹簧跟手 + 甩抛反弹 + Q 弹）**：点击有随机回应动画（开心 / 害羞 / 傲娇）并「**Q 弹**」挤压回弹（垂直压扁 55% → easeOutBack 回弹过冲，贴地锚定）；拖拽为**过阻尼弹簧跟手**（不抖不飘、无 overshoot）；松手按最后轨迹估速（停顿/慢速 = 温柔放下原地停住）——**用力甩出即沿抛物线飞行：撞屏幕边缘按恢复系数反弹、落地摩擦减速至停，每次落地按冲击速度 Q 弹（轻落 0.8 ~ 重砸 0.55）**；物理与挤压曲线纯函数在 `src/shared/physics.ts`，浏览器与桌面严格同一手感（`prefers-reduced-motion` 时跳过挤压）
- **物理参数化（0.2.5）**：拖拽抛掷手感全部由配置 `physics` 段驱动——重力（`gravity`，**0 = 无重力漂浮**）/ 碰壁恢复系数（`restitution`）/ 地面摩擦（`groundFriction`，0 = 冰面）/ 顶部反弹开关（`ceilingBounce`）/ 总力度（`throwPower`）/ 多宠物碰撞开关（`petCollision`），所有宠物共用（**全局默认**：设置页改一次对含 pet pack 的全部宠物生效；种类文件顶层可覆盖，见配置节）；**设置页「物理」区可图形化编辑**（6 个控件，与四个全局开关同一套语义：改完点「保存」整包写入用户配置，非法值就地红字提示、宿主也会拒绝）
- **多宠物碰撞（0.2.5）**：`petCollision: true` 开启后，飞行中的宠物撞到其它宠物按**动量守恒 + 恢复系数 0.995** 弹开（质量 ∝ size²，被撞方从落点以新初速抛出去），浏览器与桌面跨窗口同语义（默认关闭）
- **碎碎念**：宠物时不时自己冒一句——按 `eventsRefreshSec.whisper` 周期（默认 300 秒）调用当前会话所用的模型生成（人设 = **所属条目**的 `whisperPrompt`，每个种类一份，另追加一句名字声明），气泡展示 10 秒；右键菜单「碎碎念」可立即催一句（绕过节流，同一实例的多端一起看到）。**默认开启**，按宠物可关（`pets[].whisperEnabled`）
- **对话**：右键「对话」弹窗跟宠物聊天，也能用 `/chat <消息>` 命令（留空 = 催一句碎碎念）——记忆持久化在 `$DSH_HOME/dsh-pet/memory.json`（**全存不删**，每次请求只带最近 `chatMemoryRounds` 轮），浏览器与桌面共享同一份记忆；对话目标为 `/pet` 选中的那只，未选则取列表第一只
- **表情包配图（0.2.9）**：气泡可以带一张表情包——碎碎念**随机抽 1 张**（`whisperImageEnabled`：只是把这张图的描述加进同一次请求，约 +100 字符 / ≈60 token，增量可忽略）；对话把**整张清单**交给模型按语境选（`chatImageEnabled`：每条消息约 +1.1k 字符 / ≈650 token，约碎碎念配图的 11 倍，随图片数量线性增长）。图片与描述的映射在 `memes`（键 = 表情包目录里的 `<键>.png`；素材归属与动画同一套，见「高级自定义」），两个开关默认都开
- **右键级联菜单**：右键宠物弹出（桌面与浏览器共用同一份组件，`src/shared/menu.ts`）——桌面端根项「**打开网站** / **查看余额** / **回到初始位置** / **重载配置** + **动作**」、浏览器端「**回到初始位置** + **动作**」；「打开网站」用**系统默认浏览器**打开 DSH 网站（等效网页里 Ctrl+点击链接）；「查看余额」立即拉余额弹气泡播档位动画（与周期触发同一展示路径）；「回到初始位置」停漫游回配置角落；「重载配置」（桌面专属）请宿主重启桌面 Helper——**全部桌面宠物窗口按最新配置重建**，改完配置文件不必回设置页点保存（与保存同一条重启路径：等旧进程退出再起新的；代价是会话内拖拽落点清空、回配置角落）；**动作 → 分类 → 具体动画**（分类 = 待机/转向/拖拽/点击回应/移动/随机动作分类/余额档位；**点播「移动」分类动画会真实行走一段**——边界检查/随机距离/起停时段与随机移动完全一致；noMirror 文字类朝右时自动强制朝左）——浏览器端只在宠物命中区拦截右键（`preventDefault`），完全不进入/改动 DSH 页面自己的菜单。**「打开网站」按宿主自动分派**：宿主是 DSH 桌面应用时该项自动变「**打开 DeepSeek Harness**」，用应用自注册的 `dsh://open` 协议把桌面应用主窗口唤到前台（已运行前置 / 未运行拉起）；`dsh web` 与独立模式仍是系统默认浏览器打开站点
- **左右朝向**：所有动画 CSS 镜像，人物可朝左 / 朝右
- **落地对齐**：动画统一脚底线，宠物始终站在"地面"上
- **流畅切换**：双缓冲 video 交叉淡入，切换零空白帧



## 🎬 效果预览

> 动画为透明背景；GIF 预览中透明部分显示为页面底色，实际播放为透明。

<p>
  <img src="https://raw.githubusercontent.com/PC2005-cloud/dsh-pet/main/dsh-pet/assets/preview/daiji-huxi-xiuxian.gif" width="160" alt="待机呼吸休闲" title="待机呼吸休闲">
  <img src="https://raw.githubusercontent.com/PC2005-cloud/dsh-pet/main/dsh-pet/assets/preview/dongzhangxiwang.gif" width="160" alt="东张西望" title="东张西望">
  <img src="https://raw.githubusercontent.com/PC2005-cloud/dsh-pet/main/dsh-pet/assets/preview/yuandi-piaofu-tabu.gif" width="160" alt="原地漂浮踏步" title="原地漂浮踏步">
  <img src="https://raw.githubusercontent.com/PC2005-cloud/dsh-pet/main/dsh-pet/assets/preview/yuandi-xiaoqi-chenmian.gif" width="160" alt="原地小憩沉眠" title="原地小憩沉眠">
  <img src="https://raw.githubusercontent.com/PC2005-cloud/dsh-pet/main/dsh-pet/assets/preview/dianji-huiying-kaixin-yuedong.gif" width="160" alt="点击回应 - 开心跃动" title="点击回应 - 开心跃动">
  <img src="https://raw.githubusercontent.com/PC2005-cloud/dsh-pet/main/dsh-pet/assets/preview/beishubiao-tuozhuai-xuankong-fankui.gif" width="160" alt="被鼠标拖拽悬空反馈" title="被鼠标拖拽悬空反馈">
</p>

全部动画见仓库：`dsh-pet/assets/webm/`（VP9-alpha，唯一发布格式）。


## 📄 许可

- 代码：MIT
- 素材（动画/提示词/源视频）：允许开源使用，**禁止商用**
