//! 配置读取与合并：把 dsh-pet 的「内置默认 config.jsonc + 用户 main-config.jsonc」
//! 合成为宿主 /config 的成品聚合（{ main: {...} }）。
//!
//! 合并口径对齐插件 src/host/config.ts：
//!   - 顶层字段整段替换（用户写了就用用户那份）；
//!   - pets 数组是条目级：用户写了就用用户的条目，缺失的实例字段从默认 pets[0] 补；
//!   - 缺失的顶层字段静默回落内置默认。
//!
//! 唯一的本地差异：桌面壳里的主窗口就是"桌面"，用户若把 display 写成 web（仅浏览器），
//! 默认把它提升为 both，否则窗口里什么都不会出现（可用 DSH_PET_KEEP_WEB=1 关闭）。

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

/// $DSH_HOME（默认 ~/.dsh）
pub fn home_dir() -> PathBuf {
    if let Ok(v) = std::env::var("DSH_HOME") {
        if !v.trim().is_empty() {
            return PathBuf::from(v);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".dsh")
}

/// JSONC → JSON：去掉 // 行注释与 /* */ 块注释（字符串内的 // 不误伤），并允许尾逗号。
/// 注意：全程按字节处理 —— 不能用 `bytes[i] as char`，那会把 UTF-8 多字节序列拆成乱码
/// （配置里的动画名/宠物名全是中文，踩过一次）。
pub fn strip_jsonc(input: &str) -> String {
    let b = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    while i < b.len() {
        let c = b[i];
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_string = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == b'/' && i + 1 < b.len() {
            if b[i + 1] == b'/' {
                i += 2;
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if b[i + 1] == b'*' {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(b.len());
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    // 尾逗号：, 后只跟空白与 } / ]
    let mut cleaned: Vec<u8> = Vec::with_capacity(out.len());
    let mut k = 0usize;
    while k < out.len() {
        if out[k] == b',' {
            let mut j = k + 1;
            while j < out.len() && (out[j] as char).is_ascii_whitespace() {
                j += 1;
            }
            if j < out.len() && (out[j] == b'}' || out[j] == b']') {
                k += 1;
                continue;
            }
        }
        cleaned.push(out[k]);
        k += 1;
    }
    String::from_utf8_lossy(&cleaned).into_owned()
}

pub fn read_jsonc(path: &Path) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&strip_jsonc(&raw)).ok()
}

fn overlay_entry(merged: &mut Map<String, Value>, user: Map<String, Value>, base_pets: &Value) {
    for (k, v) in user {
        if k == "pets" {
            let merged_pets = merge_pets(base_pets, &v);
            merged.insert(k, merged_pets);
        } else {
            merged.insert(k, v);
        }
    }
}

fn merge_pets(base_pets: &Value, user_pets: &Value) -> Value {
    let base0 = base_pets
        .as_array()
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut out: Vec<Value> = Vec::new();
    if let Some(list) = user_pets.as_array() {
        for p in list {
            if !p.is_object() {
                continue;
            }
            let mut pet = base0.clone();
            if let (Some(dst), Some(src)) = (pet.as_object_mut(), p.as_object()) {
                for (k, v) in src {
                    dst.insert(k.clone(), v.clone());
                }
            }
            out.push(pet);
        }
    }
    if out.is_empty() {
        base_pets.clone()
    } else {
        Value::Array(out)
    }
}

/// 桌面壳的可见性修正 + 实例字段兜底
fn normalize(merged: &mut Value, default: &Value) {
    let keep_web = std::env::var("DSH_PET_KEEP_WEB").map(|v| v == "1").unwrap_or(false);
    let base0 = default
        .get("pets")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Some(list) = merged.get_mut("pets").and_then(|v| v.as_array_mut()) {
        for pet in list.iter_mut() {
            let Some(obj) = pet.as_object_mut() else { continue };
            if !obj.get("id").map(|v| v.is_string()).unwrap_or(false) {
                obj.insert("id".into(), json!("main"));
            }
            if !obj.get("name").map(|v| v.as_string_nonempty()).unwrap_or(false) {
                let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("main").to_string();
                obj.insert("name".into(), json!(id));
            }
            let display = obj.get("display").and_then(|v| v.as_str()).unwrap_or("both").to_string();
            match display.as_str() {
                "desktop" | "both" => {}
                "web" => {
                    if !keep_web {
                        obj.insert("display".into(), json!("both"));
                    }
                }
                "none" => {}
                _ => {
                    obj.insert("display".into(), json!("both"));
                }
            }
            if !obj.get("size").map(|v| v.as_f64().is_some()).unwrap_or(false) {
                let size = base0.get("size").cloned().unwrap_or(json!(462));
                obj.insert("size".into(), size);
            }
            if !obj.get("position").map(|v| v.is_object()).unwrap_or(false) {
                let pos = base0.get("position").cloned().unwrap_or_else(|| {
                    json!({ "corner": "top-right", "marginX": 24, "marginY": 100 })
                });
                obj.insert("position".into(), pos);
            }
            for flag in ["balanceEnabled", "whisperEnabled", "workStatusEnabled", "fixedEnabled"] {
                if !obj.get(flag).map(|v| v.is_boolean()).unwrap_or(false) {
                    let v = base0.get(flag).cloned().unwrap_or(json!(false));
                    obj.insert(flag.into(), v);
                }
            }
        }
    }
}

trait JsonStr {
    fn as_string_nonempty(&self) -> bool;
}
impl JsonStr for Value {
    fn as_string_nonempty(&self) -> bool {
        self.as_str().map(|s| !s.trim().is_empty()).unwrap_or(false)
    }
}

/// 宿主 /config 的返回值：{ main: <合并后的完整条目> }
pub fn merged_config(assets: &Path) -> Value {
    let default = read_jsonc(&assets.join("config.jsonc")).unwrap_or_else(|| json!({}));
    let user_path = home_dir().join("dsh-pet").join("main-config.jsonc");
    let user = read_jsonc(&user_path);

    let mut merged = default.clone();
    if let (Some(dst), Some(src)) = (merged.as_object_mut(), user.and_then(|u| u.as_object().cloned())) {
        let base_pets = default.get("pets").cloned().unwrap_or_else(|| json!([]));
        overlay_entry(dst, src, &base_pets);
    }
    normalize(&mut merged, &default);
    json!({ "main": merged })
}

/// 用户数据目录（$DSH_HOME/dsh-pet）：配置、记忆、自定义动画/表情包
pub fn user_data_dir() -> PathBuf {
    home_dir().join("dsh-pet")
}
