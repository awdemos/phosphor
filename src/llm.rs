use crate::generative::{SceneSpec, extract_spec};

#[derive(Clone, Debug)]
pub struct LlmConfig {
    /// e.g. http://localhost:11434/v1 (Ollama) or http://localhost:8080/v1 (llama.cpp)
    pub base: String,
    pub model: String,
    pub api_key: Option<String>,
}

impl LlmConfig {
    pub fn from_env() -> Option<LlmConfig> {
        let base = std::env::var("PHOSPHOR_LLM_URL").ok()?;
        let model = std::env::var("PHOSPHOR_LLM_MODEL").ok()?;
        if base.is_empty() || model.is_empty() {
            return None;
        }
        Some(LlmConfig {
            base: base.trim_end_matches('/').to_string(),
            model,
            api_key: std::env::var("PHOSPHOR_LLM_API_KEY").ok(),
        })
    }

    /// Probe common local serving ports for an OpenAI-compatible endpoint.
    pub fn autodetect() -> Option<LlmConfig> {
        for base in ["http://localhost:11434/v1", "http://localhost:8080/v1"] {
            let url = base.replace("/v1", "") + "/health";
            if ureq::get(&url)
                .timeout(std::time::Duration::from_millis(400))
                .call()
                .is_ok()
            {
                let model = probe_model(base).unwrap_or_else(|| "local".to_string());
                return Some(LlmConfig {
                    base: base.to_string(),
                    model,
                    api_key: None,
                });
            }
        }
        None
    }

    pub fn resolve(offline: bool) -> Option<LlmConfig> {
        if offline {
            None
        } else {
            LlmConfig::from_env().or_else(LlmConfig::autodetect)
        }
    }
}

fn probe_model(base: &str) -> Option<String> {
    let v: serde_json::Value = ureq::get(&format!("{base}/models"))
        .timeout(std::time::Duration::from_millis(600))
        .call()
        .ok()?
        .into_json()
        .ok()?;
    v["data"][0]["id"].as_str().map(str::to_string)
}

pub fn parse_chat_content(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
}

const SYSTEM_PROMPT: &str = "You design terminal screensaver scenes. Reply with ONLY a JSON object: {\"palette\": one of ember,lagoon,orchard,polaris,rainforest,monolith, \"mode\": one of orbital,rain,plasma,pipes, \"density\": 0..1, \"speed\": 0..1, \"glyphs\": one of katakana,braille,ascii,blocks, \"hue_drift\": 0..1}. Invent a fresh variation on the given theme.";

/// Ask the LLM for a scene; None on any failure (caller falls back to the
/// offline synthesizer).
pub fn fetch_spec(config: &LlmConfig, prompt: &str) -> Option<SceneSpec> {
    let body = serde_json::json!({
        "model": config.model,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": prompt},
        ],
        "temperature": 0.9,
        "max_tokens": 300,
    });
    let mut req = ureq::post(&format!("{}/chat/completions", config.base))
        .timeout(std::time::Duration::from_secs(5))
        .set("Content-Type", "application/json");
    if let Some(key) = &config.api_key {
        req = req.set("Authorization", &format!("Bearer {key}"));
    }
    let resp = req.send_json(body).ok()?;
    let text = resp.into_string().ok()?;
    let content = parse_chat_content(&text)?;
    extract_spec(&content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    #[test]
    fn from_env_requires_url_and_model() {
        unsafe {
            std::env::set_var("PHOSPHOR_LLM_URL", "http://x:1/v1");
            std::env::set_var("PHOSPHOR_LLM_MODEL", "m");
            assert!(LlmConfig::from_env().is_some());
            std::env::remove_var("PHOSPHOR_LLM_MODEL");
            assert!(LlmConfig::from_env().is_none());
            std::env::remove_var("PHOSPHOR_LLM_URL");
        }
    }

    #[test]
    fn parse_chat_content_reads_openai_shape() {
        let body = r#"{"choices":[{"message":{"content":"{\"palette\":\"ember\"}"}}]}"#;
        let c = parse_chat_content(body).unwrap();
        assert!(c.contains("ember"));
        assert!(parse_chat_content("{}").is_none());
        assert!(parse_chat_content("not json").is_none());
    }

    #[test]
    fn fetch_spec_against_mock_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).unwrap();
            let _req = String::from_utf8_lossy(&buf[..n]);
            let body = r#"{"choices":[{"message":{"content":"{\"palette\":\"lagoon\",\"mode\":\"plasma\",\"density\":0.8,\"speed\":0.4,\"glyphs\":\"braille\",\"hue_drift\":0.1}"}}]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            use std::io::Write;
            stream.write_all(resp.as_bytes()).unwrap();
        });
        let cfg = LlmConfig {
            base: format!("http://{addr}/v1"),
            model: "mock".into(),
            api_key: None,
        };
        let spec = fetch_spec(&cfg, "deep ocean").unwrap();
        assert_eq!(spec.palette, "lagoon");
        assert_eq!(spec.mode, "plasma");
        server.join().unwrap();
    }

    #[test]
    fn fetch_spec_fails_gracefully_on_dead_port() {
        let cfg = LlmConfig {
            base: "http://127.0.0.1:1/v1".into(),
            model: "x".into(),
            api_key: None,
        };
        assert!(fetch_spec(&cfg, "anything").is_none());
    }
}
