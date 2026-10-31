# 星遇同频（vrcc）排查指南

给帮忙复现问题的人看。目标是：**只靠日志，能判断问题出在哪一层。**

## 一、先拿日志

错误日志在应用数据目录：

```
%APPDATA%\VRCX-0\error-log.txt
```

完整路径通常是：

```
C:\Users\<你的用户名>\AppData\Roaming\VRCX-0\error-log.txt
```

在文件管理器地址栏粘 `%APPDATA%\VRCX-0` 就能直接打开。

**这个文件只装 `ERROR` 级别的事件**，所以它回答的是「出过什么错」，不是
「一切正常时走到哪一步了」。要看后者，见下一节的 `RUST_LOG`。

> 早先这里还有一个 `diagnostic-log.txt`（收 debug 及以上的完整时间线）。
> 合并上游时已移除：那一层把整个应用所有 crate 的 DEBUG 都写盘，每条都要
> 在全局日志锁下同步打开并追加文件；文件超过 10MB 后几乎每条都要读写整个
> 文件做裁剪，而且都发生在 tokio worker 线程上。为了排查一个功能而持续
> 拖慢整个应用不划算。需要同样效果时请显式设 `RUST_LOG`。

## 二、想要更详细的日志

默认级别是 `info`。要看每一轮轮询的细节（包括请求耗时、跳过原因），用环境变量启动：

```powershell
$env:RUST_LOG = "vrcx_0=debug"
& "路径\vrcx-0.exe"
```

改完记得新开终端，或 `Remove-Item Env:RUST_LOG` 复位。

## 三、日志怎么看

### 启动阶段（正常应该看到这些）

```
vrcc: starting background poll  has_desktop_notifier=true token_connected=false client=dev-VRCX-0-Galaxies client_is_dev=true
vrcc: background poll task started  interval_secs=60 jitter_secs=20
vrcc: background poll scheduled
```

逐个字段的含义：

| 字段                   | 期望                  | 不对时的含义                                                          |
| ---------------------- | --------------------- | --------------------------------------------------------------------- |
| `has_desktop_notifier` | `true`                | `false` = 通知桥没挂上（Headless 或装配顺序问题），**不会有系统通知** |
| `token_connected`      | 连过之后 `true`       | `false` = 还没连星遇同频，轮询会跳过                                  |
| `client`               | `dev-VRCX-0-Galaxies` | 换正式标识后应是 `app_` 开头                                          |
| `client_is_dev`        | 开发态 `true`         | `true` 时拿不到敏感权限，`mutuals` 等必然 403                         |

**如果连这四行都没有**，说明轮询根本没起来——检查日志里有没有 `background poll not scheduled`（任务被拒绝）。

### 运行阶段（每轮一条，需 debug 级）

每轮先问 `me/notifications?since=`（新提醒，没有每天上限），再问 `me/unread`（几处未读数，每天 1500 次）。

```
vrcc: background poll round finished  round=3 fresh=1 unread=Some(3) toast_attempted=1 changed=true
vrcc: background poll skipped (not connected)  round=4
vrcc: background poll sleeping         round=3 delay_ms=71234
```

`unread=None` 表示服务器还没有 `me/unread`（旧版接口），这时只在刚连上或来了新提醒时拉一次「最近的提醒」。

### 拿到提醒时

```
vrcc: fetched new notices      fresh=1
vrcc: desktop notification dispatched  notice_id=1026 body_len=42
```

每条提醒只弹一次：`me/notifications` 只给游标之后的。刚连上（或刚启动）的第一轮服务器只告诉「现在到哪了」，不给旧提醒，所以启动时不会把旧提醒再弹一遍——它们在「提醒」页里看得到。

### 通知没弹出去时

```
vrcc: desktop notification not delivered  notice_id=1026 error=...
```

| 错误内容            | 原因                                                |
| ------------------- | --------------------------------------------------- |
| 无错误但也没弹      | 窗口处于后台且系统通知被关闭（Windows 设置 → 通知） |
| `notification: ...` | 系统级失败，通常是应用未注册到通知中心              |

### 超过每轮上限时

```
vrcc: new notices exceed per-poll toast limit; older ones not toasted  fresh=5 limit=3 skipped=2
```

一轮来了太多，只弹最新的三条，其余的在「提醒」页里看。**不是错误**，但连续出现说明轮询被中断过。

### 连接失效时

```
vrcc: token is gone (disconnected on the site, re-authorized elsewhere, or unused for 90 days); dropping it
vrcc: token gone, background poll idle until reconnect
```

服务器回了 `404 gone`：玩家在网页上断开了、在别处重新授权过，或 90 天没调用过任何接口被自动断开。本地令牌已经清掉，「我的」页会提示重新连接。

## 四、前端侧的对应证据

前端日志在同一个文件里，前缀是 `[vrcc]`：

| 日志                                                  | 含义                                           |
| ----------------------------------------------------- | ---------------------------------------------- |
| `[vrcc] vrccNoticesChanged event subscribed`          | 事件订阅成功。**没有这行，界面就不会自动刷新** |
| `[vrcc] notices changed event received`               | 后端推来了信号                                 |
| `[vrcc] connection changed event received`            | 后端发现令牌失效，界面改显示「请重新连接」     |
| `[vrcc] events unsubscribed`                          | 页面切走或连接断开，正常退订                   |
| `[vrcc] subscribe vrccNoticesChanged event failed: …` | 订阅失败，界面只能手动刷新                     |
| `[vrcc] load failed: ...`                             | 首屏加载失败                                   |

**关键判断**：如果日志里有 `desktop notification dispatched`（后端弹了通知）但**没有** `notices changed event received`（前端没收到），问题在事件通道；如果两者都有但界面没变，问题在前端刷新逻辑。

## 五、常见症状对照

### 症状：通知完全不来

按顺序核对：

1. `starting background poll` 有没有出现？没有 → 轮询没起
2. `has_desktop_notifier` 是 `true` 吗？`false` → 通知桥没挂
3. `token_connected` 是 `true` 吗？`false` → 没连星遇同频
4. `round finished` 的 `fresh` 是 0 吗？是 → 这段时间确实没有新提醒
5. `fresh > 0` 但无 `dispatched` → 看有没有 `not delivered`
6. 以上都正常 → 检查 Windows 系统通知设置，以及「专注助手」是否开启

### 症状：网页上有提醒，但没弹系统通知

提醒是在应用关着的时候来的：启动后的第一轮只记下「现在到哪了」，不补弹旧提醒。它们在「提醒」页里，页签上有未读数。这是按官方的轮询设计来的，不是 bug。

### 症状：界面显示「未连接」但日志 `token_connected=true`

前端 `connection` 状态没刷到。看 `[vrcc] load failed`。

### 症状：`403 scope` / `scope_unreviewed`

`dev-` 标识只能要普通权限。`mutuals`、`today`、`crossings` 等敏感权限需要
应用登记并通过审核后，换成 `app_…` 标识才能用。**这是预期行为，不是缺陷。**

用 `app_…` 标识、「我的」里权限只有 `profile`：这是旧版本连上的令牌。旧版本
申请连接码时没带 `scope`，服务器对自助登记的应用默认只给 `profile`。更新后在
「我的」里重新连接一次就好。同意页没有勾选框，玩家只能全部同意或取消，所以要哪些
权限只取决于客户端发的 `scope`（见 `device_auth_scope`）。

### 症状：`接口暂时不可用` / `error sending request`

网络层。检查：

1. 应用设置里的代理是否填对（本机需要代理才能访问 `vrcc.cc`）
2. 日志里有没有 `vrcc: no proxy configured, connecting directly`
3. 有没有 `vrcc: invalid proxy url, using direct`（代理串写错）

## 六、手工验证网络

不依赖应用，直接测接口：

```powershell
# 不走代理
curl.exe -sS -o NUL -w "%{http_code}`n" https://vrcc.cc/api/v1/status

# 走代理（按你实际的代理端口改）
curl.exe -sS -o NUL -w "%{http_code}`n" --ssl-no-revoke -x http://127.0.0.1:7890 https://vrcc.cc/api/v1/status
```

返回 `200` 说明网络通。两个都通说明代理不是必需的；只有走代理才通，就要在应用里配代理。

## 七、报告问题时请附上

1. `error-log.txt`，以及按上一节设 `RUST_LOG=vrcx_0=debug` 之后的完整输出
   （**建议先搜 `vrcc` 只贴相关行**，其余内容可能含个人信息）
2. 上面「手工验证网络」两条命令的输出
3. `has_desktop_notifier` / `token_connected` 两行的实际值
4. 系统版本（Win10 / Win11），以及是否开了专注助手

---

## 附：探针清单（给开发者）

| 位置                                   | 级别  | 内容                                   |
| -------------------------------------- | ----- | -------------------------------------- |
| `state.rs::start_vrcc_background_poll` | info  | 启动前提四要素                         |
| `vrcc.rs::spawn_background_poll`       | info  | 任务启动、停止请求                     |
| 同上                                   | debug | 每轮结果、休眠时长、跳过原因           |
| 同上                                   | warn  | 轮询失败、任务未被调度                 |
| `poll_once`                            | debug | 这一轮的新提醒条数                     |
| `poll_once`                            | info  | 每条通知投递成功（含 id 与正文字节数） |
| `poll_once`                            | warn  | 投递失败、超每轮上限、未读数拿不到     |
| 应用层 `VrccService::settle`           | info  | 令牌失效（`404 gone`），已清掉         |
| 前端 `useVrccController`               | info  | 订阅、收事件、退订                     |
| 前端 `useVrccController`               | error | 订阅失败、加载失败、跳转失败           |

**正文内容从不进日志**，只记 `body_len` 与 `notice_id`——提醒正文属于玩家数据。
