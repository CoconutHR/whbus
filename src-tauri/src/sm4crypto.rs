//! 公交 API 的加密层：SM4-ECB + PKCS#7。
//!
//! # 协议
//!
//! `/sec/list` 返回当日密钥素材（明文）：
//! ```json
//! { "version": "20261013", "content": "3b9d361d3687f208" }
//! ```
//!
//! 加密接口（`/bus/line`、`/bus/detailData`、`/bus/station`、`/bus/nearBy`）
//! 的 `data` 字段是 **hex 编码的 SM4-ECB 密文**，密钥为固定盐值与 `content`
//! 拼接后的前 16 字节：
//!
//! ```text
//! key = bytes.fromhex("5ba86bc8c561d139" + content)[:16]
//! ```
//!
//! `version` 需作为 `secretVersion` 回传给加密接口，**每日轮换**。
//! 过期会得到 `500001 密钥已失效，请更新`。
//!
//! # 为什么解密必须在 Rust 侧
//!
//! 密钥是全 App 共用的敏感素材，且每日轮换。放在 Rust 侧意味着：
//! 密钥永不进入 WebView；前端只拿到干净的 JSON；轮换逻辑集中一处。

use cipher::{BlockCipherDecrypt, KeyInit};
use sm4::Sm4;

/// 协议固定盐值（来自上游实现，非本项目机密）
pub const SALT: &str = "5ba86bc8c561d139";

/// `/sec/list` 的响应素材
#[derive(Debug, Clone)]
pub struct SecretMaterial {
    /// 密钥版本，格式 `yyyyMMdd`，需回传为 `secretVersion`
    pub version: String,
    /// 16 位 hex 字符串，参与密钥构成
    pub content: String,
    /// 派生出的 16 字节 SM4 密钥
    pub key: [u8; 16],
}

impl SecretMaterial {
    /// 由 `/sec/list` 的 `version` / `content` 派生密钥。
    pub fn derive(version: impl Into<String>, content: &str) -> Result<Self, CryptoError> {
        let raw = format!("{SALT}{content}");
        let bytes = hex::decode(&raw).map_err(|e| CryptoError::BadKeyMaterial(e.to_string()))?;
        if bytes.len() < 16 {
            return Err(CryptoError::BadKeyMaterial(format!(
                "密钥素材不足 16 字节：{}",
                bytes.len()
            )));
        }
        let mut key = [0u8; 16];
        key.copy_from_slice(&bytes[..16]);
        Ok(Self {
            version: version.into(),
            content: content.to_string(),
            key,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("密钥素材非法：{0}")]
    BadKeyMaterial(String),
    #[error("密文不是合法 hex：{0}")]
    BadHex(String),
    #[error("密文长度非法：{0} 字节（必须是 16 的整数倍）")]
    BadLength(usize),
    #[error("SM4 解密失败：{0}")]
    Decrypt(String),
    #[error("解密后的内容里找不到 JSON（前 64 字节：{0:?}）")]
    NoJson(String),
    #[error("解密出的 JSON 无法解析：{0}")]
    Json(#[from] serde_json::Error),
}

/// 解密一个加密接口的 `data` 字段并解析为 `T`。
///
/// 完整的四步：hex 解码 → SM4 解密 → 去 PKCS#7 填充 → 截取 JSON 主体。
pub fn decrypt_data<T: serde::de::DeserializeOwned>(
    data_hex: &str,
    key: &[u8; 16],
) -> Result<T, CryptoError> {
    let raw = decrypt_bytes(data_hex, key)?;
    let json = extract_json(&raw)?;
    Ok(serde_json::from_slice(json)?)
}

/// 解密到原始字节（已去填充），不做 JSON 解析。
pub fn decrypt_bytes(data_hex: &str, key: &[u8; 16]) -> Result<Vec<u8>, CryptoError> {
    let cleaned: String = data_hex.chars().filter(|c| !c.is_whitespace()).collect();
    let ciphertext = hex::decode(&cleaned).map_err(|e| CryptoError::BadHex(e.to_string()))?;
    if ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
        return Err(CryptoError::BadLength(ciphertext.len()));
    }

    // sm4 0.6 只提供裸分组密码，因此 ECB 模式按 16 字节分组自行迭代。
    // 上游用的是 **ECB** —— 相同明文分组会产生相同密文分组，这一点无法回避：
    // 协议就是如此定义的，密钥每日轮换是唯一的缓解。
    let cipher = Sm4::new_from_slice(key).map_err(|e| CryptoError::Decrypt(e.to_string()))?;

    let mut out = Vec::with_capacity(ciphertext.len());
    for chunk in ciphertext.chunks_exact(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        cipher.decrypt_block((&mut block).into());
        out.extend_from_slice(&block);
    }

    // 去 PKCS#7 填充。填充非法时不报错——上游可能在明文尾部残留额外字节，
    // 由后续的 extract_json 负责截出 JSON 主体。
    if let Some(&pad) = out.last() {
        let pad = pad as usize;
        if (1..=16).contains(&pad) && pad <= out.len() {
            out.truncate(out.len() - pad);
        }
    }

    Ok(out)
}

/// 从解密缓冲里截取最外层的 JSON 对象或数组。
///
/// # 为什么需要这一步
///
/// 上游对明文做了 PKCS#7 填充后才加密，而某些实现会在明文尾部残留填充字节，
/// 导致 `serde_json` 直接解析失败。这里按括号配对截出 JSON 主体。
///
/// 关键细节：**必须先比较 `{` 和 `[` 谁先出现**，而不是优先找 `[`——
/// 否则字符串值里的 `[` 会被误认为文档根。扫描时还要跳过字符串字面量
/// （含 `\"` 转义），否则字符串里的括号会破坏配对计数。
pub fn extract_json(raw: &[u8]) -> Result<&[u8], CryptoError> {
    let obj_start = raw.iter().position(|&b| b == b'{');
    let arr_start = raw.iter().position(|&b| b == b'[');

    let start = match (obj_start, arr_start) {
        (Some(o), Some(a)) => o.min(a),
        (Some(o), None) => o,
        (None, Some(a)) => a,
        (None, None) => {
            let preview = String::from_utf8_lossy(&raw[..raw.len().min(64)]).to_string();
            return Err(CryptoError::NoJson(preview));
        }
    };

    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false;

    for (i, &b) in raw.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(&raw[start..=i]);
                }
            }
            _ => {}
        }
    }

    Err(CryptoError::NoJson(format!(
        "括号未闭合（起始 offset={start}），前 64 字节：{:?}",
        String::from_utf8_lossy(&raw[..raw.len().min(64)])
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 国标测试向量（GB/T 32907-2016 附录 A）
    ///
    /// 这是整个加密层的基石：如果这个向量对不上，说明 SM4 实现有问题，
    /// 所有加密接口都会解出乱码。
    #[test]
    fn sm4_matches_gb_test_vector() {
        use cipher::BlockCipherEncrypt;

        let key = hex::decode("0123456789abcdeffedcba9876543210").unwrap();
        let mut block = [0u8; 16];
        block.copy_from_slice(&hex::decode("0123456789abcdeffedcba9876543210").unwrap());

        let cipher = Sm4::new_from_slice(&key).unwrap();
        cipher.encrypt_block((&mut block).into());

        assert_eq!(
            hex::encode_upper(block),
            "681EDF34D206965E86B3E94F536E4246",
            "SM4 实现与国标向量不符"
        );
    }

    #[test]
    fn salt_key_derivation_matches_python_reference() {
        // 与 wuhanbus-api 的 sm4_lib.py 对齐：key = bytes.fromhex(SALT + content)[:16]
        let m = SecretMaterial::derive("20261013", "3b9d361d3687f208").unwrap();
        assert_eq!(hex::encode(m.key), "5ba86bc8c561d1393b9d361d3687f208");
    }

    #[test]
    fn extract_json_handles_brace_inside_string() {
        // 关键回归：字符串里的括号不能破坏配对。
        // 注意用 `as_bytes()` 而非 `br#"..."#` —— 后者不允许非 ASCII 字符。
        let raw = r#"{"stopName":"某某站(东行)[临时]","n":1}"#.as_bytes();
        assert_eq!(extract_json(raw).unwrap(), raw);

        // 字符串里带转义引号
        let raw2 = r#"{"a":"say \"hi\" {ok}","b":2}"#.as_bytes();
        assert_eq!(extract_json(raw2).unwrap(), raw2);
    }

    #[test]
    fn extract_json_picks_earliest_root() {
        // 数组根，且元素内含有对象
        let raw = br#"[{"a":1},{"b":2}]"#;
        assert_eq!(extract_json(raw).unwrap(), &raw[..]);
    }

    #[test]
    fn extract_json_drops_trailing_padding_junk() {
        let raw = b"\x01\x02{\"success\":true}\x03\x03\x03";
        assert_eq!(extract_json(raw).unwrap(), br#"{"success":true}"#);
    }

    #[test]
    fn extract_json_errors_on_no_json() {
        assert!(matches!(
            extract_json(b"\x00\x01\x02garbage"),
            Err(CryptoError::NoJson(_))
        ));
        assert!(matches!(
            extract_json(br#"{"unclosed":1"#),
            Err(CryptoError::NoJson(_))
        ));
    }

    /// SM4-ECB 往返：加密后再解密应还原（同时验证 PKCS#7 去填充）
    #[test]
    fn sm4_ecb_roundtrip_with_padding() {
        use cipher::BlockCipherEncrypt;

        let key = [7u8; 16];
        let plain = br#"{"lineName":"715","stopDetailList":[]}"#;

        // 手工做 PKCS#7 填充 + ECB 逐块加密（与 decrypt_bytes 对称）
        let pad_len = 16 - (plain.len() % 16);
        let mut buf = plain.to_vec();
        buf.extend(std::iter::repeat(pad_len as u8).take(pad_len));

        let cipher = Sm4::new_from_slice(&key).unwrap();
        // chunks_exact_mut 给出的是 `&mut [u8]`，需先落成 `[u8; 16]`
        // 才能满足 cipher 的 `Block` 类型（固定长度数组）。
        for chunk in buf.chunks_exact_mut(16) {
            let mut block = [0u8; 16];
            block.copy_from_slice(chunk);
            cipher.encrypt_block((&mut block).into());
            chunk.copy_from_slice(&block);
        }

        let back = decrypt_bytes(&hex::encode(&buf), &key).unwrap();
        assert_eq!(back, plain);
    }

    /// 解密出的明文含非法填充时不应 panic，交由 extract_json 兜底
    #[test]
    fn tolerates_invalid_padding() {
        use cipher::BlockCipherEncrypt;

        let key = [3u8; 16];
        // 16 字节整倍数且末字节 0xFF —— 不是合法填充长度
        let raw = b"AAAAAAAAAAAAAAAA";
        let cipher = Sm4::new_from_slice(&key).unwrap();
        let mut block = [0u8; 16];
        block.copy_from_slice(raw);
        cipher.encrypt_block((&mut block).into());

        let out = decrypt_bytes(&hex::encode(block), &key);
        assert!(out.is_ok(), "非法填充不应导致解密报错");
        assert_eq!(out.unwrap(), raw, "非法填充应原样返回，不裁剪");
    }

    #[test]
    fn rejects_malformed_ciphertext() {
        let key = [0u8; 16];
        assert!(matches!(
            decrypt_bytes("zzzz", &key),
            Err(CryptoError::BadHex(_))
        ));
        // 长度不是 16 的倍数
        assert!(matches!(
            decrypt_bytes("00112233", &key),
            Err(CryptoError::BadLength(4))
        ));
    }
}
