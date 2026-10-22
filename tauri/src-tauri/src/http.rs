//! 极简本机 HTTP 宿主：替代 dsh-pet 的 Node 宿主半边，只提供桌面渲染端真正用到的端点。
//!
//!   GET  /dsh-pet-7340/config            合并后的配置成品（{ main: ... }）
//!   GET  /dsh-pet-7340/state             统一状态仓（本壳无 DSH 会话 → 空 sections）
//!   GET  /dsh-pet-7340/thumb/<根>/<名>   动画素材（用户 main-animation/webm 优先 → 包内 assets/webm）
//!   GET  /dsh-pet-7340/pic/...           光标/通知图与表情包
//!   GET  /dsh-pet-7340/font/<名>.ttf     气泡字体
//!   GET  /dsh-pet-7340/chat              记忆窗口（壳里恒空）
//!   POST /dsh-pet-7340/chat              明确失败（无模型）
//!   POST /dsh-pet-7340/whisper           本地「碎碎念」：立刻生成一句（预设文案 + 随机表情包）
//!   POST /dsh-pet-7340/balance           明确失败（桌面壳不接余额）
//!   POST /dsh-pet-7340/reload            返回 ok（重载由托盘/菜单触发，壳里不重启窗口）
//!
//! 只监听 127.0.0.1，带 `Access-Control-Allow-Origin: *`（页面来自 tauri://localhost，属跨源）。

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use percent_encoding::percent_decode_str;
use serde_json::{json, Value};

use crate::config;

#[derive(Clone)]
pub struct Host {
    pub assets: PathBuf,
    whisper: Arc<Mutex<Whisper>>,
}

/// 本地碎碎念：纯离线，不接模型。
///
/// `/state` 里 `pets.<id>.say = { counter, data: { text, image } }`，渲染端轮询到 counter
/// 变化就播说话动画 + 气泡；`image` 是配置 memes 的键，渲染端拼成
/// `/pic/memes/<素材根>/<名>.png`（本壳回落 `assets/memes/<名>.png`）。
pub struct Whisper {
    pet_id: String,
    counter: u64,
    text: String,
    image: String,
    last: Instant,
    every: Duration,
    auto: bool,
    with_image: bool,
    lines: Vec<String>,
    memes: Vec<String>,
    last_meme: Option<usize>,
}

const WHISPER_LINES: &[&str] = &[
    "主人，喝口水再继续吧喵～",
    "坐太久啦，起来伸个懒腰嘛。",
    "今天也要好好吃饭哦。",
    "这个 Bug 看起来很好吃……不对，很好修！",
    "要不要摸摸头？就一下下。",
    "我在这里陪你，慢慢来就好。",
    "屏幕有点亮，眼睛也要休息一下。",
    "刚刚是不是又忘了保存？",
    "忙完记得来找我玩呀。",
    "偷偷说：你已经做得很好了。",
    "要不要休息五分钟？我帮你看着。",
    "别急，一口一口来。",
];

impl Host {
    pub fn new(assets: PathBuf) -> Self {
        let cfg = config::merged_config(&assets);
        let main = cfg.get("main");
        let pet = main
            .and_then(|m| m.get("pets"))
            .and_then(|p| p.as_array())
            .and_then(|a| a.first());
        let pet_id = pet
            .and_then(|p| p.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("main")
            .to_string();
        let whisper_enabled = pet
            .and_then(|p| p.get("whisperEnabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let with_image = main
            .and_then(|m| m.get("whisperImageEnabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // 周期：DSH_PET_WHISPER_SEC，默认 120s；0 = 关闭自动碎碎念（菜单手动触发仍可用）。
        let every_sec = std::env::var("DSH_PET_WHISPER_SEC")
            .ok()
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(120.0);
        let every = Duration::from_secs_f64(every_sec.max(0.001));

        // 文案：可用 DSH_PET_WHISPER_LINES 指向一个「一行一句」的文本文件覆盖。
        let mut lines: Vec<String> = WHISPER_LINES.iter().map(|s| s.to_string()).collect();
        if let Some(path) = std::env::var_os("DSH_PET_WHISPER_LINES") {
            if let Ok(text) = fs::read_to_string(PathBuf::from(path)) {
                let custom: Vec<String> = text
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                if !custom.is_empty() {
                    lines = custom;
                }
            }
        }

        // 表情包：包内 assets/memes + 用户 <素材根>-memes，取文件名（去 .png）当键。
        let mut memes: Vec<String> = Vec::new();
        let mut collect = |dir: &Path| {
            if let Ok(rd) = fs::read_dir(dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("png") {
                        if let Some(stem) = p.file_stem().and_then(|x| x.to_str()) {
                            memes.push(stem.to_string());
                        }
                    }
                }
            }
        };
        collect(&assets.join("memes"));
        collect(&config::user_data_dir().join(format!("{pet_id}-memes")));
        memes.sort();
        memes.dedup();

        let auto = whisper_enabled && every_sec > 0.0;
        if auto {
            eprintln!(
                "[dsh-pet] 碎碎念：每 {every_sec:.0}s 一句（{} 条文案 / {} 张表情包）",
                lines.len(),
                memes.len()
            );
        }

        Self {
            assets,
            whisper: Arc::new(Mutex::new(Whisper {
                pet_id,
                counter: 0,
                text: String::new(),
                image: String::new(),
                last: Instant::now(),
                every,
                auto,
                with_image,
                lines,
                memes,
                last_meme: None,
            })),
        }
    }
}

impl Whisper {
    fn rand_index(n: usize) -> usize {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize)
            .unwrap_or(0);
        nanos % n.max(1)
    }

    /// 立刻产出一句（菜单 / POST /whisper 也走这里）
    fn emit(&mut self) {
        if self.lines.is_empty() {
            return;
        }
        self.counter = self.counter.saturating_add(1);
        self.text = self.lines[Self::rand_index(self.lines.len())].clone();
        self.image = if self.with_image && !self.memes.is_empty() {
            let mut idx = Self::rand_index(self.memes.len());
            if self.memes.len() > 1 && Some(idx) == self.last_meme {
                idx = (idx + 1) % self.memes.len();
            }
            self.last_meme = Some(idx);
            self.memes[idx].clone()
        } else {
            String::new()
        };
        self.last = Instant::now();
    }

    fn leaf(&self) -> Value {
        json!({
            "counter": self.counter,
            "data": { "text": self.text, "image": self.image }
        })
    }
}

pub fn spawn(host: Host) -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();

    // 周期碎碎念：每秒看一眼是否到点（自动模式才跑）
    {
        let whisper = host.whisper.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(500));
            let Ok(mut w) = whisper.lock() else { break };
            if w.auto && w.last.elapsed() >= w.every {
                w.emit();
            }
        });
    }

    thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(stream) = conn else { break };
            let host = host.clone();
            thread::spawn(move || {
                if let Err(e) = handle(stream, &host) {
                    eprintln!("[dsh-pet-http] 连接处理失败: {e}");
                }
            });
        }
    });
    Ok(port)
}

fn handle(mut stream: TcpStream, host: &Host) -> std::io::Result<()> {
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).ok();
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(());
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_ascii_uppercase();
    let target = parts.next().unwrap_or("/").to_string();

    let mut headers: HashMap<String, String> = HashMap::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 {
            break;
        }
        let t = h.trim_end_matches(['\r', '\n']);
        if t.is_empty() {
            break;
        }
        if let Some((k, v)) = t.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    if len > 0 {
        reader.read_exact(&mut body)?;
    }

    let (raw_path, _query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target.clone(), String::new()),
    };
    let path = percent_decode_str(&raw_path).decode_utf8_lossy().to_string();
    let resp = route(&method, &path, &headers, host);
    if std::env::var("DSH_PET_VERBOSE").map(|v| v == "1").unwrap_or(false) {
        eprintln!(
            "[dsh-pet-http] {} {} -> {}{}",
            method,
            path,
            resp.status,
            headers.get("range").map(|r| format!(" (range {r})")).unwrap_or_default()
        );
    }
    write_response(&mut stream, resp)
}

struct Resp {
    status: u16,
    ctype: &'static str,
    body: Vec<u8>,
    extra: Vec<(String, String)>,
}

impl Resp {
    fn json(v: Value) -> Self {
        Resp {
            status: 200,
            ctype: "application/json; charset=utf-8",
            body: serde_json::to_vec(&v).unwrap_or_default(),
            extra: Vec::new(),
        }
    }
    fn text(body: &str) -> Self {
        Resp {
            status: 200,
            ctype: "text/plain; charset=utf-8",
            body: body.as_bytes().to_vec(),
            extra: Vec::new(),
        }
    }
    fn status(status: u16, msg: &str) -> Self {
        Resp {
            status,
            ctype: "text/plain; charset=utf-8",
            body: msg.as_bytes().to_vec(),
            extra: Vec::new(),
        }
    }
    fn not_found(what: &str) -> Self {
        Resp {
            status: 404,
            ctype: "application/json; charset=utf-8",
            body: serde_json::to_vec(&json!({ "ok": false, "reason": "not-found", "message": what }))
                .unwrap_or_default(),
            extra: Vec::new(),
        }
    }
}

const PREFIX: &str = "/dsh-pet-7340";

fn route(method: &str, path: &str, headers: &HashMap<String, String>, host: &Host) -> Resp {
    if method == "OPTIONS" {
        return Resp {
            status: 204,
            ctype: "text/plain",
            body: Vec::new(),
            extra: Vec::new(),
        };
    }
    if path == "/" || path == "/health" {
        return Resp::json(json!({
            "name": "dsh-pet-tauri-host",
            "ok": true,
            "assets": host.assets.display().to_string(),
        }));
    }
    if !path.starts_with(PREFIX) {
        return Resp::not_found("未知路径");
    }

    let config_path = format!("{PREFIX}/config");
    let state_path = format!("{PREFIX}/state");
    let chat_path = format!("{PREFIX}/chat");
    let balance_path = format!("{PREFIX}/balance");
    let whisper_path = format!("{PREFIX}/whisper");
    let reload_path = format!("{PREFIX}/reload");
    if path == config_path {
        return Resp::json(config::merged_config(&host.assets));
    }
    if path == state_path {
        // 只有 say 这一个叶子：counter 从 0 起（首拉记基线），之后每句 +1 渲染端就播。
        let w = host.whisper.lock().map(|g| g.leaf()).unwrap_or(Value::Null);
        let pet_id = host
            .whisper
            .lock()
            .map(|g| g.pet_id.clone())
            .unwrap_or_else(|_| "main".into());
        return Resp::json(json!({
            "sections": {},
            "pets": { pet_id: { "say": w } }
        }));
    }
    if path == chat_path {
        return if method == "GET" {
            Resp::json(json!({ "ok": true, "messages": [], "rounds": 5 }))
        } else {
            Resp::json(json!({
                "ok": false,
                "reason": "no-model",
                "message": "Tauri 独立壳未接入 DSH 模型，桌宠对话不可用"
            }))
        };
    }
    if path == balance_path {
        return Resp::json(json!({
            "ok": false,
            "reason": "unsupported",
            "message": "Tauri 独立壳未接入余额接口"
        }));
    }
    if path == whisper_path {
        if method == "POST" {
            // 菜单「碎碎念」/ 外部触发：立刻产出一句，文本随下一次 /state 拉取到达渲染端。
            let ok = host.whisper.lock().map(|mut w| { w.emit(); true }).unwrap_or(false);
            return Resp::json(json!({ "ok": ok }));
        }
        return Resp::json(json!({
            "ok": false,
            "reason": "no-model",
            "message": "Tauri 独立壳未接入 DSH 模型；碎碎念改由本地文案提供"
        }));
    }
    if path == reload_path {
        return Resp::json(json!({ "ok": true, "reloaded": false }));
    }

    let thumb_prefix = format!("{PREFIX}/thumb/");
    if let Some(rest) = path.strip_prefix(&thumb_prefix) {
        return match rest.split_once('/') {
            Some((root, file)) if !file.is_empty() => {
                let user = config::user_data_dir()
                    .join(format!("{root}-animation"))
                    .join("webm")
                    .join(file);
                let p = if user.is_file() {
                    user
                } else {
                    host.assets.join("webm").join(file)
                };
                serve_file(&p, headers)
            }
            _ => Resp::not_found("thumb 路径应为 <素材根>/<文件名>"),
        };
    }

    let pic_prefix = format!("{PREFIX}/pic/");
    if let Some(rest) = path.strip_prefix(&pic_prefix) {
        if let Some(memes) = rest.strip_prefix("memes/") {
            return match memes.split_once('/') {
                Some((root, file)) => {
                    let user = config::user_data_dir()
                        .join(format!("{root}-memes"))
                        .join(file);
                    let p = if user.is_file() {
                        user
                    } else {
                        host.assets.join("memes").join(file)
                    };
                    serve_file(&p, headers)
                }
                None => serve_file(&host.assets.join("memes").join(memes), headers),
            };
        }
        return serve_file(&host.assets.join("pic").join(rest), headers);
    }

    let font_prefix = format!("{PREFIX}/font/");
    if let Some(rest) = path.strip_prefix(&font_prefix) {
        return serve_file(&host.assets.join("fonts").join(rest), headers);
    }

    Resp::not_found("未知 dsh-pet 端点")
}

/// 只允许"没有 .. / 绝对路径 / 空段"的相对路径
fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() {
        return None;
    }
    let mut out = root.to_path_buf();
    for seg in rel.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." || seg.contains('\\') {
            return None;
        }
        out.push(seg);
    }
    if out.starts_with(root) {
        Some(out)
    } else {
        None
    }
}

fn serve_file(path: &Path, headers: &HashMap<String, String>) -> Resp {
    let Some(safe) = safe_join(path.parent().unwrap_or(Path::new(".")), path.file_name().and_then(|s| s.to_str()).unwrap_or("")) else {
        return Resp::not_found("非法文件名");
    };
    let Ok(data) = fs::read(&safe) else {
        return Resp::not_found(&format!("素材不存在: {}", path.display()));
    };
    let ctype = mime_for(&safe);
    let total = data.len();
    if let Some((start, end)) = headers
        .get("range")
        .and_then(|raw| parse_range(raw, total))
    {
        let slice = data[start..=end].to_vec();
        return Resp {
            status: 206,
            ctype,
            body: slice,
            extra: vec![(
                "Content-Range".into(),
                format!("bytes {start}-{end}/{total}"),
            )],
        };
    }
    Resp {
        status: 200,
        ctype,
        body: data,
        extra: Vec::new(),
    }
}

/// 单段 Range：bytes=start-end / bytes=start- / bytes=-suffix
fn parse_range(raw: &str, total: usize) -> Option<(usize, usize)> {
    if total == 0 {
        return None;
    }
    let spec = raw.trim().strip_prefix("bytes=")?;
    let spec = spec.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    let (start, end) = if a.is_empty() {
        let n: usize = b.parse().ok()?;
        (total.saturating_sub(n), total - 1)
    } else {
        let start: usize = a.parse().ok()?;
        let end = if b.is_empty() {
            total - 1
        } else {
            b.parse::<usize>().ok()?.min(total - 1)
        };
        (start, end)
    };
    if start > end || start >= total {
        return None;
    }
    Some((start, end))
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "json" | "jsonc" => "application/json; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn write_response(stream: &mut TcpStream, r: Resp) -> std::io::Result<()> {
    let reason = match r.status {
        200 => "OK",
        204 => "No Content",
        206 => "Partial Content",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\n\
         Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: content-type\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\nCache-Control: no-store\r\n\
         Connection: close\r\n",
        r.status,
        reason,
        r.ctype,
        r.body.len()
    );
    for (k, v) in &r.extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    if r.status != 204 {
        stream.write_all(&r.body)?;
    }
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}
